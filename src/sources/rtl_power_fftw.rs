//! The `rtl_power_fftw` backend: whitespace separated `freq power` text, framed
//! by blank lines, one frequency hop at a time.
//!
//! Replaces QSpectrumAnalyzer's `backends/rtl_power_fftw.py`.

use std::sync::Arc;

use super::process::{self, LineParser};
use super::{
    EventSink, Frame, Limits, SourceError, SourceInfo, SourceKind, SourceSession, SpectrumSource,
    SweepConfig,
};
use crate::util::{now_unix, split_args};

static INFO: SourceInfo = SourceInfo {
    id: "rtl_power_fftw",
    label: "rtl_power_fftw (RTL-SDR)",
    kind: SourceKind::Process,
    default_executable: "rtl_power_fftw",
    additional_params: "",
    has_device_help: false,
    limits: Limits::DEFAULT,
};

/// `rtl_power_fftw` cannot resolve bins finer than this, so the GUI's value is
/// clamped before the FFT size is derived from it.
const MAX_BIN_SIZE_KHZ: f64 = 2800.0;

/// Ceiling on the hop count.
///
/// Hops grow as span / sample rate, and the sample rate spin box reaches all the
/// way down to a few hertz -- without a bound, a mistyped sample rate would ask
/// for a multi-gigabyte crop table before the backend is even started.
const MAX_HOPS: usize = 100_000;

/// Cap on how much buffer space a sweep may claim up front; the vectors still
/// grow if a backend reports more.
const MAX_RESERVE: usize = 1 << 18;

pub struct RtlPowerFftw;

impl SpectrumSource for RtlPowerFftw {
    fn info(&self) -> &'static SourceInfo {
        &INFO
    }

    fn start(
        &self,
        cfg: SweepConfig,
        sink: EventSink,
    ) -> Result<Box<dyn SourceSession>, SourceError> {
        let setup = Setup::new(&cfg)?;
        let cmdline = command_line(&cfg, &setup);
        let parser = Parser::new(&cfg, &setup);
        process::spawn_lines(cmdline, sink, Box::new(parser), setup.hops)
    }
}

// ---------------------------------------------------------------------------
// Hop arithmetic
// ---------------------------------------------------------------------------

/// Everything `PowerThread.setup()` worked out before spawning the process.
#[derive(Clone, Debug, PartialEq)]
struct Setup {
    hops: usize,
    /// FFT bins per hop.
    bins: usize,
    /// Integration time of a single hop, seconds.
    hop_time: f64,
    /// Hop overlap in percent, which is the unit `-o` expects.
    overlap_pct: f64,
    /// Per-hop inclusive window of frequencies worth keeping, Hz, with the LNB
    /// offset already added so the parser can compare reported points directly.
    crop_windows: Vec<(f64, f64)>,
}

impl Setup {
    fn new(cfg: &SweepConfig) -> Result<Self, SourceError> {
        let sample_rate = cfg.sample_rate;
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(SourceError::InvalidConfig(
                "sample rate must be a positive number".to_owned(),
            ));
        }

        let crop_pct = cfg.crop * 100.0;
        let overlap_pct = crop_pct * 2.0;
        let min_overhang = sample_rate * overlap_pct * 0.01;

        // What a hop contributes once the mandatory overlap is deducted. At a
        // crop of 50% or more the sweep would never advance.
        let net_per_hop = sample_rate - min_overhang;
        if !net_per_hop.is_finite() || net_per_hop <= 0.0 {
            return Err(SourceError::InvalidConfig(format!(
                "a crop of {crop_pct:.0}% consumes a whole hop; use less than 50%"
            )));
        }

        let freq_range = cfg.span_hz();
        if !(freq_range > 0.0) {
            return Err(SourceError::InvalidConfig(
                "stop frequency must be above the start frequency".to_owned(),
            ));
        }

        let hops_f = ((freq_range - min_overhang) / net_per_hop).ceil();
        if !hops_f.is_finite() || hops_f < 1.0 {
            return Err(SourceError::InvalidConfig(format!(
                "the {:.3} MHz span is narrower than the hop overlap",
                freq_range / 1e6
            )));
        }
        if hops_f > MAX_HOPS as f64 {
            return Err(SourceError::InvalidConfig(format!(
                "{hops_f:.0} hops would be needed; raise the sample rate or narrow the span"
            )));
        }
        let hops = hops_f as usize;

