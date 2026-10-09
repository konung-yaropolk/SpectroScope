//! The main spectrum plot: live trace, peak holds, average, baseline and the
//! persistence fan, with a crosshair readout.
//!
//! Replaces `SpectrumPlotWidget` from the Python version's `plot.py`.
//!
//! Two things here are about speed rather than appearance. Plot points are
//! cached in `[f64; 2]` buffers and rebuilt only when the underlying series
//! actually changed, so a repaint with no new sweep costs nothing; and each
//! curve is decimated to roughly two points per pixel column, keeping the peaks,
//! because a 100k-bin sweep has far more points than a plot has columns. The Qt
//! build had the equivalent (`setDownsampling(mode="peak")`) commented out and
//! pushed every point into Qt instead.

use std::collections::VecDeque;

use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::config::Config;
use crate::data::{DataStorage, Updated};

use super::LINKED_X_AXIS;

/// Points-per-pixel budget. Two is what min/max decimation needs to keep a
/// spike visible: one point for the top, one for the bottom.
const POINTS_PER_PIXEL: f32 = 2.0;

/// Below this many points there is nothing to gain from decimating.
const DECIMATION_FLOOR: usize = 2048;

#[derive(Default)]
pub struct SpectrumPlot {
    main: Vec<[f64; 2]>,
    average: Vec<[f64; 2]>,
    peak_max: Vec<[f64; 2]>,
    peak_min: Vec<[f64; 2]>,
    baseline: Vec<[f64; 2]>,

    /// Raw traces behind the persistence fan, newest first. Kept as `f32`
    /// rather than plot points because the fan is re-decimated on zoom.
    persistence: VecDeque<Vec<f32>>,
    persistence_points: Vec<Vec<[f64; 2]>>,

    /// Plot geometry from the previous frame. Decimation needs the pixel width
    /// and the visible range, which are only known after layout, so it uses
    /// last frame's values -- off by one frame during an active drag, which is
    /// invisible.
    last_width_px: f32,
    last_view: Option<(f64, f64)>,
    /// Set when the cached buffers must be rebuilt because the view changed
    /// enough to alter the decimation.
    view_dirty: bool,

    /// Visible x range in Hz, handed to the waterfall so the two stay aligned.
    pub view_x: (f64, f64),
    /// Crosshair position in plot coordinates, when the pointer is inside.
    pub readout: Option<(f64, f64)>,
    /// Fit the axes to the data on the next frame.
    ///
    /// egui_plot keeps each plot's zoom in egui memory, which eframe persists
    /// across restarts, so without an explicit refit a new run inherits
    /// whatever window was left over -- including from a completely different
    /// frequency range.
    reset_view: bool,
    /// Screen rectangle of the plot data area, excluding the axis labels.
    ///
    /// The waterfall aligns its image to exactly these columns.
    pub plot_rect: Option<egui::Rect>,
    /// An x range requested from outside, e.g. by zooming the waterfall.
    pending_x: Option<(f64, f64)>,
    /// Axis ranges to install on the next frame, computed from the full sweep.
    ///
    /// `set_auto_bounds` alone cannot do this job: decimation clips the cached
    /// points to the *current* view, so an auto-fit would just re-fit the
    /// clipped subset and the plot could never widen back out.
    pending_bounds: Option<[f64; 4]>,
}

impl SpectrumPlot {
    pub fn new() -> Self {
        Self {
            last_width_px: 1024.0,
            reset_view: true,
            ..Default::default()
        }
    }

    /// Fit the axes to the whole sweep on the next frame.
    pub fn request_refit(&mut self) {
        self.reset_view = true;
    }

    /// Adopt an x range chosen elsewhere, keeping the current y range.
    pub fn set_view_x(&mut self, x0: f64, x1: f64) {
        if x1 > x0 && x0.is_finite() && x1.is_finite() {
            self.pending_x = Some((x0, x1));
        }
    }

