//! The `rx_power` backend (rx_tools), replacing
//! `qspectrumanalyzer/backends/rx_power.py`.
//!
//! Output is byte-identical to `rtl_power`, so the CSV stitching lives in
//! [`super::rtl_power`]; only the limits and the command line differ -- notably
//! `rx_power` picks its own sample rate and rejects `-r`.

use super::process;
use super::rtl_power::{tuner_start_mhz, tuner_stop_mhz, CsvStitcher};
use super::{
    EventSink, Limit, Limits, SourceError, SourceInfo, SourceKind, SourceSession, SpectrumSource,
    SweepConfig,
};
use crate::util::split_args;

static INFO: SourceInfo = SourceInfo {
    id: "rx_power",
    label: "rx_power (rx_tools)",
    kind: SourceKind::Process,
    default_executable: "rx_power",
    additional_params: "",
    has_device_help: false,
    limits: Limits {
        // rx_power derives the sample rate from the bin size itself, so the GUI
        // must offer nothing to set: a fixed limit hides the control.
        sample_rate: Limit::new(0.0, 0.0, 0.0),
        gain: Limit::new(-1.0, 999.0, 37.0),
        start_freq: Limit::new(0.0, 7250.0, 87.0),
        stop_freq: Limit::new(0.0, 7250.0, 108.0),
        bin_size: Limit::new(0.0, 2800.0, 10.0),
        ..Limits::DEFAULT
    },
};

pub struct RxPower;

impl SpectrumSource for RxPower {
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

/// The `rx_power` command line: `rtl_power`'s, minus `-r`, and with no bin-size
/// clamp because the limit is already enforced by the GUI's declared maximum.
fn cmdline(cfg: &SweepConfig) -> Vec<String> {
    let mut cmd = split_args(&cfg.executable);
    if cmd.is_empty() {
        cmd.push(INFO.default_executable.to_owned());
    }

    cmd.push("-f".to_owned());
    cmd.push(format!(
        "{}M:{}M:{}k",
        tuner_start_mhz(cfg),
        tuner_stop_mhz(cfg),
        cfg.bin_size_khz
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::process::LineParser;
    use crate::sources::{Frame, SourceEvent};
    use crossbeam_channel::{unbounded, Receiver};

    fn sink() -> (EventSink, Receiver<SourceEvent>) {
        let (tx, rx) = unbounded();
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

    fn cfg_2_4g() -> SweepConfig {
        SweepConfig {
            start_freq_mhz: 2400.0,
            stop_freq_mhz: 2402.0,
            bin_size_khz: 500.0,
            device: "0".to_owned(),
            executable: "rx_power".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn shares_the_rtl_power_parser() {
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg_2_4g());
        p.feed(
            "2024-03-01, 12:00:00, 2400000000, 2401000000, 500000.00, 16, -70.00, -71.00",
            &s,
        );
        p.feed(
            "2024-03-01, 12:00:00, 2401000000, 2402000000, 500000.00, 16, -72.00, -73.00",
            &s,
        );

        let f = frames(&rx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].y, vec![-70.0, -71.0, -72.0, -73.0]);
        assert!((f[0].x[0] - 2_400e6).abs() < 1e-3);
        assert!((f[0].x[3] - 2_402e6).abs() < 1e-3);
    }

    #[test]
    fn empty_input_produces_nothing() {
        let (s, rx) = sink();
        let mut p = CsvStitcher::new(&cfg_2_4g());
        p.feed("", &s);
        p.finish(&s);
        assert!(rx.try_iter().next().is_none());
    }

    #[test]
    fn command_line_has_no_sample_rate_flag() {
        let cfg = SweepConfig {
            gain_db: 40.0,
            sample_rate: 2_560_000.0,
            single_shot: true,
            ppm: -3,
            crop: 0.5,
            interval_s: 2.0,
            extra_params: "-F 9".to_owned(),
            ..cfg_2_4g()
        };
        assert_eq!(
            cmdline(&cfg),
            vec![
                "rx_power",
                "-f",
                "2400M:2402M:500k",
                "-i",
                "2",
                "-d",
                "0",
                "-p",
                "-3",
                "-c",
                "0.5",
                "-g",
                "40",
                "-1",
                "-F",
                "9",
            ]
        );
    }

    #[test]
    fn auto_gain_drops_the_gain_flag() {
        let cfg = SweepConfig {
            gain_db: -1.0,
            ..cfg_2_4g()
        };
        let cmd = cmdline(&cfg);
        assert!(!cmd.iter().any(|a| a == "-g"), "{cmd:?}");
    }

    #[test]
    fn lnb_offset_is_removed_from_the_command_line() {
        let cfg = SweepConfig {
            start_freq_mhz: 10_000.0,
            stop_freq_mhz: 10_100.0,
            lnb_lo_hz: 9_750e6,
            ..cfg_2_4g()
        };
        assert!(cmdline(&cfg).iter().any(|a| a == "250M:350M:500k"));
    }

    #[test]
    fn empty_executable_falls_back_to_the_default() {
        let cfg = SweepConfig {
            executable: String::new(),
            ..cfg_2_4g()
        };
        assert_eq!(cmdline(&cfg).first().map(String::as_str), Some("rx_power"));
    }

    #[test]
    fn limits_match_the_python_info_class() {
        let l = INFO.limits;
        assert!(l.sample_rate.is_fixed());
        assert_eq!(l.start_freq.max, 7250.0);
        assert_eq!(l.stop_freq.max, 7250.0);
        assert_eq!(l.gain.min, -1.0);
        assert_eq!(l.gain.max, 999.0);
        assert_eq!(l.bin_size.max, 2800.0);
        // Everything else is inherited from BaseInfo.
        assert_eq!(l.interval, Limits::DEFAULT.interval);
        assert_eq!(l.ppm, Limits::DEFAULT.ppm);
        assert_eq!(l.crop, Limits::DEFAULT.crop);
        assert_eq!(l.bandwidth, Limits::DEFAULT.bandwidth);
    }
}
