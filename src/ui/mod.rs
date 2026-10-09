//! Widgets and dialogs.

pub mod dialogs;
pub mod levels;
pub mod spectrum;
pub mod waterfall;

pub use dialogs::{DialogOutcome, Dialogs};
pub use levels::{LevelsOutcome, LevelsPanel};
pub use spectrum::SpectrumPlot;
pub use waterfall::{Waterfall, WaterfallView};

/// Shared id for the linked x axis of the spectrum and waterfall views, so
/// panning or zooming either moves both -- the equivalent of the Qt version's
/// `setXLink()`.
pub const LINKED_X_AXIS: &str = "spectroscope.freq-axis";