    /// Drop every cached curve, as `clear_*` did on the Qt widget.
    pub fn clear(&mut self) {
        self.reset_view = true;
        self.main.clear();
        self.average.clear();
        self.peak_max.clear();
        self.peak_min.clear();
        self.baseline.clear();
        self.persistence.clear();
        self.persistence_points.clear();
        self.last_view = None;
        self.readout = None;
    }

    /// Rebuild the caches for whichever series changed.
    ///
    /// Curves that are switched off are still rebuilt when their data arrives,
    /// so that ticking the checkbox shows the trace immediately instead of
    /// waiting for the next sweep -- the same reason the Qt version called
    /// `update_*` from its checkbox handlers.
    pub fn sync(&mut self, storage: &DataStorage, updated: Updated) {
        let Some(x) = storage.x() else { return };
        let x = x.as_slice();

        // A pending refit decimates over everything, not the stale window, so
        // the points the new bounds are computed from really span the sweep.
        if self.reset_view {
            self.pending_bounds = full_bounds(x, storage.y());
            self.last_view = None;
        }

        let (x0, x1) = self.decimation_range(x);
        let width = self.last_width_px;
        let force = self.view_dirty || updated.recalculated;
        self.view_dirty = false;

        if updated.data || force {
            decimate(x, storage.y(), x0, x1, width, &mut self.main);
        }
        if updated.average || force {
            decimate(x, storage.average(), x0, x1, width, &mut self.average);
        }
        if updated.peak_hold_max || force {
            decimate(
                x,
                storage.peak_hold_max(),
                x0,
                x1,
                width,
                &mut self.peak_max,
            );
        }
        if updated.peak_hold_min || force {
            decimate(
                x,
                storage.peak_hold_min(),
                x0,
                x1,
                width,
                &mut self.peak_min,
            );
        }
        if updated.baseline || force {
            self.baseline.clear();
            let b = storage.baseline();
            if let (Some(bx), Some(by)) = (b.x.as_ref(), b.y.as_ref()) {
                if bx.len() == by.len() {
                    let (bx0, bx1) = self.decimation_range(bx);
                    decimate(bx, by, bx0, bx1, width, &mut self.baseline);
                }
            }
        }

        if updated.data || force {
            self.rebuild_persistence_points(x, x0, x1, width);
        }
    }

    /// Record the newest trace for the persistence fan.
    ///
    /// Call once per sweep, before [`sync`](Self::sync).
    pub fn push_persistence(&mut self, y: &[f32], length: usize) {
        // One more than the fan length: slot 0 is the live trace, which the
        // main curve already draws, so the fan starts at slot 1.
        let cap = length.saturating_add(1).max(2);
        self.persistence.push_front(y.to_vec());
        while self.persistence.len() > cap {
            self.persistence.pop_back();
        }
    }

    /// Refill the fan from the stored history, after the fan length changed or
    /// a recalculation invalidated it.
    pub fn refill_persistence(&mut self, storage: &mut DataStorage, length: usize) {
        self.persistence.clear();
        let cap = length.saturating_add(1).max(2);
        for age in 0..cap {
            match storage.history_row_smoothed(age) {
                Some(row) => self.persistence.push_back(row),
                None => break,
            }
        }
        self.view_dirty = true;
    }

    fn rebuild_persistence_points(&mut self, x: &[f64], x0: f64, x1: f64, width: f32) {
        let fan = self.persistence.len().saturating_sub(1);
        self.persistence_points.resize_with(fan, Vec::new);
        for (slot, buf) in self.persistence_points.iter_mut().enumerate() {
            match self.persistence.get(slot + 1) {
                Some(y) => decimate(x, y, x0, x1, width, buf),
                None => buf.clear(),
            }
        }
    }

