//! Persisted application settings.
//!
//! One serialisable struct replaces QSpectrumAnalyzer's flat `QSettings` keys.
//! It is stored through `eframe`'s own storage, which means a RON file under the
//! platform config directory natively and `localStorage` on the web.

use serde::{Deserialize, Serialize};

use crate::colormap;
use crate::dsp::smooth::SmoothWindow;
use crate::sources::{self, Limits, SweepConfig};

/// The storage key the whole config lives under.
pub const STORAGE_KEY: &str = "spectroscope.config";

/// How persistence curves fade out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecayFn {
    Exponential,
    Linear,
}

impl DecayFn {
    pub const ALL: [Self; 2] = [Self::Exponential, Self::Linear];

    pub fn label(self) -> &'static str {
        match self {
            Self::Exponential => "exponential",
            Self::Linear => "linear",
        }
    }

    /// Alpha multiplier for persistence curve `x` of `length`.
    ///
    /// Ported verbatim from `SpectrumPlotWidget.decay_linear` /
    /// `decay_exponential` (the exponential constant is 1/3).
    pub fn alpha(self, x: f64, length: f64) -> f32 {
        let a = match self {
            Self::Linear => (-x / length) + 1.0,
            Self::Exponential => std::f64::consts::E.powf(-x / (length * (1.0 / 3.0))),
        };
        a.clamp(0.0, 1.0) as f32
    }
}

/// An RGBA colour, stored the way the Qt version stored it so that curve
/// colours survive a round trip through the config file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba8(pub [u8; 4]);

impl Rgba8 {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    pub fn to_color32(self) -> egui::Color32 {
        let [r, g, b, a] = self.0;
        egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    }

    pub fn from_color32(c: egui::Color32) -> Self {
        Self(c.to_srgba_unmultiplied())
    }

    /// The same colour with its alpha scaled by `factor`.
    pub fn with_alpha_scaled(self, factor: f32) -> egui::Color32 {
        let [r, g, b, a] = self.0;
        let a = (a as f32 * factor.clamp(0.0, 1.0)).round() as u8;
        egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}

/// Curve colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colors {
    pub main: Rgba8,
    pub peak_hold_max: Rgba8,
    pub peak_hold_min: Rgba8,
    pub average: Rgba8,
    pub persistence: Rgba8,
    pub baseline: Rgba8,
}

impl Default for Colors {
    fn default() -> Self {
        // Same defaults as QSpectrumAnalyzer: y / r / b / c / g / m.
        Self {
            main: Rgba8::new(255, 255, 0, 255),
            peak_hold_max: Rgba8::new(255, 0, 0, 255),
            peak_hold_min: Rgba8::new(0, 0, 255, 255),
            average: Rgba8::new(0, 255, 255, 255),
            persistence: Rgba8::new(0, 255, 0, 255),
            baseline: Rgba8::new(255, 0, 255, 255),
        }
    }
}

/// Which traces are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traces {
    pub main_curve: bool,
    pub peak_hold_max: bool,
    pub peak_hold_min: bool,
    pub average: bool,
    pub smooth: bool,
    pub persistence: bool,
    pub baseline: bool,
    pub subtract_baseline: bool,
}

impl Default for Traces {
    fn default() -> Self {
        Self {
            main_curve: true,
            peak_hold_max: false,
            peak_hold_min: false,
            average: false,
            smooth: false,
            persistence: false,
            baseline: false,
            subtract_baseline: false,
        }
    }
}

/// Waterfall colour mapping and level window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelsConfig {
    /// Index into [`colormap::NAMES`].
    pub colormap: usize,
    /// dB value mapped to the bottom of the colour map.
    pub low: f32,
    /// dB value mapped to the top of the colour map.
    pub high: f32,
    /// Track the data range instead of using `low`/`high`.
    pub auto: bool,
    /// Draw the colour map upside down.
    pub reverse: bool,
}

impl Default for LevelsConfig {
    fn default() -> Self {
        Self {
            // "Magma" is the closest match to pyqtgraph's "flame" preset, which
            // was QSpectrumAnalyzer's default waterfall gradient.
            colormap: 0,
            low: -70.0,
            high: 0.0,
            auto: true,
            reverse: false,
        }
    }
}

