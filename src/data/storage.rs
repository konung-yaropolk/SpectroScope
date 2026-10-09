//! Derived spectrum data: current trace, running average, peak holds, history
//! and baseline.
//!
//! Port of QSpectrumAnalyzer's `data.DataStorage`. The Python version pushed
//! each of these onto a one-thread `QThreadPool` and emitted a Qt signal per
//! result; here the work is a handful of `O(bins)` passes done inline and the
//! GUI is told what changed through [`Updated`] flags. At 100k bins the whole
//! update is well under a millisecond, so the thread hop would cost more than
//! it saves.

use std::sync::Arc;

use crate::dsp::smooth::{SmoothWindow, Smoother};
use crate::sources::Frame;

/// Ceiling on the history ring, in bytes.
///
/// The requested row count is a *time* depth, but the cost of it scales with
/// the sweep width: 4096 rows of a 56k-bin sweep is 920 MB, which would be an
/// out-of-memory abort rather than a deep waterfall. Rows are given up first
/// because a narrower time window is a far smaller loss than failing to run.
const HISTORY_BYTE_BUDGET: usize = 128 << 20;

/// Rows the ring can actually afford at this sweep width.
fn affordable_rows(requested: usize, bins: usize) -> usize {
    if bins == 0 {
        return requested.max(1);
    }
    let max_rows = HISTORY_BYTE_BUDGET / (bins * std::mem::size_of::<f32>());
    requested.min(max_rows.max(1)).max(1)
}

/// Which derived series changed during an update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Updated {
    pub data: bool,
    pub history: bool,
    pub average: bool,
    pub peak_hold_max: bool,
    pub peak_hold_min: bool,
    pub baseline: bool,
    /// The whole series were rebuilt, so cached plot geometry must be dropped.
    pub recalculated: bool,
}

impl Updated {
    pub fn any(&self) -> bool {
        self.data
            || self.history
            || self.average
            || self.peak_hold_max
            || self.peak_hold_min
            || self.baseline
            || self.recalculated
    }

    pub const ALL: Self = Self {
        data: true,
        history: true,
        average: true,
        peak_hold_max: true,
        peak_hold_min: true,
        baseline: true,
        recalculated: true,
    };
}

/// A loaded baseline: a reference sweep subtracted from live data.
#[derive(Clone, Debug, Default)]
pub struct Baseline {
    pub x: Option<Arc<Vec<f64>>>,
    pub y: Option<Vec<f32>>,
}

impl Baseline {
    pub fn is_loaded(&self) -> bool {
        self.y.is_some()
    }

    pub fn len(&self) -> usize {
        self.y.as_ref().map_or(0, |v| v.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Default)]
pub struct DataStorage {
    /// Bin centre frequencies, Hz. `None` until the first sweep arrives.
    x: Option<Arc<Vec<f64>>>,
    /// Current trace, smoothed if smoothing is on.
    y: Vec<f32>,
    average: Vec<f32>,
    peak_hold_max: Vec<f32>,
    peak_hold_min: Vec<f32>,
    history: super::history::HistoryBuffer,

    /// Sweeps folded into `average` so far.
    average_counter: u64,
    /// Timestamp of the newest sweep, unix seconds.
    timestamp: f64,

    max_history_size: usize,

    smooth_enabled: bool,
    smoother: Smoother,

    subtract_baseline: bool,
    baseline: Baseline,
    /// Baseline that is currently folded into `history`, so it can be added
    /// back before a different one is subtracted.
    applied_baseline: Option<Vec<f32>>,

    /// Scratch space for the smoother, kept to avoid a per-sweep allocation.
    scratch: Vec<f32>,
}

impl DataStorage {
    pub fn new(max_history_size: usize) -> Self {
        Self {
            max_history_size: max_history_size.max(1),
            smoother: Smoother::new(SmoothWindow::Hanning, 11),
            ..Default::default()
        }
    }

    // --- accessors --------------------------------------------------------

