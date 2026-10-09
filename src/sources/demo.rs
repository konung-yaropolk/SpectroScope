//! A synthetic spectrum source: the application stays usable, demonstrable and
//! testable with no radio attached at all.
//!
//! This has no counterpart in QSpectrumAnalyzer, which always needed hardware.
//! It matters most on the web and on Android, where there is no helper process
//! to spawn and no USB device to open, and it is what the screenshots show.

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use super::{
    EventSink, Frame, Limit, Limits, SourceError, SourceInfo, SourceKind, SourceSession,
    SpectrumSource, SweepConfig,
};

static INFO: SourceInfo = SourceInfo {
    id: "demo",
    label: "demo (synthetic signal)",
    kind: SourceKind::Synthetic,
    default_executable: "",
    additional_params: "",
    has_device_help: false,
    // Nothing constrains a generator, so the limits only need to keep the spin
    // boxes from producing something unplottable.
    limits: Limits {
        sample_rate: Limit::new(0.0, 61_440_000.0, 2_560_000.0),
        bandwidth: Limit::new(0.0, 0.0, 0.0),
        gain: Limit::new(-1.0, 100.0, 37.0),
        start_freq: Limit::new(0.0, 7250.0, 87.0),
        stop_freq: Limit::new(0.0, 7250.0, 108.0),
        bin_size: Limit::new(0.001, 10_000.0, 10.0),
        interval: Limit::new(0.0, 3600.0, 0.1),
        ppm: Limit::new(-999, 999, 0),
        crop: Limit::new(0, 49, 0),
    },
};

// ---------------------------------------------------------------------------
// Shape of the synthetic spectrum
// ---------------------------------------------------------------------------

const NOISE_FLOOR_DBM: f32 = -95.0;

/// Spread of the per-bin noise, dB. A power detector that averages a handful of
/// FFTs is close to Gaussian in the dB domain with roughly this deviation,
/// which is what the real backends deliver once their averaging has run.
const NOISE_SIGMA_DB: f32 = 1.1;

/// Front-end gain slope from one end of the span to the other, dB.
const NOISE_TILT_DB: f32 = 4.0;

/// Slow wander of the whole floor, so the waterfall is not a flat wall.
const NOISE_DRIFT_DB: f32 = 1.5;
const NOISE_DRIFT_PERIOD_S: f64 = 37.0;

/// The gain control is referenced to the backend's own default, so that leaving
/// it alone (or choosing "auto") puts the floor exactly at [`NOISE_FLOOR_DBM`].
const REFERENCE_GAIN_DB: f64 = 37.0;

/// An `interval` of zero would generate sweeps as fast as a core can manage,
/// pegging it and filling the event channel faster than the GUI drains it, so
/// zero means "as fast as is sensible" rather than "as fast as possible".
const MIN_INTERVAL_S: f64 = 0.02;
const MAX_INTERVAL_S: f64 = 3600.0;

/// Fewer bins than this is not a spectrum; more is almost always a mistyped bin
/// size that would otherwise allocate gigabytes.
const MIN_BINS: usize = 16;
const MAX_BINS: usize = 1_000_000;

/// How far down from its peak a carrier is still evaluated. Past this it is far
/// below the noise floor, so skipping it keeps a sweep O(bins) instead of
/// O(bins x carriers).
const CARRIER_RANGE_DB: f64 = 60.0;

/// `10 / ln(10)`: converts a natural-log power ratio to dB.
const DB_PER_NEPER: f64 = 4.342_944_819_032_518;

/// One synthetic emitter.
struct Carrier {
    /// Centre as a fraction of the swept span, so the demo looks right whatever
    /// frequency range the user dials in.
    centre: f64,
    /// Fractions of the span the centre advances per second. The one non-zero
    /// entry draws a diagonal streak, which is what makes it obvious that the
    /// waterfall's time axis runs the way it claims to.
    drift_per_s: f64,
    /// Nominal width as a Gaussian sigma, Hz.
    sigma_hz: f64,
    /// Bounds on that width as `(min, max)` fractions of the span. A carrier
    /// that kept its absolute width would swamp a narrow span and disappear
    /// from a wide one, so these bounds are what make the demo look like a
    /// spectrum at every zoom level; `min` is also how the wideband emission
    /// stays recognisably wide.
    sigma_frac: (f64, f64),
    peak_dbm: f32,
    /// Peak-to-peak amplitude modulation and its period, which gives the
    /// waterfall vertical structure.
    am_db: f32,
    am_period_s: f64,
    am_phase: f64,
    /// Super-Gaussian order. 1 is a plain Gaussian, i.e. a tone plus FFT
    /// leakage; 3 is flat-topped with steep skirts, like an occupied FM channel.
    /// Without this a Gaussian's skirts would be ten sigma wide by the time they
    /// reached the noise floor and every carrier would look like a hill.
    order: i32,
}

