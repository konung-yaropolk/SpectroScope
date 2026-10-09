//! FFT window functions.
//!
//! These are exactly the `--fft-window` choices `soapy_power` accepts, so the
//! Settings UI can build that backend's parameter string from
//! [`FftWindow::label`], and the `rtl_tcp` source -- which does its own
//! transform -- can taper with the same shapes.
//!
//! The coefficients follow `scipy.signal.get_window(..., sym=True)`, i.e. the
//! symmetric definitions that divide by `N - 1`. That is the convention NumPy's
//! `hanning`/`hamming`/`blackman`/`bartlett` use and therefore the one the rest
//! of the port is already consistent with. A symmetric window is marginally
//! worse than the periodic variant for spectral analysis, but matching the
//! reference implementation matters more than a fraction of a dB of scalloping.

use serde::{Deserialize, Serialize};

/// Tapers offered wherever a window can be chosen.
///
/// `Serialize`/`Deserialize` because the choice is persisted in the config file
/// alongside the rest of the acquisition settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FftWindow {
    Boxcar,
    #[default]
    Hann,
    Hamming,
    Blackman,
    Bartlett,
    Kaiser,
    Tukey,
}

impl FftWindow {
    pub const ALL: [Self; 7] = [
        Self::Boxcar,
        Self::Hann,
        Self::Hamming,
        Self::Blackman,
        Self::Bartlett,
        Self::Kaiser,
        Self::Tukey,
    ];

    /// The spelling `soapy_power --fft-window` expects; also what the combo box
    /// shows, since the two must not be allowed to drift apart.
    pub fn label(self) -> &'static str {
        match self {
            Self::Boxcar => "boxcar",
            Self::Hann => "hann",
            Self::Hamming => "hamming",
            Self::Blackman => "blackman",
            Self::Bartlett => "bartlett",
            Self::Kaiser => "kaiser",
            Self::Tukey => "tukey",
        }
    }

    /// Whether the window takes a shape parameter, which SciPy passes as the
    /// second element of a `(name, param)` tuple.
    pub fn needs_param(self) -> bool {
        matches!(self, Self::Kaiser | Self::Tukey)
    }

    /// Kaiser `beta` / Tukey `alpha` to start the spin box at.
    ///
    /// `beta = 8.6` is the classic Kaiser setting whose side lobes sit around
    /// -90 dB, roughly matching a Blackman window; `alpha = 0.5` is SciPy's own
    /// Tukey default.
    pub fn default_param(self) -> f64 {
        match self {
            Self::Kaiser => 8.6,
            Self::Tukey => 0.5,
            _ => 0.0,
        }
    }

    /// `len` coefficients. `param` is ignored by the parameterless windows and
    /// replaced by [`Self::default_param`] when absent or not finite.
    pub fn coefficients(self, len: usize, param: Option<f64>) -> Vec<f32> {
        match len {
            0 => return Vec::new(),
            // Every window degenerates to a single unity tap, and the `N - 1`
            // denominators below would divide by zero.
            1 => return vec![1.0],
            _ => {}
        }

        let last = (len - 1) as f64;
        let tau = std::f64::consts::TAU;
        let p = self.resolved_param(param);

        // Precomputed once rather than per sample: `I0(beta)` is a series sum
        // and `alpha` drives the Tukey branch bounds.
        let kaiser_norm = if matches!(self, Self::Kaiser) {
            bessel_i0(p)
        } else {
            1.0
        };

        (0..len)
            .map(|i| {
                let n = i as f64;
                let v = match self {
                    Self::Boxcar => 1.0,
                    Self::Hann => 0.5 - 0.5 * (tau * n / last).cos(),
                    Self::Hamming => 0.54 - 0.46 * (tau * n / last).cos(),
                    Self::Blackman => {
                        0.42 - 0.5 * (tau * n / last).cos() + 0.08 * (2.0 * tau * n / last).cos()
                    }
                    Self::Bartlett => {
                        let half = last / 2.0;
                        (2.0 / last) * (half - (n - half).abs())
                    }
                    Self::Kaiser => {
                        let half = last / 2.0;
                        let r = (n - half) / half;
                        // `1 - r^2` can go a hair negative at the ends through
                        // rounding, and `sqrt` of that is NaN.
                        bessel_i0(p * (1.0 - r * r).max(0.0).sqrt()) / kaiser_norm
                    }
                    Self::Tukey => tukey_tap(n / last, p),
                };
                v as f32
            })
            .collect()
    }

    /// Clamp the shape parameter into the range where the window is defined and
    /// the Bessel series cannot overflow.
    fn resolved_param(self, param: Option<f64>) -> f64 {
        let v = param
            .filter(|p| p.is_finite())
            .unwrap_or_else(|| self.default_param());
        match self {
            // `I0(100)` is about 1e42; well past that the ratio would overflow
            // to `inf / inf` and the whole window would come out NaN.
            Self::Kaiser => v.clamp(0.0, 100.0),
            Self::Tukey => v.clamp(0.0, 1.0),
            _ => v,
        }
    }
}