        let overhang = if hops > 1 {
            (hops_f * sample_rate - freq_range) / (hops_f - 1.0)
        } else {
            0.0
        };

        // The finite check has to come before the clamp: `f64::min` returns the
        // *other* operand when one side is NaN, so `NaN.min(2800.0)` is 2800.0
        // and a clamp-first order would silently accept a NaN bin size.
        if !cfg.bin_size_khz.is_finite() || cfg.bin_size_khz <= 0.0 {
            return Err(SourceError::InvalidConfig(
                "bin size must be a positive number".to_owned(),
            ));
        }
        let bin_size_hz = cfg.bin_size_khz.min(MAX_BIN_SIZE_KHZ) * 1e3;
        let bins_f = (sample_rate / bin_size_hz).ceil();
        if !bins_f.is_finite() || bins_f < 1.0 {
            return Err(SourceError::InvalidConfig(format!(
                "{} kHz bins do not fit into the sample rate",
                cfg.bin_size_khz
            )));
        }
        let bins = bins_f as usize;

        // The topmost bin centre sits one bin short of the hop's upper edge.
        let hop_span = sample_rate - sample_rate / bins_f;
        let crop_freq = sample_rate * crop_pct * 0.01;
        let first_hop_start = cfg.start_freq_mhz * 1e6;
        let crop_windows = (0..hops)
            .map(|hop| {
                let lo = first_hop_start + (sample_rate - overhang) * hop as f64;
                (lo + crop_freq, lo + hop_span - crop_freq)
            })
            .collect();

        Ok(Self {
            hops,
            bins,
            hop_time: cfg.interval_s / hops_f,
            overlap_pct,
            crop_windows,
        })
    }
}

fn command_line(cfg: &SweepConfig, setup: &Setup) -> Vec<String> {
    let mut cmdline = split_args(&cfg.executable);
    if cmdline.is_empty() {
        cmdline.push(INFO.default_executable.to_owned());
    }

    let device = cfg.device.trim();
    let device = if device.is_empty() { "0" } else { device };
    let lnb_mhz = cfg.lnb_lo_hz / 1e6;

    // `-g` wants tenths of a dB, and the negative value an auto-gain request
    // turns into is how the Python version decided not to pass `-g` at all.
    let gain_tenths = (cfg.gain_db * 10.0) as i64;

    cmdline.extend([
        "-f".to_owned(),
        format!(
            "{}M:{}M",
            cfg.start_freq_mhz - lnb_mhz,
            cfg.stop_freq_mhz - lnb_mhz
        ),
        "-b".to_owned(),
        setup.bins.to_string(),
        "-t".to_owned(),
        setup.hop_time.to_string(),
        "-d".to_owned(),
        device.to_owned(),
        "-r".to_owned(),
        (cfg.sample_rate as i64).to_string(),
        "-p".to_owned(),
        cfg.ppm.to_string(),
        "-q".to_owned(),
    ]);

    if gain_tenths >= 0 {
        cmdline.push("-g".to_owned());
        cmdline.push(gain_tenths.to_string());
    }
    if setup.overlap_pct > 0.0 {
        cmdline.push("-o".to_owned());
        cmdline.push(setup.overlap_pct.to_string());
    }
    if !cfg.single_shot {
        cmdline.push("-c".to_owned());
    }

    cmdline.extend(split_args(&cfg.extra_params));
    cmdline
}

// ---------------------------------------------------------------------------
// Output parsing
// ---------------------------------------------------------------------------

const ACQUISITION_START: &str = "# Acquisition start:";

/// Accumulates hops into sweeps.
///
/// Frame boundaries are blank lines: one ends a hop, two in a row end a sweep.
struct Parser {
    crop_windows: Vec<(f64, f64)>,
    lnb_lo_hz: f64,
    hop: usize,
    /// The previous line was blank, so the next blank one closes the sweep.
    prev_blank: bool,
    x: Vec<f64>,
    y: Vec<f32>,
    hop_x: Vec<f64>,
    hop_y: Vec<f32>,
    timestamp: Option<f64>,
    /// Last emitted axis, reused while the backend keeps reporting the same
    /// grid -- which is every sweep of a run that is not retuned.
    axis: Option<Arc<Vec<f64>>>,
}