const CARRIERS: &[Carrier] = &[
    // Broadcast-FM-like channels: order 3 gives them a flat top and steep
    // skirts, so 60 kHz of sigma reads as the usual ~200 kHz of occupancy.
    Carrier {
        centre: 0.14,
        drift_per_s: 0.0,
        sigma_hz: 60e3,
        sigma_frac: (0.0, 0.012),
        peak_dbm: -42.0,
        am_db: 5.0,
        am_period_s: 11.0,
        am_phase: 0.0,
        order: 3,
    },
    Carrier {
        centre: 0.33,
        drift_per_s: 0.0,
        sigma_hz: 55e3,
        sigma_frac: (0.0, 0.012),
        peak_dbm: -36.0,
        am_db: 3.0,
        am_period_s: 7.0,
        am_phase: 1.7,
        order: 3,
    },
    Carrier {
        centre: 0.61,
        drift_per_s: 0.0,
        sigma_hz: 75e3,
        sigma_frac: (0.0, 0.012),
        peak_dbm: -48.0,
        am_db: 8.0,
        am_period_s: 19.0,
        am_phase: 0.6,
        order: 3,
    },
    // Unmodulated carriers: order 1 is the skirt an FFT leaves around a tone.
    Carrier {
        centre: 0.47,
        drift_per_s: 0.0,
        sigma_hz: 1.5e3,
        sigma_frac: (0.0, 0.004),
        peak_dbm: -30.0,
        am_db: 1.0,
        am_period_s: 29.0,
        am_phase: 2.4,
        order: 1,
    },
    Carrier {
        centre: 0.88,
        drift_per_s: 0.0,
        sigma_hz: 1.2e3,
        sigma_frac: (0.0, 0.004),
        peak_dbm: -34.0,
        am_db: 2.0,
        am_period_s: 13.0,
        am_phase: 3.9,
        order: 1,
    },
    // A wideband emission; the sigma floor keeps it wide on a narrow span.
    Carrier {
        centre: 0.74,
        drift_per_s: 0.0,
        sigma_hz: 400e3,
        sigma_frac: (0.03, 0.05),
        peak_dbm: -66.0,
        am_db: 4.0,
        am_period_s: 23.0,
        am_phase: 5.2,
        order: 2,
    },
    // The drifter: one full pass of the band every 100 s.
    Carrier {
        centre: 0.05,
        drift_per_s: 0.01,
        sigma_hz: 25e3,
        sigma_frac: (0.0, 0.01),
        peak_dbm: -52.0,
        am_db: 2.0,
        am_period_s: 5.0,
        am_phase: 4.1,
        order: 2,
    },
];

impl Carrier {
    fn centre_frac(&self, t: f64) -> f64 {
        (self.centre + self.drift_per_s * t).rem_euclid(1.0)
    }

    fn peak_now(&self, t: f64) -> f32 {
        let phase = TAU * t / self.am_period_s + self.am_phase;
        self.peak_dbm + 0.5 * self.am_db * phase.sin() as f32
    }

    fn sigma(&self, span_hz: f64, bin_hz: f64) -> f64 {
        let (lo, hi) = self.sigma_frac;
        let scaled = self.sigma_hz.min(hi * span_hz).max(lo * span_hz);
        // A carrier narrower than a bin falls between samples and vanishes,
        // which would silently empty the demo of signals; visibility beats
        // cosmetics, so this floor overrides the span-relative cap.
        scaled.max(0.75 * bin_hz)
    }

