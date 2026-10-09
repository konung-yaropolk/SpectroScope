//! The `hackrf_sweep` backend: length-prefixed binary records on stdout, each
//! holding one tuning step's worth of power values.
//!
//! Replaces QSpectrumAnalyzer's `backends/hackrf_sweep.py`.

use std::io::{BufRead, ErrorKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::process::{self, ByteParser};
use super::{
    EventSink, Frame, Limit, Limits, SourceError, SourceInfo, SourceKind, SourceSession,
    SpectrumSource, SweepConfig,
};
use crate::util::{now_monotonic, now_unix, split_args};

static INFO: SourceInfo = SourceInfo {
    id: "hackrf_sweep",
    label: "hackrf_sweep (HackRF)",
    kind: SourceKind::Process,
    default_executable: "hackrf_sweep",
    additional_params: "",
    has_device_help: false,
    limits: Limits {
        // The HackRF sweeps in fixed 20 MHz steps; none of these are choices.
        sample_rate: Limit::new(20e6, 20e6, 20e6),
        bandwidth: Limit::new(0.0, 0.0, 0.0),
        gain: Limit::new(-1.0, 102.0, 40.0),
        start_freq: Limit::new(0.0, 7230.0, 0.0),
        stop_freq: Limit::new(0.0, 7250.0, 6000.0),
        // Below 3 kHz the sweep never keeps up, whatever the interval.
        bin_size: Limit::new(3.0, 5000.0, 1000.0),
        interval: Limit::new(0.0, 3600.0, 0.0),
        ppm: Limit::new(0, 0, 0),
        crop: Limit::new(0, 0, 0),
    },
};

/// Bytes of edge frequencies in front of the power values of a record.
const EDGES_LEN: usize = 16;

/// Largest record we are willing to allocate for. A 20 MHz step at the finest
/// supported bin size is a few tens of kilobytes, so anything past this means
/// the stream is out of sync rather than unusually detailed.
const MAX_RECORD_LEN: usize = 1 << 20;

/// Stop accumulating a sweep that never sees its start frequency again; the
/// whole 7.25 GHz range at the finest bin size is under 2.5 M points.
const MAX_SWEEP_POINTS: usize = 1 << 24;

/// The tuner covers 7.25 GHz in 20 MHz steps, so a few hundred steps is the
/// most a real sweep can need. Anything beyond it is a nonsense span.
const MAX_STEPS: f64 = 100_000.0;

pub struct HackrfSweep;

impl SpectrumSource for HackrfSweep {
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
        // hackrf_sweep retunes internally, so there is no hop count to show.
        process::spawn_bytes(cmdline, sink, Box::new(parser), 0)
    }
}

// ---------------------------------------------------------------------------
// Range quantisation and gain distribution
// ---------------------------------------------------------------------------

/// What `PowerThread.setup()` worked out before spawning the process.
#[derive(Debug)]
struct Setup {
    /// MHz as displayed, i.e. LNB offset included.
    start_mhz: f64,
    /// MHz as displayed, rounded up to a whole number of tuning steps.
    stop_mhz: f64,
    bin_size_khz: f64,
    gain_db: f64,
    lna_gain: i64,
    vga_gain: i64,
}

