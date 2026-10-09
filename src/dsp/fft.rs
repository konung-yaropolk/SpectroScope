//! The forward complex FFT, behind a trait so the library can vary.
//!
//! `rustfft` is the implementation. It is pure Rust, so it builds unchanged for
//! every target including wasm32 and Android, and for the power-of-two sizes
//! this application plans it is competitive with FFTW. The original toolchain
//! used FFTW -- `pyfftw` inside `soapy_power`, libfftw in `rtl_power_fftw` --
//! but only inside those helper processes, which are still free to.
//!
//! Both implementations produce the **unnormalised** forward transform, i.e. the
//! textbook `sum x[m] exp(-2i pi k m / N)` with no `1/N` anywhere. Scaling is
//! the caller's business because only the caller knows whether it wants an
//! amplitude or a power spectrum; see [`crate::dsp::psd`].

use std::sync::{Arc, Mutex};

use num_complex::Complex;
use rustfft::{Fft as RustFftTrait, FftPlanner};

/// How many distinct transform lengths are kept planned between uses.
///
/// Planning dominates the cost of a short transform and the GUI re-plans on
/// every edit of the bin size, so recent sizes are worth holding on to. The
/// bound matters as much as the cache: a plan carries twiddle tables
/// proportional to its length, and an unbounded cache would pin one per size
/// the user ever typed.
const CACHE_CAPACITY: usize = 8;

/// A planned forward FFT of one fixed length.
pub trait Fft: Send {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Transform `buf` in place, unnormalised.
    ///
    /// A buffer shorter than [`Self::len`] is left untouched rather than
    /// treated as an error: this runs inside the acquisition loop, where the
    /// only useful response to a mismatch is to drop the segment.
    fn process(&mut self, buf: &mut [Complex<f32>]);

    fn backend_name(&self) -> &'static str;
}

/// Name of the FFT library this build prefers.
pub fn backend_name() -> &'static str {
    "rustfft"
}

/// Plan a forward transform of `len` points.
///
/// Never fails: lengths 0 and 1 need no transform at all.
pub fn plan(len: usize) -> Box<dyn Fft> {
    if len <= 1 {
        return Box::new(Trivial(len));
    }
    Box::new(RustFft::new(len))
}

/// Lengths 0 and 1, where the DFT is the identity (or nothing at all).
struct Trivial(usize);

impl Fft for Trivial {
    fn len(&self) -> usize {
        self.0
    }

    fn process(&mut self, _buf: &mut [Complex<f32>]) {}

    fn backend_name(&self) -> &'static str {
        self::backend_name()
    }
}

// ---------------------------------------------------------------------------
// rustfft
// ---------------------------------------------------------------------------

/// A planned rustfft transform, keyed by length in the cache below.
type CachedPlan = (usize, Arc<dyn RustFftTrait<f32>>);

static RUSTFFT_CACHE: Mutex<Vec<CachedPlan>> = Mutex::new(Vec::new());

/// Fetch a plan for `len`, most-recently-used first.
///
/// On a miss the planner is built fresh and dropped again, so that `rustfft`'s
/// own (unbounded) internal plan cache never outlives the call and ours stays
/// the single source of retained plans.
fn cached_rustfft(len: usize) -> Arc<dyn RustFftTrait<f32>> {
    let Ok(mut cache) = RUSTFFT_CACHE.lock() else {
        // A poisoned lock means some other thread panicked mid-planning; plan
        // privately rather than propagate the failure into a sweep.
        return FftPlanner::new().plan_fft_forward(len);
    };

    if let Some(pos) = cache.iter().position(|(n, _)| *n == len) {
        let entry = cache.remove(pos);
        let fft = Arc::clone(&entry.1);
        cache.insert(0, entry);
        return fft;
    }

    let fft = FftPlanner::new().plan_fft_forward(len);
    if cache.len() >= CACHE_CAPACITY {
        cache.pop();
    }
    cache.insert(0, (len, Arc::clone(&fft)));
    fft
}

struct RustFft {
    fft: Arc<dyn RustFftTrait<f32>>,
    scratch: Vec<Complex<f32>>,
    len: usize,
}

impl RustFft {
    fn new(len: usize) -> Self {
        let fft = cached_rustfft(len);
        let scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        Self { fft, scratch, len }
    }
}

impl Fft for RustFft {
    fn len(&self) -> usize {
        self.len
    }

    fn process(&mut self, buf: &mut [Complex<f32>]) {
        if self.len == 0 || buf.len() < self.len {
            return;
        }
        // Exactly one chunk: `rustfft` would otherwise require the length to be
        // a whole multiple of the plan size.
        self.fft
            .process_with_scratch(&mut buf[..self.len], &mut self.scratch);
    }

