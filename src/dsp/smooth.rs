//! Spectrum smoothing -- a port of QSpectrumAnalyzer's `utils.smooth()`.
//!
//! The signal is padded on both sides by reflecting it *through* its end points
//! (`2*x[0] - x[k]`), convolved with a normalised window, and the valid centre
//! is sliced back out. Reflecting through the end point rather than mirroring
//! about it is what stops the first and last bins from sagging towards the
//! middle, because it extrapolates the local slope instead of folding it back.
//!
//! One deliberate fix. The original took its padding indices from the SciPy
//! cookbook's `smooth()`, which mirrors (`x[window_len-1:0:-1]`), but changed
//! the reflection to the through-the-endpoint form without shifting the
//! indices: it used `x[window_len:1:-1]`, skipping `x[1]`. That leaves a
//! one-sample step in the padded signal, so a straight line is not a fixed
//! point of the filter and the outermost bins droop by `slope * 0.4` for an
//! 11-tap Hann window -- worst exactly at the band edges, where sweep data is
//! steepest. Using the indices the reflection calls for (`m-1 ..= 1` on the
//! left, `n-2 ..= n-m` on the right) makes a ramp exact; see
//! `linear_ramp_is_preserved`. Traces therefore differ from the Python build by
//! up to that amount within one window length of each edge, and nowhere else.

use serde::{Deserialize, Serialize};

/// The window functions offered in the *Smoothing* dialog.
///
/// Names match NumPy's (and therefore the Python version's config values), so
/// `rectangular` is a plain moving average and the rest are the classic tapers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SmoothWindow {
    Rectangular,
    #[default]
    Hanning,
    Hamming,
    Bartlett,
    Blackman,
}

impl SmoothWindow {
    pub const ALL: [Self; 5] = [
        Self::Rectangular,
        Self::Hanning,
        Self::Hamming,
        Self::Bartlett,
        Self::Blackman,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangular => "rectangular",
            Self::Hanning => "hanning",
            Self::Hamming => "hamming",
            Self::Bartlett => "bartlett",
            Self::Blackman => "blackman",
        }
    }

    /// The window's `len` coefficients, matching `numpy.<name>(len)`.
    pub fn coefficients(self, len: usize) -> Vec<f64> {
        if len == 0 {
            return Vec::new();
        }
        if len == 1 {
            return vec![1.0];
        }

        let n_minus_1 = (len - 1) as f64;
        let tau = std::f64::consts::TAU;

        (0..len)
            .map(|i| {
                let n = i as f64;
                match self {
                    Self::Rectangular => 1.0,
                    Self::Hanning => 0.5 - 0.5 * (tau * n / n_minus_1).cos(),
                    Self::Hamming => 0.54 - 0.46 * (tau * n / n_minus_1).cos(),
                    Self::Bartlett => {
                        let half = n_minus_1 / 2.0;
                        (2.0 / n_minus_1) * (half - (n - half).abs())
                    }
                    Self::Blackman => {
                        0.42 - 0.5 * (tau * n / n_minus_1).cos()
                            + 0.08 * (2.0 * tau * n / n_minus_1).cos()
                    }
                }
            })
            .collect()
    }
}

/// A reusable smoother. Holding one avoids reallocating the window and the
/// padded scratch buffer on every sweep.
#[derive(Clone, Debug, Default)]
pub struct Smoother {
    window: SmoothWindow,
    len: usize,
    /// Window coefficients, already divided by their sum.
    kernel: Vec<f64>,
    /// Padded signal scratch space.
    padded: Vec<f32>,
}

impl Smoother {
    pub fn new(window: SmoothWindow, len: usize) -> Self {
        let mut s = Self::default();
        s.configure(window, len);
        s
    }

    /// Re-plan if the parameters changed; a no-op otherwise.
    pub fn configure(&mut self, window: SmoothWindow, len: usize) {
        if self.window == window && self.len == len && !self.kernel.is_empty() {
            return;
        }
        self.window = window;
        self.len = len;

        if len < 3 {
            self.kernel.clear();
            return;
        }

        let coeffs = window.coefficients(len);
        let sum: f64 = coeffs.iter().sum();
        self.kernel = if sum.abs() < f64::EPSILON {
            // Degenerate window (cannot happen for len >= 3, but stay safe).
            vec![1.0 / len as f64; len]
        } else {
            coeffs.iter().map(|c| c / sum).collect()
        };
    }