impl Setup {
    fn new(cfg: &SweepConfig) -> Result<Self, SourceError> {
        if !cfg.start_freq_mhz.is_finite() || !cfg.stop_freq_mhz.is_finite() {
            return Err(SourceError::InvalidConfig(
                "start and stop frequency must be numbers".to_owned(),
            ));
        }

        // Only whole steps of one full bandwidth are supported, so the stop
        // frequency moves up to the next step boundary.
        let step_bandwidth = cfg.sample_rate / 1e6;
        if !step_bandwidth.is_finite() || step_bandwidth <= 0.0 {
            return Err(SourceError::InvalidConfig(
                "sample rate must be a positive number".to_owned(),
            ));
        }
        let span_mhz = cfg.stop_freq_mhz - cfg.start_freq_mhz;
        if !(span_mhz > 0.0) {
            return Err(SourceError::InvalidConfig(
                "stop frequency must be above the start frequency".to_owned(),
            ));
        }
        // The Python computed `1 + (span - 1) // step_bandwidth`, which comes
        // out as zero steps for any span below 1 MHz and so asked the backend
        // for an empty range. A span that small still costs one whole step of
        // tuner time, so round it up to one instead of refusing to sweep.
        let steps = (1.0 + ((span_mhz - 1.0) / step_bandwidth).floor()).max(1.0);
        if !steps.is_finite() || steps > MAX_STEPS {
            return Err(SourceError::InvalidConfig(format!(
                "a {span_mhz:.3} MHz span needs more steps than hackrf_sweep can take"
            )));
        }
        let stop_mhz = cfg.start_freq_mhz + steps * step_bandwidth;

        let bin_size_khz = cfg.bin_size_khz.clamp(3.0, 5000.0);

        // The HackRF has two analog gain stages with different granularity:
        // the LNA moves in 8 dB steps, the VGA in 2 dB steps.
        let gain_db = cfg.gain_db.min(102.0);
        let (lna_gain, vga_gain) = if gain_db >= 0.0 {
            let lna = 8.0 * (gain_db / 18.0).floor();
            let vga = 2.0 * ((gain_db - lna) / 2.0).floor();
            (lna as i64, vga as i64)
        } else {
            (0, 0)
        };

        Ok(Self {
            start_mhz: cfg.start_freq_mhz,
            stop_mhz,
            bin_size_khz,
            gain_db,
            lna_gain,
            vga_gain,
        })
    }
}

fn command_line(cfg: &SweepConfig, setup: &Setup) -> Vec<String> {
    let mut cmdline = split_args(&cfg.executable);
    if cmdline.is_empty() {
        cmdline.push(INFO.default_executable.to_owned());
    }

    let lnb_mhz = cfg.lnb_lo_hz / 1e6;
    cmdline.extend([
        "-f".to_owned(),
        format!(
            "{}:{}",
            (setup.start_mhz - lnb_mhz) as i64,
            (setup.stop_mhz - lnb_mhz) as i64
        ),
        // Binary output on stdout; the text mode is not parseable at speed.
        "-B".to_owned(),
        "-w".to_owned(),
        ((setup.bin_size_khz * 1000.0) as i64).to_string(),
    ]);

    if setup.gain_db >= 0.0 {
        cmdline.push("-l".to_owned());
        cmdline.push(setup.lna_gain.to_string());
        cmdline.push("-g".to_owned());
        cmdline.push(setup.vga_gain.to_string());
    }
    if cfg.single_shot {
        cmdline.push("-1".to_owned());
    }

    cmdline.extend(split_args(&cfg.extra_params));
    cmdline
}

// ---------------------------------------------------------------------------
// Record parsing
// ---------------------------------------------------------------------------

/// Reassembles the per-step records into whole sweeps.
struct Parser {
    /// Tuner MHz, LNB removed, so they can be compared with the edges the
    /// backend reports.
    start_mhz: f64,
    stop_mhz: f64,
    lnb_lo_hz: f64,
    interval_s: f64,
    points: Vec<(f64, f32)>,
    /// Monotonic time of the last sweep we let through.
    last_sweep: f64,
    axis: Option<Arc<Vec<f64>>>,
}

impl Parser {
    fn new(cfg: &SweepConfig, setup: &Setup) -> Self {
        let lnb_mhz = cfg.lnb_lo_hz / 1e6;
        Self {
            start_mhz: setup.start_mhz - lnb_mhz,
            stop_mhz: setup.stop_mhz - lnb_mhz,
            lnb_lo_hz: cfg.lnb_lo_hz,
            interval_s: cfg.interval_s.max(0.0),
            points: Vec::new(),
            // The first sweep is never rate limited, whatever the interval.
            last_sweep: f64::NEG_INFINITY,
            axis: None,
        }
    }