impl Parser {
    fn new(cfg: &SweepConfig, setup: &Setup) -> Self {
        let sweep = setup.bins.saturating_mul(setup.hops).min(MAX_RESERVE);
        let hop = setup.bins.min(MAX_RESERVE);
        Self {
            crop_windows: setup.crop_windows.clone(),
            lnb_lo_hz: cfg.lnb_lo_hz,
            hop: 0,
            // Matches the Python version starting from an empty `prev_line`: a
            // leading blank line is a sweep boundary, not a hop boundary.
            prev_blank: true,
            x: Vec::with_capacity(sweep),
            y: Vec::with_capacity(sweep),
            hop_x: Vec::with_capacity(hop),
            hop_y: Vec::with_capacity(hop),
            timestamp: None,
            axis: None,
        }
    }

    fn end_hop(&mut self) {
        self.hop = self.hop.saturating_add(1);
        self.x.append(&mut self.hop_x);
        self.y.append(&mut self.hop_y);
    }

    fn end_sweep(&mut self, sink: &EventSink) {
        self.hop = 0;
        let timestamp = self.timestamp.take().unwrap_or_else(now_unix);
        if self.x.is_empty() {
            self.y.clear();
            return;
        }

        let y = std::mem::take(&mut self.y);
        let axis = match self.axis.take() {
            Some(a) if a.as_slice() == self.x.as_slice() => {
                self.x.clear();
                a
            }
            _ => Arc::new(std::mem::take(&mut self.x)),
        };
        self.axis = Some(Arc::clone(&axis));

        sink.frame(Frame {
            timestamp,
            x: axis,
            y,
        });
    }

    fn push_point(&mut self, freq: f64, power: f32) {
        let Some(&(lo, hi)) = self.crop_windows.get(self.hop) else {
            // More hops than the arithmetic predicted: there is no window to
            // judge these points by, so drop them rather than mis-stitch.
            return;
        };
        if !(freq >= lo && freq <= hi) {
            return;
        }
        // The hop overlap shows up as frequencies at or below the end of the
        // previous hop; keeping only strictly increasing points removes it.
        if self.x.last().is_some_and(|&last| freq <= last) {
            return;
        }
        self.hop_x.push(freq);
        self.hop_y.push(power);
    }
}

impl LineParser for Parser {
    fn feed(&mut self, line: &str, sink: &EventSink) {
        let line = line.trim();

        if line.is_empty() {
            if self.prev_blank {
                self.end_sweep(sink);
            } else {
                self.end_hop();
            }
        } else if let Some(rest) = line.strip_prefix(ACQUISITION_START) {
            if self.timestamp.is_none() {
                self.timestamp = parse_acquisition_time(rest);
            }
        } else if !line.starts_with('#') && starts_with_digit(line) {
            if let Some((freq, power)) = parse_point(line) {
                self.push_point(freq + self.lnb_lo_hz, power);
            }
        }

        self.prev_blank = line.is_empty();
    }
}

fn starts_with_digit(line: &str) -> bool {
    line.as_bytes().first().is_some_and(u8::is_ascii_digit)
}

fn parse_point(line: &str) -> Option<(f64, f32)> {
    let mut fields = line.split_whitespace();
    let freq = fields.next()?.parse::<f64>().ok()?;
    let power = fields.next()?.parse::<f32>().ok()?;
    Some((freq, power))
}

/// `# Acquisition start: 2016-04-13 18:03:34 UTC` as Unix seconds.
///
/// Hand-rolled because the crate carries no date library and this is the only
/// place that would need one; anything unexpected falls back to the wall clock.
fn parse_acquisition_time(rest: &str) -> Option<f64> {
    let mut fields = rest.split_whitespace();
    let mut date = fields.next()?.split('-');
    let mut time = fields.next()?.split(':');

    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;
    let hour: i64 = time.next()?.parse().ok()?;
    let minute: i64 = time.next()?.parse().ok()?;
    let second: f64 = time.next()?.parse().ok()?;

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0.0..61.0).contains(&second)
    {
        return None;
    }

    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + hour * 3_600 + minute * 60) as f64 + second)
}

