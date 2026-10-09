//! End-to-end: a real source's sweeps through the recorder and back off disk.
//!
//! The unit tests cover each piece against hand-built data; this one checks
//! that the pieces agree when the data comes from an actual backend, including
//! the case the format was chosen for -- a recording cut off mid-sweep.

use std::fs::File;
use std::time::{Duration, Instant};

use spectroscope::data::{soapy_bin, RecordFormat, Recorder};
use spectroscope::sources::{self, EventSink, Frame, SourceEvent, SweepConfig};

/// Run the demo backend until `want` sweeps have arrived, or time out.
fn capture(want: usize) -> Vec<Frame> {
    let demo = sources::find("demo").expect("the demo source is always compiled in");

    let cfg = SweepConfig {
        start_freq_mhz: 100.0,
        stop_freq_mhz: 102.0,
        bin_size_khz: 50.0,
        // Fast enough that the test is not dominated by waiting.
        interval_s: 0.02,
        ..Default::default()
    };

    let (tx, rx) = crossbeam_channel::unbounded();
    let mut session = demo
        .start(cfg, EventSink::new(tx, None))
        .expect("the demo source starts without hardware");

    let mut frames = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while frames.len() < want && Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(SourceEvent::Frame(f)) => frames.push(f),
            Ok(SourceEvent::Stopped) => break,
            Ok(_) => {}
            Err(_) => {}
        }
    }
    session.stop();
    frames
}

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("spectroscope-it-{name}-{}.bin", std::process::id()));
    p
}

#[test]
fn demo_sweeps_survive_a_recording_round_trip() {
    let frames = capture(5);
    assert!(frames.len() >= 2, "the demo produced no sweeps");

    let path = temp_path("roundtrip");
    let mut rec = Recorder::create(&path, RecordFormat::SoapyPowerBin).expect("create");
    for f in &frames {
        rec.write(f).expect("write");
    }
    assert_eq!(rec.sweeps(), frames.len() as u64);
    rec.finish().expect("finish");

    let mut file = File::open(&path).expect("open");
    let read_back = soapy_bin::read_frames(&mut file);

    assert_eq!(read_back.len(), frames.len(), "sweep count must match");
    for (i, (want, got)) in frames.iter().zip(read_back.iter()).enumerate() {
        assert_eq!(got.y.len(), want.y.len(), "sweep {i} bin count");
        // f32 is stored verbatim, so this is exact rather than approximate.
        assert_eq!(got.y, want.y, "sweep {i} values");
        // The axis is reconstructed from start/stop/step, so allow a bin edge.
        let step = (want.x[want.x.len() - 1] - want.x[0]) / want.x.len() as f64;
        assert!(
            (got.x[0] - want.x[0]).abs() <= step,
            "sweep {i} first bin: {} vs {}",
            got.x[0],
            want.x[0]
        );
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_recording_killed_mid_sweep_still_opens() {
    let frames = capture(4);
    assert!(frames.len() >= 3, "need a few sweeps to truncate between");

    let path = temp_path("killed");
    let mut rec = Recorder::create(&path, RecordFormat::SoapyPowerBin).expect("create");
    for f in &frames {
        rec.write(f).expect("write");
    }
    rec.finish().expect("finish");

    // Cut the file in the middle of its last record, the way a power cut would.
    let bytes = std::fs::read(&path).expect("read");
    let record_len = soapy_bin::RECORD_HEADER_LEN + frames[0].y.len() * 4;
    let complete = frames.len() - 1;
    std::fs::write(&path, &bytes[..record_len * complete + record_len / 3]).expect("truncate");

    let mut file = File::open(&path).expect("open");
    let read_back = soapy_bin::read_frames(&mut file);
    assert_eq!(
        read_back.len(),
        complete,
        "every complete sweep before the cut must still load"
    );
    assert_eq!(read_back[0].y, frames[0].y);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_recording_can_be_reloaded_as_a_baseline() {
    // Recording into the same container soapy_power writes is what makes this
    // work, and it is the main reason that format is the default.
    let frames = capture(3);
    assert!(!frames.is_empty());

    let path = temp_path("baseline");
    let mut rec = Recorder::create(&path, RecordFormat::SoapyPowerBin).expect("create");
    for f in &frames {
        rec.write(f).expect("write");
    }
    rec.finish().expect("finish");

    let mut file = File::open(&path).expect("open");
    let loaded = soapy_bin::read_frames(&mut file);
    let (x, y) = soapy_bin::average_frames(&loaded).expect("average");

    assert_eq!(y.len(), frames[0].y.len());
    assert_eq!(x.len(), y.len());
    assert!(y.iter().all(|v| v.is_finite()));

    let _ = std::fs::remove_file(&path);
}
