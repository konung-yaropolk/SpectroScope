//! The *Levels* panel: colour map choice and the dB window the waterfall maps
//! through it, over a histogram of the stored sweeps.
//!
//! Replaces pyqtgraph's `HistogramLUTItem`, which QSpectrumAnalyzer dropped into
//! its *Levels* dock and linked to the waterfall image.
//!
//! The histogram is cached and rebuilt only when new sweeps arrive, and it is
//! built from a strided sample rather than the whole ring: a 100k-bin, 100-row
//! history is ten million values, which is far more than a 256-bucket histogram
//! needs and far too much to touch every frame.

use crate::colormap;
use crate::config::LevelsConfig;
use crate::data::DataStorage;

/// Buckets in the histogram. Matches the colour map's resolution, so each
/// bucket is one LUT entry wide when the window covers the whole data range.
const BUCKETS: usize = 256;

/// Upper bound on values read per rebuild. Chosen so a rebuild stays well under
/// a millisecond even on the largest histories.
const MAX_SAMPLES: usize = 200_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelsOutcome {
    /// The LUT must be re-uploaded to the GPU.
    pub colormap_changed: bool,
}

#[derive(Default)]
pub struct LevelsPanel {
    counts: Vec<u32>,
    /// dB range the cached histogram spans.
    range: Option<(f32, f32)>,
    /// `HistoryBuffer::counter()` the cache was built from, so a rebuild only
    /// happens when sweeps have actually been added.
    built_at: u64,
}