    pub fn x(&self) -> Option<&Arc<Vec<f64>>> {
        self.x.as_ref()
    }

    pub fn y(&self) -> &[f32] {
        &self.y
    }

    pub fn average(&self) -> &[f32] {
        &self.average
    }

    pub fn peak_hold_max(&self) -> &[f32] {
        &self.peak_hold_max
    }

    pub fn peak_hold_min(&self) -> &[f32] {
        &self.peak_hold_min
    }

    pub fn history(&self) -> &super::history::HistoryBuffer {
        &self.history
    }

    pub fn baseline(&self) -> &Baseline {
        &self.baseline
    }

    pub fn timestamp(&self) -> f64 {
        self.timestamp
    }

    pub fn average_counter(&self) -> u64 {
        self.average_counter
    }

    pub fn has_data(&self) -> bool {
        self.x.is_some() && !self.y.is_empty()
    }

    pub fn bins(&self) -> usize {
        self.y.len()
    }

    pub fn max_history_size(&self) -> usize {
        self.max_history_size
    }

    pub fn smooth_enabled(&self) -> bool {
        self.smooth_enabled
    }

    pub fn subtract_baseline(&self) -> bool {
        self.subtract_baseline
    }

    /// The `age`-th newest history row with smoothing applied, which is what
    /// the persistence curves draw.
    pub fn history_row_smoothed(&mut self, age: usize) -> Option<Vec<f32>> {
        let row = self.history.row_from_newest(age)?.to_vec();
        if self.smooth_enabled {
            Some(self.smoother.apply(&row))
        } else {
            Some(row)
        }
    }

