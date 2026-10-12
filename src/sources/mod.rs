//! The SDR input abstraction: everything the GUI knows about an acquisition
//! backend goes through [`SpectrumSource`].
//!
//! A *source* turns a [`SweepConfig`] into a stream of [`Frame`]s (one complete
//! sweep each). It may do that by driving an external helper process
//! (`soapy_power`, `rtl_power`, `hackrf_sweep`, ...), by talking to a device
//! over the network (`rtl_tcp`), or by synthesising data (`demo`). The GUI never
//! learns which.
//!
//! # Adding a new device
//!
//! Three steps, no changes anywhere else in the application:
//!
//! 1. Write a type implementing [`SpectrumSource`]. Return a `&'static`
//!    [`SourceInfo`] describing the device's name and its parameter limits --
//!    the GUI clamps every spin box to those limits and resets the fields to
//!    the declared defaults when the user selects the backend.
//! 2. Spawn your acquisition on a thread (or an async task on the web), and push
//!    [`SourceEvent`]s into the [`EventSink`] you were handed. Emit
//!    [`SourceEvent::Started`] once, a [`SourceEvent::Frame`] per completed
//!    sweep, and [`SourceEvent::Stopped`] when the stream ends -- the GUI's
//!    button states and sweep timer are driven by exactly those three.
//! 3. Return a [`SourceSession`] whose `stop()` tears the acquisition down and
//!    blocks until it is gone, then add the type to [`registry`].
//!
//! ```no_run
//! use spectroscope::sources::*;
//!
//! struct Silence;
//!
//! static SILENCE_INFO: SourceInfo = SourceInfo {
//!     id: "silence",
//!     label: "silence (test source)",
//!     kind: SourceKind::Synthetic,
//!     default_executable: "",
//!     additional_params: "",
//!     has_device_help: false,
//!     limits: Limits::DEFAULT,
//! };
//!
//! struct Session;
//! impl SourceSession for Session {
//!     fn stop(&mut self) {}
//! }
//!
//! impl SpectrumSource for Silence {
//!     fn info(&self) -> &'static SourceInfo {
//!         &SILENCE_INFO
//!     }
//!
//!     fn start(
//!         &self,
//!         cfg: SweepConfig,
//!         sink: EventSink,
//!     ) -> Result<Box<dyn SourceSession>, SourceError> {
//!         let bins = cfg.bin_count().max(1);
//!         let axis = cfg.linear_axis(bins);
//!         sink.started(0);
//!         sink.frame(Frame { timestamp: 0.0, x: axis, y: vec![-100.0; bins] });
//!         sink.stopped();
//!         Ok(Box::new(Session))
//!     }
//! }
//! ```

use std::sync::Arc;

use crossbeam_channel::Sender;

// ---------------------------------------------------------------------------
// Backend modules
// ---------------------------------------------------------------------------

pub mod demo;
pub mod rtl_tcp;

#[cfg(not(target_arch = "wasm32"))]
pub mod hackrf_sweep;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;
#[cfg(not(target_arch = "wasm32"))]
pub mod rtl_power;
#[cfg(not(target_arch = "wasm32"))]
pub mod rtl_power_fftw;
#[cfg(not(target_arch = "wasm32"))]
pub mod rx_power;
#[cfg(not(target_arch = "wasm32"))]
pub mod soapy_power;

// ---------------------------------------------------------------------------
// Limits / metadata
// ---------------------------------------------------------------------------

/// An inclusive range plus the value the GUI should start from.
///
/// This is the direct equivalent of the `*_min` / `*_max` / plain-name triples
/// on QSpectrumAnalyzer's `BaseInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limit<T> {
    pub min: T,
    pub max: T,
    pub default: T,
}

impl<T: Copy> Limit<T> {
    pub const fn new(min: T, max: T, default: T) -> Self {
        Self { min, max, default }
    }
}

impl Limit<f64> {
    /// True when the control should be hidden/disabled: the backend accepts
    /// exactly one value, so there is nothing for the user to choose.
    pub fn is_fixed(&self) -> bool {
        self.min >= self.max
    }