impl LevelsPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate(&mut self) {
        self.built_at = 0;
        self.counts.clear();
        self.range = None;
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        levels: &mut LevelsConfig,
        storage: &DataStorage,
    ) -> LevelsOutcome {
        let mut outcome = LevelsOutcome::default();

        ui.horizontal(|ui| {
            ui.label("Color map:");
            let before = (levels.colormap, levels.reverse);
            egui::ComboBox::from_id_salt("levels.colormap")
                .selected_text(levels.colormap_name())
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (i, name) in colormap::NAMES.iter().enumerate() {
                        ui.selectable_value(&mut levels.colormap, i, *name);
                    }
                });
            ui.checkbox(&mut levels.reverse, "Reverse")
                .on_hover_text("Flip the colour map");
            if (levels.colormap, levels.reverse) != before {
                outcome.colormap_changed = true;
            }
        });

        ui.add_space(4.0);
        ui.checkbox(&mut levels.auto, "Auto levels")
            .on_hover_text("Track the range of the stored sweeps");

        // Cached, not recomputed: this used to call `history_range()` every
        // frame, which walks the whole ring. At the default history that is
        // tens of millions of values per frame and the UI crawls.
        self.rebuild_if_stale(storage);
        let data_range = self.range;
        if levels.auto {
            if let Some((lo, hi)) = data_range {
                // A little headroom keeps the strongest signals off the very
                // top of the colour map, where detail is hardest to read.
                let pad = ((hi - lo) * 0.02).max(0.5);
                levels.low = lo - pad;
                levels.high = hi + pad;
            }
        }

        ui.add_enabled_ui(!levels.auto, |ui| {
            egui::Grid::new("levels.grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Max:");
                    ui.add(
                        egui::DragValue::new(&mut levels.high)
                            .suffix(" dB")
                            .speed(0.5)
                            .range(-300.0..=300.0),
                    );
                    ui.end_row();

                    ui.label("Min:");
                    ui.add(
                        egui::DragValue::new(&mut levels.low)
                            .suffix(" dB")
                            .speed(0.5)
                            .range(-300.0..=300.0),
                    );
                    ui.end_row();
                });
        });

        // A collapsed window would make the waterfall a single flat colour.
        if levels.high <= levels.low {
            levels.high = levels.low + 1.0;
        }

        // The strip and the histogram share one dB axis, so a level handle sits
        // at the same x in both and the window's effect on the colours is read
        // straight off the strip.
        let axis = self.axis_range(levels);
        self.histogram(ui, levels, axis);
        self.color_bar(ui, levels, axis);

        if let Some((lo, hi)) = data_range {
            ui.label(
                egui::RichText::new(format!("Data: {lo:.1} .. {hi:.1} dB"))
                    .small()
                    .weak(),
            );
        }

        outcome
    }

    /// The dB range the strip and the histogram are drawn across.
    ///
    /// The union of the data range and the level window, so a window dragged
    /// outside the data stays visible instead of sliding off the edge.
    fn axis_range(&self, levels: &LevelsConfig) -> (f32, f32) {
        let (mut lo, mut hi) = self.range.unwrap_or((levels.low, levels.high));
        lo = lo.min(levels.low);
        hi = hi.max(levels.high);
        if !(hi > lo) {
            hi = lo + 1.0;
        }
        (lo, hi)
    }

    /// Colour strip along the shared dB axis.
    ///
    /// This shows the colour each dB value is *currently* mapped to, not the
    /// raw colour map: it is flat below `low`, flat above `high`, and ramps
    /// between, which makes the level window legible at a glance.
    fn color_bar(&self, ui: &mut egui::Ui, levels: &LevelsConfig, axis: (f32, f32)) {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }

        let lut = levels.lut();
        let painter = ui.painter();
        let (lo, hi) = axis;
        let span = (hi - lo).max(f32::EPSILON);
        let inv_window = 1.0 / (levels.high - levels.low).max(f32::EPSILON);

        // One quad per LUT entry would be 256 draws; stepping by pixel is
        // cheaper and visually identical.
        let steps = rect.width().round().max(1.0) as usize;
        for i in 0..steps {
            let db = lo + span * (i as f32 + 0.5) / steps as f32;
            let color = lut_color(lut, (db - levels.low) * inv_window, levels.reverse);
            let x0 = rect.left() + rect.width() * i as f32 / steps as f32;
            let x1 = rect.left() + rect.width() * (i + 1) as f32 / steps as f32;
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())),
                0.0,
                color,
            );
        }
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            egui::StrokeKind::Inside,
        );
    }

    fn rebuild_if_stale(&mut self, storage: &DataStorage) {
        let history = storage.history();
        if history.is_empty() {
            self.invalidate();
            return;
        }
        if history.counter() == self.built_at {
            return;
        }

        // Both passes sample the same bounded subset. The range feeds the
        // auto-levels, which is a display convenience rather than a
        // measurement, so trading exactness for a cost that does not grow with
        // the history depth is the right way round.
        let total = history.len() * history.bins().max(1);
        let stride = (total / MAX_SAMPLES).max(1);

        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for row in history.iter_rows() {
            for &v in row.iter().step_by(stride) {
                if v.is_finite() {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        if !lo.is_finite() || !hi.is_finite() {
            self.invalidate();
            return;
        }
        let span = (hi - lo).max(1e-6);

        self.counts.clear();
        self.counts.resize(BUCKETS, 0);
        for row in history.iter_rows() {
            for &v in row.iter().step_by(stride) {
                if v.is_finite() {
                    let t = ((v - lo) / span).clamp(0.0, 1.0);
                    let b = ((t * (BUCKETS - 1) as f32) as usize).min(BUCKETS - 1);
                    self.counts[b] += 1;
                }
            }
        }

        self.range = Some((lo, hi));
        self.built_at = history.counter();
    }

    /// Counts as upright bars along the shared dB axis.
    ///
    /// dB runs left to right, matching the colour strip underneath, and the
    /// level handles are vertical lines at the same x in both.
    fn histogram(&self, ui: &mut egui::Ui, levels: &LevelsConfig, axis: (f32, f32)) {
        let height = 110.0;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );
        if !ui.is_rect_visible(rect) {
            return;
        }

        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

        let (Some((lo, hi)), false) = (self.range, self.counts.is_empty()) else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "no data",
                egui::FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
            return;
        };

        let (axis_lo, axis_hi) = axis;
        let axis_span = (axis_hi - axis_lo).max(f32::EPSILON);
        let x_of = |db: f32| rect.left() + rect.width() * ((db - axis_lo) / axis_span);

        let peak = self.counts.iter().copied().max().unwrap_or(1).max(1) as f32;
        let span = (hi - lo).max(1e-6);
        let lut = levels.lut();
        let inv_window = 1.0 / (levels.high - levels.low).max(f32::EPSILON);

        // Counts are square-rooted: a spectrum histogram is dominated by the
        // noise floor, and on a linear scale every signal bucket would be a
        // single invisible pixel next to it.
        let bar_w = (rect.width() / BUCKETS as f32).max(1.0);
        for (b, &count) in self.counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let t = b as f32 / (BUCKETS - 1) as f32;
            let db = lo + t * span;
            let x = x_of(db);
            let h = rect.height() * (count as f32 / peak).sqrt();

            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x, rect.bottom() - h),
                    egui::pos2(x + bar_w, rect.bottom()),
                ),
                0.0,
                lut_color(lut, (db - levels.low) * inv_window, levels.reverse),
            );
        }

        // The window edges. Drawn in axis space, which already covers them, so
        // a window set outside the data is still visible.
        let edge = ui.visuals().strong_text_color();
        let handle = |v: f32, label: &str, align: egui::Align2| {
            let x = x_of(v);
            if x < rect.left() - 1.0 || x > rect.right() + 1.0 {
                return;
            }
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, edge),
            );
            painter.text(
                egui::pos2(x, rect.top() + 1.0),
                align,
                label,
                egui::FontId::proportional(10.0),
                edge,
            );
        };
        handle(levels.low, "min", egui::Align2::LEFT_TOP);
        handle(levels.high, "max", egui::Align2::RIGHT_TOP);
    }
}