    fn backend_name(&self) -> &'static str {
        "rustfft"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f32, im: f32) -> Complex<f32> {
        Complex::new(re, im)
    }

    /// Reference DFT in f64, so the tests check the backends rather than each
    /// other.
    fn naive_dft(input: &[Complex<f32>]) -> Vec<Complex<f64>> {
        let n = input.len();
        (0..n)
            .map(|k| {
                input
                    .iter()
                    .enumerate()
                    .fold(Complex::new(0.0_f64, 0.0_f64), |acc, (m, x)| {
                        let angle = -std::f64::consts::TAU * (k as f64) * (m as f64) / (n as f64);
                        let (s, cs) = angle.sin_cos();
                        acc + Complex::new(x.re as f64, x.im as f64) * Complex::new(cs, s)
                    })
            })
            .collect()
    }

    fn assert_matches_dft(len: usize) {
        let input: Vec<Complex<f32>> = (0..len)
            .map(|i| {
                let t = i as f32;
                c((0.3 * t).sin() + 0.25 * t.cos(), (0.11 * t + 1.0).cos())
            })
            .collect();
        let want = naive_dft(&input);

        let mut got = input.clone();
        let mut fft = plan(len);
        assert_eq!(fft.len(), len);
        fft.process(&mut got);

        let scale = (len as f64).max(1.0);
        for (k, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            let err = ((g.re as f64 - w.re).powi(2) + (g.im as f64 - w.im).powi(2)).sqrt();
            assert!(
                err < 1e-3 * scale,
                "len {len} bin {k}: {g:?} vs {w:?} (err {err})"
            );
        }
    }

    #[test]
    fn matches_a_reference_dft_for_mixed_radices() {
        // Powers of two, a prime (Bluestein/Raders path) and an odd composite.
        for len in [2, 3, 4, 8, 15, 16, 17, 64, 243] {
            assert_matches_dft(len);
        }
    }

    #[test]
    fn dc_input_concentrates_in_bin_zero() {
        let len = 32;
        let mut buf = vec![c(1.0, 0.0); len];
        plan(len).process(&mut buf);
        assert!((buf[0].re - len as f32).abs() < 1e-3, "{:?}", buf[0]);
        assert!(buf[0].im.abs() < 1e-3, "{:?}", buf[0]);
        for (k, v) in buf.iter().enumerate().skip(1) {
            assert!(v.norm() < 1e-3, "bin {k}: {v:?}");
        }
    }

    #[test]
    fn a_single_exponential_lands_in_one_bin() {
        let len = 64;
        let bin = 9usize;
        let buf: Vec<Complex<f32>> = (0..len)
            .map(|m| {
                let a = std::f64::consts::TAU * bin as f64 * m as f64 / len as f64;
                c(a.cos() as f32, a.sin() as f32)
            })
            .collect();
        let mut out = buf.clone();
        plan(len).process(&mut out);
        assert!(
            (out[bin].norm() - len as f32).abs() < 1e-2,
            "{:?}",
            out[bin]
        );
        for (k, v) in out.iter().enumerate() {
            if k != bin {
                assert!(v.norm() < 1e-2, "bin {k}: {v:?}");
            }
        }
    }

    #[test]
    fn degenerate_lengths_are_inert() {
        let mut empty: Vec<Complex<f32>> = Vec::new();
        let mut zero = plan(0);
        assert_eq!(zero.len(), 0);
        assert!(zero.is_empty());
        zero.process(&mut empty);
        assert!(empty.is_empty());

        let mut one = vec![c(3.0, -4.0)];
        let mut unit = plan(1);
        assert_eq!(unit.len(), 1);
        assert!(!unit.is_empty());
        unit.process(&mut one);
        assert_eq!(one, vec![c(3.0, -4.0)]);
    }

    #[test]
    fn a_short_buffer_is_left_alone_instead_of_panicking() {
        let mut fft = plan(16);
        let before = vec![c(1.0, 2.0); 15];
        let mut buf = before.clone();
        fft.process(&mut buf);
        assert_eq!(buf, before);
        fft.process(&mut []);
    }

    /// A longer buffer transforms its leading `len` samples and leaves the tail,
    /// which is what makes `process` safe to call with an over-sized scratch.
    #[test]
    fn a_long_buffer_transforms_only_its_prefix() {
        let len = 8;
        let mut buf = vec![c(1.0, 0.0); len];
        buf.extend(std::iter::repeat_n(c(7.0, 7.0), 3));
        plan(len).process(&mut buf);
        assert!((buf[0].re - len as f32).abs() < 1e-3);
        assert_eq!(&buf[len..], [c(7.0, 7.0); 3].as_slice());
    }

    #[test]
    fn replanning_a_size_keeps_working() {
        // Exercises the cache: hand the plan back, take it out again, and make
        // sure the recycled plan still transforms correctly. Also pushes more
        // distinct sizes through than the cache can hold, to exercise eviction.
        for _ in 0..3 {
            for len in [4usize, 8, 16, 32, 64, 128, 256, 512, 1024, 2048] {
                let mut buf = vec![c(1.0, 0.0); len];
                plan(len).process(&mut buf);
                assert!(
                    (buf[0].re - len as f32).abs() < 1e-2,
                    "len {len}: {:?}",
                    buf[0]
                );
            }
        }
    }

    #[test]
    fn backend_name_describes_the_build() {
        assert_eq!(backend_name(), "rustfft");
        assert_eq!(plan(0).backend_name(), "rustfft");
        assert_eq!(plan(64).backend_name(), "rustfft");
    }
}