    /// Half-width, in sigmas, at which the carrier has dropped
    /// [`CARRIER_RANGE_DB`] below its peak.
    fn reach_sigmas(&self) -> f64 {
        (CARRIER_RANGE_DB / (0.5 * DB_PER_NEPER)).powf(1.0 / (2.0 * self.order as f64))
    }
}

/// dB below the peak of a super-Gaussian of the given order, `u` sigmas out.
fn shape_drop_db(u: f64, order: i32) -> f64 {
    0.5 * u.powi(2 * order) * DB_PER_NEPER
}

/// Add two powers given in dB.
///
/// Carriers sit on top of the noise incoherently, and doing the sum in the power
/// domain is what gives their skirts the right shape where they meet the floor.
fn power_sum_db(a: f32, b: f32) -> f32 {
    // `max`/`min` drop a NaN operand rather than propagating it.
    let hi = a.max(b);
    let lo = a.min(b);
    if !hi.is_finite() {
        return NOISE_FLOOR_DBM;
    }
    hi + 10.0 * (1.0 + 10f32.powf((lo - hi) / 10.0)).log10()
}

/// Clamp the requested sweep interval into a range that cannot spin or stall.
///
/// Deliberately not `f64::clamp`, which *propagates* NaN and would hand a NaN
/// straight to `Duration::from_secs_f64`, where it panics. `max` then `min`
/// each return the other operand for NaN, so nonsense lands on the floor.
#[allow(clippy::manual_clamp)]
fn sweep_period(interval_s: f64) -> f64 {
    interval_s.max(MIN_INTERVAL_S).min(MAX_INTERVAL_S)
}

// ---------------------------------------------------------------------------
// Pseudo-random numbers
// ---------------------------------------------------------------------------

