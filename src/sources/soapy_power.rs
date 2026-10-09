//! The `soapy_power` backend: a SoapySDR sweeper driven as a child process,
//! talking the framed `soapy_power_bin` container.
//!
//! Replaces `backends/soapy_power.py`.

use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::process::{self, ByteParser};
use super::{
    EventSink, Frame, Limit, Limits, SourceError, SourceInfo, SourceKind, SourceSession,
    SpectrumSource, SweepConfig,
};
use crate::data::soapy_bin::{self, Header};
use crate::util::split_args;

static INFO: SourceInfo = SourceInfo {
    id: "soapy_power",
    label: "soapy_power (SoapySDR)",
    kind: SourceKind::Process,
    default_executable: "soapy_power",
    additional_params: "--even --fft-window boxcar --remove-dc",
    has_device_help: true,
    limits: Limits {
        sample_rate: Limit::new(0.0, 61_440_000.0, 2_560_000.0),
        bandwidth: Limit::new(0.0, 61_440_000.0, 0.0),
        gain: Limit::new(-1.0, 999.0, 37.0),
        start_freq: Limit::new(0.0, 7250.0, 87.0),
        stop_freq: Limit::new(0.0, 7250.0, 108.0),
        bin_size: Limit::new(0.0, 10_000.0, 10.0),
        // Unconstrained by the backend, so they keep `BaseInfo`'s ranges.
        interval: Limits::DEFAULT.interval,
        ppm: Limits::DEFAULT.ppm,
        crop: Limits::DEFAULT.crop,
    },
};

pub struct SoapyPower;

