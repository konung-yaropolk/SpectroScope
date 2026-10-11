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

/// Shared id for the linked x axis of the spectrum and waterfall views, so
/// panning or zooming either moves both -- the equivalent of the Qt version's
/// `setXLink()`.
pub const LINKED_X_AXIS: &str = "spectroscope.freq-axis";
