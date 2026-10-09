//! Welch-averaged power spectral density from raw interleaved IQ.
//!
//! The helper-process backends arrive pre-transformed; this is for the sources
//! that receive samples instead of spectra (`rtl_tcp`), and it reproduces what
//! `soapy_power`'s `psd.py` does inside its own process: window, transform,
//! average the squared magnitudes of overlapping segments, shift, and convert
//! to dB.
//!
//! # Scaling
//!
//! The accumulated `|X[k]|^2` is divided by `segments * N^2 * noise_power_gain`.
//! `N^2` undoes the unnormalised forward transform twice over (once per factor
//! of the magnitude), and dividing by the window's mean square -- its noise
//! power gain -- makes the reading independent of which window is in use. The
//! consequence, by Parseval, is that the bins of one segment sum to the mean
//! power of the windowed block divided by that gain, which for a signal of
//! constant magnitude `A` is exactly `A^2`: a full-scale complex tone reads
//! 0 dB and the sum across the band is the total power regardless of `N`.
//!
//! The trade-off of noise-referenced scaling is that a *coherent* tone reads
//! `coherent_gain^2 / noise_power_gain` low -- about -1.8 dB under a Hann
//! window. That is the right compromise for a spectrum analyser, where the
//! noise floor has to stay put when the user changes window or bin size; an
//! amplitude-referenced scaling would instead make the floor jump by the same
//! amount.

use num_complex::Complex;

use super::fft::{self, Fft};
use super::window::{noise_power_gain, FftWindow};

/// Lowest value [`Welch::finish_db`] will report.
///
/// An empty bin is mathematically `-inf` dB. Emitting that poisons the plot's
/// autoscale and turns the waterfall texture into NaN once it is normalised
/// against the level window, so everything below this is flattened onto it.
pub const DB_FLOOR: f32 = -200.0;

/// rtl_tcp (and every other 8-bit RTL path) streams offset binary: the zero
/// level is halfway between 127 and 128, and dividing by the same figure puts
/// full scale at +-1.
const U8_ZERO: f32 = 127.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WelchConfig {
    pub fft_size: usize,
    pub window: FftWindow,
    /// Kaiser `beta` or Tukey `alpha`; ignored by the other windows.
    pub window_param: Option<f64>,
    /// Segment overlap in **percent**.
    pub overlap: f64,
    /// Interpolate the centre bin away, like `soapy_power --remove-dc`.
    pub remove_dc: bool,
}

impl Default for WelchConfig {
    fn default() -> Self {
        Self {
            fft_size: 1024,
            window: FftWindow::Hann,
            window_param: None,
            overlap: 50.0,
            remove_dc: false,
        }
    }
}

/// Streaming Welch estimator.
///
/// Feed it byte chunks as they arrive with [`Welch::push_u8_iq`] -- chunk
/// boundaries are irrelevant, including ones that split an I/Q pair -- and read
/// the averaged spectrum out with [`Welch::finish_db`] at the end of a dwell.
pub struct Welch {
    cfg: WelchConfig,
    fft: Box<dyn Fft>,
    window: Vec<f32>,
    /// Mean square of `window`, guaranteed finite and positive.
    noise_gain: f64,
    /// Samples advanced between segment starts; at least 1.
    hop: usize,
    /// Samples received but not yet consumed by a segment. Never exceeds
    /// `fft_size`, which is also its capacity, so pushing never reallocates.
    pending: Vec<Complex<f32>>,
    /// Windowed segment handed to the FFT, reused every fold.
    segment: Vec<Complex<f32>>,
    /// Running sum of `|X[k]|^2` in unshifted bin order. `f64` because a long
    /// dwell folds thousands of segments and an `f32` sum would stop growing.
    acc: Vec<f64>,
    segments: u64,
    /// An I byte whose Q byte has not arrived yet.
    carry_byte: Option<u8>,
}