    /// Lowest and highest value across the whole history, for auto levels.
    pub fn history_range(&self) -> Option<(f32, f32)> {
        if self.history.is_empty() {
            return None;
        }
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for row in self.history.iter_rows() {
            for &v in row {
                if v.is_finite() {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
    }

    // --- lifecycle --------------------------------------------------------

    /// Forget everything, including the frequency axis and history.
    pub fn reset(&mut self) {
        self.x = None;
        self.history.clear();
        self.applied_baseline = None;
        self.reset_data();
    }

    /// Forget the derived series but keep the axis and history.
    pub fn reset_data(&mut self) {
        self.y.clear();
        self.average.clear();
        self.peak_hold_max.clear();
        self.peak_hold_min.clear();
        self.average_counter = 0;
    }

    /// Resize the history ring. Existing rows are dropped, as in the Qt
    /// version, because the waterfall geometry changes with it.
    pub fn set_max_history_size(&mut self, size: usize) {
        let size = size.max(1);
        if size == self.max_history_size {
            return;
        }
        self.max_history_size = size;
        let bins = self.history.bins();
        if bins > 0 {
            self.history.reshape(bins, affordable_rows(size, bins));
        }
    }

    // --- the hot path -----------------------------------------------------

    /// Fold one sweep in.
    ///
    /// A sweep whose bin count differs from the established one is rejected
    /// (same as the Python version, which printed a line and returned): the
    /// history ring, the average and the peak holds all share one geometry, and
    /// silently resizing them would throw away the run.
    pub fn update(&mut self, frame: Frame) -> Result<Updated, BinMismatch> {
        let Frame { timestamp, x, y } = frame;

        if !self.y.is_empty() && y.len() != self.y.len() {
            return Err(BinMismatch {
                got: y.len(),
                expected: self.y.len(),
            });
        }

        let mut y = y;
        self.timestamp = timestamp;
        self.average_counter += 1;

        if self.x.is_none() {
            self.x = Some(x);
        }

        // Baseline first, so history, average and peak holds all agree. A
        // baseline whose bin count does not match is ignored rather than
        // partially applied.
        let baseline_applied = match self.baseline.y.as_ref() {
            Some(b) if self.subtract_baseline && b.len() == y.len() => {
                for (v, bv) in y.iter_mut().zip(b.iter()) {
                    *v -= *bv;
                }
                true
            }
            _ => false,
        };

        // History keeps the *unsmoothed* trace, matching the original: the
        // waterfall shows raw measurements and smoothing stays a display
        // option that can be toggled without losing data.
        let rows = affordable_rows(self.max_history_size, y.len());
        if self.history.bins() != y.len() || self.history.capacity() != rows {
            if rows < self.max_history_size {
                log::warn!(
                    "waterfall history limited to {rows} sweeps (asked for {}):                      {} bins would need {} MiB",
                    self.max_history_size,
                    y.len(),
                    self.max_history_size * y.len() * 4 / (1 << 20)
                );
            }
            self.history.reshape(y.len(), rows);
            self.applied_baseline = None;
        }
        self.history.append(&y);
        // Every live row now has exactly this baseline folded in, which is what
        // `recalculate_history` relies on to undo it later.
        self.applied_baseline = if baseline_applied {
            self.baseline.y.clone()
        } else {
            None
        };

        if self.smooth_enabled {
            self.smoother.apply_into(&y, &mut self.scratch);
            std::mem::swap(&mut self.y, &mut self.scratch);
        } else {
            self.y.clear();
            self.y.extend_from_slice(&y);
        }

        let mut updated = Updated {
            data: true,
            history: true,
            ..Default::default()
        };

        // Average, max hold and min hold all track the displayed (smoothed)
        // trace, as they did in the Qt build.
        if self.average.len() != self.y.len() || self.average_counter <= 1 {
            self.average.clear();
            self.average.extend_from_slice(&self.y);
        } else {
            // Running mean: avg = (avg * (n - 1) + y) / n.
            let n = self.average_counter as f32;
            for (a, v) in self.average.iter_mut().zip(self.y.iter()) {
                *a = (*a * (n - 1.0) + *v) / n;
            }
            updated.average = true;
        }

        if self.peak_hold_max.len() != self.y.len() {
            self.peak_hold_max.clear();
            self.peak_hold_max.extend_from_slice(&self.y);
        } else {
            for (p, v) in self.peak_hold_max.iter_mut().zip(self.y.iter()) {
                *p = p.max(*v);
            }
            updated.peak_hold_max = true;
        }

        if self.peak_hold_min.len() != self.y.len() {
            self.peak_hold_min.clear();
            self.peak_hold_min.extend_from_slice(&self.y);
        } else {
            for (p, v) in self.peak_hold_min.iter_mut().zip(self.y.iter()) {
                *p = p.min(*v);
            }
            updated.peak_hold_min = true;
        }

        Ok(updated)
    }

    // --- settings that trigger a rebuild ----------------------------------

    /// Turn smoothing on/off or change its parameters.
    pub fn set_smooth(&mut self, enabled: bool, length: usize, window: SmoothWindow) -> Updated {
        let changed = enabled != self.smooth_enabled
            || length != self.smoother.len()
            || window != self.smoother.window();
        if !changed {
            return Updated::default();
        }
        self.smooth_enabled = enabled;
        self.smoother.configure(window, length);
        self.recalculate_data()
    }

    /// Install a baseline and/or toggle subtraction.
    ///
    /// The history is re-levelled in place: whatever baseline is currently
    /// folded into it is added back before the new one is subtracted, so
    /// switching baselines repeatedly cannot accumulate error.
    pub fn set_baseline(&mut self, enabled: bool, baseline: Baseline) -> Updated {
        self.baseline = baseline;
        self.subtract_baseline = enabled;

        let mut updated = self.recalculate_history();
        updated.baseline = true;
        let d = self.recalculate_data();
        updated.data |= d.data;
        updated.average |= d.average;
        updated.peak_hold_max |= d.peak_hold_max;
        updated.peak_hold_min |= d.peak_hold_min;
        updated.recalculated = true;
        updated
    }

    /// Re-level the stored history for the current baseline setting.
    pub fn recalculate_history(&mut self) -> Updated {
        if self.history.is_empty() {
            // Nothing stored yet, so there is nothing to re-level; the next
            // `update` establishes the invariant itself.
            self.applied_baseline = None;
            return Updated::default();
        }

        let bins = self.history.bins();
        let add_back = self.applied_baseline.take().filter(|b| b.len() == bins);
        let subtract = if self.subtract_baseline {
            self.baseline
                .y
                .as_ref()
                .filter(|b| b.len() == bins)
                .cloned()
        } else {
            None
        };

        if add_back.is_none() && subtract.is_none() {
            self.applied_baseline = None;
            return Updated::default();
        }

        self.history.for_each_row_mut(|row| {
            if let Some(b) = &add_back {
                for (v, bv) in row.iter_mut().zip(b.iter()) {
                    *v += *bv;
                }
            }
            if let Some(b) = &subtract {
                for (v, bv) in row.iter_mut().zip(b.iter()) {
                    *v -= *bv;
                }
            }
        });

        self.applied_baseline = subtract;
        Updated {
            history: true,
            recalculated: true,
            ..Default::default()
        }
    }

    /// Rebuild the current trace, average and peak holds from the history.
    ///
    /// Unlike the Python original -- whose smoothing branch seeded the average
    /// with the newest row and then folded the rest in with a weight of zero on
    /// the first step, discarding the seed -- this computes the plain mean of
    /// every (smoothed) row, which is what both branches were meant to produce.
    pub fn recalculate_data(&mut self) -> Updated {
        if self.history.is_empty() {
            return Updated::default();
        }

        let bins = self.history.bins();
        let rows = self.history.len();

        let mut sum = vec![0.0f64; bins];
        let mut max = vec![f32::NEG_INFINITY; bins];
        let mut min = vec![f32::INFINITY; bins];

        // Collect oldest to newest so `self.y` ends up holding the newest row.
        let mut newest_smoothed: Vec<f32> = Vec::new();
        for age in (0..rows).rev() {
            let raw = match self.history.row_from_newest(age) {
                Some(r) => r,
                None => continue,
            };
            let smoothed: &[f32] = if self.smooth_enabled {
                self.smoother.apply_into(raw, &mut self.scratch);
                &self.scratch
            } else {
                raw
            };
            for i in 0..bins.min(smoothed.len()) {
                let v = smoothed[i];
                sum[i] += v as f64;
                max[i] = max[i].max(v);
                min[i] = min[i].min(v);
            }
            if age == 0 {
                newest_smoothed = smoothed.to_vec();
            }
        }

        let n = rows as f64;
        self.y = newest_smoothed;
        self.average = sum.iter().map(|s| (*s / n) as f32).collect();
        self.peak_hold_max = max;
        self.peak_hold_min = min;
        self.average_counter = rows as u64;

        Updated {
            data: true,
            average: true,
            peak_hold_max: true,
            peak_hold_min: true,
            recalculated: true,
            ..Default::default()
        }
    }
}

/// A sweep arrived with the wrong number of bins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinMismatch {
    pub got: usize,
    pub expected: usize,
}

impl std::fmt::Display for BinMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} bins coming from backend, expected {}",
            self.got, self.expected
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ts: f64, y: &[f32]) -> Frame {
        Frame {
            timestamp: ts,
            x: Arc::new((0..y.len()).map(|i| i as f64 * 1e3).collect()),
            y: y.to_vec(),
        }
    }

