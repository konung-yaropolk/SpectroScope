//! Diagnostic: drive the real `rtl_tcp` backend against a live server and
//! report what each sweep actually contains.
//!
//! Usage:
//!   cargo run --release --example rtl_tcp_probe -- <host[:port]> [frames]
//!
//! Prints, per frame: level statistics, the strongest peaks, and how well the
//! frame correlates with the previous one. A real spectrum correlates with
//! itself across sweeps; noise does not.

use std::sync::mpsc;
use std::time::Instant;

use spectroscope::sources::{find, EventSink, SourceEvent, SweepConfig};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let device = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:1234".to_owned());
    let want_frames: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(4);

    let backend = std::env::var("BACKEND").unwrap_or_else(|_| "rtl_tcp".to_owned());
    let single_shot = std::env::var("SINGLE").is_ok();
    // A station known to be on the air, to check the sweep puts it where it
    // belongs rather than merely producing peaks.
    let expect_mhz = std::env::var("EXPECT")
        .ok()
        .and_then(|v| v.parse::<f64>().ok());

    let cfg = SweepConfig {
        start_freq_mhz: env_f64("START", 87.0),
        stop_freq_mhz: env_f64("STOP", 108.0),
        bin_size_khz: env_f64("BIN", 10.0),
        interval_s: env_f64("INTERVAL", 1.0),
        gain_db: env_f64("GAIN", 37.0),
        crop: env_f64("CROP", 0.0),
        sample_rate: env_f64("RATE", 2_048_000.0),
        ppm: env_f64("PPM", 0.0) as i32,
        lnb_lo_hz: env_f64("LNB", 0.0),
        single_shot,
        device,
        ..Default::default()
    };
    eprintln!(
        "config: {}-{} MHz, bin {} kHz, rate {} Hz, gain {} dB, crop {}, interval {} s",
        cfg.start_freq_mhz,
        cfg.stop_freq_mhz,
        cfg.bin_size_khz,
        cfg.sample_rate,
        cfg.gain_db,
        cfg.crop,
        cfg.interval_s
    );

    let source = find(&backend).unwrap_or_else(|| panic!("no backend {backend:?}"));
    let (tx, rx) = crossbeam_channel::unbounded();
    let sink = EventSink::new(tx, None);

    let started = Instant::now();
    let mut session = match source.start(cfg, sink) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("start failed: {e}");
            std::process::exit(1);
        }
    };

    let mut frames = 0usize;
    let mut previous: Option<Vec<f32>> = None;
    let (_done_tx, _done_rx) = mpsc::channel::<()>();

    while frames < want_frames {
        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(SourceEvent::Started { hops }) => eprintln!("started: {hops} hops"),
            Ok(SourceEvent::Log(m)) => eprintln!("log: {m}"),
            // Not fatal: the application shows these in the status bar and
            // keeps acquiring, so the probe has to carry on too or it would
            // never see what the warning is warning about.
            Ok(SourceEvent::Error(m)) => eprintln!("error: {m}"),
            Ok(SourceEvent::Stopped) => {
                eprintln!("stopped");
                break;
            }
            Ok(SourceEvent::Frame(frame)) => {
                frames += 1;
                report(frames, started, &frame.x, &frame.y, previous.as_deref());
                if let Some(want) = expect_mhz {
                    check_station(want, &frame.x, &frame.y);
                }
                previous = Some(frame.y.clone());
            }
            Err(e) => {
                eprintln!("no frame: {e}");
                break;
            }
        }
    }

    session.stop();
}

fn env_f64(key: &str, fallback: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

fn report(n: usize, started: Instant, x: &[f64], y: &[f32], previous: Option<&[f32]>) {
    let mut sorted = y.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2];
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];

    let correlation = previous
        .filter(|p| p.len() == y.len())
        .map(|p| pearson(p, y))
        .unwrap_or(f64::NAN);

    println!(
        "\n--- frame {n} at {:.2}s --- bins {}, {:.3}-{:.3} MHz",
        started.elapsed().as_secs_f64(),
        y.len(),
        x.first().copied().unwrap_or(0.0) / 1e6,
        x.last().copied().unwrap_or(0.0) / 1e6,
    );
    println!("levels: min {min:.1} dB, median {median:.1} dB, max {max:.1} dB");
    println!("correlation with previous frame: {correlation:.3}");

    // Peaks at least 10 dB over the median, thinned to one per 150 kHz.
    let mut peaks: Vec<(f64, f32)> = Vec::new();
    for (i, v) in y.iter().enumerate() {
        if *v - median < 10.0 {
            continue;
        }
        let f = x.get(i).copied().unwrap_or(0.0);
        match peaks.last_mut() {
            Some(last) if (f - last.0).abs() < 150e3 => {
                if *v > last.1 {
                    *last = (f, *v);
                }
            }
            _ => peaks.push((f, *v)),
        }
    }
    println!("peaks >10 dB over median: {}", peaks.len());
    for (f, v) in peaks.iter().take(40) {
        println!("   {:8.3} MHz  {:7.1} dB  (+{:.1})", f / 1e6, v, v - median);
    }
}

/// Does the sweep put a station known to be on the air where it belongs?
///
/// Peaks alone prove nothing -- a scrambled sweep is full of them. This looks
/// for the strongest bin within 300 kHz of a frequency the radio is known to
/// carry and reports both how far off it landed and how far it stands above the
/// sweep's own median.
fn check_station(want_mhz: f64, x: &[f64], y: &[f32]) {
    let want = want_mhz * 1e6;
    let mut best: Option<(f64, f32)> = None;
    for (i, v) in y.iter().enumerate() {
        let f = match x.get(i) {
            Some(f) if (f - want).abs() <= 300e3 => *f,
            _ => continue,
        };
        if best.is_none_or(|(_, bv)| *v > bv) {
            best = Some((f, *v));
        }
    }
    let mut sorted = y.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2];

    match best {
        Some((f, v)) => println!(
            "  station check {want_mhz:.3} MHz -> found at {:.3} MHz ({:+.0} kHz), \
             {:.1} dB, {:+.1} dB over median  [{}]",
            f / 1e6,
            (f - want) / 1e3,
            v,
            v - median,
            if v - median >= 15.0 { "OK" } else { "WEAK" }
        ),
        None => println!("  station check {want_mhz:.3} MHz -> outside the swept span"),
    }
}

fn pearson(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len() as f64;
    let ma = a.iter().map(|v| *v as f64).sum::<f64>() / n;
    let mb = b.iter().map(|v| *v as f64).sum::<f64>() / n;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for (p, q) in a.iter().zip(b.iter()) {
        let u = *p as f64 - ma;
        let v = *q as f64 - mb;
        num += u * v;
        da += u * u;
        db += v * v;
    }
    if da <= 0.0 || db <= 0.0 {
        return f64::NAN;
    }
    num / (da.sqrt() * db.sqrt())
}
