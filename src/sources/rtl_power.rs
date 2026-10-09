//! The `rtl_power` backend, replacing `qspectrumanalyzer/backends/rtl_power.py`.
//!
//! The CSV stitcher lives here but is shared with [`super::rx_power`], whose
//! helper emits byte-identical output.

use std::sync::Arc;

use super::process::{self, LineParser};
use super::{
    linspace, EventSink, Frame, Limits, SourceError, SourceInfo, SourceKind, SourceSession,
    SpectrumSource, SweepConfig,
};
use crate::util::{now_unix, split_args};

static INFO: SourceInfo = SourceInfo {
    id: "rtl_power",
    label: "rtl_power (RTL-SDR)",
    kind: SourceKind::Process,
    default_executable: "rtl_power",
    additional_params: "",
    has_device_help: false,
    limits: Limits::DEFAULT,
};

/// `rtl_power` refuses to tune wider than this per hop.
const MAX_BIN_SIZE_KHZ: f64 = 2800.0;

pub struct RtlPower;

impl SpectrumSource for RtlPower {
    fn info(&self) -> &'static SourceInfo {
        &INFO
    }

    fn start(
        &self,
        cfg: SweepConfig,
        sink: EventSink,
    ) -> Result<Box<dyn SourceSession>, SourceError> {
        process::spawn_lines(cmdline(&cfg), sink, Box::new(CsvStitcher::new(&cfg)), 0)
    }
}

/// The `rtl_power` command line, argument for argument as the Python built it.
fn cmdline(cfg: &SweepConfig) -> Vec<String> {
    let mut cmd = split_args(&cfg.executable);
    if cmd.is_empty() {
        cmd.push(INFO.default_executable.to_owned());
    }

    let bin_khz = cfg.bin_size_khz.min(MAX_BIN_SIZE_KHZ);
    cmd.push("-f".to_owned());
    cmd.push(format!(
        "{}M:{}M:{}k",
        tuner_start_mhz(cfg),
        tuner_stop_mhz(cfg),
        bin_khz
    ));
    cmd.push("-i".to_owned());
    cmd.push(cfg.interval_s.to_string());
    cmd.push("-d".to_owned());
    cmd.push(cfg.device.clone());
    cmd.push("-p".to_owned());
    cmd.push(cfg.ppm.to_string());
    // `-c` wants the same 0.0..=1.0 fraction the GUI stores, not a percentage.
    cmd.push("-c".to_owned());
    cmd.push(cfg.crop.to_string());

    if cfg.sample_rate > 0.0 {
        cmd.push("-r".to_owned());
        cmd.push(format!("{}M", cfg.sample_rate / 1e6));
    }
    if cfg.gain_db >= 0.0 {
        cmd.push("-g".to_owned());
        cmd.push(cfg.gain_db.to_string());
    }
    if cfg.single_shot {
        cmd.push("-1".to_owned());
    }
    cmd.extend(split_args(&cfg.extra_params));
    cmd
}

/// Tuner frequencies for the `-f` argument.
///
/// The LNB LO is removed in MHz rather than via [`SweepConfig::tuner_start_hz`]
/// so that a frequency the user typed survives into the command line as typed:
/// a round trip through Hz turns `433.92` into `433.92000000000007`.
pub(crate) fn tuner_start_mhz(cfg: &SweepConfig) -> f64 {
    cfg.start_freq_mhz - cfg.lnb_lo_hz / 1e6
}

pub(crate) fn tuner_stop_mhz(cfg: &SweepConfig) -> f64 {
    cfg.stop_freq_mhz - cfg.lnb_lo_hz / 1e6
}

// ---------------------------------------------------------------------------
// CSV stitching
// ---------------------------------------------------------------------------

/// Upper bound on the point count a single line may ask for.
///
/// The count is arithmetic over three parsed numbers, so a corrupted line could
/// otherwise demand an unbounded allocation. No real chunk comes close: a hop
/// spans at most one sample rate's worth of bins.
const MAX_CHUNK_BINS: f64 = 16_777_216.0;