    /// The x range to decimate against: the visible window, or everything on
    /// the first frame.
    fn decimation_range(&self, x: &[f64]) -> (f64, f64) {
        match self.last_view {
            Some(v) => v,
            None => (
                x.first().copied().unwrap_or(0.0),
                x.last().copied().unwrap_or(1.0),
            ),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cfg: &Config, height: f32) {
        let colors = &cfg.colors;
        let traces = &cfg.traces;

        // The readout sits above the plot like the Qt version's posLabel.
        ui.horizontal(|ui| {
            ui.add_space(ui.available_width().min(8.0));
            let text = match self.readout {
                Some((f, p)) => format!("f = {:.3} MHz    P = {:.3} dB", f / 1e6, p),
                None => String::new(),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(text);
            });
        });

        // Only clear the request once the bounds have actually been computed
        // from a sweep; a refit asked for before any data arrived must survive
        // until there is something to fit to.
        let refit = self.pending_bounds.take();
        if refit.is_some() {
            self.reset_view = false;
        }
        let pending_x = self.pending_x.take();

        let response = Plot::new("spectroscope.spectrum")
            .height(height)
            .legend(Legend::default().position(egui_plot::Corner::LeftTop))
            .link_axis(LINKED_X_AXIS, [true, false])
            .x_axis_label("Frequency (Hz)")
            .y_axis_label("Power (dB)")
            .allow_scroll(false)
            .label_formatter(|name, value| {
                if name.is_empty() {
                    format!("{:.4} MHz\n{:.2} dB", value.x / 1e6, value.y)
                } else {
                    format!("{name}\n{:.4} MHz\n{:.2} dB", value.x / 1e6, value.y)
                }
            })
            .show(ui, |pu| {
                if let Some([x0, y0, x1, y1]) = refit {
                    pu.set_plot_bounds(egui_plot::PlotBounds::from_min_max([x0, y0], [x1, y1]));
                } else if let Some((x0, x1)) = pending_x {
                    // Only the horizontal range is adopted; the vertical one is
                    // whatever the user or the last refit left in place.
                    let b = pu.plot_bounds();
                    pu.set_plot_bounds(egui_plot::PlotBounds::from_min_max(
                        [x0, b.min()[1]],
                        [x1, b.max()[1]],
                    ));
                }

                // Draw order mirrors the Qt z-values: baseline at the back,
                // then the persistence fan, average, peak holds, main curve.
                if traces.baseline && !self.baseline.is_empty() {
                    pu.line(
                        Line::new("Baseline", PlotPoints::from(self.baseline.clone()))
                            .color(colors.baseline.to_color32()),
                    );
                }

                if traces.persistence {
                    let length = cfg.persistence_length.max(1) as f64;
                    for (slot, pts) in self.persistence_points.iter().enumerate() {
                        if pts.is_empty() {
                            continue;
                        }
                        let factor = cfg.persistence_decay.alpha(slot as f64 + 1.0, length + 1.0);
                        pu.line(
                            Line::new("", PlotPoints::from(pts.clone()))
                                .color(colors.persistence.with_alpha_scaled(factor)),
                        );
                    }
                }

                if traces.average && !self.average.is_empty() {
                    pu.line(
                        Line::new("Average", PlotPoints::from(self.average.clone()))
                            .color(colors.average.to_color32()),
                    );
                }
                if traces.peak_hold_min && !self.peak_min.is_empty() {
                    pu.line(
                        Line::new("Min. hold", PlotPoints::from(self.peak_min.clone()))
                            .color(colors.peak_hold_min.to_color32()),
                    );
                }
                if traces.peak_hold_max && !self.peak_max.is_empty() {
                    pu.line(
                        Line::new("Max. hold", PlotPoints::from(self.peak_max.clone()))
                            .color(colors.peak_hold_max.to_color32()),
                    );
                }
                if traces.main_curve && !self.main.is_empty() {
                    pu.line(
                        Line::new("Spectrum", PlotPoints::from(self.main.clone()))
                            .color(colors.main.to_color32()),
                    );
                }

                // Reading the pointer does not affect the plot; drawing the
                // crosshair as VLine/HLine would. Those are plot items, so
                // auto-bounds grows to include them, which moves the pointer's
                // plot coordinate further out, which grows the bounds again --
                // the axes run away to 1e20 within seconds. pyqtgraph had the
                // same hazard and the Qt build passed `ignoreBounds=True`; here
                // the crosshair is drawn in screen space after the plot closes.
                self.readout = pu.pointer_coordinate().map(|p| (p.x, p.y));

                let bounds = pu.plot_bounds();
                (bounds.min()[0], bounds.max()[0])
            });

        if let Some(pos) = response.response.hover_pos() {
            let rect = response.response.rect;
            let painter = ui.painter_at(rect);
            let guide = egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.7));
            painter.line_segment(
                [
                    egui::pos2(pos.x, rect.top()),
                    egui::pos2(pos.x, rect.bottom()),
                ],
                guide,
            );
            painter.line_segment(
                [
                    egui::pos2(rect.left(), pos.y),
                    egui::pos2(rect.right(), pos.y),
                ],
                guide,
            );
        } else {
            self.readout = None;
        }

        let (vx0, vx1) = response.inner;
        self.view_x = (vx0, vx1);
        self.plot_rect = Some(*response.transform.frame());

        let width = response.response.rect.width().max(1.0);
        // Re-decimate when the view moved by more than a pixel's worth, or the
        // widget was resized: anything smaller would not change a single point.
        let moved = match self.last_view {
            Some((a, b)) => {
                let span = (b - a).abs().max(f64::EPSILON);
                ((a - vx0).abs() / span) > 0.001 || ((b - vx1).abs() / span) > 0.001
            }
            None => true,
        };
        if moved || (width - self.last_width_px).abs() > 1.0 {
            self.view_dirty = true;
        }
        self.last_view = Some((vx0, vx1));
        self.last_width_px = width;
    }
}