    /// Consume one record payload: two `u64` edge frequencies then `f32` powers.
    fn feed(&mut self, payload: &[u8], sink: &EventSink) {
        let Some((edges, values)) = payload.split_at_checked(EDGES_LEN) else {
            return;
        };
        let (Some(low), Some(high)) = (u64_le(edges), edges.get(8..).and_then(u64_le)) else {
            return;
        };

        let count = values.len() / 4;
        if count == 0 || high <= low {
            return;
        }
        if values.len() % 4 != 0 {
            sink.log("hackrf_sweep: record is not a whole number of samples; ignoring it");
            return;
        }

        // The backend restarts every pass at the requested start frequency, so
        // the low edge arriving back there is what delimits sweeps.
        if low / 1_000_000 <= self.start_mhz as i64 as u64 || self.start_mhz <= 0.0 && low == 0 {
            self.points.clear();
        }
        if self.points.len() > MAX_SWEEP_POINTS {
            sink.log("hackrf_sweep: sweep never returned to its start frequency; restarting it");
            self.points.clear();
        }

        let step = (high - low) as f64 / count as f64;
        let first_centre = low as f64 + self.lnb_lo_hz + step / 2.0;
        self.points.reserve(count);
        for (i, value) in values.chunks_exact(4).enumerate() {
            let Some(bytes) = value.first_chunk::<4>().copied() else {
                continue;
            };
            self.points
                .push((first_centre + step * i as f64, f32::from_le_bytes(bytes)));
        }

        if high as f64 / 1e6 >= self.stop_mhz {
            // A pass that finished sooner than the requested interval is
            // dropped rather than queued, exactly as the Python version did.
            let finished = now_monotonic();
            if finished < self.last_sweep + self.interval_s {
                return;
            }
            self.last_sweep = finished;
            self.emit(sink);
        }
    }

    /// Records arrive out of frequency order, so a finished sweep is sorted
    /// before it is handed over.
    fn emit(&mut self, sink: &EventSink) {
        if self.points.is_empty() {
            return;
        }
        self.points.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut x = Vec::with_capacity(self.points.len());
        let mut y = Vec::with_capacity(self.points.len());
        for &(freq, power) in &self.points {
            x.push(freq);
            y.push(power);
        }

        let axis = match self.axis.take() {
            Some(a) if a.as_slice() == x.as_slice() => a,
            _ => Arc::new(x),
        };
        self.axis = Some(Arc::clone(&axis));

        sink.frame(Frame {
            timestamp: now_unix(),
            x: axis,
            y,
        });
    }
}

impl ByteParser for Parser {
    fn run(&mut self, stdout: &mut dyn BufRead, sink: &EventSink, alive: &AtomicBool) {
        let mut header = [0u8; 4];
        let mut payload = Vec::new();

        while alive.load(Ordering::SeqCst) {
            match fill(stdout, &mut header) {
                Fill::Done => {}
                Fill::Eof => break,
                Fill::Failed(e) => {
                    if alive.load(Ordering::SeqCst) {
                        sink.log(format!("hackrf_sweep: {e}"));
                    }
                    break;
                }
            }

            let len = u32::from_le_bytes(header) as usize;
            if !(EDGES_LEN..=MAX_RECORD_LEN).contains(&len) {
                sink.error(format!(
                    "hackrf_sweep: refusing a {len}-byte record; the output stream is out of sync"
                ));
                break;
            }

            payload.clear();
            payload.resize(len, 0);
            match fill(stdout, &mut payload) {
                Fill::Done => {}
                // A short final record is how a killed child ends its stream.
                Fill::Eof => break,
                Fill::Failed(e) => {
                    if alive.load(Ordering::SeqCst) {
                        sink.log(format!("hackrf_sweep: {e}"));
                    }
                    break;
                }
            }

            self.feed(&payload, sink);
        }
    }
}

enum Fill {
    Done,
    Eof,
    Failed(std::io::Error),
}

fn fill(stream: &mut dyn BufRead, buf: &mut [u8]) -> Fill {
    match stream.read_exact(buf) {
        Ok(()) => Fill::Done,
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => Fill::Eof,
        Err(e) => Fill::Failed(e),
    }
}