/// How many unparseable lines get reported before the parser goes quiet.
const MAX_REPORTED_BAD_LINES: u32 = 5;

/// Accumulates `rtl_power`-format CSV into whole sweeps.
///
/// The helper prints one line per frequency chunk, and every chunk of a sweep
/// repeats the same `date time` prefix, so a change of prefix begins a new
/// sweep. Line layout:
///
/// ```text
/// date, time, start_hz, stop_hz, step_hz, samples, db, db, db, ...
/// ```
pub(crate) struct CsvStitcher {
    /// Configured tuner stop frequency in Hz; the end-of-sweep test needs it.
    stop_hz: f64,
    lnb_lo_hz: f64,
    /// The raw `date time` prefix, used only to spot a sweep boundary. It is
    /// never converted to a time: frame timestamps come from the local clock.
    last_label: String,
    x: Vec<f64>,
    y: Vec<f32>,
    /// Every sweep of a run has the same axis, so the `Arc` of the previous one
    /// is reused and the history stops holding a private copy per frame.
    axis: Option<Arc<Vec<f64>>>,
    bad_lines: u32,
}

impl CsvStitcher {
    pub(crate) fn new(cfg: &SweepConfig) -> Self {
        Self {
            stop_hz: tuner_stop_mhz(cfg) * 1e6,
            lnb_lo_hz: cfg.lnb_lo_hz,
            last_label: String::new(),
            x: Vec::new(),
            y: Vec::new(),
            axis: None,
            bad_lines: 0,
        }
    }

    fn emit(&mut self, sink: &EventSink) {
        let axis = match &self.axis {
            Some(a) if a.as_slice() == self.x.as_slice() => Arc::clone(a),
            _ => {
                let fresh = Arc::new(self.x.clone());
                self.axis = Some(Arc::clone(&fresh));
                fresh
            }
        };
        sink.frame(Frame {
            timestamp: now_unix(),
            x: axis,
            y: self.y.clone(),
        });
    }

    fn report_bad_line(&mut self, line: &str, reason: &str, sink: &EventSink) {
        self.bad_lines += 1;
        if self.bad_lines <= MAX_REPORTED_BAD_LINES {
            sink.log(format!("Ignoring backend output ({reason}): {line}"));
            if self.bad_lines == MAX_REPORTED_BAD_LINES {
                sink.log("Further unparseable backend output will not be reported.");
            }
        }
    }
}

impl LineParser for CsvStitcher {
    fn feed(&mut self, line: &str, sink: &EventSink) {
        if line.trim().is_empty() {
            return;
        }

        let chunk = match parse_line(line, self.lnb_lo_hz) {
            Ok(c) => c,
            Err(reason) => return self.report_bad_line(line, reason, sink),
        };

        if let Some((nx, ny)) = chunk.mismatch {
            sink.log(format!(
                "Backend reported {nx} frequencies but {ny} power values; \
                 trimming to {} -- use a newer rtl_power",
                nx.min(ny)
            ));
        }

        if chunk.label != self.last_label {
            self.last_label = chunk.label;
            self.x = chunk.x;
            self.y = chunk.y;
        } else {
            self.x.extend_from_slice(&chunk.x);
            self.y.extend_from_slice(&chunk.y);
        }

        // Deliberately `>` against one step short of the end rather than an
        // equality: some rtl_power releases stop a fraction of a step before
        // the requested stop frequency, and an `==` test would never fire for
        // them, so no sweep would ever be emitted.
        if chunk.stop_hz > self.stop_hz - chunk.step_hz {
            self.emit(sink);
        }
    }
}

/// One parsed CSV line.
struct Chunk {
    label: String,
    stop_hz: f64,
    step_hz: f64,
    x: Vec<f64>,
    y: Vec<f32>,
    /// Pre-trim `(x, y)` lengths when the helper disagreed with itself.
    mismatch: Option<(usize, usize)>,
}