impl Welch {
    pub fn new(cfg: WelchConfig) -> Self {
        let n = cfg.fft_size;
        let window = cfg.window.coefficients(n, cfg.window_param);
        let noise_gain = {
            let g = noise_power_gain(&window) as f64;
            // A NaN shape parameter or an all-zero window would otherwise turn
            // every bin into NaN; falling back to 1.0 keeps the numbers usable.
            if g.is_finite() && g > 0.0 {
                g
            } else {
                1.0
            }
        };

        Self {
            fft: fft::plan(n),
            window,
            noise_gain,
            hop: hop_for(n, cfg.overlap),
            pending: Vec::with_capacity(n),
            segment: vec![Complex::new(0.0, 0.0); n],
            acc: vec![0.0; n],
            segments: 0,
            carry_byte: None,
            cfg,
        }
    }

    pub fn fft_size(&self) -> usize {
        self.cfg.fft_size
    }

    /// Segments folded since the last [`Welch::finish_db`] or [`Welch::reset`].
    pub fn segments(&self) -> u64 {
        self.segments
    }

    /// Discard everything, including samples carried over from the previous
    /// dwell. Call this after retuning, where the samples still in flight
    /// belong to the old centre frequency.
    pub fn reset(&mut self) {
        self.clear_accumulator();
        self.pending.clear();
        self.carry_byte = None;
    }

    /// Absorb a chunk of interleaved unsigned 8-bit I/Q and return how many
    /// segments it completed.
    ///
    /// Chunks may be any length, including odd: the stranded byte is held until
    /// its partner arrives. Allocation-free.
    pub fn push_u8_iq(&mut self, bytes: &[u8]) -> usize {
        let n = self.cfg.fft_size;
        if n == 0 || bytes.is_empty() {
            return 0;
        }

        let mut folded = 0;
        let mut rest = bytes;

        if let Some(i) = self.carry_byte.take() {
            if let Some((&q, tail)) = rest.split_first() {
                if self.absorb(i, q) {
                    folded += 1;
                }
                rest = tail;
            }
        }

        let mut pairs = rest.chunks_exact(2);
        for pair in &mut pairs {
            if let [i, q] = *pair {
                if self.absorb(i, q) {
                    folded += 1;
                }
            }
        }
        if let Some(&odd) = pairs.remainder().first() {
            self.carry_byte = Some(odd);
        }

        folded
    }

    /// Write `fft_size` dB values, fftshifted, and start a fresh average.
    ///
    /// Index 0 is the most negative frequency and index `fft_size / 2` is the
    /// centre bin, matching `numpy.fft.fftshift`. With no segments folded the
    /// whole band reads [`DB_FLOOR`] rather than whatever was left in the
    /// accumulator.
    pub fn finish_db(&mut self, out: &mut Vec<f32>) {
        let n = self.cfg.fft_size;
        out.clear();
        out.resize(n, DB_FLOOR);

        if n == 0 {
            return;
        }
        if self.segments == 0 {
            self.clear_accumulator();
            return;
        }

        let norm = 1.0 / (self.segments as f64 * (n as f64) * (n as f64) * self.noise_gain);
        let half = n / 2;
        let acc = &self.acc;
        // `fftshift` is a roll by `n / 2`, so shifted index `i` reads unshifted
        // bin `i - n / 2` modulo `n`.
        let linear = |shifted: usize| -> f64 {
            let src = (shifted + n - half) % n;
            acc.get(src).copied().unwrap_or(0.0) * norm
        };

        // soapy_power interpolates the DC spike in the linear domain, and so do
        // we: averaging two dB values would be a geometric mean of powers.
        let interpolate_dc = self.cfg.remove_dc && half >= 1 && half + 1 < n;

        for (i, slot) in out.iter_mut().enumerate() {
            let p = if interpolate_dc && i == half {
                0.5 * (linear(i - 1) + linear(i + 1))
            } else {
                linear(i)
            };
            *slot = if p.is_finite() && p > 0.0 {
                ((10.0 * p.log10()) as f32).max(DB_FLOOR)
            } else {
                DB_FLOOR
            };
        }

        self.clear_accumulator();
    }