    pub fn clamp(&self, v: f64) -> f64 {
        v.max(self.min).min(self.max)
    }
}

impl Limit<i32> {
    pub fn is_fixed(&self) -> bool {
        self.min >= self.max
    }

    pub fn clamp(&self, v: i32) -> i32 {
        v.max(self.min).min(self.max)
    }
}

/// Sample rates an RTL-SDR is actually used at, Hz.
///
/// librtlsdr accepts anything in 225001-300000 or 900001-3200000 Hz, but the
/// RTL2832 only divides its 28.8 MHz clock exactly at some of them, and above
/// roughly 2.4 MS/s most dongles start dropping samples over USB. These are the
/// values every other RTL tool offers, which is the point: a list the user
/// recognises beats a spin box that can be dragged to a rate the radio will
/// quietly refuse.
pub const RTL_SAMPLE_RATES: &[f64] = &[
    250_000.0,
    1_024_000.0,
    1_400_000.0,
    1_800_000.0,
    1_920_000.0,
    2_048_000.0,
    2_160_000.0,
    2_400_000.0,
    2_560_000.0,
    2_880_000.0,
    3_200_000.0,
];

/// The R820T/R828D tuner's gain table, dB.
///
/// librtlsdr stores these in tenths of a dB and a dongle accepts nothing else:
/// `rtlsdr_set_tuner_gain` snaps whatever it is given to the nearest entry, so
/// the spin box's 0.1 dB steps were 29 real settings wearing 497 costumes.
/// These are the 29, and an R828D reports exactly that many in its `rtl_tcp`
/// greeting.
pub const RTL_GAINS: &[f64] = &[
    0.0, 0.9, 1.4, 2.7, 3.7, 7.7, 8.7, 12.5, 14.4, 15.7, 16.6, 19.7, 20.7, 22.9, 25.4, 28.0, 29.7,
    32.8, 33.8, 36.4, 37.2, 38.6, 40.2, 42.1, 43.4, 43.9, 44.5, 48.0, 49.6,
];

/// Nothing discrete to offer: the control stays a free numeric entry.
pub const CONTINUOUS: &[f64] = &[];

/// Every tunable the GUI exposes, with the limits of one backend.
///
/// Units match the GUI widgets rather than the wire protocols: frequencies in
/// MHz, bin size in kHz, sample rate and bandwidth in Hz, interval in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Hz.
    pub sample_rate: Limit<f64>,
    /// Hz. `max == 0` means "backend does not support setting bandwidth".
    pub bandwidth: Limit<f64>,
    /// dB. A `min` of `-1` is the conventional "auto gain" sentinel.
    pub gain: Limit<f64>,
    /// MHz.
    pub start_freq: Limit<f64>,
    /// MHz.
    pub stop_freq: Limit<f64>,
    /// kHz.
    pub bin_size: Limit<f64>,
    /// Seconds.
    pub interval: Limit<f64>,
    /// Frequency correction, ppm.
    pub ppm: Limit<i32>,
    /// Hop crop, percent.
    pub crop: Limit<i32>,
    /// The only sample rates this device accepts, Hz, ascending; empty when it
    /// takes any value in [`Self::sample_rate`] and the GUI should show a spin
    /// box instead of a drop-down.
    pub sample_rate_choices: &'static [f64],
    /// The only gains this device accepts, dB, ascending; empty for a spin box.
    /// "Auto" is offered alongside these whenever [`Self::gain`]'s minimum is
    /// negative, and is not listed here.
    pub gain_choices: &'static [f64],
}

