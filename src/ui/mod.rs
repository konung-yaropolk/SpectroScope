//! Widgets and dialogs.

pub mod dialogs;
pub mod levels;
pub mod spectrum;
pub mod waterfall;

pub use dialogs::{DialogOutcome, Dialogs};
pub use levels::{LevelsOutcome, LevelsPanel};
pub use spectrum::SpectrumPlot;
pub use waterfall::{Waterfall, WaterfallResponse, WaterfallView};

/// Width for a combo box that must not stop the side panel from shrinking.
///
/// A fixed `ComboBox::width` is a floor: the panel stores the width its
/// contents reported, so one over-wide row pins the whole panel and a drag
/// inwards springs straight back. Asking for what is left over instead lets
/// the row give the width back.
pub fn combo_width(ui: &egui::Ui, max: f32) -> f32 {
    // The arrow and the button's own padding sit outside the requested width.
    //
    // Pair this with `ComboBox::truncate`: inside a horizontal layout the
    // default wrap mode is `Extend`, which sizes the button to the whole
    // selected text no matter what width was asked for.
    (ui.available_width() - 26.0).clamp(48.0, max)
}

/// Hold a frequency window inside the sweep it is a window onto.
///
/// Two rules, which are the same rule seen from either end: the window may not
/// be *wider* than the data, and it may not sit outside it. Together they mean
/// the image always fills the pane -- you cannot zoom out until the waterfall
/// is a strip floating in an empty window, and you cannot drag it off the edge
/// and be left looking at nothing. Zooming out therefore stops exactly where
/// the whole sweep fits, which is also where *Reset view* puts you.
///
/// A degenerate limit or window is left alone rather than enforced against:
/// that is a caller with no data yet, or a pane mid-resize, and there is
/// nothing meaningful to clamp to. NaN falls into the same branch, because a
/// comparison against it is false.
pub fn clamp_view_x(view: (f64, f64), limit: (f64, f64)) -> (f64, f64) {
    let (lo, hi) = limit;
    let limit_span = hi - lo;
    let span = view.1 - view.0;
    if !(limit_span > 0.0) || !limit_span.is_finite() || !(span > 0.0) || !span.is_finite() {
        return view;
    }

    // Narrow the window first, then slide it in. Doing it the other way round
    // would pin an over-wide window to the left edge and hide the right of the
    // sweep, instead of showing all of it.
    let span = span.min(limit_span);
    let start = view.0.clamp(lo, hi - span);
    (start, start + span)
}

/// Shared id for the linked x axis of the spectrum and waterfall views, so
/// panning or zooming either moves both -- the equivalent of the Qt version's
/// `setXLink()`.
pub const LINKED_X_AXIS: &str = "spectroscope.freq-axis";

#[cfg(test)]
mod tests {
    use super::clamp_view_x;

    const SWEEP: (f64, f64) = (87.0e6, 108.0e6);

    #[test]
    fn a_window_inside_the_sweep_is_left_alone() {
        let view = (90.0e6, 95.0e6);
        assert_eq!(clamp_view_x(view, SWEEP), view);
        // And the two extremes, which are inside by a hair.
        assert_eq!(clamp_view_x(SWEEP, SWEEP), SWEEP);
    }

    #[test]
    fn zooming_out_stops_where_the_whole_sweep_fits() {
        // Ten times too wide, centred: the answer is the sweep itself, not a
        // ten-times-too-wide window shoved against one edge.
        let got = clamp_view_x((-10.0e6, 200.0e6), SWEEP);
        assert_eq!(got, SWEEP);
        // And a hair too wide is still the sweep.
        assert_eq!(clamp_view_x((86.9e6, 108.1e6), SWEEP), SWEEP);
    }

    #[test]
    fn panning_off_an_edge_slides_back_without_changing_the_zoom() {
        let span = 2.0e6;
        for view in [(80.0e6, 80.0e6 + span), (120.0e6, 120.0e6 + span)] {
            let (lo, hi) = clamp_view_x(view, SWEEP);
            assert!(
                (hi - lo - span).abs() < 1.0,
                "the zoom changed: {lo}..{hi} is not {span} wide"
            );
            assert!(lo >= SWEEP.0 - 1.0 && hi <= SWEEP.1 + 1.0, "{lo}..{hi}");
        }
        // Off the left edge lands on the left edge, not somewhere in between.
        assert_eq!(clamp_view_x((0.0, span), SWEEP).0, SWEEP.0);
        assert_eq!(clamp_view_x((200.0e6, 200.0e6 + span), SWEEP).1, SWEEP.1);
    }

    /// Nothing to clamp against, or nothing to clamp: pass it through rather
    /// than inventing a range. A plot mid-resize and a run with no sweep yet
    /// both arrive here.
    #[test]
    fn a_degenerate_range_is_not_enforced() {
        let view = (90.0e6, 95.0e6);
        for limit in [
            (0.0, 0.0),
            (108.0e6, 87.0e6),
            (f64::NAN, 1.0),
            (0.0, f64::INFINITY),
        ] {
            assert_eq!(clamp_view_x(view, limit), view, "limit {limit:?}");
        }
        // Compared component-wise rather than with `assert_eq!` on the
        // tuple, because one of these carries a NaN and NaN is not equal to
        // itself -- the point is that the value came back untouched, which
        // tuple equality cannot express.
        for bad in [(1.0, 1.0), (2.0, 1.0), (f64::NAN, 1.0)] {
            let got = clamp_view_x(bad, SWEEP);
            let same = |a: f64, b: f64| a == b || (a.is_nan() && b.is_nan());
            assert!(
                same(got.0, bad.0) && same(got.1, bad.1),
                "view {bad:?} came back as {got:?}"
            );
        }
    }

    /// Whatever goes in, what comes out is a usable window inside the sweep.
    #[test]
    fn the_result_is_always_inside_and_never_inverted() {
        let xs = [-1.0e9, 0.0, 86.0e6, 87.0e6, 97.5e6, 108.0e6, 109.0e6, 1.0e9];
        for a in xs {
            for b in xs {
                if !(b > a) {
                    continue;
                }
                let (lo, hi) = clamp_view_x((a, b), SWEEP);
                assert!(hi > lo, "{a}..{b} -> inverted {lo}..{hi}");
                assert!(lo >= SWEEP.0, "{a}..{b} -> {lo} left of the sweep");
                assert!(hi <= SWEEP.1, "{a}..{b} -> {hi} right of the sweep");
                // Never widened, only ever narrowed to fit.
                assert!(hi - lo <= (b - a) + 1.0, "{a}..{b} -> {lo}..{hi} is wider");
            }
        }
    }
}