/// Sample a baked LUT at `t` in `0.0..=1.0`, honouring `reverse`.
///
/// Shared with the GPU path's shader logic so the preview, the histogram and
/// the waterfall always agree.
pub fn lut_color(lut: &[[u8; 3]; 256], t: f32, reverse: bool) -> egui::Color32 {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let t = if reverse { 1.0 - t } else { t };
    let i = ((t * 255.0).round() as usize).min(255);
    let [r, g, b] = lut[i];
    egui::Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Frame;
    use std::sync::Arc;

    fn frame(y: &[f32]) -> Frame {
        Frame {
            timestamp: 0.0,
            x: Arc::new((0..y.len()).map(|i| i as f64).collect()),
            y: y.to_vec(),
        }
    }

    #[test]
    fn lut_endpoints_and_clamping() {
        let lut = &colormap::LUTS[0];
        let [r0, g0, b0] = lut[0];
        let [r1, g1, b1] = lut[255];

        assert_eq!(
            lut_color(lut, 0.0, false),
            egui::Color32::from_rgb(r0, g0, b0)
        );
        assert_eq!(
            lut_color(lut, 1.0, false),
            egui::Color32::from_rgb(r1, g1, b1)
        );
        // Out-of-range input clamps rather than wrapping or panicking.
        assert_eq!(
            lut_color(lut, -5.0, false),
            egui::Color32::from_rgb(r0, g0, b0)
        );
        assert_eq!(
            lut_color(lut, 9.0, false),
            egui::Color32::from_rgb(r1, g1, b1)
        );
    }

    #[test]
    fn reverse_swaps_the_ends() {
        let lut = &colormap::LUTS[3];
        assert_eq!(lut_color(lut, 0.0, true), lut_color(lut, 1.0, false));
        assert_eq!(lut_color(lut, 1.0, true), lut_color(lut, 0.0, false));
    }

    #[test]
    fn nan_maps_to_the_bottom_rather_than_panicking() {
        let lut = &colormap::LUTS[0];
        assert_eq!(lut_color(lut, f32::NAN, false), lut_color(lut, 0.0, false));
    }

    #[test]
    fn histogram_is_built_once_per_new_sweep() {
        let mut p = LevelsPanel::new();
        let mut st = DataStorage::new(8);
        st.update(frame(&[-50.0, -40.0, -30.0])).ok();

        p.rebuild_if_stale(&st);
        assert_eq!(p.built_at, 1);
        assert_eq!(p.counts.iter().sum::<u32>(), 3);

        // No new sweep: the cache must not be rebuilt.
        p.counts[0] = 12345;
        p.rebuild_if_stale(&st);
        assert_eq!(p.counts[0], 12345);

        st.update(frame(&[-20.0, -20.0, -20.0])).ok();
        p.rebuild_if_stale(&st);
        assert_eq!(p.built_at, 2);
        assert_eq!(p.counts.iter().sum::<u32>(), 6);
    }

    #[test]
    fn histogram_spans_the_data_range() {
        let mut p = LevelsPanel::new();
        let mut st = DataStorage::new(4);
        st.update(frame(&[-90.0, -10.0])).ok();
        p.rebuild_if_stale(&st);

        assert_eq!(p.range, Some((-90.0, -10.0)));
        // The extremes land in the first and last buckets.
        assert_eq!(p.counts[0], 1);
        assert_eq!(p.counts[BUCKETS - 1], 1);
    }

    #[test]
    fn empty_history_clears_the_cache() {
        let mut p = LevelsPanel::new();
        let mut st = DataStorage::new(4);
        st.update(frame(&[-50.0])).ok();
        p.rebuild_if_stale(&st);
        assert!(!p.counts.is_empty());

        st.reset();
        p.rebuild_if_stale(&st);
        assert!(p.counts.is_empty());
        assert!(p.range.is_none());
    }

    #[test]
    fn non_finite_values_are_skipped() {
        let mut p = LevelsPanel::new();
        let mut st = DataStorage::new(4);
        st.update(frame(&[-50.0, f32::NAN, f32::INFINITY, -40.0]))
            .ok();
        p.rebuild_if_stale(&st);
        // Only the two finite values are counted, and the range ignores the rest.
        assert_eq!(p.counts.iter().sum::<u32>(), 2);
        assert_eq!(p.range, Some((-50.0, -40.0)));
    }

    #[test]
    fn flat_data_does_not_divide_by_zero() {
        let mut p = LevelsPanel::new();
        let mut st = DataStorage::new(4);
        st.update(frame(&[-50.0, -50.0, -50.0])).ok();
        p.rebuild_if_stale(&st);
        assert_eq!(p.counts.iter().sum::<u32>(), 3);
    }
}