    #[test]
    fn first_sweep_seeds_every_series() {
        let mut d = DataStorage::new(10);
        d.update(frame(1.0, &[-10.0, -20.0])).unwrap();
        assert_eq!(d.y(), [-10.0, -20.0]);
        assert_eq!(d.average(), [-10.0, -20.0]);
        assert_eq!(d.peak_hold_max(), [-10.0, -20.0]);
        assert_eq!(d.peak_hold_min(), [-10.0, -20.0]);
        assert_eq!(d.history().len(), 1);
        assert_eq!(d.timestamp(), 1.0);
    }

    #[test]
    fn average_is_a_running_mean() {
        let mut d = DataStorage::new(10);
        d.update(frame(1.0, &[0.0])).unwrap();
        d.update(frame(2.0, &[10.0])).unwrap();
        assert!((d.average()[0] - 5.0).abs() < 1e-5);
        d.update(frame(3.0, &[20.0])).unwrap();
        assert!((d.average()[0] - 10.0).abs() < 1e-5);
        assert_eq!(d.average_counter(), 3);
    }

    #[test]
    fn peak_holds_track_extremes() {
        let mut d = DataStorage::new(10);
        d.update(frame(1.0, &[0.0, 0.0])).unwrap();
        d.update(frame(2.0, &[5.0, -5.0])).unwrap();
        d.update(frame(3.0, &[-2.0, 3.0])).unwrap();
        assert_eq!(d.peak_hold_max(), [5.0, 3.0]);
        assert_eq!(d.peak_hold_min(), [-2.0, -5.0]);
    }