/// Collapse `y` to at most about `2 * width_px` points over the visible range,
/// keeping the extremes of every bucket so narrow peaks survive.
///
/// `x` must be ascending. Buckets are taken over the index range that covers
/// `[x0, x1]`, extended by one point on each side so the trace still reaches
/// the edges of the plot while zoomed in.
pub fn decimate(x: &[f64], y: &[f32], x0: f64, x1: f64, width_px: f32, out: &mut Vec<[f64; 2]>) {
    out.clear();
    let n = x.len().min(y.len());
    if n == 0 {
        return;
    }

    let (lo, hi) = visible_index_range(x, n, x0, x1);
    let count = hi - lo;
    let budget = ((width_px.max(1.0) * POINTS_PER_PIXEL) as usize).max(64);

    if count <= budget.max(DECIMATION_FLOOR) {
        out.reserve(count);
        for i in lo..hi {
            out.push([x[i], y[i] as f64]);
        }
        return;
    }

    // Integer bucket boundaries, so no bucket is ever empty and the last one
    // always ends exactly at `hi`.
    let buckets = budget / 2;
    out.reserve(buckets * 2 + 2);
    for b in 0..buckets {
        let start = lo + count * b / buckets;
        let end = lo + count * (b + 1) / buckets;
        if start >= end {
            continue;
        }

        let mut min_i = start;
        let mut max_i = start;
        for i in start..end {
            if y[i] < y[min_i] {
                min_i = i;
            }
            if y[i] > y[max_i] {
                max_i = i;
            }
        }

        // Emit in index order so the polyline never doubles back on itself.
        let (first, second) = if min_i <= max_i {
            (min_i, max_i)
        } else {
            (max_i, min_i)
        };
        out.push([x[first], y[first] as f64]);
        if second != first {
            out.push([x[second], y[second] as f64]);
        }
    }
}

/// Axis ranges covering a whole sweep, with a little vertical headroom.
///
/// Returns `None` for an empty or entirely non-finite sweep, so a refit waits
/// for real data rather than installing a degenerate window.
fn full_bounds(x: &[f64], y: &[f32]) -> Option<[f64; 4]> {
    let n = x.len().min(y.len());
    if n == 0 {
        return None;
    }

    let (x0, x1) = (x[0], x[n - 1]);
    if !x0.is_finite() || !x1.is_finite() || x1 <= x0 {
        return None;
    }

    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for v in &y[..n] {
        let v = *v as f64;
        if v.is_finite() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }

    let margin = ((hi - lo) * 0.05).max(1.0);
    Some([x0, lo - margin, x1, hi + margin])
}