impl LevelsConfig {
    pub fn colormap_name(&self) -> &'static str {
        colormap::NAMES
            .get(self.colormap)
            .copied()
            .unwrap_or(colormap::NAMES[0])
    }

    pub fn lut(&self) -> &'static [[u8; 3]; 256] {
        colormap::LUTS
            .get(self.colormap)
            .unwrap_or(&colormap::LUTS[0])
    }
}

/// The whole persisted state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // --- backend ----------------------------------------------------------
    /// [`sources::SourceInfo::id`] of the selected backend.
    pub backend: String,
    pub executable: String,
    pub params: String,
    pub device: String,
    /// Hz.
    pub sample_rate: f64,
    /// Hz.
    pub bandwidth: f64,
    /// Hz.
    pub lnb_lo: f64,
    /// Sweeps kept for the waterfall. Costs `bins * size * 4` bytes.
    pub waterfall_history_size: usize,

    // --- sweep ------------------------------------------------------------
    /// MHz.
    pub start_freq: f64,
    /// MHz.
    pub stop_freq: f64,
    /// kHz.
    pub bin_size: f64,
    /// Seconds.
    pub interval: f64,
    /// dB, `-1` meaning auto.
    pub gain: f64,
    pub ppm: i32,
    /// Percent.
    pub crop: i32,

    // --- display ----------------------------------------------------------
    pub traces: Traces,
    pub colors: Colors,
    pub levels: LevelsConfig,

    pub smooth_length: usize,
    pub smooth_window: SmoothWindow,

    pub persistence_length: usize,
    pub persistence_decay: DecayFn,

    pub baseline_file: String,

    // --- layout -----------------------------------------------------------
    /// Share of the plot area given to the spectrum (the rest is waterfall).
    pub plot_split: f32,
    pub show_waterfall: bool,
    pub show_levels_panel: bool,
    pub show_controls_panel: bool,
    pub dark_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: sources::default_source().info().id.to_owned(),
            executable: sources::default_source()
                .info()
                .default_executable
                .to_owned(),
            params: sources::default_source()
                .info()
                .additional_params
                .to_owned(),
            device: String::new(),
            sample_rate: 2_560_000.0,
            bandwidth: 0.0,
            lnb_lo: 0.0,
            waterfall_history_size: 4096,

            start_freq: 87.0,
            stop_freq: 108.0,
            bin_size: 10.0,
            interval: 1.0,
            gain: -1.0,
            ppm: 0,
            crop: 0,

            traces: Traces::default(),
            colors: Colors::default(),
            levels: LevelsConfig::default(),

            smooth_length: 11,
            smooth_window: SmoothWindow::Hanning,

            persistence_length: 5,
            persistence_decay: DecayFn::Exponential,

            baseline_file: String::new(),

            plot_split: 0.5,
            show_waterfall: true,
            show_levels_panel: true,
            show_controls_panel: true,
            dark_mode: true,
        }
    }
}