    #[test]
    fn bin_count_change_is_rejected() {
        let mut d = DataStorage::new(10);
        d.update(frame(1.0, &[0.0, 0.0])).unwrap();
        let err = d.update(frame(2.0, &[0.0, 0.0, 0.0])).unwrap_err();
        assert_eq!(
            err,
            BinMismatch {
                got: 3,
                expected: 2
            }
        );
        // The rejected sweep must not have disturbed anything.
        assert_eq!(d.history().len(), 1);
        assert_eq!(d.bins(), 2);
    }

    #[test]
    fn history_stores_unsmoothed_data() {
        let mut d = DataStorage::new(10);
        d.set_smooth(true, 5, SmoothWindow::Rectangular);
        let spike: Vec<f32> = (0..64).map(|i| if i == 32 { 100.0 } else { 0.0 }).collect();
        d.update(frame(1.0, &spike)).unwrap();

        // Displayed trace is smoothed: the spike is spread over 5 bins.
        assert!(d.y()[32] < 50.0, "{}", d.y()[32]);
        // History is raw.
        assert_eq!(d.history().newest().unwrap()[32], 100.0);
    }

    #[test]
    fn baseline_is_subtracted_and_can_be_swapped() {
        let mut d = DataStorage::new(10);
        d.update(frame(1.0, &[-10.0, -10.0])).unwrap();

        let b1 = Baseline {
            x: Some(Arc::new(vec![0.0, 1e3])),
            y: Some(vec![-2.0, -4.0]),
        };
        d.set_baseline(true, b1.clone());
        // History had -10; subtracting -2/-4 leaves -8/-6.
        assert_eq!(d.history().newest().unwrap(), [-8.0, -6.0]);

        // Swapping baselines must not accumulate: the old one is added back.
        let b2 = Baseline {
            x: Some(Arc::new(vec![0.0, 1e3])),
            y: Some(vec![-1.0, -1.0]),
        };
        d.set_baseline(true, b2);
        assert_eq!(d.history().newest().unwrap(), [-9.0, -9.0]);

        // Turning it off restores the raw values.
        d.set_baseline(false, Baseline::default());
        assert_eq!(d.history().newest().unwrap(), [-10.0, -10.0]);
    }

    #[test]
    fn baseline_with_wrong_bin_count_is_ignored() {
        let mut d = DataStorage::new(10);
        d.update(frame(1.0, &[-10.0, -10.0])).unwrap();
        d.set_baseline(
            true,
            Baseline {
                x: None,
                y: Some(vec![-1.0, -1.0, -1.0]),
            },
        );
        assert_eq!(d.history().newest().unwrap(), [-10.0, -10.0]);
        d.update(frame(2.0, &[-20.0, -20.0])).unwrap();
        assert_eq!(d.y(), [-20.0, -20.0]);
    }