fn parse_line(line: &str, lnb_lo_hz: f64) -> Result<Chunk, &'static str> {
    let cols: Vec<&str> = line.split(',').map(str::trim).collect();
    if cols.len() < 6 {
        return Err("too few fields");
    }

    let start_hz: f64 = cols[2].parse().map_err(|_| "unparseable start frequency")?;
    let stop_hz: f64 = cols[3].parse().map_err(|_| "unparseable stop frequency")?;
    let step_hz: f64 = cols[4].parse().map_err(|_| "unparseable frequency step")?;
    // The sample count is unused, but a line that lacks it is not a record.
    let _samples: f64 = cols[5].parse().map_err(|_| "unparseable sample count")?;

    if !start_hz.is_finite() || !stop_hz.is_finite() {
        return Err("non-finite frequency");
    }
    if !step_hz.is_finite() || step_hz == 0.0 {
        return Err("zero frequency step");
    }

    // The point count comes from the rounded span-to-step ratio, not from how
    // many power values the line actually carries.
    let bins = ((stop_hz - start_hz) / step_hz).round();
    if !bins.is_finite() || bins > MAX_CHUNK_BINS {
        return Err("implausible bin count");
    }
    let bins = if bins > 0.0 { bins as usize } else { 0 };

    let mut powers = &cols[6..];
    // Tolerate a trailing separator rather than throwing the whole sweep away.
    if powers.last() == Some(&"") {
        powers = &powers[..powers.len() - 1];
    }

    let mut x = linspace(start_hz + lnb_lo_hz, stop_hz + lnb_lo_hz, bins);
    let mut y = Vec::with_capacity(powers.len());
    for col in powers {
        y.push(col.parse::<f32>().map_err(|_| "unparseable power value")?);
    }

    let mismatch = if x.len() == y.len() {
        None
    } else {
        let pre = (x.len(), y.len());
        let keep = x.len().min(y.len());
        x.truncate(keep);
        y.truncate(keep);
        Some(pre)
    };

    Ok(Chunk {
        label: format!("{} {}", cols[0], cols[1]),
        stop_hz,
        step_hz,
        x,
        y,
        mismatch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceEvent;
    use crossbeam_channel::{unbounded, Receiver};

    fn sink() -> (EventSink, Receiver<SourceEvent>) {
        let (tx, rx) = unbounded();
        (EventSink::new(tx, None), rx)
    }

    /// Takes everything queued; a receiver cannot be read twice, so frames and
    /// logs are both pulled out of one drain.
    fn drain(rx: &Receiver<SourceEvent>) -> Vec<SourceEvent> {
        rx.try_iter().collect()
    }

    fn frames(events: &[SourceEvent]) -> Vec<Frame> {
        events
            .iter()
            .filter_map(|e| match e {
                SourceEvent::Frame(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }

    fn logs(events: &[SourceEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                SourceEvent::Log(m) => Some(m.clone()),
                _ => None,
            })
            .collect()
    }

    /// 88-90 MHz in two 1 MHz chunks of four 250 kHz bins each.
    fn two_chunk_sweep(stamp: &str) -> [String; 2] {
        [
            format!("{stamp}, 88000000, 89000000, 250000.00, 20, -30.00, -31.00, -32.00, -33.00"),
            format!("{stamp}, 89000000, 90000000, 250000.00, 20, -40.00, -41.00, -42.00, -43.00"),
        ]
    }

    fn fm_cfg() -> SweepConfig {
        SweepConfig {
            start_freq_mhz: 88.0,
            stop_freq_mhz: 90.0,
            bin_size_khz: 250.0,
            ..Default::default()
        }
    }

    #[test]
    fn stitches_chunks_into_one_sweep() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        for line in two_chunk_sweep("2024-03-01, 12:00:00") {
            p.feed(&line, &s);
        }
        p.finish(&s);

        let f = frames(&drain(&rx));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].x.len(), 8);
        assert_eq!(f[0].y.len(), 8);
        assert!((f[0].x[0] - 88e6).abs() < 1e-6);
        assert!((f[0].x[7] - 90e6).abs() < 1e-6);
        assert_eq!(f[0].y[0], -30.0);
        assert_eq!(f[0].y[7], -43.0);
        assert!(f[0].timestamp > 0.0);
    }

    #[test]
    fn a_new_timestamp_starts_a_new_sweep() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        for stamp in ["2024-03-01, 12:00:00", "2024-03-01, 12:00:01"] {
            for line in two_chunk_sweep(stamp) {
                p.feed(&line, &s);
            }
        }

        let f = frames(&drain(&rx));
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].x.len(), 8);
        assert_eq!(f[1].x.len(), 8);
        // The axis is identical across sweeps, so the `Arc` must be shared.
        assert!(Arc::ptr_eq(&f[0].x, &f[1].x));
    }

    #[test]
    fn partial_sweep_emits_nothing() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        p.feed(&two_chunk_sweep("2024-03-01, 12:00:00")[0], &s);
        p.finish(&s);
        assert!(frames(&drain(&rx)).is_empty());
    }

    #[test]
    fn lnb_offset_shifts_the_axis() {
        let cfg = SweepConfig {
            start_freq_mhz: 9838.0,
            stop_freq_mhz: 9840.0,
            bin_size_khz: 250.0,
            lnb_lo_hz: 9_750e6,
            ..Default::default()
        };
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        for line in two_chunk_sweep("2024-03-01, 12:00:00") {
            p.feed(&line, &s);
        }

        let f = frames(&drain(&rx));
        assert_eq!(f.len(), 1);
        assert!((f[0].x[0] - 9_838e6).abs() < 1e-3);
        assert!((f[0].x[7] - 9_840e6).abs() < 1e-3);
    }

    #[test]
    fn end_of_sweep_tolerates_a_short_final_chunk() {
        // A chunk ending one step early must still close the sweep; this is the
        // broken-rtl_power case the `>` comparison exists for.
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        p.feed(
            "2024-03-01, 12:00:00, 88000000, 89800000, 250000.00, 20, -30.00, \
             -31.00, -32.00, -33.00, -34.00, -35.00, -36.00",
            &s,
        );
        assert_eq!(frames(&drain(&rx)).len(), 1);
    }

    #[test]
    fn mismatched_lengths_are_trimmed_and_logged() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        // Four bins claimed, two powers given, and the chunk reaches the end.
        p.feed(
            "2024-03-01, 12:00:00, 88000000, 90000000, 500000.00, 20, -30.00, -31.00",
            &s,
        );

        let events = drain(&rx);
        let f = frames(&events);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].x.len(), 2);
        assert_eq!(f[0].y.len(), 2);
        let msgs = logs(&events);
        assert!(msgs.iter().any(|m| m.contains("trimming")), "{msgs:?}");
    }

    #[test]
    fn surplus_powers_are_trimmed() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        p.feed(
            "2024-03-01, 12:00:00, 88000000, 90000000, 1000000.00, 20, -30.00, \
             -31.00, -32.00, -33.00",
            &s,
        );
        let f = frames(&drain(&rx));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].x.len(), 2);
        assert_eq!(f[0].y, vec![-30.0, -31.0]);
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        for line in [
            "",
            "   ",
            "garbage",
            "2024-03-01, 12:00:00, 88000000",
            "2024-03-01, 12:00:00, eighty-eight, 90000000, 250000.00, 20, -30.00",
            "2024-03-01, 12:00:00, 88000000, 90000000, 0.00, 20, -30.00",
            "2024-03-01, 12:00:00, 88000000, 90000000, 250000.00, nope, -30.00",
            "2024-03-01, 12:00:00, 88000000, 90000000, 250000.00, 20, -30.00, oops",
            "rtl_power banner line without commas",
        ] {
            p.feed(line, &s);
        }
        p.finish(&s);
        assert!(frames(&drain(&rx)).is_empty());
    }

    #[test]
    fn empty_input_produces_nothing() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        p.finish(&s);
        assert!(rx.try_iter().next().is_none());
    }

    #[test]
    fn bad_line_reports_are_capped() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        for _ in 0..50 {
            p.feed("garbage", &s);
        }
        assert!(logs(&drain(&rx)).len() <= MAX_REPORTED_BAD_LINES as usize + 1);
    }

    #[test]
    fn nan_powers_survive_parsing() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        p.feed(
            "2024-03-01, 12:00:00, 88000000, 90000000, 1000000.00, 20, nan, -inf",
            &s,
        );
        let f = frames(&drain(&rx));
        assert_eq!(f.len(), 1);
        assert!(f[0].y[0].is_nan());
        assert!(f[0].y[1].is_infinite());
    }

    #[test]
    fn zero_span_emits_an_empty_frame_without_panicking() {
        let cfg = SweepConfig {
            start_freq_mhz: 88.0,
            stop_freq_mhz: 88.0,
            bin_size_khz: 250.0,
            ..Default::default()
        };
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        p.feed(
            "2024-03-01, 12:00:00, 88000000, 88000000, 250000.00, 20",
            &s,
        );
        let f = frames(&drain(&rx));
        assert_eq!(f.len(), 1);
        assert!(f[0].x.is_empty());
        assert!(f[0].y.is_empty());
    }

    #[test]
    fn implausible_bin_count_is_rejected() {
        let cfg = fm_cfg();
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg);
        p.feed(
            "2024-03-01, 12:00:00, 0, 90000000000, 0.001, 20, -30.00",
            &s,
        );
        assert!(frames(&drain(&rx)).is_empty());
    }

    #[test]
    fn command_line_matches_the_python_order() {
        let cfg = SweepConfig {
            start_freq_mhz: 88.0,
            stop_freq_mhz: 108.0,
            bin_size_khz: 10.0,
            interval_s: 1.0,
            gain_db: 37.0,
            ppm: 12,
            crop: 0.2,
            device: "0".to_owned(),
            sample_rate: 2_560_000.0,
            executable: "rtl_power".to_owned(),
            extra_params: "--extra \"a b\"".to_owned(),
            single_shot: true,
            ..Default::default()
        };
        assert_eq!(
            cmdline(&cfg),
            vec![
                "rtl_power",
                "-f",
                "88M:108M:10k",
                "-i",
                "1",
                "-d",
                "0",
                "-p",
                "12",
                "-c",
                "0.2",
                "-r",
                "2.56M",
                "-g",
                "37",
                "-1",
                "--extra",
                "a b",
            ]
        );
    }

    #[test]
    fn auto_gain_and_zero_sample_rate_drop_their_flags() {
        let cfg = SweepConfig {
            gain_db: -1.0,
            sample_rate: 0.0,
            executable: "rtl_power".to_owned(),
            ..Default::default()
        };
        let cmd = cmdline(&cfg);
        assert!(!cmd.iter().any(|a| a == "-g"), "{cmd:?}");
        assert!(!cmd.iter().any(|a| a == "-r"), "{cmd:?}");
        assert!(!cmd.iter().any(|a| a == "-1"), "{cmd:?}");
    }

    #[test]
    fn bin_size_is_clamped_to_the_tuner_limit() {
        let cfg = SweepConfig {
            bin_size_khz: 5000.0,
            executable: "rtl_power".to_owned(),
            ..Default::default()
        };
        assert!(cmdline(&cfg).iter().any(|a| a.ends_with(":2800k")));
    }

    #[test]
    fn lnb_offset_is_removed_from_the_command_line() {
        let cfg = SweepConfig {
            start_freq_mhz: 9838.0,
            stop_freq_mhz: 9840.0,
            bin_size_khz: 100.0,
            lnb_lo_hz: 9_750e6,
            executable: "rtl_power".to_owned(),
            ..Default::default()
        };
        let cmd = cmdline(&cfg);
        assert!(cmd.iter().any(|a| a == "88M:90M:100k"), "{cmd:?}");
    }

    #[test]
    fn empty_executable_falls_back_to_the_default() {
        let cmd = cmdline(&SweepConfig::default());
        assert_eq!(cmd.first().map(String::as_str), Some("rtl_power"));
    }
}
