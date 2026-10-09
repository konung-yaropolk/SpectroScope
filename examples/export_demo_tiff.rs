//! Capture a few demo sweeps and write them out, for eyeballing the result in
//! ImageJ.
//!
//! `cargo run --example export_demo_tiff -- out.tif`

use std::time::{Duration, Instant};

use spectroscope::data::{image_export, HistoryBuffer};
use spectroscope::sources::{self, EventSink, SourceEvent, SweepConfig};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "waterfall.tif".to_owned());

    let demo = sources::find("demo").expect("demo source");
    let cfg = SweepConfig {
        start_freq_mhz: 87.0,
        stop_freq_mhz: 108.0,
        bin_size_khz: 50.0,
        interval_s: 0.01,
        ..Default::default()
    };

    let (tx, rx) = crossbeam_channel::unbounded();
    let mut session = demo
        .start(cfg, EventSink::new(tx, None))
        .expect("demo starts");

    let mut history = HistoryBuffer::default();
    let mut axis: Option<std::sync::Arc<Vec<f64>>> = None;
    let want = 120;
    let deadline = Instant::now() + Duration::from_secs(60);

    while history.len() < want && Instant::now() < deadline {
        if let Ok(SourceEvent::Frame(f)) = rx.recv_timeout(Duration::from_millis(500)) {
            if history.bins() != f.y.len() {
                history = HistoryBuffer::new(f.y.len(), want);
            }
            history.append(&f.y);
            axis.get_or_insert(f.x);
        }
    }
    session.stop();

    let axes = match axis.as_ref() {
        Some(x) if x.len() > 1 => image_export::Axes {
            start_hz: x[0],
            bin_hz: (x[x.len() - 1] - x[0]) / (x.len() - 1) as f64,
            sweep_s: 0.01,
        },
        _ => image_export::Axes::default(),
    };

    let lut = spectroscope::colormap::LUTS[4]; // Turbo
    match image_export::save(path.as_ref(), &history, &lut, -100.0, -20.0, axes) {
        Ok((w, h)) => println!("wrote {path}: {w} x {h}, {} sweeps", history.len()),
        Err(e) => eprintln!("export failed: {e}"),
    }
}