impl Limits {
    /// QSpectrumAnalyzer's `BaseInfo` defaults -- an RTL-SDR, essentially.
    pub const DEFAULT: Self = Self {
        sample_rate: Limit::new(0.0, 3_200_000.0, 2_560_000.0),
        bandwidth: Limit::new(0.0, 0.0, 0.0),
        // 37.2 rather than the round 37.0 the Qt build used, because 37.0 is
        // not a gain an R820T has: librtlsdr snaps it to this very value. The
        // old default was therefore a number the drop-down could not offer and
        // the radio never used.
        gain: Limit::new(-1.0, 49.6, 37.2),
        start_freq: Limit::new(0.0, 2200.0, 87.0),
        stop_freq: Limit::new(0.0, 2200.0, 108.0),
        bin_size: Limit::new(0.0, 2800.0, 10.0),
        interval: Limit::new(0.0, 3600.0, 1.0),
        ppm: Limit::new(-999, 999, 0),
        // A fifth off each hop's edges by default. The RTL's decimation filter
        // rolls off well before Nyquist, so the outermost bins of every hop read
        // low; cropping them is what stops a sweep showing one scallop per hop
        // instead of a flat noise floor.
        crop: Limit::new(0, 99, 20),
        sample_rate_choices: RTL_SAMPLE_RATES,
        gain_choices: RTL_GAINS,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How a source gets its data -- drives which controls the GUI shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// Drives an external executable; the *Executable* and *Additional
    /// parameters* settings apply.
    Process,
    /// Talks to a device over the network; the *Device* field holds `host:port`.
    Network,
    /// Generates data internally; needs no device at all.
    Synthetic,
}

/// Static description of one backend.
#[derive(Clone, Debug)]
pub struct SourceInfo {
    /// Stable identifier used in the config file. Never translate this.
    pub id: &'static str,
    /// What the backend combo box shows.
    pub label: &'static str,
    pub kind: SourceKind,
    /// Pre-filled into the *Executable* field when the backend is selected.
    pub default_executable: &'static str,
    /// Pre-filled into *Additional parameters*.
    pub additional_params: &'static str,
    /// Whether [`SpectrumSource::device_help`] returns anything useful.
    pub has_device_help: bool,
    pub limits: Limits,
}

// ---------------------------------------------------------------------------
// Sweep configuration
// ---------------------------------------------------------------------------

/// Everything a source needs to start one acquisition.
///
/// Mirrors the arguments of QSpectrumAnalyzer's `PowerThread.setup()`, plus the
/// executable and extra-parameter strings that the Python version read straight
/// out of `QSettings` from inside each backend.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepConfig {
    /// MHz, as displayed. Already includes the LNB LO offset, exactly like the
    /// GUI's spin box: subtract [`Self::lnb_lo_hz`] to get the tuner frequency.
    pub start_freq_mhz: f64,
    /// MHz, as displayed.
    pub stop_freq_mhz: f64,
    /// kHz.
    pub bin_size_khz: f64,
    /// Seconds.
    pub interval_s: f64,
    /// dB; negative means "auto".
    pub gain_db: f64,
    pub ppm: i32,
    /// Fraction in `0.0..=1.0` (the GUI shows percent).
    pub crop: f64,
    /// Acquire exactly one sweep, then stop.
    pub single_shot: bool,
    /// Backend-specific device selector (SoapySDR args, RTL index, `host:port`).
    pub device: String,
    /// Hz.
    pub sample_rate: f64,
    /// Hz; `0` means "leave at the device default".
    pub bandwidth: f64,
    /// Hz. Negative for upconverters, positive for downconverters.
    pub lnb_lo_hz: f64,
    /// Command line of the helper process, parsed with shell quoting rules.
    pub executable: String,
    /// Extra arguments appended verbatim, parsed with shell quoting rules.
    pub extra_params: String,
}

impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 108.0,
            bin_size_khz: 10.0,
            interval_s: 1.0,
            gain_db: -1.0,
            ppm: 0,
            crop: 0.0,
            single_shot: false,
            device: String::new(),
            sample_rate: 2_560_000.0,
            bandwidth: 0.0,
            lnb_lo_hz: 0.0,
            executable: String::new(),
            extra_params: String::new(),
        }
    }
}

impl SweepConfig {
    /// Tuner start frequency in Hz, i.e. with the LNB LO removed.
    pub fn tuner_start_hz(&self) -> f64 {
        self.start_freq_mhz * 1e6 - self.lnb_lo_hz
    }

    /// Tuner stop frequency in Hz, i.e. with the LNB LO removed.
    pub fn tuner_stop_hz(&self) -> f64 {
        self.stop_freq_mhz * 1e6 - self.lnb_lo_hz
    }