/// Days between 1970-01-01 and the given proleptic Gregorian date.
///
/// Hinnant's `days_from_civil`, which relies on the truncating integer division
/// that Rust and C++ share.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceEvent;
    use crossbeam_channel::Receiver;

    fn sink() -> (EventSink, Receiver<SourceEvent>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (EventSink::new(tx, None), rx)
    }

    fn frames(rx: &Receiver<SourceEvent>) -> Vec<Frame> {
        rx.try_iter()
            .filter_map(|e| match e {
                SourceEvent::Frame(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    /// 100..101.5 MHz at 1 Msps with 500 kHz bins: two hops whose cropped
    /// windows touch at exactly 100.5 MHz, which is what makes the overlap
    /// rejection observable.
    fn touching_hops() -> SweepConfig {
        SweepConfig {
            start_freq_mhz: 100.0,
            stop_freq_mhz: 101.5,
            bin_size_khz: 500.0,
            sample_rate: 1e6,
            interval_s: 1.0,
            crop: 0.0,
            ..Default::default()
        }
    }

    fn run(cfg: &SweepConfig, text: &str) -> Vec<Frame> {
        let setup = Setup::new(cfg).expect("test config must be valid");
        let mut parser = Parser::new(cfg, &setup);
        let (s, rx) = sink();
        for line in text.lines() {
            parser.feed(line, &s);
        }
        parser.finish(&s);
        frames(&rx)
    }

    #[test]
    fn setup_matches_the_python_arithmetic() {
        let cfg = SweepConfig {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 108.0,
            bin_size_khz: 10.0,
            sample_rate: 2_560_000.0,
            interval_s: 1.0,
            crop: 0.0,
            ..Default::default()
        };
        let setup = Setup::new(&cfg).expect("valid");

        assert_eq!(setup.hops, 9);
        assert_eq!(setup.bins, 256);
        assert!((setup.hop_time - 1.0 / 9.0).abs() < 1e-12);
        assert_eq!(setup.overlap_pct, 0.0);
        assert_eq!(setup.crop_windows.len(), 9);

        // overhang = (9 * 2.56e6 - 21e6) / 8 = 255 kHz
        let (lo0, hi0) = setup.crop_windows[0];
        let (lo1, _) = setup.crop_windows[1];
        assert!((lo0 - 87e6).abs() < 1e-6);
        assert!((hi0 - 89.55e6).abs() < 1e-6);
        assert!((lo1 - lo0 - (2_560_000.0 - 255_000.0)).abs() < 1e-6);
    }

    #[test]
    fn setup_applies_crop_and_overlap() {
        let cfg = SweepConfig {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 108.0,
            bin_size_khz: 10.0,
            sample_rate: 2_560_000.0,
            interval_s: 1.0,
            crop: 0.1,
            ..Default::default()
        };
        let setup = Setup::new(&cfg).expect("valid");

        assert_eq!(setup.hops, 11);
        assert!((setup.overlap_pct - 20.0).abs() < 1e-9);

        // crop_freq = 2.56e6 * 10% = 256 kHz, overhang = (11*2.56e6 - 21e6)/10
        let (lo0, hi0) = setup.crop_windows[0];
        let (lo1, _) = setup.crop_windows[1];
        assert!((lo0 - 87.256e6).abs() < 1.0);
        assert!((hi0 - 89.294e6).abs() < 1.0);
        assert!((lo1 - lo0 - (2_560_000.0 - 716_000.0)).abs() < 1.0);
    }

    #[test]
    fn setup_clamps_the_bin_size() {
        let cfg = SweepConfig {
            bin_size_khz: 999_999.0,
            sample_rate: 2_800_000.0,
            ..touching_hops()
        };
        // Clamped to 2800 kHz, so exactly one bin fits the sample rate.
        assert_eq!(Setup::new(&cfg).expect("valid").bins, 1);
    }

    #[test]
    fn setup_rejects_degenerate_configurations() {
        let bad = |cfg: SweepConfig| {
            assert!(
                matches!(Setup::new(&cfg), Err(SourceError::InvalidConfig(_))),
                "expected InvalidConfig for {cfg:?}"
            );
        };

        // A crop of 50% makes the hop overlap consume a whole hop.
        bad(SweepConfig {
            crop: 0.5,
            ..touching_hops()
        });
        bad(SweepConfig {
            crop: 0.9,
            ..touching_hops()
        });
        // Zero and inverted spans.
        bad(SweepConfig {
            stop_freq_mhz: 100.0,
            ..touching_hops()
        });
        bad(SweepConfig {
            stop_freq_mhz: 99.0,
            ..touching_hops()
        });
        // Nonsense numbers must not reach the arithmetic.
        bad(SweepConfig {
            sample_rate: 0.0,
            ..touching_hops()
        });
        bad(SweepConfig {
            sample_rate: f64::NAN,
            ..touching_hops()
        });
        bad(SweepConfig {
            sample_rate: f64::INFINITY,
            ..touching_hops()
        });
        bad(SweepConfig {
            bin_size_khz: 0.0,
            ..touching_hops()
        });
        bad(SweepConfig {
            bin_size_khz: f64::NAN,
            ..touching_hops()
        });
        bad(SweepConfig {
            stop_freq_mhz: f64::NAN,
            ..touching_hops()
        });
    }

    #[test]
    fn crop_window_count_stays_bounded() {
        // 1.5 MHz of span at 10 sps would be 150 000 hops.
        let cfg = SweepConfig {
            sample_rate: 10.0,
            ..touching_hops()
        };
        match Setup::new(&cfg) {
            Err(SourceError::InvalidConfig(m)) => assert!(m.contains("hops"), "{m}"),
            other => panic!("expected a hop-count rejection, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_two_hop_sweep() {
        let text = "\
# rtl-power-fftw output
# Acquisition start: 2016-04-13 18:03:34 UTC
100000000 -10.0
100500000 -11.0
101000000 -12.0

# Acquisition start: 2016-04-13 18:03:35 UTC
100500000 -20.0
101000000 -13.0
101500000 -14.0


";
        let frames = run(&touching_hops(), text);
        assert_eq!(frames.len(), 1);

        let f = &frames[0];
        // 101.0 is above hop 0's window, 100.5 repeats hop 0's last point and
        // 101.5 is above hop 1's window.
        assert_eq!(*f.x, vec![100e6, 100.5e6, 101e6]);
        assert_eq!(f.y, vec![-10.0, -11.0, -13.0]);
        assert_eq!(f.timestamp, 1_460_570_614.0);
    }

    #[test]
    fn a_hop_boundary_commits_without_emitting_a_frame() {
        let frames = run(&touching_hops(), "100000000 -10.0\n\n101000000 -13.0\n");
        assert!(frames.is_empty(), "an unfinished sweep must not be emitted");
    }

    #[test]
    fn extra_hops_are_dropped_instead_of_mis_stitched() {
        let text = "100000000 -10.0\n\n101000000 -13.0\n\n102000000 -14.0\n\n\n";
        let frames = run(&touching_hops(), text);
        assert_eq!(frames.len(), 1);
        assert_eq!(*frames[0].x, vec![100e6, 101e6]);
    }

    #[test]
    fn multiple_sweeps_share_one_axis_allocation() {
        let cfg = touching_hops();
        let setup = Setup::new(&cfg).expect("valid");
        let mut parser = Parser::new(&cfg, &setup);
        let (s, rx) = sink();
        for line in "100000000 -10.0\n\n\n100000000 -20.0\n\n\n".lines() {
            parser.feed(line, &s);
        }

        let got = frames(&rx);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].y, vec![-10.0]);
        assert_eq!(got[1].y, vec![-20.0]);
        assert!(Arc::ptr_eq(&got[0].x, &got[1].x), "axis should be reused");
    }

    #[test]
    fn empty_and_comment_only_input_emits_nothing() {
        for text in ["", "\n", "\n\n\n\n", "# only comments\n# more\n\n\n"] {
            assert!(
                run(&touching_hops(), text).is_empty(),
                "unexpected frame from {text:?}"
            );
        }
    }

    #[test]
    fn truncated_and_malformed_lines_are_ignored() {
        let text = "\
100000000
100000000 not-a-number
100000000 -10.0 extra
-100000000 -11.0
100500000 -11.0

101000000 -13.0


";
        let frames = run(&touching_hops(), text);
        assert_eq!(frames.len(), 1);
        assert_eq!(*frames[0].x, vec![100e6, 100.5e6, 101e6]);
        assert_eq!(frames[0].y, vec![-10.0, -11.0, -13.0]);
    }

    #[test]
    fn nan_power_is_kept_and_nan_frequency_is_dropped() {
        let text = "100000000 nan\n100500000 -11.0\n\nnan -13.0\n101000000 -13.0\n\n\n";
        let frames = run(&touching_hops(), text);
        assert_eq!(frames.len(), 1);
        assert_eq!(*frames[0].x, vec![100e6, 100.5e6, 101e6]);
        assert!(frames[0].y[0].is_nan());
    }

    #[test]
    fn lnb_offset_is_added_back_to_reported_frequencies() {
        let cfg = SweepConfig {
            start_freq_mhz: 10_100.0,
            stop_freq_mhz: 10_101.5,
            lnb_lo_hz: 10_000e6,
            ..touching_hops()
        };
        let frames = run(&cfg, "100000000 -10.0\n\n101000000 -13.0\n\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(*frames[0].x, vec![10_100e6, 10_101e6]);
    }

    #[test]
    fn a_sweep_without_a_timestamp_comment_uses_the_wall_clock() {
        let before = now_unix();
        let frames = run(&touching_hops(), "100000000 -10.0\n\n\n");
        assert_eq!(frames.len(), 1);
        assert!(frames[0].timestamp >= before);
    }

    #[test]
    fn command_line_matches_the_python_invocation() {
        let cfg = SweepConfig {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 108.0,
            bin_size_khz: 10.0,
            sample_rate: 2_560_000.0,
            interval_s: 1.0,
            gain_db: 37.0,
            ppm: 12,
            crop: 0.0,
            device: "1".to_owned(),
            executable: "rtl_power_fftw".to_owned(),
            ..Default::default()
        };
        let setup = Setup::new(&cfg).expect("valid");
        let cmd = command_line(&cfg, &setup);

        assert_eq!(cmd[0], "rtl_power_fftw");
        assert_eq!(&cmd[1..5], ["-f", "87M:108M", "-b", "256"]);
        assert_eq!(cmd[5], "-t");
        assert_eq!(&cmd[7..14], ["-d", "1", "-r", "2560000", "-p", "12", "-q"]);
        // 37 dB arrives as tenths of a dB.
        assert_eq!(&cmd[14..16], ["-g", "370"]);
        // Continuous mode, and no -o because the crop is zero.
        assert!(cmd.contains(&"-c".to_owned()));
        assert!(!cmd.contains(&"-o".to_owned()));
    }

    #[test]
    fn command_line_omits_gain_and_adds_overlap_and_extras() {
        let cfg = SweepConfig {
            gain_db: -1.0,
            crop: 0.1,
            single_shot: true,
            lnb_lo_hz: 50e6,
            start_freq_mhz: 150.0,
            stop_freq_mhz: 160.0,
            executable: "\"rtl power fftw\"".to_owned(),
            extra_params: "--baseline file.txt".to_owned(),
            ..touching_hops()
        };
        let setup = Setup::new(&cfg).expect("valid");
        let cmd = command_line(&cfg, &setup);

        assert_eq!(cmd[0], "rtl power fftw");
        assert_eq!(cmd[2], "100M:110M");
        assert!(!cmd.contains(&"-g".to_owned()));
        assert!(cmd.contains(&"-o".to_owned()));
        assert!(!cmd.contains(&"-c".to_owned()));
        assert_eq!(&cmd[cmd.len() - 2..], ["--baseline", "file.txt"]);
        // An empty device field still has to produce a valid -d argument.
        assert_eq!(
            cmd[cmd.iter().position(|a| a == "-d").expect("-d") + 1],
            "0"
        );
    }

    #[test]
    fn command_line_falls_back_to_the_default_executable() {
        let cfg = SweepConfig {
            executable: "   ".to_owned(),
            ..touching_hops()
        };
        let setup = Setup::new(&cfg).expect("valid");
        assert_eq!(command_line(&cfg, &setup)[0], "rtl_power_fftw");
    }

    #[test]
    fn civil_dates_convert_to_unix_seconds() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2016, 4, 13), 16_904);
    }

    #[test]
    fn acquisition_timestamps_parse_or_fall_back() {
        assert_eq!(
            parse_acquisition_time(" 2016-04-13 18:03:34 UTC"),
            Some(1_460_570_614.0)
        );
        assert_eq!(
            parse_acquisition_time(" 1970-01-01 00:00:00 UTC"),
            Some(0.0)
        );
        let t = parse_acquisition_time(" 1970-01-01 00:00:01.5 UTC").expect("parsed");
        assert!((t - 1.5).abs() < 1e-9);

        for junk in [
            "",
            " not a date",
            " 2016-04-13",
            " 2016-13-01 00:00:00",
            " 2016-04-32 00:00:00",
            " 2016-04-13 25:00:00",
            " 2016-04-13 00:61:00",
            " 2016-04-13 00:00:99",
            " x-y-z a:b:c",
        ] {
            assert_eq!(parse_acquisition_time(junk), None, "{junk:?}");
        }
    }
}
