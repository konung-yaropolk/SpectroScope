//! Small shared helpers.

/// Unix time in seconds, on every platform.
///
/// `std::time::SystemTime` is unavailable on wasm32-unknown-unknown, so the web
/// build reads the browser clock instead.
pub fn now_unix() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() / 1000.0
    }
}

/// A monotonic instant in seconds since an arbitrary origin.
pub fn now_monotonic() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        static ORIGIN: OnceLock<std::time::Instant> = OnceLock::new();
        ORIGIN
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_secs_f64()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // `performance.now()` is monotonic and in milliseconds.
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now() / 1000.0)
            .unwrap_or_else(|| js_sys::Date::now() / 1000.0)
    }
}

/// `1 h 2 min 3 s`, as QSpectrumAnalyzer's `utils.human_time()` formatted it.
pub fn human_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0 s".to_owned();
    }
    let total = seconds as u64;
    let (m, s) = (total / 60, total % 60);
    let (h, m) = (m / 60, m % 60);

    if h > 0 {
        format!("{h} h {m} min {s} s")
    } else if m > 0 {
        format!("{m} min {s} s")
    } else {
        format!("{s} s")
    }
}

/// A frequency in Hz rendered with an SI prefix, for axis ticks and readouts.
pub fn format_hz(hz: f64) -> String {
    let a = hz.abs();
    if a >= 1e9 {
        format!("{:.4} GHz", hz / 1e9)
    } else if a >= 1e6 {
        format!("{:.3} MHz", hz / 1e6)
    } else if a >= 1e3 {
        format!("{:.3} kHz", hz / 1e3)
    } else {
        format!("{hz:.0} Hz")
    }
}

/// Split a command line the way the Python version's `shlex.split()` did, so
/// quoted paths in the *Executable* field keep working.
pub fn split_args(s: &str) -> Vec<String> {
    shell_words::split(s).unwrap_or_else(|_| s.split_whitespace().map(|w| w.to_owned()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_time_matches_python_format() {
        assert_eq!(human_time(0.0), "0 s");
        assert_eq!(human_time(5.7), "5 s");
        assert_eq!(human_time(63.0), "1 min 3 s");
        assert_eq!(human_time(3723.0), "1 h 2 min 3 s");
        assert_eq!(human_time(7200.0), "2 h 0 min 0 s");
    }

    #[test]
    fn human_time_handles_nonsense() {
        assert_eq!(human_time(-1.0), "0 s");
        assert_eq!(human_time(f64::NAN), "0 s");
        assert_eq!(human_time(f64::INFINITY), "0 s");
    }

    #[test]
    fn format_hz_picks_a_prefix() {
        assert_eq!(format_hz(500.0), "500 Hz");
        assert_eq!(format_hz(1_500.0), "1.500 kHz");
        assert_eq!(format_hz(88_500_000.0), "88.500 MHz");
        assert_eq!(format_hz(2_400_000_000.0), "2.4000 GHz");
    }

    #[test]
    fn split_args_respects_quotes() {
        assert_eq!(
            split_args("soapy_power --device \"driver=rtlsdr, serial=1\""),
            vec!["soapy_power", "--device", "driver=rtlsdr, serial=1"]
        );
        assert_eq!(split_args(""), Vec::<String>::new());
        // An unbalanced quote must not lose the whole command line.
        assert_eq!(split_args("foo \"bar"), vec!["foo", "\"bar"]);
    }

    #[test]
    fn monotonic_clock_moves_forward() {
        let a = now_monotonic();
        let b = now_monotonic();
        assert!(b >= a);
    }
}
