//! How fast the sweep pipeline chews through samples, against how fast a radio
//! produces them.
//!
//! The question this answers is whether the DSP is what limits the sweep rate.
//! Run: `cargo run --release --example dsp_bench`

use std::time::Instant;

use spectroscope::dsp::{FftWindow, Welch, WelchConfig};
use spectroscope::sources::rtl_tcp::SweepEngine;
use spectroscope::sources::SweepConfig;

/// Pseudo-random 8-bit IQ, so the transforms see real data rather than zeros.
fn noise(bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes);
    let mut state = 0x1234_5678u32;
    for _ in 0..bytes {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        out.push((state >> 24) as u8);
    }
    out
}

fn main() {
    let samples = 8_000_000usize;
    let data = noise(samples * 2);

    println!("Welch, {samples} complex samples per run, 50% overlap:\n");
    println!("{:>6}  {:>12}  {:>12}", "FFT", "throughput", "vs 2.4 MS/s");
    for n in [64usize, 128, 256, 1024, 4096] {
        let mut welch = Welch::new(WelchConfig {
            fft_size: n,
            window: FftWindow::Hann,
            window_param: None,
            overlap: 50.0,
            remove_dc: true,
        });
        let started = Instant::now();
        welch.push_u8_iq(&data);
        let mut out = Vec::new();
        welch.finish_db(&mut out);
        let secs = started.elapsed().as_secs_f64();
        let rate = samples as f64 / secs;
        println!("{n:>6}  {:>9.1} MS/s  {:>11.0}x", rate / 1e6, rate / 2.4e6);
    }

    // The whole engine: greeting, settling discards, hop changes and stitching.
    //
    // The interesting column is the last one. Processing runs hundreds of times
    // faster than the radio, so what a sweep costs is entirely the samples that
    // have to come off the radio to make it: hops * (settle + integration).
    println!("\nSweepEngine, 87-108 MHz end to end:\n");
    for (rate, bin_khz, crop) in [
        (1_024_000.0, 10.0, 0.2),
        (1_024_000.0, 10.0, 0.0),
        (2_048_000.0, 10.0, 0.2),
        (2_048_000.0, 10.0, 0.0),
        (2_400_000.0, 10.0, 0.2),
        (2_400_000.0, 10.0, 0.0),
        (3_200_000.0, 10.0, 0.2),
        (3_200_000.0, 10.0, 0.0),
    ] {
        let cfg = SweepConfig {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 108.0,
            bin_size_khz: bin_khz,
            interval_s: 1.0,
            sample_rate: rate,
            crop,
            ..Default::default()
        };
        let mut engine = match SweepEngine::new(&cfg) {
            Ok(e) => e,
            Err(e) => {
                println!("  {rate} S/s: {e}");
                continue;
            }
        };
        let hops = engine.hops();

        // Feed it in 64 kiB chunks, the size the real driver reads.
        let chunk = 64 * 1024;
        let started = Instant::now();
        let mut fed = 0usize;
        let mut frames = 0usize;
        let mut at = 0usize;
        while frames < 1 {
            if at + chunk > data.len() {
                at = 0;
            }
            let mut slice: &[u8] = &data[at..at + chunk];
            at += chunk;
            fed += chunk;
            // Drain the chunk: the engine yields at most one sweep per call,
            // so a large chunk has to be asked again with nothing new.
            while engine.push(slice).is_some() {
                frames += 1;
                slice = &[];
            }
        }
        let secs = started.elapsed().as_secs_f64();
        let consumed = fed as f64 / 2.0;
        println!(
            "  {:.3} MS/s, crop {:>3.0}%: {hops:>2} hops, {:.2} M samples per sweep -> \
             {:.1} MS/s processed ({:.0}x real time); radio needs {:.2} s per sweep",
            rate / 1e6,
            crop * 100.0,
            consumed / 1e6,
            consumed / secs / 1e6,
            (consumed / secs) / rate,
            consumed / rate,
        );
    }
}