/// Shortest exact decimal form, so an integral setting does not reach the
/// command line as `2560000.000000` and a crop of 20 % does not arrive as
/// `20.000000000000004` from the GUI's fraction.
fn fmt_num(v: f64) -> String {
    let text = format!("{v:.6}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn build_cmdline(cfg: &SweepConfig) -> Result<Vec<String>, SourceError> {
    if !(cfg.stop_freq_mhz > cfg.start_freq_mhz) {
        return Err(SourceError::InvalidConfig(
            "stop frequency must be above start frequency".to_owned(),
        ));
    }
    if !(cfg.bin_size_khz > 0.0) {
        return Err(SourceError::InvalidConfig(
            "bin size must be greater than zero".to_owned(),
        ));
    }

    let mut cmd = split_args(&cfg.executable);
    if cmd.is_empty() {
        cmd.push(INFO.default_executable.to_owned());
    }

    // The *displayed* range: soapy_power is told the LNB LO separately and does
    // the arithmetic itself.
    //
    // No `--output-fd`: soapy_power already writes measurements to stdout and
    // its log to stderr. The Python version passed an inheritable OS pipe only
    // to keep the two apart, which reading stdout achieves portably.
    cmd.extend([
        "-f".to_owned(),
        format!(
            "{}M:{}M",
            fmt_num(cfg.start_freq_mhz),
            fmt_num(cfg.stop_freq_mhz)
        ),
        "-B".to_owned(),
        format!("{}k", fmt_num(cfg.bin_size_khz)),
        "-T".to_owned(),
        fmt_num(cfg.interval_s),
        "-d".to_owned(),
        cfg.device.clone(),
        "-r".to_owned(),
        fmt_num(cfg.sample_rate),
        "-p".to_owned(),
        cfg.ppm.to_string(),
        "-F".to_owned(),
        "soapy_power_bin".to_owned(),
    ]);

    if cfg.lnb_lo_hz != 0.0 {
        cmd.extend(["--lnb-lo".to_owned(), fmt_num(cfg.lnb_lo_hz)]);
    }
    if cfg.bandwidth > 0.0 {
        cmd.extend(["-w".to_owned(), fmt_num(cfg.bandwidth)]);
    }
    if cfg.gain_db >= 0.0 {
        cmd.extend(["-g".to_owned(), fmt_num(cfg.gain_db)]);
    }
    if cfg.crop > 0.0 {
        // soapy_power wants percent, the GUI keeps a fraction.
        cmd.extend(["-k".to_owned(), fmt_num(cfg.crop * 100.0)]);
    }
    if !cfg.single_shot {
        cmd.push("-c".to_owned());
    }

    cmd.extend(split_args(&cfg.extra_params));
    Ok(cmd)
}

/// Reassembles the hops of one sweep and emits it once the last hop lands.
struct SweepParser {
    /// Configured stop frequency in Hz, as displayed. A hop reaching within one
    /// bin of it is the final hop of a sweep.
    stop_hz: f64,
    /// Lowest hop start seen; a record there starts a new sweep.
    min_start: Option<f64>,
    timestamp: f64,
    x: Vec<f64>,
    y: Vec<f32>,
    /// Last emitted axis. Every sweep of a run normally repeats it, so the
    /// frames can share one allocation.
    axis: Option<Arc<Vec<f64>>>,
}

impl SweepParser {
    fn new(stop_hz: f64) -> Self {
        Self {
            stop_hz,
            min_start: None,
            timestamp: 0.0,
            x: Vec::new(),
            y: Vec::new(),
            axis: None,
        }
    }

    fn axis(&mut self) -> Arc<Vec<f64>> {
        match &self.axis {
            Some(a) if a.as_slice() == self.x.as_slice() => Arc::clone(a),
            _ => {
                let fresh = Arc::new(self.x.clone());
                self.axis = Some(Arc::clone(&fresh));
                fresh
            }
        }
    }

    /// Returns `false` once the GUI is gone and the run should end.
    fn push(&mut self, header: &Header, y: Vec<f32>, sink: &EventSink) -> bool {
        let Some(x) = soapy_bin::record_axis(header, y.len()) else {
            return sink.log(format!(
                "Discarding hop at {} Hz: {} power values do not fit its frequency range",
                fmt_num(header.start),
                y.len()
            ));
        };

        if self.min_start.is_some_and(|m| header.start > m) {
            self.x.extend_from_slice(&x);
            self.y.extend_from_slice(&y);
        } else {
            self.min_start = Some(match self.min_start {
                Some(m) => m.min(header.start),
                None => header.start,
            });
            // As in the Python version, a sweep carries its first hop's time.
            self.timestamp = header.time_stop;
            self.x = x;
            self.y = y;
        }

        // Hop centres are spaced so that the last one usually overshoots the
        // requested stop; the Python version's `>` test is what tolerates that.
        if header.stop > self.stop_hz - header.step {
            let frame = Frame {
                timestamp: self.timestamp,
                x: self.axis(),
                y: self.y.clone(),
            };
            return sink.frame(frame);
        }
        true
    }
}

impl ByteParser for SweepParser {
    fn run(&mut self, stdout: &mut dyn BufRead, sink: &EventSink, alive: &AtomicBool) {
        // One more indirection so the generic reader sees a sized type.
        let mut src = stdout;

        while alive.load(Ordering::SeqCst) {
            match soapy_bin::read_record(&mut src) {
                Ok(Some((header, y))) => {
                    if !self.push(&header, y, sink) {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    // A framed stream cannot resynchronise. While stopping, the
                    // kill itself cuts a record short, which is not an error.
                    if alive.load(Ordering::SeqCst) {
                        sink.error(format!("soapy_power output: {e}"));
                    }
                    break;
                }
            }
        }
    }
}

impl SpectrumSource for SoapyPower {
    fn info(&self) -> &'static SourceInfo {
        &INFO
    }

    fn start(
        &self,
        cfg: SweepConfig,
        sink: EventSink,
    ) -> Result<Box<dyn SourceSession>, SourceError> {
        let cmdline = build_cmdline(&cfg)?;
        let parser = SweepParser::new(cfg.stop_freq_mhz * 1e6);
        // soapy_power picks its own hop plan, so the hop count is unknown here.
        process::spawn_bytes(cmdline, sink, Box::new(parser), 0)
    }

    fn device_help(&self, executable: &str, device: &str) -> Option<String> {
        let mut text = process::capture_stdout(executable, &["--detect"]);
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
        text.push_str(&process::capture_stdout(
            executable,
            &["--device", device, "--info"],
        ));
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::soapy_bin::{encode_record, test_header};
    use crate::sources::SourceEvent;
    use std::io::Cursor;

    fn base_cfg() -> SweepConfig {
        SweepConfig {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 108.0,
            bin_size_khz: 10.0,
            interval_s: 1.0,
            gain_db: -1.0,
            ppm: 0,
            crop: 0.0,
            single_shot: false,
            device: "driver=rtlsdr".to_owned(),
            sample_rate: 2_560_000.0,
            bandwidth: 0.0,
            lnb_lo_hz: 0.0,
            executable: "soapy_power".to_owned(),
            extra_params: "--even --fft-window boxcar --remove-dc".to_owned(),
        }
    }

    fn run_parser(stop_hz: f64, stream: Vec<u8>) -> Vec<SourceEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = EventSink::new(tx, None);
        let alive = AtomicBool::new(true);
        let mut parser = SweepParser::new(stop_hz);
        let mut cursor = Cursor::new(stream);
        parser.run(&mut cursor, &sink, &alive);
        drop(sink);
        rx.try_iter().collect()
    }

    fn frames(events: &[SourceEvent]) -> Vec<&Frame> {
        events
            .iter()
            .filter_map(|e| match e {
                SourceEvent::Frame(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn default_cmdline_matches_the_python_argv() {
        let argv = build_cmdline(&base_cfg()).expect("cmdline");
        assert_eq!(
            argv,
            vec![
                "soapy_power",
                "-f",
                "87M:108M",
                "-B",
                "10k",
                "-T",
                "1",
                "-d",
                "driver=rtlsdr",
                "-r",
                "2560000",
                "-p",
                "0",
                "-F",
                "soapy_power_bin",
                "-c",
                "--even",
                "--fft-window",
                "boxcar",
                "--remove-dc",
            ]
        );
    }

    #[test]
    fn optional_arguments_are_appended_in_order() {
        let cfg = SweepConfig {
            start_freq_mhz: 10_000.0,
            stop_freq_mhz: 10_100.5,
            bin_size_khz: 100.0,
            interval_s: 2.5,
            gain_db: 30.0,
            ppm: -12,
            crop: 0.2,
            single_shot: true,
            device: "driver=lime".to_owned(),
            sample_rate: 10e6,
            bandwidth: 8e6,
            lnb_lo_hz: 9_750e6,
            executable: "/opt/bin/soapy_power --verbose".to_owned(),
            extra_params: String::new(),
        };

        assert_eq!(
            build_cmdline(&cfg).expect("cmdline"),
            vec![
                "/opt/bin/soapy_power",
                "--verbose",
                "-f",
                "10000M:10100.5M",
                "-B",
                "100k",
                "-T",
                "2.5",
                "-d",
                "driver=lime",
                "-r",
                "10000000",
                "-p",
                "-12",
                "-F",
                "soapy_power_bin",
                "--lnb-lo",
                "9750000000",
                "-w",
                "8000000",
                "-g",
                "30",
                "-k",
                "20",
            ]
        );
    }

    #[test]
    fn empty_executable_falls_back_to_the_default() {
        let cfg = SweepConfig {
            executable: "   ".to_owned(),
            ..base_cfg()
        };
        assert_eq!(
            build_cmdline(&cfg)
                .expect("cmdline")
                .first()
                .map(|s| s.as_str()),
            Some("soapy_power")
        );
    }

    #[test]
    fn auto_gain_and_zero_crop_add_no_arguments() {
        let argv = build_cmdline(&base_cfg()).expect("cmdline");
        assert!(!argv.iter().any(|a| a == "-g"));
        assert!(!argv.iter().any(|a| a == "-k"));
        assert!(!argv.iter().any(|a| a == "--lnb-lo"));
        assert!(!argv.iter().any(|a| a == "--output-fd"));
    }

    #[test]
    fn degenerate_spans_are_refused() {
        let flat = SweepConfig {
            stop_freq_mhz: 87.0,
            ..base_cfg()
        };
        assert!(matches!(
            build_cmdline(&flat),
            Err(SourceError::InvalidConfig(_))
        ));

        let no_bins = SweepConfig {
            bin_size_khz: 0.0,
            ..base_cfg()
        };
        assert!(matches!(
            build_cmdline(&no_bins),
            Err(SourceError::InvalidConfig(_))
        ));
    }

    #[test]
    fn number_formatting_is_short_and_exact() {
        assert_eq!(fmt_num(87.0), "87");
        assert_eq!(fmt_num(2_560_000.0), "2560000");
        assert_eq!(fmt_num(0.2 * 100.0), "20");
        assert_eq!(fmt_num(10_100.5), "10100.5");
        assert_eq!(fmt_num(-0.0), "0");
        assert_eq!(fmt_num(-12.25), "-12.25");
    }

    #[test]
    fn sweep_is_emitted_once_its_last_hop_arrives() {
        let stop_hz = 3e6;
        let first = test_header(1e6, 2e6, 4, 100.0);
        let last = test_header(2e6, 3e6, 4, 100.4);

        let mut stream = encode_record(&first, &[1.0, 2.0, 3.0, 4.0]);
        stream.extend(encode_record(&last, &[5.0, 6.0, 7.0, 8.0]));

        let events = run_parser(stop_hz, stream);
        let got = frames(&events);
        assert_eq!(got.len(), 1, "{events:?}");
        assert_eq!(got[0].y, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(got[0].x.len(), 8);
        assert_eq!(got[0].x[0], 1e6);
        assert_eq!(got[0].x[7], 3e6);
        // The first hop's stop time stamps the sweep.
        assert_eq!(got[0].timestamp, 100.0);
    }

    #[test]
    fn repeated_sweeps_share_one_axis() {
        let stop_hz = 2e6;
        let a = test_header(1e6, 2e6, 4, 10.0);
        let b = test_header(1e6, 2e6, 4, 11.0);

        let mut stream = encode_record(&a, &[1.0, 2.0, 3.0, 4.0]);
        stream.extend(encode_record(&b, &[5.0, 6.0, 7.0, 8.0]));

        let events = run_parser(stop_hz, stream);
        let got = frames(&events);
        assert_eq!(got.len(), 2);
        assert!(Arc::ptr_eq(&got[0].x, &got[1].x));
        assert_eq!(got[1].y, vec![5.0, 6.0, 7.0, 8.0]);
    }

    #[test]
    fn hop_short_of_the_stop_frequency_emits_nothing() {
        // Only the lower half of a 1..3 MHz sweep arrives.
        let hop = test_header(1e6, 2e6, 4, 1.0);
        let events = run_parser(3e6, encode_record(&hop, &[1.0, 2.0, 3.0, 4.0]));
        assert!(frames(&events).is_empty(), "{events:?}");
    }

    #[test]
    fn mismatched_record_is_logged_and_skipped() {
        let stop_hz = 2e6;
        let mut bad = test_header(1e6, 2e6, 4, 1.0);
        bad.size = 8;
        let good = test_header(1e6, 2e6, 4, 2.0);

        let mut stream = encode_record(&bad, &[9.0, 9.0]);
        stream.extend(encode_record(&good, &[1.0, 2.0, 3.0, 4.0]));

        let events = run_parser(stop_hz, stream);
        assert!(events
            .iter()
            .any(|e| matches!(e, SourceEvent::Log(m) if m.contains("Discarding hop"))));
        let got = frames(&events);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].y, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn empty_stream_produces_no_events() {
        assert!(run_parser(2e6, Vec::new()).is_empty());
    }

    #[test]
    fn corrupt_stream_reports_an_error_and_stops() {
        let good = test_header(1e6, 2e6, 4, 1.0);
        let mut stream = encode_record(&good, &[1.0, 2.0, 3.0, 4.0]);
        stream.extend_from_slice(b"GARBAGE-NOT-A-RECORD");
        let after = encode_record(&good, &[1.0, 2.0, 3.0, 4.0]);
        stream.extend_from_slice(&after);

        let events = run_parser(2e6, stream);
        assert_eq!(frames(&events).len(), 1, "reading must stop at the garbage");
        assert!(events.iter().any(|e| matches!(e, SourceEvent::Error(_))));
    }

    #[test]
    fn truncated_final_record_reports_an_error() {
        let h = test_header(1e6, 2e6, 4, 1.0);
        let full = encode_record(&h, &[1.0, 2.0, 3.0, 4.0]);
        let events = run_parser(2e6, full[..full.len() - 6].to_vec());
        assert!(frames(&events).is_empty());
        assert!(events.iter().any(|e| matches!(e, SourceEvent::Error(_))));
    }

    #[test]
    fn declared_limits_match_the_python_info_class() {
        let l = INFO.limits;
        assert_eq!(l.sample_rate, Limit::new(0.0, 61_440_000.0, 2_560_000.0));
        assert_eq!(l.bandwidth.max, 61_440_000.0);
        assert_eq!(l.gain, Limit::new(-1.0, 999.0, 37.0));
        assert_eq!(l.start_freq.max, 7250.0);
        assert_eq!(l.stop_freq.default, 108.0);
        assert_eq!(l.bin_size.max, 10_000.0);
        assert_eq!(l.interval, Limits::DEFAULT.interval);
        assert_eq!(l.ppm, Limits::DEFAULT.ppm);
        assert_eq!(l.crop, Limits::DEFAULT.crop);
        assert!(INFO.has_device_help);
    }
}