/// Half-open index range covering `[x0, x1]`, widened by one point each side.
fn visible_index_range(x: &[f64], n: usize, x0: f64, x1: f64) -> (usize, usize) {
    if !x0.is_finite() || !x1.is_finite() || x1 <= x0 {
        return (0, n);
    }
    let lo = x[..n].partition_point(|v| *v < x0).saturating_sub(1);
    let hi = (x[..n].partition_point(|v| *v <= x1) + 1).min(n);
    if hi <= lo {
        (0, n)
    } else {
        (lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DecayFn;

    fn axis(n: usize) -> Vec<f64> {
        (0..n).map(|i| i as f64).collect()
    }

    #[test]
    fn small_series_is_passed_through() {
        let x = axis(100);
        let y: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let mut out = Vec::new();
        decimate(&x, &y, 0.0, 99.0, 800.0, &mut out);
        assert_eq!(out.len(), 100);
        assert_eq!(out[0], [0.0, 0.0]);
        assert_eq!(out[99], [99.0, 99.0]);
    }

    #[test]
    fn large_series_is_reduced_but_bounded() {
        let n = 200_000;
        let x = axis(n);
        let y: Vec<f32> = (0..n).map(|i| (i as f32 * 0.001).sin()).collect();
        let mut out = Vec::new();
        decimate(&x, &y, 0.0, (n - 1) as f64, 1000.0, &mut out);
        assert!(out.len() <= 2 * 1000 + 2, "{}", out.len());
        assert!(out.len() > 100, "{}", out.len());
    }

    #[test]
    fn decimation_keeps_a_single_bin_spike() {
        // The whole point of min/max bucketing: a one-bin carrier must not be
        // averaged away when the sweep has far more bins than pixels.
        let n = 100_000;
        let x = axis(n);
        let mut y = vec![-100.0f32; n];
        y[54_321] = -5.0;

        let mut out = Vec::new();
        decimate(&x, &y, 0.0, (n - 1) as f64, 800.0, &mut out);
        let peak = out.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        assert!((peak - -5.0).abs() < 1e-6, "spike lost, peak was {peak}");
        assert!(out.iter().any(|p| (p[0] - 54_321.0).abs() < 1.0));
    }

    #[test]
    fn decimation_keeps_a_single_bin_notch() {
        let n = 100_000;
        let x = axis(n);
        let mut y = vec![-50.0f32; n];
        y[1234] = -140.0;

        let mut out = Vec::new();
        decimate(&x, &y, 0.0, (n - 1) as f64, 800.0, &mut out);
        let trough = out.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        assert!(
            (trough - -140.0).abs() < 1e-6,
            "notch lost, trough was {trough}"
        );
    }

    #[test]
    fn output_is_ascending_in_x() {
        let n = 50_000;
        let x = axis(n);
        let y: Vec<f32> = (0..n).map(|i| ((i * 7919) % 1000) as f32).collect();
        let mut out = Vec::new();
        decimate(&x, &y, 0.0, (n - 1) as f64, 400.0, &mut out);
        for w in out.windows(2) {
            assert!(w[1][0] >= w[0][0], "{:?} then {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn zoomed_view_only_covers_the_visible_range() {
        let n = 10_000;
        let x = axis(n);
        let y = vec![0.0f32; n];
        let mut out = Vec::new();
        decimate(&x, &y, 4_000.0, 4_100.0, 800.0, &mut out);
        // One point of slack on each side so the line reaches the plot edges.
        assert!(out.first().unwrap()[0] <= 4_000.0);
        assert!(out.last().unwrap()[0] >= 4_100.0);
        assert!(out.len() < 120, "{}", out.len());
    }

    #[test]
    fn degenerate_inputs_are_safe() {
        let mut out = vec![[1.0, 1.0]];
        decimate(&[], &[], 0.0, 1.0, 100.0, &mut out);
        assert!(out.is_empty());

        // Mismatched lengths use the shorter of the two.
        let x = axis(10);
        let y = vec![0.0f32; 4];
        decimate(&x, &y, 0.0, 9.0, 100.0, &mut out);
        assert_eq!(out.len(), 4);

        // A collapsed or non-finite view falls back to everything.
        decimate(&x, &y, 5.0, 5.0, 100.0, &mut out);
        assert_eq!(out.len(), 4);
        decimate(&x, &y, f64::NAN, 1.0, 100.0, &mut out);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn nan_values_do_not_panic_or_drop_points() {
        let n = 5_000;
        let x = axis(n);
        let mut y = vec![-50.0f32; n];
        y[10] = f32::NAN;
        let mut out = Vec::new();
        decimate(&x, &y, 0.0, (n - 1) as f64, 100.0, &mut out);
        assert!(!out.is_empty());
    }

    #[test]
    fn full_bounds_spans_the_sweep_with_headroom() {
        let x = axis(100);
        let y: Vec<f32> = (0..100).map(|i| -100.0 + i as f32).collect();
        let [x0, y0, x1, y1] = full_bounds(&x, &y).expect("bounds");
        assert_eq!((x0, x1), (0.0, 99.0));
        // -100 .. -1, so the margin is 5% of the 99 dB span.
        assert!(y0 < -100.0 && y0 > -110.0, "{y0}");
        assert!(y1 > -1.0 && y1 < 10.0, "{y1}");
    }

    #[test]
    fn full_bounds_refuses_degenerate_sweeps() {
        assert!(full_bounds(&[], &[]).is_none());
        assert!(full_bounds(&[1.0], &[0.0]).is_none(), "zero-width span");
        assert!(full_bounds(&[0.0, f64::NAN], &[0.0, 0.0]).is_none());
        assert!(
            full_bounds(&[0.0, 1.0], &[f32::NAN, f32::NAN]).is_none(),
            "all-NaN powers"
        );
    }

    #[test]
    fn full_bounds_survives_a_flat_sweep() {
        // A constant trace still needs a non-empty y window.
        let [_, y0, _, y1] = full_bounds(&[0.0, 1.0], &[-50.0, -50.0]).expect("bounds");
        assert!(y1 > y0);
    }

    #[test]
    fn a_refit_request_waits_for_data() {
        let mut p = SpectrumPlot::new();
        p.clear();
        assert!(p.reset_view);
        // Nothing to fit to yet, so the request must still be pending.
        assert!(p.pending_bounds.is_none());
    }

    #[test]
    fn visible_range_brackets_the_request() {
        let x = axis(1000);
        let (lo, hi) = visible_index_range(&x, 1000, 100.0, 200.0);
        assert!(x[lo] <= 100.0);
        assert!(x[hi - 1] >= 200.0);
    }

    #[test]
    fn persistence_fan_tracks_its_length() {
        let mut p = SpectrumPlot::new();
        for i in 0..10 {
            p.push_persistence(&[i as f32], 3);
        }
        // Fan length 3 plus the live trace.
        assert_eq!(p.persistence.len(), 4);
        assert_eq!(p.persistence[0], vec![9.0]);
        assert_eq!(p.persistence[3], vec![6.0]);
    }

    #[test]
    fn clear_empties_everything() {
        let mut p = SpectrumPlot::new();
        p.push_persistence(&[1.0], 3);
        p.main.push([0.0, 0.0]);
        p.clear();
        assert!(p.main.is_empty());
        assert!(p.persistence.is_empty());
        assert!(p.last_view.is_none());
    }

    #[test]
    fn decay_fan_alpha_decreases_with_age() {
        let mut prev = f32::INFINITY;
        for slot in 0..5 {
            let a = DecayFn::Exponential.alpha(slot as f64 + 1.0, 6.0);
            assert!(a < prev, "slot {slot}: {a} !< {prev}");
            prev = a;
        }
    }
}