    pub fn window(&self) -> SmoothWindow {
        self.window
    }

    /// Configured window length in samples.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when the window is too short to filter anything, so
    /// [`apply`](Self::apply) is the identity.
    pub fn is_empty(&self) -> bool {
        self.len < 3
    }

    /// Smooth `input` into `out`, which is resized to `input.len()`.
    ///
    /// Returns the input unchanged when the window is shorter than 3 samples or
    /// longer than the signal. The Python version raised `ValueError` for the
    /// latter, which is not a useful thing to do while a sweep is running.
    pub fn apply_into(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let n = input.len();
        let m = self.len;

        if m < 3 || self.kernel.is_empty() || n < m {
            out.clear();
            out.extend_from_slice(input);
            return;
        }

        // s = [2*x[0] - x[m-1], ..., 2*x[0] - x[1],  x,  2*x[-1] - x[n-2], ..., 2*x[-1] - x[n-m]]
        let pad = m - 1;
        let first = input[0];
        let last = input[n - 1];

        self.padded.clear();
        self.padded.reserve(n + 2 * pad);

        // Left: indices m-1 down to 1, reflected through x[0].
        for k in (1..m).rev() {
            self.padded.push(2.0 * first - input[k]);
        }
        self.padded.extend_from_slice(input);
        // Right: indices n-2 down to n-m, reflected through x[-1].
        for k in (n - m..n - 1).rev() {
            self.padded.push(2.0 * last - input[k]);
        }

        debug_assert_eq!(self.padded.len(), n + 2 * pad);

        // `np.convolve(kernel, s, mode="same")` keeps the middle len(s)
        // samples of the full convolution, then the caller slices off `pad`
        // from each end. Folding both steps together, output sample `i` is
        //
        //   sum_j kernel[j] * s[i + (m - 1) + (m - 1)/2 - j]
        //
        // and every index that expression produces is inside `s`, so there is
        // no edge case to special-case here.
        let offset = pad + (m - 1) / 2;

        out.clear();
        out.resize(n, 0.0);
        for (i, slot) in out.iter_mut().enumerate() {
            let base = i + offset;
            let mut acc = 0.0f64;
            for (j, &k) in self.kernel.iter().enumerate() {
                acc += k * self.padded[base - j] as f64;
            }
            *slot = acc as f32;
        }
    }

    /// Convenience wrapper allocating a fresh vector.
    pub fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        self.apply_into(input, &mut out);
        out
    }
}