    /// Displayed span in Hz.
    pub fn span_hz(&self) -> f64 {
        (self.stop_freq_mhz - self.start_freq_mhz) * 1e6
    }

    /// Bin size in Hz.
    pub fn bin_size_hz(&self) -> f64 {
        self.bin_size_khz * 1e3
    }

    /// How many bins the span works out to, for sources that choose their own
    /// FFT size.
    pub fn bin_count(&self) -> usize {
        let step = self.bin_size_hz();
        if step <= 0.0 {
            return 0;
        }
        (self.span_hz() / step).round().max(0.0) as usize
    }

    /// A uniform frequency axis across the displayed span, in Hz.
    ///
    /// Matches NumPy's `linspace(start, stop, len)`, which is what every
    /// QSpectrumAnalyzer backend used to build its x axis.
    pub fn linear_axis(&self, len: usize) -> Arc<Vec<f64>> {
        Arc::new(linspace(
            self.start_freq_mhz * 1e6,
            self.stop_freq_mhz * 1e6,
            len,
        ))
    }
}

/// `numpy.linspace(start, stop, len)`: `len` points with both ends included.
pub fn linspace(start: f64, stop: f64, len: usize) -> Vec<f64> {
    match len {
        0 => Vec::new(),
        1 => vec![start],
        n => {
            let step = (stop - start) / (n - 1) as f64;
            (0..n).map(|i| start + step * i as f64).collect()
        }
    }
}

// ---------------------------------------------------------------------------
// Frames and events
// ---------------------------------------------------------------------------

/// One complete sweep.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Unix time in seconds at which the sweep finished.
    pub timestamp: f64,
    /// Bin centre frequencies in Hz, ascending.
    ///
    /// Shared because the axis is usually identical for every sweep of a run:
    /// a source builds it once and clones the `Arc` per frame.
    pub x: Arc<Vec<f64>>,
    /// Power per bin, dB. Same length as `x`.
    pub y: Vec<f32>,
}

/// What a running source reports back to the GUI.
#[derive(Debug)]
pub enum SourceEvent {
    /// Acquisition is live. `hops` is shown in the status bar; pass `0` when
    /// the backend does not hop.
    Started { hops: usize },
    /// One finished sweep.
    Frame(Frame),
    /// Informational message for the log pane (e.g. the backend command line).
    Log(String),
    /// Something went wrong. The GUI shows this and the run is expected to end.
    Error(String),
    /// Acquisition has finished and all resources are released.
    Stopped,
}

/// Where a source pushes its events.
///
/// Cloneable and `Send`, so reader threads can hold their own handle. Sends are
/// non-blocking and never panic: once the GUI is gone the events are dropped.
#[derive(Clone)]
pub struct EventSink {
    tx: Sender<SourceEvent>,
    ctx: Option<egui::Context>,
}

impl EventSink {
    pub fn new(tx: Sender<SourceEvent>, ctx: Option<egui::Context>) -> Self {
        Self { tx, ctx }
    }

    /// Send one event and wake the GUI.
    ///
    /// Returns `false` once the GUI has dropped the receiver, which is a source
    /// loop's cue to shut itself down.
    pub fn send(&self, event: SourceEvent) -> bool {
        if self.tx.send(event).is_err() {
            return false;
        }
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
        true
    }

    pub fn started(&self, hops: usize) -> bool {
        self.send(SourceEvent::Started { hops })
    }

    pub fn frame(&self, frame: Frame) -> bool {
        self.send(SourceEvent::Frame(frame))
    }

    pub fn log(&self, msg: impl Into<String>) -> bool {
        self.send(SourceEvent::Log(msg.into()))
    }

    pub fn error(&self, msg: impl Into<String>) -> bool {
        self.send(SourceEvent::Error(msg.into()))
    }