/// One Tukey tap at normalised position `x` in `0.0..=1.0`.
///
/// Closed form of SciPy's three-piece construction: a cosine taper over the
/// outer `alpha / 2` of each end and unity in between.
fn tukey_tap(x: f64, alpha: f64) -> f64 {
    if alpha <= 0.0 {
        return 1.0;
    }
    let edge = alpha / 2.0;
    let tau = std::f64::consts::TAU;
    if x < edge {
        0.5 * (1.0 + (tau / alpha * (x - edge)).cos())
    } else if x > 1.0 - edge {
        0.5 * (1.0 + (tau / alpha * (x - 1.0 + edge)).cos())
    } else {
        1.0
    }
}

/// Zeroth-order modified Bessel function of the first kind.
///
/// Straight from the series `sum_k (x/2)^(2k) / (k!)^2`, accumulated as a
/// running ratio so no factorial is ever materialised. The terms grow until
/// `k ~ x/2` and then fall away geometrically, which is why the convergence test
/// is relative to the partial sum rather than absolute.
fn bessel_i0(x: f64) -> f64 {
    let half_sq = (x / 2.0) * (x / 2.0);
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    for k in 1..=200 {
        term *= half_sq / ((k * k) as f64);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// Mean of the coefficients: the gain the window applies to a tone sitting
/// exactly on a bin centre.
///
/// An empty window yields `1.0`, the neutral value, so callers dividing by it
/// cannot produce infinities.
pub fn coherent_gain(coeffs: &[f32]) -> f32 {
    if coeffs.is_empty() {
        return 1.0;
    }
    let sum: f64 = coeffs.iter().map(|c| *c as f64).sum();
    (sum / coeffs.len() as f64) as f32
}

/// Mean of the squared coefficients: the gain the window applies to broadband
/// noise power, and hence the Welch scaling factor.
pub fn noise_power_gain(coeffs: &[f32]) -> f32 {
    if coeffs.is_empty() {
        return 1.0;
    }
    let sum: f64 = coeffs.iter().map(|c| (*c as f64) * (*c as f64)).sum();
    (sum / coeffs.len() as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values of `I0` tabulated to full double precision.
    #[test]
    fn bessel_i0_matches_reference_values() {
        let cases = [
            (0.0, 1.0),
            (0.5, 1.063_483_370_741_323_6),
            (1.0, 1.266_065_877_752_008_2),
            (2.0, 2.279_585_302_336_067),
            (3.0, 4.880_792_585_865_024_5),
            (3.75, 9.118_945_860_844_565),
            (5.0, 27.239_871_823_604_45),
            (8.6, 750.461_159_563_166_1),
            (10.0, 2_815.716_628_466_255),
            (20.0, 43_558_282.559_553_53),
        ];
        for (x, want) in cases {
            let got = bessel_i0(x);
            assert!(
                (got - want).abs() <= want.abs() * 1e-13,
                "I0({x}) = {got}, want {want}"
            );
        }
        // Even function.
        assert_eq!(bessel_i0(-7.5), bessel_i0(7.5));
    }

    #[test]
    fn bessel_i0_stays_finite_at_the_clamp_limit() {
        let v = bessel_i0(FftWindow::Kaiser.resolved_param(Some(1e9)));
        assert!(v.is_finite() && v > 0.0, "{v}");
    }

    fn close(got: &[f32], want: &[f64], tol: f64) {
        assert_eq!(got.len(), want.len(), "length: {got:?} vs {want:?}");
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (*g as f64 - *w).abs() <= tol,
                "tap {i}: {g} vs {w} ({got:?})"
            );
        }
    }

    #[test]
    fn classic_windows_match_numpy_for_n5() {
        close(
            &FftWindow::Hann.coefficients(5, None),
            &[0.0, 0.5, 1.0, 0.5, 0.0],
            1e-7,
        );
        close(
            &FftWindow::Hamming.coefficients(5, None),
            &[0.08, 0.54, 1.0, 0.54, 0.08],
            1e-7,
        );
        close(
            &FftWindow::Blackman.coefficients(5, None),
            &[0.0, 0.34, 1.0, 0.34, 0.0],
            1e-7,
        );
        close(
            &FftWindow::Bartlett.coefficients(5, None),
            &[0.0, 0.5, 1.0, 0.5, 0.0],
            1e-7,
        );
        close(&FftWindow::Boxcar.coefficients(5, None), &[1.0; 5], 0.0);
    }

    #[test]
    fn hann_matches_numpy_for_n9() {
        close(
            &FftWindow::Hann.coefficients(9, None),
            &[
                0.0,
                0.146_446_609_406_726_2,
                0.5,
                0.853_553_390_593_273_7,
                1.0,
                0.853_553_390_593_273_7,
                0.5,
                0.146_446_609_406_726_2,
                0.0,
            ],
            1e-7,
        );
    }

    #[test]
    fn kaiser_matches_scipy() {
        close(
            &FftWindow::Kaiser.coefficients(5, Some(8.6)),
            &[
                0.001_332_513_997_902_419_3,
                0.340_393_622_440_188_5,
                1.0,
                0.340_393_622_440_188_5,
                0.001_332_513_997_902_419_3,
            ],
            1e-7,
        );
        close(
            &FftWindow::Kaiser.coefficients(5, Some(14.0)),
            &[
                7.726_866_835_270_366e-6,
                0.164_932_187_547_952,
                1.0,
                0.164_932_187_547_952,
                7.726_866_835_270_366e-6,
            ],
            1e-7,
        );
        // `beta = 0` degenerates to a rectangle.
        close(
            &FftWindow::Kaiser.coefficients(7, Some(0.0)),
            &[1.0; 7],
            1e-7,
        );
        // Absent parameter falls back to the default, not to zero.
        assert_eq!(
            FftWindow::Kaiser.coefficients(9, None),
            FftWindow::Kaiser.coefficients(9, Some(8.6))
        );
    }

    #[test]
    fn tukey_matches_scipy_and_its_two_degenerate_ends() {
        close(
            &FftWindow::Tukey.coefficients(9, Some(0.5)),
            &[0.0, 0.5, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0],
            1e-7,
        );
        // alpha = 1 is a Hann window, alpha = 0 a boxcar.
        let hann = FftWindow::Hann.coefficients(9, None);
        let tukey1 = FftWindow::Tukey.coefficients(9, Some(1.0));
        for (a, b) in hann.iter().zip(tukey1.iter()) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
        close(&FftWindow::Tukey.coefficients(9, Some(0.0)), &[1.0; 9], 0.0);
    }

    #[test]
    fn out_of_range_and_nan_parameters_are_sanitised() {
        for w in [FftWindow::Kaiser, FftWindow::Tukey] {
            for p in [f64::NAN, f64::INFINITY, -1e9, 1e9] {
                let c = w.coefficients(32, Some(p));
                assert_eq!(c.len(), 32);
                assert!(c.iter().all(|v| v.is_finite()), "{w:?} {p}: {c:?}");
            }
        }
    }

    #[test]
    fn degenerate_lengths() {
        for w in FftWindow::ALL {
            assert!(w.coefficients(0, None).is_empty(), "{w:?}");
            assert_eq!(w.coefficients(1, None), vec![1.0], "{w:?}");
            assert_eq!(w.coefficients(2, None).len(), 2, "{w:?}");
            assert!(
                w.coefficients(2, None).iter().all(|v| v.is_finite()),
                "{w:?}"
            );
        }
    }

    #[test]
    fn windows_are_symmetric() {
        for w in FftWindow::ALL {
            let c = w.coefficients(64, None);
            for i in 0..32 {
                assert!(
                    (c[i] - c[63 - i]).abs() < 1e-6,
                    "{w:?} asymmetric at {i}: {} vs {}",
                    c[i],
                    c[63 - i]
                );
            }
        }
    }

    #[test]
    fn gains_of_a_rectangle_are_unity() {
        let boxcar = FftWindow::Boxcar.coefficients(128, None);
        assert!((coherent_gain(&boxcar) - 1.0).abs() < 1e-6);
        assert!((noise_power_gain(&boxcar) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn hann_gains_match_the_closed_form() {
        // For even N the symmetric Hann window sums to (N - 1) / 2 and its
        // squares to 3 (N - 1) / 8, so both gains are exactly predictable.
        let n = 256usize;
        let c = FftWindow::Hann.coefficients(n, None);
        let want_cg = 0.5 * (n - 1) as f64 / n as f64;
        let want_npg = 0.375 * (n - 1) as f64 / n as f64;
        assert!(
            (coherent_gain(&c) as f64 - want_cg).abs() < 1e-6,
            "{}",
            coherent_gain(&c)
        );
        assert!(
            (noise_power_gain(&c) as f64 - want_npg).abs() < 1e-6,
            "{}",
            noise_power_gain(&c)
        );
    }

    #[test]
    fn empty_gains_are_neutral() {
        assert_eq!(coherent_gain(&[]), 1.0);
        assert_eq!(noise_power_gain(&[]), 1.0);
    }

    #[test]
    fn labels_are_the_lowercase_scipy_names_and_unique() {
        let mut seen: Vec<&str> = FftWindow::ALL.iter().map(|w| w.label()).collect();
        assert_eq!(
            seen,
            vec!["boxcar", "hann", "hamming", "blackman", "bartlett", "kaiser", "tukey"]
        );
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), FftWindow::ALL.len());
    }

    #[test]
    fn only_kaiser_and_tukey_take_a_parameter() {
        for w in FftWindow::ALL {
            let expected = matches!(w, FftWindow::Kaiser | FftWindow::Tukey);
            assert_eq!(w.needs_param(), expected, "{w:?}");
            if expected {
                assert!(w.default_param() > 0.0, "{w:?}");
            }
        }
        assert!(FftWindow::Tukey.default_param() <= 1.0);
    }
}