impl Config {
    /// The selected backend, falling back to the default if the stored id is
    /// unknown (e.g. a config written by a build that had more backends).
    pub fn source(&self) -> &'static dyn sources::SpectrumSource {
        sources::find(&self.backend).unwrap_or_else(sources::default_source)
    }

    /// Build the [`SweepConfig`] the GUI's current fields describe.
    pub fn sweep_config(&self, single_shot: bool) -> SweepConfig {
        SweepConfig {
            start_freq_mhz: self.start_freq,
            stop_freq_mhz: self.stop_freq,
            bin_size_khz: self.bin_size,
            interval_s: self.interval,
            gain_db: self.gain,
            ppm: self.ppm,
            crop: self.crop as f64 / 100.0,
            single_shot,
            device: self.device.clone(),
            sample_rate: self.sample_rate,
            bandwidth: self.bandwidth,
            lnb_lo_hz: self.lnb_lo,
            executable: self.executable.clone(),
            extra_params: self.params.clone(),
        }
    }

    /// Adopt a backend: reset every tunable to that backend's defaults.
    ///
    /// This reproduces `setup_power_thread()`'s behaviour of re-seeding the spin
    /// boxes whenever the backend changes.
    pub fn apply_backend_defaults(&mut self, source: &'static dyn sources::SpectrumSource) {
        let info = source.info();
        let l = &info.limits;

        self.backend = info.id.to_owned();
        self.executable = info.default_executable.to_owned();
        self.params = info.additional_params.to_owned();
        self.device = String::new();

        self.gain = l.gain.default;
        self.start_freq = l.start_freq.default;
        self.stop_freq = l.stop_freq.default;
        self.bin_size = l.bin_size.default;
        self.interval = l.interval.default;
        self.ppm = l.ppm.default;
        self.crop = l.crop.default;
        self.sample_rate = l.sample_rate.default;
        self.bandwidth = l.bandwidth.default;
    }

    /// Clamp every field into the selected backend's limits, shifting the
    /// frequency limits by the LNB LO exactly as the Qt version did.
    pub fn clamp_to(&mut self, limits: &Limits) {
        let lnb_mhz = self.lnb_lo / 1e6;

        self.gain = limits.gain.clamp(self.gain);
        self.bin_size = limits.bin_size.clamp(self.bin_size);
        self.interval = limits.interval.clamp(self.interval);
        self.ppm = limits.ppm.clamp(self.ppm);
        self.crop = limits.crop.clamp(self.crop);
        self.sample_rate = limits.sample_rate.clamp(self.sample_rate);
        self.bandwidth = limits.bandwidth.clamp(self.bandwidth);

        let (start_min, start_max) = self.start_freq_bounds(limits, lnb_mhz);
        if self.start_freq < start_min || self.start_freq > start_max {
            self.start_freq = start_min;
        }

        let (stop_min, stop_max) = self.stop_freq_bounds(limits, lnb_mhz);
        if self.stop_freq < stop_min || self.stop_freq > stop_max {
            self.stop_freq = stop_max;
        }

        // The waterfall scrolls, so the history is sized for how far back the
        // user may want to look rather than for the window height. The ceiling
        // is a GPU one: the ring becomes a texture that many texels tall, and
        // 16384 is the largest dimension common hardware allows.
        // `Waterfall::reset` clamps again to what this device actually reports.
        self.waterfall_history_size = self.waterfall_history_size.clamp(1, 16_384);
        self.smooth_length = self.smooth_length.clamp(1, 1001);
        self.persistence_length = self.persistence_length.clamp(1, 100);
        self.plot_split = self.plot_split.clamp(0.1, 0.9);
        if self.levels.high <= self.levels.low {
            self.levels.high = self.levels.low + 1.0;
        }
        if self.levels.colormap >= colormap::NAMES.len() {
            self.levels.colormap = 0;
        }
    }

    pub fn start_freq_bounds(&self, limits: &Limits, lnb_mhz: f64) -> (f64, f64) {
        let min = limits.start_freq.min + lnb_mhz;
        (
            (if min > 0.0 { min } else { 0.0 }),
            limits.start_freq.max + lnb_mhz,
        )
    }

    pub fn stop_freq_bounds(&self, limits: &Limits, lnb_mhz: f64) -> (f64, f64) {
        let min = limits.stop_freq.min + lnb_mhz;
        (
            (if min > 0.0 { min } else { 0.0 }),
            limits.stop_freq.max + lnb_mhz,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_ron() {
        let cfg = Config::default();
        let text = ron::ser::to_string(&cfg).expect("serialize");
        let back: Config = ron::from_str(&text).expect("deserialize");
        assert_eq!(cfg, back);
    }

    #[test]
    fn unknown_backend_falls_back() {
        let mut cfg = Config::default();
        cfg.backend = "no-such-backend".to_owned();
        assert_eq!(cfg.source().info().id, sources::default_source().info().id);
    }

    #[test]
    fn crop_percent_becomes_fraction() {
        let mut cfg = Config::default();
        cfg.crop = 25;
        assert!((cfg.sweep_config(false).crop - 0.25).abs() < 1e-12);
    }

    #[test]
    fn lnb_shifts_frequency_bounds() {
        let mut cfg = Config::default();
        cfg.lnb_lo = 9_750e6;
        cfg.start_freq = 10_000.0;
        cfg.stop_freq = 10_100.0;
        let limits = Limits::DEFAULT;
        cfg.clamp_to(&limits);
        // 2200 MHz tuner max + 9750 MHz LO = 11950 MHz, so both stay put.
        assert_eq!(cfg.start_freq, 10_000.0);
        assert_eq!(cfg.stop_freq, 10_100.0);
    }

    #[test]
    fn decay_is_monotonic_and_bounded() {
        for f in DecayFn::ALL {
            let a = f.alpha(1.0, 6.0);
            let b = f.alpha(5.0, 6.0);
            assert!((0.0..=1.0).contains(&a));
            assert!((0.0..=1.0).contains(&b));
            assert!(a > b, "{f:?} should fade with distance");
        }
    }
}
