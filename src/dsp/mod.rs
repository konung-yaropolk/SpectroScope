//! Signal processing.
//!
//! Only the sources that acquire raw IQ themselves (`rtl_tcp`, `demo`) need an
//! FFT; the helper-process backends arrive pre-transformed. The FFT is behind a
//! trait so that FFTW -- which is what the original toolchain used, via
//! `pyfftw` in `soapy_power` and libfftw in `rtl_power_fftw` -- can be used
//! where it builds, and `rustfft` everywhere else (wasm32, Android).

pub mod fft;
pub mod psd;
pub mod smooth;
pub mod window;

pub use fft::{plan, Fft};
pub use psd::{Welch, WelchConfig};
pub use smooth::{smooth, SmoothWindow, Smoother};
pub use window::FftWindow;

/// Name of the FFT library this build actually uses, for the About window.
pub fn fft_backend_name() -> &'static str {
    fft::backend_name()
}