/// xorshift64*. Three shifts and a multiply, no dependency, and reproducible
/// from a seed, which is all a noise floor needs. Not cryptographic.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Zero is a fixed point of the xorshift step.
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`, using the 24 bits an `f32` can hold exactly.
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / 16_777_216.0)
    }

    /// Approximately normal with the given standard deviation. Three uniforms
    /// are indistinguishable from Box-Muller at this scale and much cheaper.
    fn normal(&mut self, sigma: f32) -> f32 {
        let s = self.next_f32() + self.next_f32() + self.next_f32();
        (s - 1.5) * 2.0 * sigma
    }
}

/// Successive runs get different noise while the sequence stays reproducible
/// from process start, which is what lets the tests pin a seed.
fn next_seed() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// The generator
// ---------------------------------------------------------------------------

struct Generator {
    axis: Arc<Vec<f64>>,
    bins: usize,
    start_hz: f64,
    span_hz: f64,
    bin_hz: f64,
    gain_offset_db: f32,
    /// Synthetic seconds, advanced by the sweep period instead of read from a
    /// clock: a run is then reproducible, and a stalled GUI does not make the
    /// modulation jump.
    time_s: f64,
    period_s: f64,
    rng: Rng,
}

impl Generator {
    fn new(cfg: &SweepConfig, seed: u64) -> Result<Self, SourceError> {
        let requested = cfg.bin_count();
        if requested == 0 {
            return Err(SourceError::InvalidConfig(
                "the demo source needs a positive span and a positive bin size".to_owned(),
            ));
        }
        let span_hz = cfg.span_hz();
        if !span_hz.is_finite() || span_hz <= 0.0 {
            return Err(SourceError::InvalidConfig(
                "start and stop frequency do not describe a span".to_owned(),
            ));
        }

        let bins = requested.clamp(MIN_BINS, MAX_BINS);
        // A negative gain is the "auto" sentinel, and a NaN would spread through
        // every bin; both mean "whatever the backend would have picked".
        let gain = if cfg.gain_db.is_finite() && cfg.gain_db >= 0.0 {
            cfg.gain_db
        } else {
            REFERENCE_GAIN_DB
        };

        Ok(Self {
            axis: cfg.linear_axis(bins),
            bins,
            start_hz: cfg.start_freq_mhz * 1e6,
            span_hz,
            bin_hz: span_hz / (bins - 1) as f64,
            gain_offset_db: (gain - REFERENCE_GAIN_DB) as f32,
            time_s: 0.0,
            period_s: sweep_period(cfg.interval_s),
            rng: Rng::new(seed),
        })
    }

    fn next_frame(&mut self) -> Frame {
        let t = self.time_s;
        self.time_s += self.period_s;

        let drift = NOISE_DRIFT_DB * (TAU * t / NOISE_DRIFT_PERIOD_S).sin() as f32;
        let base = NOISE_FLOOR_DBM + drift + self.gain_offset_db;
        let last = self.bins - 1;

        let mut y = Vec::with_capacity(self.bins);
        for i in 0..self.bins {
            let tilt = NOISE_TILT_DB * (i as f32 / last as f32 - 0.5);
            y.push(base + tilt + self.rng.normal(NOISE_SIGMA_DB));
        }

        for carrier in CARRIERS {
            let sigma = carrier.sigma(self.span_hz, self.bin_hz);
            let centre_hz = self.start_hz + carrier.centre_frac(t) * self.span_hz;
            let reach_hz = sigma * carrier.reach_sigmas();
            if !(sigma.is_finite() && centre_hz.is_finite() && reach_hz.is_finite()) {
                continue;
            }

            // Saturating float-to-int casts turn an out-of-band carrier into an
            // empty range rather than a wild index.
            let lo = (((centre_hz - reach_hz) - self.start_hz) / self.bin_hz).floor();
            let hi = (((centre_hz + reach_hz) - self.start_hz) / self.bin_hz).ceil();
            let lo = (lo.max(0.0) as usize).min(last);
            let hi = (hi.max(0.0) as usize).min(last);
            if lo > hi {
                continue;
            }

            let peak = carrier.peak_now(t) + self.gain_offset_db;
            for (i, slot) in y[lo..=hi].iter_mut().enumerate() {
                let u = (self.axis[lo + i] - centre_hz) / sigma;
                *slot = power_sum_db(*slot, peak - shape_drop_db(u, carrier.order) as f32);
            }
        }

        Frame {
            timestamp: crate::util::now_unix(),
            x: Arc::clone(&self.axis),
            y,
        }
    }
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

/// Everything was delivered before `start` returned, so there is nothing to
/// tear down.
struct IdleSession;

impl SourceSession for IdleSession {
    fn stop(&mut self) {}
}

#[cfg(not(target_arch = "wasm32"))]
struct ThreadSession {
    alive: Arc<AtomicBool>,
    wake: crossbeam_channel::Sender<()>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SourceSession for ThreadSession {
    fn stop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        // Cuts a long inter-sweep wait short; a full or closed channel means the
        // worker is already awake or already gone.
        let _ = self.wake.try_send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for ThreadSession {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_sweeps(
    mut generator: Generator,
    sink: EventSink,
) -> Result<Box<dyn SourceSession>, SourceError> {
    use crossbeam_channel::RecvTimeoutError;
    use std::time::Duration;

    let alive = Arc::new(AtomicBool::new(true));
    // The flag is what the loop tests; the channel exists only so the wait
    // between sweeps is interruptible instead of polled in small slices.
    let (wake, wake_rx) = crossbeam_channel::bounded::<()>(1);

    let handle = {
        let alive = Arc::clone(&alive);
        std::thread::Builder::new()
            .name("spectroscope-demo".into())
            .spawn(move || {
                if sink.started(0) {
                    let period = generator.period_s;
                    let mut next = crate::util::now_monotonic() + period;
                    while alive.load(Ordering::Relaxed) {
                        if !sink.frame(generator.next_frame()) {
                            break;
                        }
                        let wait = next - crate::util::now_monotonic();
                        next = if wait > 0.0 {
                            match wake_rx.recv_timeout(Duration::from_secs_f64(wait)) {
                                Err(RecvTimeoutError::Timeout) => next + period,
                                // `stop()` signalled, or the session was dropped.
                                _ => break,
                            }
                        } else {
                            // Generating the sweep outran the interval; resync
                            // rather than fire a catch-up burst.
                            crate::util::now_monotonic() + period
                        };
                    }
                }
                sink.stopped();
            })
            .map_err(|e| SourceError::Io(e.to_string()))?
    };

    Ok(Box::new(ThreadSession {
        alive,
        wake,
        handle: Some(handle),
    }))
}

#[cfg(target_arch = "wasm32")]
struct TimerSession {
    alive: Arc<AtomicBool>,
}

#[cfg(target_arch = "wasm32")]
impl SourceSession for TimerSession {
    fn stop(&mut self) {
        // The sweep task only runs when the GUI thread yields, so it cannot
        // still be mid-sweep here and there is nothing to wait for; it observes
        // the flag and emits `Stopped` on its next turn.
        self.alive.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for TimerSession {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `wasm32-unknown-unknown` has no threads and nothing may block the GUI
/// thread, so the sweep loop is an async task on the browser's own event loop.
/// Awaiting a `setTimeout` between sweeps yields to rendering exactly the way a
/// thread's sleep yields to the scheduler, which keeps the `EventSink` contract
/// unchanged: the task pushes the same events from the same channel, and the
/// GUI drains them on its next repaint.
#[cfg(target_arch = "wasm32")]
fn spawn_sweeps(
    mut generator: Generator,
    sink: EventSink,
) -> Result<Box<dyn SourceSession>, SourceError> {
    let alive = Arc::new(AtomicBool::new(true));
    let running = Arc::clone(&alive);

    wasm_bindgen_futures::spawn_local(async move {
        if sink.started(0) {
            let delay_ms = (generator.period_s * 1000.0).round().max(1.0) as i32;
            while running.load(Ordering::Relaxed) {
                if !sink.frame(generator.next_frame()) {
                    break;
                }
                sleep_ms(delay_ms).await;
            }
        }
        sink.stopped();
    });

    Ok(Box::new(TimerSession { alive }))
}

#[cfg(target_arch = "wasm32")]
async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

// ---------------------------------------------------------------------------
// The source
// ---------------------------------------------------------------------------

pub struct Demo;

impl SpectrumSource for Demo {
    fn info(&self) -> &'static SourceInfo {
        &INFO
    }

    fn start(
        &self,
        cfg: SweepConfig,
        sink: EventSink,
    ) -> Result<Box<dyn SourceSession>, SourceError> {
        let mut generator = Generator::new(&cfg, next_seed())?;

        sink.log(format!(
            "Synthetic source: {} bins over {:.3}-{:.3} MHz, {:.3} s per sweep",
            generator.bins, cfg.start_freq_mhz, cfg.stop_freq_mhz, generator.period_s
        ));

        if cfg.single_shot {
            sink.started(0);
            sink.frame(generator.next_frame());
            sink.stopped();
            return Ok(Box::new(IdleSession));
        }

        spawn_sweeps(generator, sink)
    }

    fn params_help(&self, _executable: &str) -> String {
        "The demo source generates its data internally and runs no executable, \
         so there are no additional parameters.\n\n\
         Gain shifts the whole trace, bin size sets the number of points, and \
         interval sets the time between sweeps. The carriers sit at fixed \
         fractions of whatever span you choose, and one of them drifts."
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceEvent;

    const SEED: u64 = 0x1234_5678_9ABC_DEF0;

    fn cfg() -> SweepConfig {
        SweepConfig {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 108.0,
            bin_size_khz: 10.0,
            interval_s: 0.1,
            gain_db: REFERENCE_GAIN_DB,
            ..Default::default()
        }
    }

    fn generator_for(c: &SweepConfig) -> Generator {
        match Generator::new(c, SEED) {
            Ok(g) => g,
            Err(e) => panic!("config rejected: {e}"),
        }
    }

    fn median(values: &[f32]) -> f32 {
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sorted[sorted.len() / 2]
    }

    #[test]
    fn bin_count_follows_bin_size() {
        let coarse = cfg();
        let fine = SweepConfig {
            bin_size_khz: 2.5,
            ..cfg()
        };

        let a = generator_for(&coarse).next_frame();
        let b = generator_for(&fine).next_frame();

        assert_eq!(a.y.len(), coarse.bin_count());
        assert_eq!(b.y.len(), fine.bin_count());
        assert_eq!(b.y.len(), 4 * a.y.len());
        assert_eq!(a.x.len(), a.y.len());
        assert_eq!(b.x.len(), b.y.len());
    }

    #[test]
    fn axis_is_ascending_and_shared_between_frames() {
        let c = cfg();
        let mut g = generator_for(&c);
        let first = g.next_frame();
        let second = g.next_frame();

        assert!(
            Arc::ptr_eq(&first.x, &second.x),
            "axis is rebuilt per frame"
        );
        assert!(
            first.x.windows(2).all(|w| w[1] > w[0]),
            "axis not ascending"
        );
        assert!((first.x[0] - 87e6).abs() < 1.0);
        assert!((first.x[first.x.len() - 1] - 108e6).abs() < 1.0);
    }

    #[test]
    fn trace_has_a_floor_and_real_peaks() {
        let c = cfg();
        let f = generator_for(&c).next_frame();

        assert!(f.y.iter().all(|v| v.is_finite()), "non-finite bin");

        let mid = median(&f.y);
        let max = f.y.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((-105.0..-85.0).contains(&mid), "implausible floor {mid}");
        assert!(max > mid + 40.0, "no peaks: max {max}, floor {mid}");
        assert!(max < 0.0, "implausible peak {max}");

        // Most of the band must still be noise, or the "peaks" are just tilt.
        let near_floor = f.y.iter().filter(|v| (**v - mid).abs() < 6.0).count();
        assert!(
            near_floor * 2 > f.y.len(),
            "only {near_floor} of {} bins near the floor",
            f.y.len()
        );
    }

    #[test]
    fn gain_shifts_the_whole_trace() {
        let reference = generator_for(&cfg()).next_frame();
        let louder = generator_for(&SweepConfig {
            gain_db: REFERENCE_GAIN_DB + 20.0,
            ..cfg()
        })
        .next_frame();

        assert_eq!(reference.y.len(), louder.y.len());
        for (a, b) in reference.y.iter().zip(louder.y.iter()) {
            assert!((b - a - 20.0).abs() < 0.05, "{a} dB became {b} dB");
        }
    }

    #[test]
    fn auto_gain_matches_the_reference_gain() {
        let reference = generator_for(&cfg()).next_frame();
        let auto = generator_for(&SweepConfig {
            gain_db: -1.0,
            ..cfg()
        })
        .next_frame();
        assert_eq!(reference.y, auto.y);
    }

    #[test]
    fn nonsense_gain_falls_back_instead_of_poisoning_the_trace() {
        let f = generator_for(&SweepConfig {
            gain_db: f64::NAN,
            ..cfg()
        })
        .next_frame();
        assert!(f.y.iter().all(|v| v.is_finite()));
        assert_eq!(f.y, generator_for(&cfg()).next_frame().y);
    }

    #[test]
    fn degenerate_configs_are_rejected() {
        let cases = [
            SweepConfig {
                stop_freq_mhz: 87.0,
                ..cfg()
            },
            SweepConfig {
                bin_size_khz: 0.0,
                ..cfg()
            },
            SweepConfig {
                bin_size_khz: -10.0,
                ..cfg()
            },
            SweepConfig {
                start_freq_mhz: 108.0,
                stop_freq_mhz: 87.0,
                ..cfg()
            },
            SweepConfig {
                stop_freq_mhz: f64::NAN,
                ..cfg()
            },
            // A bin far wider than the span rounds to no bins at all.
            SweepConfig {
                stop_freq_mhz: 87.1,
                bin_size_khz: 10_000.0,
                ..cfg()
            },
        ];

        for bad in cases {
            assert!(
                matches!(
                    Generator::new(&bad, SEED),
                    Err(SourceError::InvalidConfig(_))
                ),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn start_surfaces_a_degenerate_config() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let bad = SweepConfig {
            bin_size_khz: 0.0,
            ..cfg()
        };
        let result = Demo.start(bad, EventSink::new(tx, None));
        assert!(
            matches!(result, Err(SourceError::InvalidConfig(_))),
            "expected InvalidConfig, got {:?}",
            result.err()
        );
        assert!(
            !rx.into_iter().any(|e| matches!(e, SourceEvent::Frame(_))),
            "a rejected config still produced a frame"
        );
    }

    #[test]
    fn tiny_bin_counts_are_clamped_up() {
        let c = SweepConfig {
            start_freq_mhz: 100.0,
            stop_freq_mhz: 101.0,
            bin_size_khz: 100.0,
            ..cfg()
        };
        assert_eq!(c.bin_count(), 10);

        let f = generator_for(&c).next_frame();
        assert_eq!(f.y.len(), MIN_BINS);
        assert_eq!(f.x.len(), MIN_BINS);
        assert!(f.y.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn absurd_bin_counts_are_clamped_down() {
        let c = SweepConfig {
            start_freq_mhz: 0.0,
            stop_freq_mhz: 7250.0,
            bin_size_khz: 0.001,
            ..cfg()
        };
        let g = generator_for(&c);
        assert_eq!(g.bins, MAX_BINS);
        assert_eq!(g.axis.len(), MAX_BINS);
    }

    #[test]
    fn successive_sweeps_differ() {
        let c = cfg();
        let mut g = generator_for(&c);
        let a = g.next_frame();
        let b = g.next_frame();
        assert_ne!(a.y, b.y, "the waterfall would be a flat wall");
        assert!(b.timestamp >= a.timestamp);
        assert!(b.timestamp > 0.0);
    }

    #[test]
    fn carriers_stay_in_band_and_one_of_them_moves() {
        let mut movers = 0;
        for carrier in CARRIERS {
            let positions: Vec<f64> = (0..32)
                .map(|k| carrier.centre_frac(k as f64 * 7.0))
                .collect();
            assert!(
                positions.iter().all(|f| (0.0..1.0).contains(f)),
                "carrier left the span: {positions:?}"
            );
            if positions.windows(2).any(|w| (w[1] - w[0]).abs() > 0.02) {
                movers += 1;
            } else {
                assert_eq!(carrier.centre_frac(0.0), carrier.centre_frac(1234.0));
            }
        }
        assert!(
            movers > 0,
            "no carrier drifts, so the time axis proves nothing"
        );
    }

    #[test]
    fn a_narrow_span_still_looks_like_a_spectrum() {
        // 200 kHz: narrower than a single nominal FM channel, so without the
        // span-relative width cap the carriers would swamp the whole band.
        let c = SweepConfig {
            start_freq_mhz: 433.0,
            stop_freq_mhz: 433.2,
            bin_size_khz: 0.5,
            ..cfg()
        };
        let f = generator_for(&c).next_frame();

        assert!(f.y.iter().all(|v| v.is_finite()));
        let mid = median(&f.y);
        let max = f.y.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((-105.0..-85.0).contains(&mid), "band is all signal: {mid}");
        assert!(max > mid + 30.0, "nothing visible: max {max}, floor {mid}");
    }

    #[test]
    fn carrier_widths_track_the_span() {
        let narrow = CARRIERS[0].sigma(200e3, 500.0);
        let wide = CARRIERS[0].sigma(21e6, 10e3);
        assert!(narrow < wide, "{narrow} vs {wide}");
        // A sub-bin carrier is widened so it cannot fall between samples.
        assert!(CARRIERS[3].sigma(21e6, 10e3) >= 7.5e3);
    }

    #[test]
    fn sweep_period_never_spins_or_stalls() {
        assert_eq!(sweep_period(0.0), MIN_INTERVAL_S);
        assert_eq!(sweep_period(-5.0), MIN_INTERVAL_S);
        assert_eq!(sweep_period(f64::NAN), MIN_INTERVAL_S);
        assert_eq!(sweep_period(f64::INFINITY), MAX_INTERVAL_S);
        assert_eq!(sweep_period(1e9), MAX_INTERVAL_S);
        assert_eq!(sweep_period(0.5), 0.5);
    }

    #[test]
    fn power_sum_is_incoherent_addition() {
        assert!((power_sum_db(0.0, 0.0) - 3.0103).abs() < 1e-3);
        assert!((power_sum_db(0.0, -20.0) - 0.0432).abs() < 1e-3);
        assert!((power_sum_db(-95.0, f32::NEG_INFINITY) + 95.0).abs() < 1e-3);
        assert!(power_sum_db(-95.0, f32::NAN).is_finite());
        assert!(power_sum_db(f32::NAN, f32::NAN).is_finite());
    }

    #[test]
    fn shape_drop_grows_with_distance_and_order() {
        assert_eq!(shape_drop_db(0.0, 3), 0.0);
        assert!(shape_drop_db(1.0, 1) < shape_drop_db(2.0, 1));
        // A higher order is flatter on top and steeper in the skirts.
        assert!(shape_drop_db(0.5, 3) < shape_drop_db(0.5, 1));
        assert!(shape_drop_db(2.0, 3) > shape_drop_db(2.0, 1));
    }

    #[test]
    fn rng_is_reproducible_and_bounded() {
        let draw = |seed| {
            let mut r = Rng::new(seed);
            (0..64).map(|_| r.next_f32()).collect::<Vec<f32>>()
        };
        assert_eq!(draw(7), draw(7));
        assert_ne!(draw(7), draw(8));
        assert!(draw(7).iter().all(|v| (0.0..1.0).contains(v)));

        // A zero seed must not stick at zero.
        let mut zero = Rng::new(0);
        assert_ne!(zero.next_u64(), 0);
        assert_ne!(zero.next_u64(), 0);

        let mut r = Rng::new(99);
        let samples: Vec<f32> = (0..4096).map(|_| r.normal(2.0)).collect();
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!(mean.abs() < 0.2, "noise is biased: mean {mean}");
        assert!(samples.iter().any(|v| *v > 1.0) && samples.iter().any(|v| *v < -1.0));
    }

    #[test]
    fn single_shot_emits_one_frame_then_stops() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let c = SweepConfig {
            single_shot: true,
            ..cfg()
        };
        let mut session = match Demo.start(c, EventSink::new(tx, None)) {
            Ok(s) => s,
            Err(e) => panic!("start failed: {e}"),
        };
        session.stop();
        session.stop();

        let (mut started, mut frames, mut stopped) = (0, 0, 0);
        for event in rx.try_iter() {
            match event {
                SourceEvent::Started { hops } => {
                    assert_eq!(hops, 0, "the demo does not hop");
                    started += 1;
                }
                SourceEvent::Frame(f) => {
                    assert_eq!(f.x.len(), f.y.len());
                    assert!(!f.y.is_empty());
                    frames += 1;
                }
                SourceEvent::Stopped => stopped += 1,
                SourceEvent::Error(e) => panic!("unexpected error: {e}"),
                SourceEvent::Log(_) => {}
            }
        }
        assert_eq!((started, frames, stopped), (1, 1, 1));
    }

    /// The continuous path needs a worker, which only exists off the web.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn continuous_run_streams_frames_and_stops_cleanly() {
        use std::time::{Duration, Instant};

        let (tx, rx) = crossbeam_channel::unbounded();
        // Zero interval: must still deliver frames, and must not spin.
        let c = SweepConfig {
            interval_s: 0.0,
            ..cfg()
        };
        let mut session = match Demo.start(c, EventSink::new(tx, None)) {
            Ok(s) => s,
            Err(e) => panic!("start failed: {e}"),
        };

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut started = false;
        let mut frames = 0;
        while Instant::now() < deadline && frames < 3 {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(SourceEvent::Started { .. }) => started = true,
                Ok(SourceEvent::Frame(_)) => frames += 1,
                Ok(SourceEvent::Stopped) => panic!("stopped without being asked"),
                Ok(SourceEvent::Error(e)) => panic!("unexpected error: {e}"),
                Ok(SourceEvent::Log(_)) => {}
                Err(_) => {}
            }
        }
        assert!(started, "never saw Started");
        assert!(frames >= 3, "only {frames} frames in 10 s");

        session.stop();
        session.stop();

        assert!(
            rx.try_iter().any(|e| matches!(e, SourceEvent::Stopped)),
            "no Stopped after stop()"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dropping_a_session_stops_the_worker() {
        use std::time::Duration;

        let (tx, rx) = crossbeam_channel::unbounded();
        let session = match Demo.start(cfg(), EventSink::new(tx, None)) {
            Ok(s) => s,
            Err(e) => panic!("start failed: {e}"),
        };
        drop(session);

        let mut stopped = false;
        while let Ok(event) = rx.recv_timeout(Duration::from_secs(5)) {
            if matches!(event, SourceEvent::Stopped) {
                stopped = true;
                break;
            }
        }
        assert!(stopped, "dropping the session left the worker running");
    }
}