/// One-shot smoothing, matching `utils.smooth(x, window_len, window)`.
pub fn smooth(input: &[f32], len: usize, window: SmoothWindow) -> Vec<f32> {
    Smoother::new(window, len).apply(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_window_is_identity() {
        let x = [1.0f32, 2.0, 3.0, 4.0];
        assert_eq!(smooth(&x, 2, SmoothWindow::Hanning), x.to_vec());
        assert_eq!(smooth(&x, 0, SmoothWindow::Hanning), x.to_vec());
    }

    #[test]
    fn window_longer_than_signal_is_identity() {
        let x = [1.0f32, 2.0, 3.0];
        assert_eq!(smooth(&x, 11, SmoothWindow::Hanning), x.to_vec());
    }

    #[test]
    fn window_exactly_as_long_as_the_signal_still_works() {
        // The padding reads x[m-1] and x[n-m], so n == m is the tightest case
        // that is in bounds; it must not panic or change the length.
        let x = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(smooth(&x, 5, SmoothWindow::Hanning).len(), 5);
        // One longer than the signal falls back to the identity.
        assert_eq!(smooth(&x, 6, SmoothWindow::Blackman), x.to_vec());
    }

    #[test]
    fn edge_droop_of_the_original_padding_is_gone() {
        // The Python build returned -0.1 for the first bin of this input; the
        // corrected reflection indices return the true value, 0.0.
        let x: Vec<f32> = (0..128).map(|i| i as f32 * 0.25).collect();
        let y = smooth(&x, 11, SmoothWindow::Hanning);
        assert!(y[0].abs() < 1e-4, "first bin drooped to {}", y[0]);
        assert!(
            (y[127] - 31.75).abs() < 1e-3,
            "last bin drooped to {}",
            y[127]
        );
    }

    #[test]
    fn preserves_length() {
        let x: Vec<f32> = (0..200).map(|i| (i as f32 * 0.1).sin()).collect();
        for len in [3usize, 5, 11, 31, 64] {
            for w in SmoothWindow::ALL {
                assert_eq!(smooth(&x, len, w).len(), x.len(), "{w:?} len {len}");
            }
        }
    }

    #[test]
    fn constant_signal_is_unchanged() {
        // A normalised window applied to a constant must give the constant
        // back; the reflected padding is built precisely so this holds at the
        // edges too.
        let x = vec![7.5f32; 128];
        for len in [3usize, 11, 33] {
            for w in SmoothWindow::ALL {
                for (i, v) in smooth(&x, len, w).iter().enumerate() {
                    assert!((v - 7.5).abs() < 1e-3, "{w:?} len {len} at {i}: {v}");
                }
            }
        }
    }

    #[test]
    fn linear_ramp_is_preserved() {
        // The mirror-reflection padding also makes a straight line a fixed
        // point of the filter, which is why the original chose it.
        let x: Vec<f32> = (0..128).map(|i| i as f32 * 0.25).collect();
        let y = smooth(&x, 11, SmoothWindow::Hanning);
        for (i, (a, b)) in x.iter().zip(y.iter()).enumerate() {
            assert!((a - b).abs() < 1e-2, "at {i}: {a} vs {b}");
        }
    }

    #[test]
    fn rectangular_matches_moving_average_in_the_interior() {
        let x: Vec<f32> = (0..64).map(|i| ((i * 7) % 13) as f32).collect();
        let y = smooth(&x, 5, SmoothWindow::Rectangular);
        for i in 4..60 {
            let want: f32 = x[i - 2..=i + 2].iter().sum::<f32>() / 5.0;
            assert!((y[i] - want).abs() < 1e-3, "at {i}: {} vs {want}", y[i]);
        }
    }

    #[test]
    fn numpy_window_values() {
        // Spot-check against numpy.hanning(5) / numpy.hamming(5) /
        // numpy.bartlett(5) / numpy.blackman(5).
        let h = SmoothWindow::Hanning.coefficients(5);
        for (a, b) in h.iter().zip([0.0, 0.5, 1.0, 0.5, 0.0]) {
            assert!((a - b).abs() < 1e-12, "{h:?}");
        }

        let h = SmoothWindow::Hamming.coefficients(5);
        for (a, b) in h.iter().zip([0.08, 0.54, 1.0, 0.54, 0.08]) {
            assert!((a - b).abs() < 1e-12, "{h:?}");
        }

        let b = SmoothWindow::Bartlett.coefficients(5);
        for (a, e) in b.iter().zip([0.0, 0.5, 1.0, 0.5, 0.0]) {
            assert!((a - e).abs() < 1e-12, "{b:?}");
        }

        let bl = SmoothWindow::Blackman.coefficients(5);
        for (a, e) in bl
            .iter()
            .zip([-1.38777878e-17, 0.34, 1.0, 0.34, -1.38777878e-17])
        {
            assert!((a - e).abs() < 1e-9, "{bl:?}");
        }
    }

    #[test]
    fn reconfigure_is_cheap_and_correct() {
        let x: Vec<f32> = (0..64).map(|i| i as f32).collect();
        let mut s = Smoother::new(SmoothWindow::Hanning, 11);
        let a = s.apply(&x);
        s.configure(SmoothWindow::Hanning, 11);
        let b = s.apply(&x);
        assert_eq!(a, b);
        s.configure(SmoothWindow::Blackman, 7);
        assert_eq!(s.window(), SmoothWindow::Blackman);
        assert_eq!(s.len(), 7);
        assert_eq!(s.apply(&x).len(), x.len());
    }
}