    /// Append one sample, folding a segment if it completes one.
    fn absorb(&mut self, i: u8, q: u8) -> bool {
        self.pending.push(Complex::new(
            (i as f32 - U8_ZERO) / U8_ZERO,
            (q as f32 - U8_ZERO) / U8_ZERO,
        ));
        if self.pending.len() < self.cfg.fft_size {
            return false;
        }
        self.fold();
        true
    }

    fn fold(&mut self) {
        // Zipped rather than indexed so a length that somehow disagrees costs a
        // short segment instead of a panic in the acquisition thread.
        for ((dst, src), w) in self
            .segment
            .iter_mut()
            .zip(self.pending.iter())
            .zip(self.window.iter())
        {
            *dst = *src * *w;
        }

        self.fft.process(&mut self.segment);

        for (a, x) in self.acc.iter_mut().zip(self.segment.iter()) {
            *a += x.norm_sqr() as f64;
        }

        self.segments = self.segments.saturating_add(1);

        let hop = self.hop.min(self.pending.len());
        self.pending.drain(..hop);
    }

    fn clear_accumulator(&mut self) {
        self.acc.iter_mut().for_each(|v| *v = 0.0);
        self.segments = 0;
    }
}

/// Samples between segment starts for a given overlap percentage.
///
/// Clamped to `1..=fft_size`: a hop of zero would never consume the buffer and
/// a hop beyond the segment length would silently throw samples away.
fn hop_for(fft_size: usize, overlap_percent: f64) -> usize {
    if fft_size == 0 {
        return 1;
    }
    let pct = if overlap_percent.is_finite() {
        overlap_percent.clamp(0.0, 99.0)
    } else {
        0.0
    };
    // A negative or non-finite product saturates to 0 on the cast, which the
    // clamp then lifts to 1.
    let hop = (fft_size as f64 * (1.0 - pct / 100.0)).round() as usize;
    hop.clamp(1, fft_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::window::coherent_gain;

    fn quantise(v: f64) -> u8 {
        (v * U8_ZERO as f64 + U8_ZERO as f64)
            .round()
            .clamp(0.0, 255.0) as u8
    }

    fn dequantise(b: u8) -> f64 {
        (b as f64 - U8_ZERO as f64) / U8_ZERO as f64
    }

    /// `samples` complex samples of a tone completing `cycles` full turns over
    /// the block, encoded the way rtl_tcp would send it.
    fn tone_bytes(samples: usize, cycles: f64, amplitude: f64) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 * samples);
        for m in 0..samples {
            let phase = std::f64::consts::TAU * cycles * m as f64 / samples as f64;
            out.push(quantise(amplitude * phase.cos()));
            out.push(quantise(amplitude * phase.sin()));
        }
        out
    }

    fn cfg(fft_size: usize, window: FftWindow, overlap: f64) -> WelchConfig {
        WelchConfig {
            fft_size,
            window,
            window_param: None,
            overlap,
            remove_dc: false,
        }
    }

    fn run(cfg: WelchConfig, bytes: &[u8]) -> Vec<f32> {
        let mut w = Welch::new(cfg);
        w.push_u8_iq(bytes);
        let mut out = Vec::new();
        w.finish_db(&mut out);
        out
    }

    fn peak_index(out: &[f32]) -> Option<usize> {
        out.iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
    }

    #[test]
    fn a_full_scale_tone_lands_in_its_shifted_bin_at_zero_db() {
        let n = 64;
        let bin = 8usize;
        let out = run(
            cfg(n, FftWindow::Boxcar, 0.0),
            &tone_bytes(n, bin as f64, 1.0),
        );

        assert_eq!(out.len(), n);
        // fftshift puts unshifted bin k at index k + n/2 (mod n).
        let want = (bin + n / 2) % n;
        assert_eq!(peak_index(&out), Some(want));
        assert!(out[want].abs() < 0.1, "peak {} dB", out[want]);
        for (i, v) in out.iter().enumerate() {
            if i != want {
                assert!(*v < -40.0, "bin {i} = {v} dB");
            }
        }
    }

    #[test]
    fn a_negative_frequency_lands_below_centre() {
        let n = 64;
        let bin = n - 8; // one eighth of the way down from DC
        let out = run(
            cfg(n, FftWindow::Boxcar, 0.0),
            &tone_bytes(n, bin as f64, 1.0),
        );
        assert_eq!(peak_index(&out), Some(n / 2 - 8));
    }

    #[test]
    fn dc_sits_at_the_centre_bin() {
        let n = 32;
        let out = run(cfg(n, FftWindow::Boxcar, 0.0), &tone_bytes(n, 0.0, 1.0));
        assert_eq!(peak_index(&out), Some(n / 2));
    }

    /// Noise-referenced scaling costs a coherent tone `CG^2 / NPG`; this pins
    /// that number down so a future change to the normalisation is noticed.
    #[test]
    fn a_tapered_window_reads_a_tone_predictably_low() {
        let n = 256;
        let bin = 32usize;
        let c = cfg(n, FftWindow::Hann, 0.0);
        let out = run(c, &tone_bytes(n, bin as f64, 1.0));

        let coeffs = FftWindow::Hann.coefficients(n, None);
        let cg = coherent_gain(&coeffs) as f64;
        let npg = noise_power_gain(&coeffs) as f64;
        let want = 10.0 * (cg * cg / npg).log10();

        let idx = (bin + n / 2) % n;
        assert!(
            (out[idx] as f64 - want).abs() < 0.1,
            "{} dB, expected {want} dB",
            out[idx]
        );
    }

    /// Parseval: the linear powers of all bins of one segment must sum to the
    /// mean windowed sample power divided by the window's noise power gain.
    #[test]
    fn total_power_matches_the_windowed_samples() {
        let n = 128;
        let window = FftWindow::Hamming;
        let bytes = tone_bytes(n, 19.5, 0.8);
        let out = run(cfg(n, window, 0.0), &bytes);

        let coeffs = window.coefficients(n, None);
        let npg = noise_power_gain(&coeffs) as f64;
        let mut reference = 0.0;
        for (m, pair) in bytes.chunks_exact(2).enumerate() {
            let i = dequantise(pair[0]);
            let q = dequantise(pair[1]);
            let w = coeffs[m] as f64;
            reference += (i * i + q * q) * w * w;
        }
        reference /= n as f64 * npg;

        let total: f64 = out.iter().map(|db| 10f64.powf(*db as f64 / 10.0)).sum();
        assert!(
            (total - reference).abs() < 1e-3 * reference,
            "{total} vs {reference}"
        );
    }

    #[test]
    fn remove_dc_interpolates_the_centre_bin_away() {
        let n = 64;
        let bytes = tone_bytes(n, 0.0, 1.0);
        let keep = run(cfg(n, FftWindow::Boxcar, 0.0), &bytes);
        let drop = run(
            WelchConfig {
                remove_dc: true,
                ..cfg(n, FftWindow::Boxcar, 0.0)
            },
            &bytes,
        );

        assert!(keep[n / 2] > -1.0, "spike should be there: {}", keep[n / 2]);
        assert!(drop[n / 2] < -80.0, "spike should be gone: {}", drop[n / 2]);
        // Only the centre bin changes.
        for i in 0..n {
            if i != n / 2 {
                assert_eq!(keep[i], drop[i], "bin {i}");
            }
        }
    }

    #[test]
    fn remove_dc_is_safe_for_tiny_transforms() {
        for n in [1usize, 2] {
            let c = WelchConfig {
                remove_dc: true,
                ..cfg(n, FftWindow::Boxcar, 0.0)
            };
            let out = run(c, &tone_bytes(4 * n, 0.0, 1.0));
            assert_eq!(out.len(), n);
            assert!(out.iter().all(|v| v.is_finite()), "{out:?}");
        }
    }

    #[test]
    fn chunk_boundaries_do_not_change_the_result() {
        let n = 128;
        let c = cfg(n, FftWindow::Hann, 50.0);
        let bytes = tone_bytes(4 * n, 11.25, 0.6);

        let mut whole = Welch::new(c);
        let folded_whole = whole.push_u8_iq(&bytes);
        let mut a = Vec::new();
        whole.finish_db(&mut a);

        let mut split = Welch::new(c);
        let mut folded_split = 0;
        let mut at = 0;
        // Deliberately odd strides, so I/Q pairs get cut in half repeatedly.
        for step in [1usize, 3, 7, 13, 2, 255, 1, 64].into_iter().cycle() {
            if at >= bytes.len() {
                break;
            }
            let end = (at + step).min(bytes.len());
            folded_split += split.push_u8_iq(&bytes[at..end]);
            at = end;
        }
        let mut b = Vec::new();
        split.finish_db(&mut b);

        assert_eq!(folded_whole, 7);
        assert_eq!(folded_whole, folded_split);
        assert_eq!(a, b);
    }

    #[test]
    fn an_odd_trailing_byte_is_carried_to_the_next_call() {
        let n = 16;
        let c = cfg(n, FftWindow::Boxcar, 0.0);
        let bytes = tone_bytes(n, 3.0, 1.0);

        let whole = run(c, &bytes);

        let mut w = Welch::new(c);
        // 9 bytes leaves an I byte with no Q.
        assert_eq!(w.push_u8_iq(&bytes[..9]), 0);
        assert_eq!(w.push_u8_iq(&bytes[9..]), 1);
        let mut split = Vec::new();
        w.finish_db(&mut split);

        assert_eq!(whole, split);
    }

    #[test]
    fn overlap_sets_how_many_segments_a_block_yields() {
        let n = 64;
        let bytes = tone_bytes(4 * n, 5.0, 0.5);
        for (overlap, want) in [(0.0, 4), (50.0, 7), (75.0, 13)] {
            let mut w = Welch::new(cfg(n, FftWindow::Boxcar, overlap));
            assert_eq!(w.push_u8_iq(&bytes), want, "overlap {overlap}");
            assert_eq!(w.segments(), want as u64);
        }
    }

    #[test]
    fn nonsense_overlap_still_makes_progress() {
        for overlap in [-50.0, 100.0, 1e9, f64::NAN, f64::INFINITY] {
            let n = 32;
            let mut w = Welch::new(cfg(n, FftWindow::Boxcar, overlap));
            let folded = w.push_u8_iq(&tone_bytes(4 * n, 3.0, 0.5));
            assert!(folded > 0, "overlap {overlap} folded nothing");
            let mut out = Vec::new();
            w.finish_db(&mut out);
            assert!(out.iter().all(|v| v.is_finite()), "overlap {overlap}");
        }
    }

    #[test]
    fn no_data_reads_the_floor() {
        let n = 32;
        let mut w = Welch::new(cfg(n, FftWindow::Hann, 50.0));
        let mut out = vec![1.0, 2.0];
        w.finish_db(&mut out);
        assert_eq!(out.len(), n);
        assert!(out.iter().all(|v| *v == DB_FLOOR), "{out:?}");
        assert_eq!(w.segments(), 0);
    }

    #[test]
    fn a_partial_segment_is_not_folded() {
        let n = 64;
        let mut w = Welch::new(cfg(n, FftWindow::Boxcar, 0.0));
        assert_eq!(w.push_u8_iq(&tone_bytes(n - 1, 4.0, 1.0)), 0);
        assert_eq!(w.segments(), 0);
        let mut out = Vec::new();
        w.finish_db(&mut out);
        assert!(out.iter().all(|v| *v == DB_FLOOR));
    }

    #[test]
    fn zero_fft_size_is_inert() {
        let mut w = Welch::new(cfg(0, FftWindow::Hann, 50.0));
        assert_eq!(w.fft_size(), 0);
        assert_eq!(w.push_u8_iq(&[1, 2, 3, 4, 5]), 0);
        let mut out = vec![7.0];
        w.finish_db(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn single_bin_transform_reports_total_power() {
        let mut w = Welch::new(cfg(1, FftWindow::Boxcar, 0.0));
        // Full scale on I only: power is 1.0, so 0 dB.
        assert_eq!(w.push_u8_iq(&[255, quantise(0.0)]), 1);
        let mut out = Vec::new();
        w.finish_db(&mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].abs() < 0.1, "{}", out[0]);
    }

    #[test]
    fn finish_resets_the_average_but_reset_also_drops_pending_samples() {
        let n = 32;
        let mut w = Welch::new(cfg(n, FftWindow::Boxcar, 0.0));
        let loud = tone_bytes(n, 4.0, 1.0);
        w.push_u8_iq(&loud);
        let mut first = Vec::new();
        w.finish_db(&mut first);
        assert_eq!(w.segments(), 0);

        // Second read with nothing new must not repeat the first.
        let mut second = Vec::new();
        w.finish_db(&mut second);
        assert!(second.iter().all(|v| *v == DB_FLOOR), "{second:?}");

        // Half a segment, then a reset, then a full segment: the stale half
        // must not leak into the new average.
        w.push_u8_iq(&loud[..n]);
        w.reset();
        w.push_u8_iq(&loud);
        let mut third = Vec::new();
        w.finish_db(&mut third);
        assert_eq!(first, third);
    }

    #[test]
    fn a_nan_window_parameter_does_not_poison_the_output() {
        let n = 64;
        let c = WelchConfig {
            fft_size: n,
            window: FftWindow::Kaiser,
            window_param: Some(f64::NAN),
            overlap: 50.0,
            remove_dc: true,
        };
        let out = run(c, &tone_bytes(4 * n, 7.0, 0.9));
        assert_eq!(out.len(), n);
        assert!(out.iter().all(|v| v.is_finite()), "{out:?}");
    }

    #[test]
    fn averaging_many_identical_segments_changes_nothing() {
        let n = 64;
        let c = cfg(n, FftWindow::Boxcar, 0.0);
        let one = tone_bytes(n, 9.0, 0.7);
        let single = run(c, &one);

        let mut many = Welch::new(c);
        for _ in 0..50 {
            many.push_u8_iq(&one);
        }
        assert_eq!(many.segments(), 50);
        let mut averaged = Vec::new();
        many.finish_db(&mut averaged);

        for (i, (a, b)) in single.iter().zip(averaged.iter()).enumerate() {
            assert!((a - b).abs() < 1e-3, "bin {i}: {a} vs {b}");
        }
    }

    #[test]
    fn pushing_does_not_reallocate_after_construction() {
        let n = 256;
        let mut w = Welch::new(cfg(n, FftWindow::Hann, 50.0));
        let before = w.pending.capacity();
        for _ in 0..20 {
            w.push_u8_iq(&tone_bytes(n, 3.0, 0.5));
        }
        assert_eq!(w.pending.capacity(), before);
    }

    #[test]
    fn hop_covers_its_whole_range() {
        assert_eq!(hop_for(0, 50.0), 1);
        assert_eq!(hop_for(64, 0.0), 64);
        assert_eq!(hop_for(64, 50.0), 32);
        assert_eq!(hop_for(64, 99.0), 1);
        assert_eq!(hop_for(64, 100.0), 1);
        assert_eq!(hop_for(64, -10.0), 64);
        assert_eq!(hop_for(64, f64::NAN), 64);
        assert_eq!(hop_for(1, 50.0), 1);
    }
}