    pub fn stopped(&self) -> bool {
        self.send(SourceEvent::Stopped)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum SourceError {
    /// The helper executable is missing or not runnable.
    ExecutableNotFound { executable: String, detail: String },
    /// The configuration cannot be satisfied by this backend.
    InvalidConfig(String),
    /// Transport failure (socket, pipe, ...).
    Io(String),
    /// Backend is not compiled in on this platform.
    Unsupported(&'static str),
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecutableNotFound { executable, detail } => {
                write!(f, "cannot start '{executable}': {detail}")
            }
            Self::InvalidConfig(m) => write!(f, "invalid configuration: {m}"),
            Self::Io(m) => write!(f, "I/O error: {m}"),
            Self::Unsupported(m) => write!(f, "backend unavailable on this platform: {m}"),
        }
    }
}

impl std::error::Error for SourceError {}

// ---------------------------------------------------------------------------
// The traits
// ---------------------------------------------------------------------------

/// A live acquisition. Dropping it must also stop the acquisition.
pub trait SourceSession: Send {
    /// Tear the acquisition down and block until it is really gone.
    ///
    /// Must be safe to call more than once.
    fn stop(&mut self);
}

/// An acquisition backend.
///
/// Implementations are stateless descriptors: all per-run state lives in the
/// [`SourceSession`] returned by [`start`](SpectrumSource::start).
pub trait SpectrumSource: Send + Sync + 'static {
    /// Static name and limits. Must return the same value every call.
    fn info(&self) -> &'static SourceInfo;

    /// Begin acquiring. Returns as soon as the acquisition is running; the
    /// actual data arrives through `sink`.
    fn start(
        &self,
        cfg: SweepConfig,
        sink: EventSink,
    ) -> Result<Box<dyn SourceSession>, SourceError>;