fn u64_le(bytes: &[u8]) -> Option<u64> {
    bytes.first_chunk::<8>().copied().map(u64::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceEvent;
    use crossbeam_channel::Receiver;
    use std::io::Cursor;

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

    /// One length-prefixed record, the way hackrf_sweep writes it.
    fn record(low: u64, high: u64, values: &[f32]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&low.to_le_bytes());
        payload.extend_from_slice(&high.to_le_bytes());
        for v in values {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        let mut out = (payload.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(&payload);
        out
    }

    /// 0..40 MHz, no LNB, no rate limit.
    fn parser() -> Parser {
        Parser {
            start_mhz: 0.0,
            stop_mhz: 40.0,
            lnb_lo_hz: 0.0,
            interval_s: 0.0,
            points: Vec::new(),
            last_sweep: f64::NEG_INFINITY,
            axis: None,
        }
    }

    fn drive(parser: &mut Parser, bytes: Vec<u8>) -> (Vec<Frame>, Vec<String>) {
        let (s, rx) = sink();
        let alive = AtomicBool::new(true);
        let mut stream = Cursor::new(bytes);
        parser.run(&mut stream, &s, &alive);

        // The channel can only be drained once, so partition in a single pass:
        // calling `frames()` and then `errors()` would throw the errors away.
        let mut got = Vec::new();
        let mut errs = Vec::new();
        for event in rx.try_iter() {
            match event {
                SourceEvent::Frame(f) => got.push(f),
                SourceEvent::Error(m) => errs.push(m),
                _ => {}
            }
        }
        (got, errs)
    }

    #[test]
    fn reassembles_and_sorts_an_out_of_order_sweep() {
        let mut stream = record(0, 10_000_000, &[-1.0, -2.0]);
        stream.extend(record(20_000_000, 30_000_000, &[-5.0, -6.0]));
        stream.extend(record(10_000_000, 20_000_000, &[-3.0, -4.0]));
        stream.extend(record(30_000_000, 40_000_000, &[-7.0, -8.0]));

        let (got, errs) = drive(&mut parser(), stream);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(got.len(), 1);

        // Bin centres, not a linspace: 5 MHz steps starting half a bin in.
        assert_eq!(
            *got[0].x,
            vec![2.5e6, 7.5e6, 12.5e6, 17.5e6, 22.5e6, 27.5e6, 32.5e6, 37.5e6]
        );
        assert_eq!(
            got[0].y,
            vec![-1.0, -2.0, -3.0, -4.0, -5.0, -6.0, -7.0, -8.0]
        );
        assert!(got[0].timestamp > 0.0);
    }

    #[test]
    fn a_new_low_edge_restarts_the_sweep() {
        // An incomplete pass followed by a fresh one: only the second survives.
        let mut stream = record(0, 10_000_000, &[-99.0, -99.0]);
        stream.extend(record(0, 20_000_000, &[-1.0, -2.0]));
        stream.extend(record(20_000_000, 40_000_000, &[-3.0, -4.0]));

        let (got, _) = drive(&mut parser(), stream);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].y, vec![-1.0, -2.0, -3.0, -4.0]);
    }

    #[test]
    fn a_truncated_final_record_ends_the_stream_cleanly() {
        let mut stream = record(0, 20_000_000, &[-1.0, -2.0]);
        stream.extend(record(20_000_000, 40_000_000, &[-3.0, -4.0]));
        // Header promising 24 bytes, only 10 delivered.
        stream.extend_from_slice(&24u32.to_le_bytes());
        stream.extend_from_slice(&[0u8; 10]);

        let (got, errs) = drive(&mut parser(), stream);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(got.len(), 1, "the complete sweep is still delivered");
    }

    #[test]
    fn a_truncated_header_ends_the_stream_cleanly() {
        let mut stream = record(0, 40_000_000, &[-1.0, -2.0]);
        stream.extend_from_slice(&[0u8, 1]);
        let (got, errs) = drive(&mut parser(), stream);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn an_absurd_record_length_is_refused() {
        let (got, errs) = drive(&mut parser(), u32::MAX.to_le_bytes().to_vec());
        assert!(got.is_empty());
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("out of sync"), "{}", errs[0]);
    }

    #[test]
    fn a_record_too_short_for_its_edges_is_refused() {
        let mut stream = 8u32.to_le_bytes().to_vec();
        stream.extend_from_slice(&[0u8; 8]);
        let (_, errs) = drive(&mut parser(), stream);
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn empty_input_produces_nothing() {
        let (got, errs) = drive(&mut parser(), Vec::new());
        assert!(got.is_empty());
        assert!(errs.is_empty());
    }

    #[test]
    fn a_cleared_alive_flag_stops_before_the_first_read() {
        let mut p = parser();
        let (s, rx) = sink();
        let mut stream = Cursor::new(record(0, 40_000_000, &[-1.0, -2.0]));
        p.run(&mut stream, &s, &AtomicBool::new(false));
        assert!(frames(&rx).is_empty());
    }

    #[test]
    fn degenerate_records_are_skipped() {
        let mut p = parser();
        let (s, rx) = sink();

        // No power values at all.
        p.feed(&record(0, 10_000_000, &[])[4..], &s);
        // Zero-width span, which would make the bin step a division by zero.
        p.feed(&record(10_000_000, 10_000_000, &[-1.0])[4..], &s);
        // Inverted edges.
        p.feed(&record(20_000_000, 10_000_000, &[-1.0])[4..], &s);
        // A trailing partial float.
        let mut ragged = record(0, 10_000_000, &[-1.0])[4..].to_vec();
        ragged.push(0);
        p.feed(&ragged, &s);
        // Shorter than the two edge frequencies.
        p.feed(&[0u8; 8], &s);

        assert!(p.points.is_empty());
        assert!(frames(&rx).is_empty());
    }

    #[test]
    fn nan_powers_survive_the_sort() {
        let mut p = parser();
        let (s, rx) = sink();
        p.feed(&record(0, 40_000_000, &[f32::NAN, -1.0])[4..], &s);

        let got = frames(&rx);
        assert_eq!(got.len(), 1);
        assert_eq!(*got[0].x, vec![10e6, 30e6]);
        assert!(got[0].y[0].is_nan());
        assert_eq!(got[0].y[1], -1.0);
    }

    #[test]
    fn a_sweep_faster_than_the_interval_is_discarded() {
        let mut p = Parser {
            interval_s: 3600.0,
            ..parser()
        };
        let (s, rx) = sink();

        let full = record(0, 40_000_000, &[-1.0, -2.0]);
        p.feed(&full[4..], &s);
        p.feed(&full[4..], &s);
        p.feed(&full[4..], &s);

        assert_eq!(frames(&rx).len(), 1, "only the first sweep may get through");
    }

    #[test]
    fn repeated_sweeps_share_one_axis_allocation() {
        let full = record(0, 40_000_000, &[-1.0, -2.0]);
        let mut stream = full.clone();
        stream.extend(full);

        let (got, _) = drive(&mut parser(), stream);
        assert_eq!(got.len(), 2);
        assert!(Arc::ptr_eq(&got[0].x, &got[1].x), "axis should be reused");
    }

    #[test]
    fn the_lnb_offset_shifts_reported_frequencies_up() {
        let mut p = Parser {
            start_mhz: 0.0,
            stop_mhz: 40.0,
            lnb_lo_hz: 10_000e6,
            ..parser()
        };
        let (s, rx) = sink();
        p.feed(&record(0, 40_000_000, &[-1.0, -2.0])[4..], &s);

        let got = frames(&rx);
        assert_eq!(got.len(), 1);
        assert_eq!(*got[0].x, vec![10_010e6, 10_030e6]);
    }

    #[test]
    fn setup_quantises_the_range_to_whole_steps() {
        let cfg = |start: f64, stop: f64| SweepConfig {
            start_freq_mhz: start,
            stop_freq_mhz: stop,
            sample_rate: 20e6,
            bin_size_khz: 1000.0,
            gain_db: 40.0,
            ..Default::default()
        };

        // 6000 MHz is already 300 whole steps.
        let s = Setup::new(&cfg(0.0, 6000.0)).expect("valid");
        assert_eq!(s.start_mhz, 0.0);
        assert_eq!(s.stop_mhz, 6000.0);

        // One megahertz more needs a 301st step.
        assert_eq!(
            Setup::new(&cfg(0.0, 6001.0)).expect("valid").stop_mhz,
            6020.0
        );
        // A single hertz of span still costs one whole step.
        assert_eq!(
            Setup::new(&cfg(100.0, 100.5)).expect("valid").stop_mhz,
            120.0
        );
        assert_eq!(
            Setup::new(&cfg(100.0, 120.0)).expect("valid").stop_mhz,
            120.0
        );
        assert_eq!(
            Setup::new(&cfg(100.0, 121.0)).expect("valid").stop_mhz,
            140.0
        );
    }

    #[test]
    fn setup_splits_gain_between_the_two_stages() {
        let cfg = |gain: f64| SweepConfig {
            start_freq_mhz: 0.0,
            stop_freq_mhz: 6000.0,
            sample_rate: 20e6,
            gain_db: gain,
            ..Default::default()
        };
        let stages = |gain: f64| {
            let s = Setup::new(&cfg(gain)).expect("valid");
            (s.lna_gain, s.vga_gain)
        };

        assert_eq!(stages(40.0), (16, 24));
        assert_eq!(stages(0.0), (0, 0));
        assert_eq!(stages(17.0), (0, 16));
        assert_eq!(stages(18.0), (8, 10));
        assert_eq!(stages(102.0), (40, 62));
        // Above the maximum the gain is clamped, below zero it means "auto".
        assert_eq!(stages(500.0), (40, 62));
        assert_eq!(stages(-1.0), (0, 0));
    }

    #[test]
    fn setup_clamps_the_bin_size() {
        let cfg = |bin: f64| SweepConfig {
            start_freq_mhz: 0.0,
            stop_freq_mhz: 6000.0,
            sample_rate: 20e6,
            bin_size_khz: bin,
            ..Default::default()
        };
        assert_eq!(Setup::new(&cfg(0.0)).expect("valid").bin_size_khz, 3.0);
        assert_eq!(Setup::new(&cfg(1.0)).expect("valid").bin_size_khz, 3.0);
        assert_eq!(
            Setup::new(&cfg(1000.0)).expect("valid").bin_size_khz,
            1000.0
        );
        assert_eq!(
            Setup::new(&cfg(99_999.0)).expect("valid").bin_size_khz,
            5000.0
        );
    }

    #[test]
    fn setup_rejects_degenerate_configurations() {
        let base = SweepConfig {
            start_freq_mhz: 100.0,
            stop_freq_mhz: 200.0,
            sample_rate: 20e6,
            ..Default::default()
        };
        let bad = |cfg: SweepConfig| {
            assert!(
                matches!(Setup::new(&cfg), Err(SourceError::InvalidConfig(_))),
                "expected InvalidConfig for {cfg:?}"
            );
        };

        bad(SweepConfig {
            stop_freq_mhz: 100.0,
            ..base.clone()
        });
        bad(SweepConfig {
            stop_freq_mhz: 50.0,
            ..base.clone()
        });
        bad(SweepConfig {
            sample_rate: 0.0,
            ..base.clone()
        });
        bad(SweepConfig {
            sample_rate: f64::NAN,
            ..base.clone()
        });
        bad(SweepConfig {
            start_freq_mhz: f64::NAN,
            ..base.clone()
        });
        bad(SweepConfig {
            stop_freq_mhz: f64::INFINITY,
            ..base
        });
    }

    #[test]
    fn command_line_matches_the_python_invocation() {
        let cfg = SweepConfig {
            start_freq_mhz: 0.0,
            stop_freq_mhz: 6000.0,
            bin_size_khz: 1000.0,
            sample_rate: 20e6,
            gain_db: 40.0,
            executable: "hackrf_sweep".to_owned(),
            ..Default::default()
        };
        let setup = Setup::new(&cfg).expect("valid");
        assert_eq!(
            command_line(&cfg, &setup),
            [
                "hackrf_sweep",
                "-f",
                "0:6000",
                "-B",
                "-w",
                "1000000",
                "-l",
                "16",
                "-g",
                "24"
            ]
        );
    }

    #[test]
    fn command_line_handles_auto_gain_single_shot_lnb_and_extras() {
        let cfg = SweepConfig {
            start_freq_mhz: 10_100.0,
            stop_freq_mhz: 10_120.0,
            bin_size_khz: 3.0,
            sample_rate: 20e6,
            gain_db: -1.0,
            lnb_lo_hz: 10_000e6,
            single_shot: true,
            executable: String::new(),
            extra_params: "-a 1".to_owned(),
            ..Default::default()
        };
        let setup = Setup::new(&cfg).expect("valid");
        assert_eq!(
            command_line(&cfg, &setup),
            [
                "hackrf_sweep",
                "-f",
                "100:120",
                "-B",
                "-w",
                "3000",
                "-1",
                "-a",
                "1"
            ]
        );
    }
}