    #[test]
    fn recalculate_data_gives_the_mean_of_all_rows() {
        let mut d = DataStorage::new(10);
        for v in [0.0f32, 10.0, 20.0, 30.0] {
            d.update(frame(1.0, &[v])).unwrap();
        }
        let u = d.recalculate_data();
        assert!(u.recalculated);
        assert!((d.average()[0] - 15.0).abs() < 1e-5);
        assert_eq!(d.peak_hold_max(), [30.0]);
        assert_eq!(d.peak_hold_min(), [0.0]);
        // The current trace is the newest row.
        assert_eq!(d.y(), [30.0]);
    }

    #[test]
    fn recalculate_only_uses_live_rows() {
        // A ring with room for 8 but only 3 written must not average in zeros.
        let mut d = DataStorage::new(8);
        for v in [10.0f32, 20.0, 30.0] {
            d.update(frame(1.0, &[v])).unwrap();
        }
        d.recalculate_data();
        assert!((d.average()[0] - 20.0).abs() < 1e-5);
        assert_eq!(d.peak_hold_min(), [10.0]);
    }

    #[test]
    fn toggling_smoothing_rebuilds_from_history() {
        let mut d = DataStorage::new(10);
        let ramp: Vec<f32> = (0..64).map(|i| i as f32).collect();
        d.update(frame(1.0, &ramp)).unwrap();

        let u = d.set_smooth(true, 11, SmoothWindow::Hanning);
        assert!(u.recalculated);
        // A straight line survives the filter, so the trace is ~unchanged.
        for (a, b) in ramp.iter().zip(d.y()) {
            assert!((a - b).abs() < 1e-2);
        }

        // Same settings again: nothing to do.
        assert_eq!(
            d.set_smooth(true, 11, SmoothWindow::Hanning),
            Updated::default()
        );
    }

    #[test]
    fn history_rows_are_capped_by_memory() {
        // A narrow sweep gets everything it asked for.
        assert_eq!(affordable_rows(4096, 1024), 4096);
        // A very wide one is cut down rather than allocating hundreds of MiB.
        let rows = affordable_rows(4096, 56_167);
        assert!(rows < 4096 && rows > 0, "{rows}");
        assert!(rows * 56_167 * 4 <= HISTORY_BYTE_BUDGET);
        // Never zero, however absurd the sweep.
        assert_eq!(affordable_rows(4096, usize::MAX / 8), 1);
        assert_eq!(affordable_rows(0, 1024), 1);
        // No bins yet means nothing to budget against.
        assert_eq!(affordable_rows(7, 0), 7);
    }

    #[test]
    fn a_wide_sweep_does_not_allocate_the_full_history() {
        let mut d = DataStorage::new(4096);
        let wide = vec![-90.0f32; 50_000];
        d.update(frame(1.0, &wide)).unwrap();
        assert!(d.history().capacity() < 4096);
        assert!(d.history().capacity() >= 1);
        assert_eq!(d.history().len(), 1);
    }

    #[test]
    fn resizing_history_resets_the_ring() {
        let mut d = DataStorage::new(4);
        d.update(frame(1.0, &[1.0])).unwrap();
        assert_eq!(d.history().capacity(), 4);
        d.set_max_history_size(16);
        assert_eq!(d.history().capacity(), 16);
        assert!(d.history().is_empty());
    }

    #[test]
    fn reset_clears_axis_and_history() {
        let mut d = DataStorage::new(4);
        d.update(frame(1.0, &[1.0])).unwrap();
        assert!(d.has_data());
        d.reset();
        assert!(!d.has_data());
        assert!(d.x().is_none());
        assert!(d.history().is_empty());
    }

    #[test]
    fn history_range_reports_extremes() {
        let mut d = DataStorage::new(4);
        d.update(frame(1.0, &[-30.0, -10.0])).unwrap();
        d.update(frame(2.0, &[-50.0, -20.0])).unwrap();
        let (lo, hi) = d.history_range().unwrap();
        assert_eq!((lo, hi), (-50.0, -10.0));
    }
}