    /// Text for the *Additional parameters* help window. The default runs
    /// `<executable> -h` and returns whatever it prints.
    fn params_help(&self, executable: &str) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        {
            process::capture_help(executable, &["-h"])
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = executable;
            "Running helper processes is not supported on the web.".to_owned()
        }
    }

    /// Text for the *Device* help window, when [`SourceInfo::has_device_help`].
    fn device_help(&self, executable: &str, device: &str) -> Option<String> {
        let _ = (executable, device);
        None
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Every backend compiled into this build, in display order.
///
/// Register a new device by adding it here.
pub fn registry() -> &'static [&'static dyn SpectrumSource] {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<Vec<&'static dyn SpectrumSource>> = OnceLock::new();

    REGISTRY.get_or_init(|| {
        // These shell out to a local binary, which a browser has not got.
        #[cfg(not(target_arch = "wasm32"))]
        let native: &[&'static dyn SpectrumSource] = &[
            &soapy_power::SoapyPower,
            &hackrf_sweep::HackrfSweep,
            &rtl_power_fftw::RtlPowerFftw,
            &rtl_power::RtlPower,
            &rx_power::RxPower,
        ];
        #[cfg(target_arch = "wasm32")]
        let native: &[&'static dyn SpectrumSource] = &[];

        // A socket and a function are all these two need, so every target
        // gets them, and they come after the real hardware in the list.
        let portable: [&'static dyn SpectrumSource; 2] = [&rtl_tcp::RtlTcp, &demo::Demo];
        native.iter().copied().chain(portable).collect()
    })
}

/// Look a backend up by its [`SourceInfo::id`].
pub fn find(id: &str) -> Option<&'static dyn SpectrumSource> {
    registry().iter().copied().find(|s| s.info().id == id)
}

/// The backend used when the config names an unknown one.
///
/// Android goes with the browser rather than with the desktop. It is a
/// `not(wasm32)` target, so it used to take `soapy_power` -- a backend that
/// cannot possibly work there, because Android does not let an application
/// execute a helper binary. A first run therefore opened on a backend whose
/// Start button could only ever fail. `demo` needs nothing, which is the whole
/// point of it being there.
pub fn default_source() -> &'static dyn SpectrumSource {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        &soapy_power::SoapyPower
    }
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    {
        &demo::Demo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drop-downs walk these in order and `Limit::clamp` assumes the ends
    /// are the ends, so an entry out of place would be a silently wrong menu.
    #[test]
    fn the_rtl_tables_are_ascending_and_within_their_limits() {
        for table in [RTL_SAMPLE_RATES, RTL_GAINS] {
            assert!(!table.is_empty());
            assert!(
                table.windows(2).all(|w| w[0] < w[1]),
                "not ascending: {table:?}"
            );
        }

        let limits = Limits::DEFAULT;
        for rate in RTL_SAMPLE_RATES {
            assert!(
                *rate >= limits.sample_rate.min && *rate <= limits.sample_rate.max,
                "{rate} Hz is outside the spin box's own range"
            );
        }
        for gain in RTL_GAINS {
            assert!(
                *gain >= 0.0 && *gain <= limits.gain.max,
                "{gain} dB is outside the spin box's own range"
            );
        }
    }

    /// `rtl_tcp`'s greeting reports how many gains the tuner has, and a real
    /// R828D reports 29 -- the length of librtlsdr's `r820t_gains`. If this
    /// ever disagrees, the table has drifted from the hardware.
    #[test]
    fn the_gain_table_is_the_length_the_tuner_reports() {
        assert_eq!(RTL_GAINS.len(), 29);
        assert_eq!(RTL_GAINS.first().copied(), Some(0.0));
        assert_eq!(RTL_GAINS.last().copied(), Some(49.6));
    }

    /// Every backend's declared default has to be pickable, or selecting that
    /// backend would leave the drop-down showing a value not in its own list.
    #[test]
    fn every_backend_default_is_on_its_own_menu() {
        for source in registry() {
            let limits = source.info().limits;
            let id = source.info().id;
            // A fixed limit hides the control altogether, so whatever it
            // nominally defaults to is never shown.
            if !limits.sample_rate_choices.is_empty() && !limits.sample_rate.is_fixed() {
                assert!(
                    limits
                        .sample_rate_choices
                        .contains(&limits.sample_rate.default),
                    "{id}: default rate {} is not on its list",
                    limits.sample_rate.default
                );
            }
            if !limits.gain_choices.is_empty() {
                assert!(
                    limits.gain_choices.contains(&limits.gain.default) || limits.gain.default < 0.0,
                    "{id}: default gain {} is not on its list",
                    limits.gain.default
                );
            }
            assert!(
                limits.crop.default >= limits.crop.min && limits.crop.default <= limits.crop.max,
                "{id}: crop default {} is outside its own range",
                limits.crop.default
            );
        }
    }

    #[test]
    fn linspace_matches_numpy() {
        assert_eq!(linspace(0.0, 1.0, 0), Vec::<f64>::new());
        assert_eq!(linspace(5.0, 9.0, 1), vec![5.0]);
        assert_eq!(linspace(0.0, 3.0, 4), vec![0.0, 1.0, 2.0, 3.0]);
        let v = linspace(10.0, 20.0, 11);
        assert_eq!(v.len(), 11);
        assert!((v[10] - 20.0).abs() < 1e-12);
    }

    #[test]
    fn registry_ids_are_unique() {
        let mut ids: Vec<&str> = registry().iter().map(|s| s.info().id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate source id in registry");
    }

    #[test]
    fn default_source_is_registered() {
        assert!(find(default_source().info().id).is_some());
    }

    /// Whatever a first run opens on has to be a backend that can actually
    /// run on that platform.
    #[test]
    fn the_default_backend_needs_no_helper_binary_where_none_can_run() {
        let id = default_source().info().id;
        if cfg!(any(target_arch = "wasm32", target_os = "android")) {
            assert_eq!(
                id, "demo",
                "neither a browser nor Android can spawn a sweep program, so \
                 anything but a built-in backend opens on a dead Start button"
            );
        }
    }

    #[test]
    fn lnb_offset_applies_to_tuner_frequency() {
        let cfg = SweepConfig {
            start_freq_mhz: 10_000.0,
            stop_freq_mhz: 10_100.0,
            lnb_lo_hz: 9_750e6,
            ..Default::default()
        };
        assert!((cfg.tuner_start_hz() - 250e6).abs() < 1.0);
        assert!((cfg.tuner_stop_hz() - 350e6).abs() < 1.0);
    }
}
