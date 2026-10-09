//! The `rtl_tcp` backend: an RTL-SDR reached over the network, with the
//! frequency hopping and the FFT done here instead of in a helper process.
//!
//! QSpectrumAnalyzer had no equivalent -- all of its backends shelled out to a
//! binary that did the tuning and the transform. This one speaks the `rtl_tcp`
//! wire protocol itself, which is the only way to reach a radio from the web
//! build and the only way to use one with no SDR tooling installed at all.
//!
//! # The *Device* field
//!
//! Native builds take `host[:port]`, where the port defaults to 1234:
//! `192.168.1.10`, `rtl.lan:1234`, `[fe80::1]:1234`, `tcp://10.0.0.5`, or an
//! empty field for `127.0.0.1:1234`.
//!
//! Web builds take a WebSocket URL, `ws://host:port` (`wss://` for TLS); a bare
//! `host[:port]` is turned into `ws://host:port`. A browser cannot open a TCP
//! socket, so a WebSocket-to-TCP bridge has to sit in front of `rtl_tcp`.
//!
//! # Layout
//!
//! Three parts, in this order: [`SweepEngine`], which owns every decision and
//! performs no I/O; the native driver, a worker thread around
//! `std::net::TcpStream`; and the web driver, the same engine fed from
//! `WebSocket` callbacks. Keeping the arithmetic out of the drivers is what
//! makes hop planning, cropping and stitching testable without a socket, and
//! stops the two transports from drifting apart.

use std::sync::Arc;

use crate::dsp::{FftWindow, Welch, WelchConfig};
use crate::util::now_unix;

use super::{
    EventSink, Frame, Limit, Limits, SourceError, SourceInfo, SourceKind, SourceSession,
    SpectrumSource, SweepConfig,
};

// ---------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------

/// The port `rtl_tcp` listens on unless it was told otherwise.
pub const DEFAULT_PORT: u16 = 1234;

/// `RTL0`, tuner type, number of tuner gains -- all big endian.
const GREETING_LEN: usize = 12;
const GREETING_MAGIC: [u8; 4] = *b"RTL0";

/// One `rtl_tcp` command. The wire form is the discriminant byte followed by a
/// big-endian `u32` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    /// Centre frequency, Hz.
    CentreFreq = 0x01,
    /// Sample rate, Hz.
    SampleRate = 0x02,
    /// 0 = tuner AGC, 1 = manual gain.
    GainMode = 0x03,
    /// Tuner gain, tenths of a dB.
    TunerGain = 0x04,
    /// Frequency correction, ppm.
    FreqCorrection = 0x05,
    /// IF gain, stage in the high half-word.
    IfGain = 0x06,
    TestMode = 0x07,
    /// RTL2832 digital AGC.
    AgcMode = 0x08,
    /// 0 = off, 1 = I branch, 2 = Q branch.
    DirectSampling = 0x09,
    OffsetTuning = 0x0a,
    RtlXtal = 0x0b,
    TunerXtal = 0x0c,
    /// Tuner gain selected by index into the server's gain table.
    GainByIndex = 0x0d,
    BiasTee = 0x0e,
}

impl Command {
    pub const ALL: [Self; 14] = [
        Self::CentreFreq,
        Self::SampleRate,
        Self::GainMode,
        Self::TunerGain,
        Self::FreqCorrection,
        Self::IfGain,
        Self::TestMode,
        Self::AgcMode,
        Self::DirectSampling,
        Self::OffsetTuning,
        Self::RtlXtal,
        Self::TunerXtal,
        Self::GainByIndex,
        Self::BiasTee,
    ];

    /// The five bytes to put on the wire.
    pub fn encode(self, arg: u32) -> [u8; 5] {
        let [a, b, c, d] = arg.to_be_bytes();
        [self as u8, a, b, c, d]
    }
}

/// Tuner chip reported in the greeting, as librtlsdr numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunerType {
    Unknown,
    E4000,
    Fc0012,
    Fc0013,
    Fc2580,
    R820T,
    R828D,
}

impl TunerType {
    fn from_wire(v: u32) -> Self {
        match v {
            1 => Self::E4000,
            2 => Self::Fc0012,
            3 => Self::Fc0013,
            4 => Self::Fc2580,
            5 => Self::R820T,
            6 => Self::R828D,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown tuner",
            Self::E4000 => "E4000",
            Self::Fc0012 => "FC0012",
            Self::Fc0013 => "FC0013",
            Self::Fc2580 => "FC2580",
            Self::R820T => "R820T",
            Self::R828D => "R828D",
        }
    }
}

/// The server's 12-byte hello.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Greeting {
    pub tuner: TunerType,
    pub gain_count: u32,
}

/// What the start of the stream turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handshake {
    /// Fewer than [`GREETING_LEN`] bytes have arrived.
    Waiting,
    Greeted(Greeting),
    /// No `RTL0` magic: a bridge that strips the greeting, so the first bytes
    /// are already IQ.
    Raw,
}

// ---------------------------------------------------------------------------
// Sweep engine
// ---------------------------------------------------------------------------

/// Smallest and largest FFT we will plan. The lower bound keeps a coarse bin
/// size from degenerating into a handful of bins per hop; the upper one bounds
/// both the planning cost and the memory a sweep needs.
const MIN_FFT: usize = 64;
const MAX_FFT: usize = 65_536;

/// Even with `interval` at zero a hop averages a few periodograms, because a
/// single one is 5.6 dB of noise spread and unreadable.
const MIN_INTEGRATION_SEGMENTS: usize = 4;

/// librtlsdr hands `rtl_tcp` 256 kiB (131072 complex samples) per USB transfer
/// and `rtl_tcp` forwards whole buffers, so after a retune command at least one
/// complete buffer sampled at the *old* frequency can still be in flight.
/// Those samples have to be thrown away or every hop smears its neighbour.
const IN_FLIGHT_SAMPLES: usize = 131_072;

/// Time allowance on top of [`IN_FLIGHT_SAMPLES`] for the command's round trip
/// and the R820T PLL, which locks in well under a millisecond.
const SETTLE_SECONDS: f64 = 0.005;

/// Refuse configurations whose sweep would be absurd rather than spending
/// minutes per sweep or hundreds of megabytes on one frame.
const MAX_HOPS: usize = 100_000;
const MAX_SWEEP_BINS: usize = 2_097_152;

/// Substituted for a bin the PSD did not produce; low enough to be obviously
/// wrong on screen rather than silently plausible.
const MISSING_BIN_DB: f32 = -200.0;

/// All of the backend's logic: hop planning, settling, integration, cropping
/// and stitching, driven purely by bytes in and frames out.
pub struct SweepEngine {
    welch: Welch,
    fft_size: usize,
    /// Bins kept from the centre of each hop's PSD.
    keep: usize,
    /// Index of the first kept bin, i.e. how many bins the crop discards per
    /// edge.
    crop_bins: usize,
    /// Tuner centre frequency of every hop, Hz.
    centres: Vec<f64>,
    hop: usize,
    axis: Arc<Vec<f64>>,
    sweep: Vec<f32>,
    psd: Vec<f32>,
    commands: Vec<[u8; 5]>,
    /// Bytes that did not form a whole IQ pair, or arrived after a frame was
    /// handed out.
    pending: Vec<u8>,
    settle_samples: usize,
    settle_left: usize,
    integration_samples: usize,
    integrated: usize,
    handshake: Handshake,
    single_shot: bool,
    done: bool,
}

impl SweepEngine {
    pub fn new(cfg: &SweepConfig) -> Result<Self, SourceError> {
        let sample_rate = cfg.sample_rate;
        if !(sample_rate > 0.0) || !sample_rate.is_finite() {
            return Err(SourceError::InvalidConfig(
                "sample rate must be greater than zero".to_owned(),
            ));
        }
        let bin_size = cfg.bin_size_hz();
        if !(bin_size > 0.0) || !bin_size.is_finite() {
            return Err(SourceError::InvalidConfig(
                "bin size must be greater than zero".to_owned(),
            ));
        }

        let fft_size = fft_size_for(sample_rate, bin_size);
        let crop = if cfg.crop.is_finite() {
            cfg.crop.clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Cropping in whole bins, not in Hz: the kept block is then an exact
        // number of bins wide, so stepping the hop centres by it stitches into
        // one uniformly spaced axis with neither a gap nor an overlap.
        let crop_bins = (fft_size as f64 * crop).floor() as usize;
        let keep = fft_size.saturating_sub(2 * crop_bins);
        if keep == 0 {
            return Err(SourceError::InvalidConfig(format!(
                "a crop of {:.0}% leaves no usable bandwidth",
                crop * 100.0
            )));
        }
        let bin_width = sample_rate / fft_size as f64;
        let usable = keep as f64 * bin_width;

        let (tuner_start, tuner_stop) = (cfg.tuner_start_hz(), cfg.tuner_stop_hz());
        let span = tuner_stop - tuner_start;
        if !(span > 0.0) || !span.is_finite() {
            return Err(SourceError::InvalidConfig(
                "stop frequency must be above start frequency".to_owned(),
            ));
        }

        // Saturating, because `as` on a float that large yields usize::MAX
        // rather than wrapping; the limit check below rejects it either way.
        let hops = ((span / usable).ceil() as usize).max(1);
        let total_bins = hops.saturating_mul(keep);
        if hops > MAX_HOPS || total_bins > MAX_SWEEP_BINS {
            return Err(SourceError::InvalidConfig(format!(
                "{} MHz at {:.0} kHz usable bandwidth needs {hops} hops and {total_bins} bins; \
                 narrow the span, raise the sample rate or lower the crop",
                span / 1e6,
                usable / 1e3,
            )));
        }

        // Hop h is centred so that its kept bins occupy
        // [start + h*usable, start + (h+1)*usable), and an fftshifted PSD puts
        // DC at index fft_size/2; together that makes the stitched axis exactly
        // `displayed start + i * bin_width`. Built once, shared by every frame.
        let centres = (0..hops)
            .map(|h| tuner_start + usable * (h as f64 + 0.5))
            .collect::<Vec<f64>>();
        let axis_origin = cfg.start_freq_mhz * 1e6;
        let axis = Arc::new(
            (0..total_bins)
                .map(|i| axis_origin + bin_width * i as f64)
                .collect::<Vec<f64>>(),
        );

        let hop_seconds = if cfg.interval_s.is_finite() && cfg.interval_s > 0.0 {
            cfg.interval_s / hops as f64
        } else {
            0.0
        };
        let integration_samples = ((sample_rate * hop_seconds) as usize)
            .max(MIN_INTEGRATION_SEGMENTS.saturating_mul(fft_size));

        let settle_samples = ((sample_rate * SETTLE_SECONDS) as usize)
            .max(IN_FLIGHT_SAMPLES)
            .max(fft_size);

        let welch = Welch::new(WelchConfig {
            fft_size,
            window: FftWindow::Hann,
            window_param: None,
            overlap: 50.0,
            remove_dc: true,
        });

        let mut engine = Self {
            welch,
            fft_size,
            keep,
            crop_bins,
            centres,
            hop: 0,
            axis,
            sweep: Vec::with_capacity(total_bins),
            psd: Vec::with_capacity(fft_size),
            commands: Vec::new(),
            pending: Vec::new(),
            settle_samples,
            settle_left: settle_samples,
            integration_samples,
            integrated: 0,
            handshake: Handshake::Waiting,
            single_shot: cfg.single_shot,
            done: false,
        };
        engine.queue_setup(cfg);
        Ok(engine)
    }

    /// Everything the server must be told before the first hop, in an order
    /// that matters: the gain mode has to be manual before a gain is accepted.
    fn queue_setup(&mut self, cfg: &SweepConfig) {
        self.commands
            .push(Command::SampleRate.encode(u32_arg(cfg.sample_rate)));
        if cfg.ppm != 0 {
            // librtlsdr logs an error for a redundant zero, and zero is the
            // state the server already starts in.
            self.commands
                .push(Command::FreqCorrection.encode(cfg.ppm as u32));
        }
        if cfg.gain_db.is_finite() && cfg.gain_db >= 0.0 {
            self.commands.push(Command::GainMode.encode(1));
            self.commands
                .push(Command::TunerGain.encode(u32_arg(cfg.gain_db * 10.0)));
        } else {
            self.commands.push(Command::GainMode.encode(0));
        }
        self.queue_retune();
    }

    fn queue_retune(&mut self) {
        if let Some(centre) = self.centres.get(self.hop) {
            self.commands
                .push(Command::CentreFreq.encode(u32_arg(*centre)));
        }
    }

    pub fn hops(&self) -> usize {
        self.centres.len()
    }

    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// Bins in one frame.
    pub fn bins(&self) -> usize {
        self.axis.len()
    }

    /// Tuner centre frequency of every hop, Hz -- the LNB LO is *not* added
    /// back, because this is what the radio is told.
    pub fn hop_centres_hz(&self) -> &[f64] {
        &self.centres
    }

    pub fn axis(&self) -> &Arc<Vec<f64>> {
        &self.axis
    }

    pub fn handshake(&self) -> Handshake {
        self.handshake
    }

    /// True once a single-shot run has delivered its sweep; the driver should
    /// close the connection.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Commands that must be sent before the current hop can be sampled.
    pub fn pending_commands(&mut self) -> Vec<[u8; 5]> {
        std::mem::take(&mut self.commands)
    }

    /// Feed received IQ bytes; yields a completed sweep when one is ready.
    ///
    /// At most one frame comes back per call, so a driver handed a large chunk
    /// must keep calling with an empty slice until it gets `None`.
    pub fn push(&mut self, bytes: &[u8]) -> Option<Frame> {
        if self.pending.is_empty() {
            let (frame, used) = self.consume(bytes);
            if used < bytes.len() {
                self.pending.extend_from_slice(&bytes[used..]);
            }
            frame
        } else {
            let mut buffered = std::mem::take(&mut self.pending);
            buffered.extend_from_slice(bytes);
            let (frame, used) = self.consume(&buffered);
            buffered.drain(..used);
            self.pending = buffered;
            frame
        }
    }

    /// Returns the frame that completed, if any, and how much of `bytes` was
    /// taken. Whatever is left is either an incomplete IQ pair, an incomplete
    /// greeting, or data belonging to the sweep after the one just returned.
    fn consume(&mut self, bytes: &[u8]) -> (Option<Frame>, usize) {
        if self.done {
            return (None, bytes.len());
        }

        let mut off = match self.handshake {
            Handshake::Waiting => {
                if bytes.len() < GREETING_LEN {
                    return (None, 0);
                }
                match parse_greeting(&bytes[..GREETING_LEN]) {
                    Some(g) => {
                        self.handshake = Handshake::Greeted(g);
                        GREETING_LEN
                    }
                    None => {
                        self.handshake = Handshake::Raw;
                        0
                    }
                }
            }
            _ => 0,
        };

        loop {
            let available = (bytes.len() - off) / 2;
            if available == 0 {
                return (None, off);
            }

            if self.settle_left > 0 {
                let take = available.min(self.settle_left);
                self.settle_left -= take;
                off += take * 2;
                continue;
            }

            let take = available.min(self.integration_samples - self.integrated);
            self.welch.push_u8_iq(&bytes[off..off + take * 2]);
            self.integrated += take;
            off += take * 2;

            if self.integrated >= self.integration_samples {
                if let Some(frame) = self.finish_hop() {
                    return (Some(frame), off);
                }
            }
        }
    }

    /// Reduce the hop's accumulated periodograms to its kept bins, then set up
    /// for the next hop. Returns a frame when that was the last hop.
    fn finish_hop(&mut self) -> Option<Frame> {
        self.welch.finish_db(&mut self.psd);
        for i in self.crop_bins..self.crop_bins + self.keep {
            self.sweep
                .push(self.psd.get(i).copied().unwrap_or(MISSING_BIN_DB));
        }
        self.welch.reset();
        self.integrated = 0;

        let wrapped = self.hop + 1 >= self.hops();
        self.hop = if wrapped { 0 } else { self.hop + 1 };
        if wrapped && self.single_shot {
            self.done = true;
        }

        // A single-hop run never changes frequency, so it neither retunes nor
        // pays the settling discard -- it watches one channel continuously.
        if !self.done && self.hops() > 1 {
            self.queue_retune();
            self.settle_left = self.settle_samples;
        }

        if !wrapped {
            return None;
        }
        let y = std::mem::replace(&mut self.sweep, Vec::with_capacity(self.axis.len()));
        Some(Frame {
            timestamp: now_unix(),
            x: Arc::clone(&self.axis),
            y,
        })
    }
}

/// The smallest power of two that resolves `bin_size_hz`, clamped to a range
/// both FFT backends handle quickly.
fn fft_size_for(sample_rate: f64, bin_size_hz: f64) -> usize {
    let wanted = sample_rate / bin_size_hz;
    if !wanted.is_finite() || wanted >= MAX_FFT as f64 {
        return MAX_FFT;
    }
    (wanted.ceil() as usize)
        .max(MIN_FFT)
        .next_power_of_two()
        .min(MAX_FFT)
}

/// Round a quantity to the unsigned argument the protocol carries, without
/// wrapping nonsense into a plausible value.
fn u32_arg(v: f64) -> u32 {
    if !v.is_finite() || v <= 0.0 {
        0
    } else {
        v.round().min(u32::MAX as f64) as u32
    }
}

fn parse_greeting(bytes: &[u8]) -> Option<Greeting> {
    if bytes.len() < GREETING_LEN || bytes[..4] != GREETING_MAGIC {
        return None;
    }
    let word = |at: usize| -> u32 {
        u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };
    Some(Greeting {
        tuner: TunerType::from_wire(word(4)),
        gain_count: word(8),
    })
}

/// Normalise the *Device* field to `host:port`, applying [`DEFAULT_PORT`].
fn endpoint(device: &str) -> Result<String, SourceError> {
    let text = device.trim();
    let text = text.strip_prefix("tcp://").unwrap_or(text).trim();
    if text.is_empty() {
        return Ok(format!("127.0.0.1:{DEFAULT_PORT}"));
    }

    if text.starts_with('[') {
        let Some(close) = text.rfind(']') else {
            return Err(bad_device(device));
        };
        let (host, rest) = text.split_at(close + 1);
        return match rest {
            "" => Ok(format!("{host}:{DEFAULT_PORT}")),
            _ => match rest.strip_prefix(':') {
                Some(port) => with_port(host, port),
                None => Err(bad_device(device)),
            },
        };
    }

    // A bare IPv6 literal has colons of its own, so it has to be recognised
    // before the host:port split and then bracketed for `to_socket_addrs`.
    if text.parse::<std::net::Ipv6Addr>().is_ok() {
        return Ok(format!("[{text}]:{DEFAULT_PORT}"));
    }

    match text.rsplit_once(':') {
        Some(("", _)) => Err(bad_device(device)),
        Some((host, port)) => with_port(host, port),
        None => Ok(format!("{text}:{DEFAULT_PORT}")),
    }
}

fn with_port(host: &str, port: &str) -> Result<String, SourceError> {
    let port = port.trim();
    if port.is_empty() {
        return Ok(format!("{host}:{DEFAULT_PORT}"));
    }
    match port.parse::<u16>() {
        Ok(0) | Err(_) => Err(SourceError::InvalidConfig(format!(
            "'{port}' is not a TCP port number"
        ))),
        Ok(p) => Ok(format!("{host}:{p}")),
    }
}

fn bad_device(device: &str) -> SourceError {
    SourceError::InvalidConfig(format!("cannot read '{device}' as host:port"))
}

/// One line for the log pane, the first time the stream's shape is known.
fn handshake_message(handshake: Handshake) -> Option<String> {
    match handshake {
        Handshake::Waiting => None,
        Handshake::Greeted(g) => Some(format!(
            "rtl_tcp: server reports {} with {} gain settings",
            g.tuner.label(),
            g.gain_count
        )),
        Handshake::Raw => {
            Some("rtl_tcp: no RTL0 greeting; treating the stream as raw 8-bit IQ".to_owned())
        }
    }
}

// ---------------------------------------------------------------------------
// Native driver
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::{ErrorKind, Read, Write};
    use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use super::{
        handshake_message, EventSink, SourceError, SourceSession, SweepConfig, SweepEngine,
    };

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
    const READ_TIMEOUT: Duration = Duration::from_millis(500);
    const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

    /// A quarter of [`super::IN_FLIGHT_SAMPLES`] in bytes, so a retune queued
    /// while a chunk is being consumed always goes out well inside the
    /// settling discard of the hop it belongs to.
    const READ_BUF: usize = 64 * 1024;

    /// A streaming server is never quiet for this long, so treat it as dead
    /// rather than waiting for a TCP timeout that may never come.
    const STALL_TIMEOUTS: u32 = 20;

    struct TcpSession {
        stream: Arc<TcpStream>,
        alive: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    impl SourceSession for TcpSession {
        fn stop(&mut self) {
            self.alive.store(false, Ordering::SeqCst);
            // The worker is usually parked in `read`; only shutting the socket
            // down wakes it immediately.
            let _ = self.stream.shutdown(Shutdown::Both);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    impl Drop for TcpSession {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn connect(endpoint: &str) -> Result<(TcpStream, SocketAddr), SourceError> {
        let addrs: Vec<SocketAddr> = endpoint
            .to_socket_addrs()
            .map_err(|e| SourceError::Io(format!("cannot resolve '{endpoint}': {e}")))?
            .collect();

        let mut last = None;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                Ok(stream) => return Ok((stream, addr)),
                Err(e) => last = Some(format!("{addr}: {e}")),
            }
        }
        Err(SourceError::Io(match last {
            Some(detail) => format!("cannot connect to {endpoint} ({detail})"),
            None => format!("'{endpoint}' resolved to no address"),
        }))
    }

    pub(super) fn start(
        cfg: SweepConfig,
        sink: EventSink,
    ) -> Result<Box<dyn SourceSession>, SourceError> {
        let engine = SweepEngine::new(&cfg)?;
        let endpoint = super::endpoint(&cfg.device)?;
        let (stream, addr) = connect(&endpoint)?;

        // Commands are five bytes and their latency is what limits the hop
        // rate, so Nagle has to go.
        let _ = stream.set_nodelay(true);
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| SourceError::Io(e.to_string()))?;
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));

        sink.log(format!(
            "rtl_tcp: connected to {addr}; {} hops, {}-point FFT, {} bins per sweep",
            engine.hops(),
            engine.fft_size(),
            engine.bins()
        ));

        let stream = Arc::new(stream);
        let alive = Arc::new(AtomicBool::new(true));
        let worker = {
            let stream = Arc::clone(&stream);
            let alive = Arc::clone(&alive);
            std::thread::Builder::new()
                .name("spectroscope-rtl-tcp".into())
                .spawn(move || {
                    sink.started(engine.hops());
                    run(&stream, engine, &sink, &alive);
                    alive.store(false, Ordering::SeqCst);
                    sink.stopped();
                })
                .map_err(|e| SourceError::Io(e.to_string()))?
        };

        Ok(Box::new(TcpSession {
            stream,
            alive,
            worker: Some(worker),
        }))
    }

    fn is_timeout(e: &std::io::Error) -> bool {
        matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    }

    fn send(socket: &TcpStream, commands: &[[u8; 5]]) -> std::io::Result<()> {
        let mut out = socket;
        for command in commands {
            out.write_all(command)?;
        }
        Ok(())
    }

    fn run(socket: &TcpStream, mut engine: SweepEngine, sink: &EventSink, alive: &AtomicBool) {
        let mut buf = vec![0u8; READ_BUF];
        let mut reader = socket;
        let mut announced = false;
        let mut timeouts = 0u32;

        'run: loop {
            if !alive.load(Ordering::SeqCst) {
                break;
            }

            let commands = engine.pending_commands();
            if !commands.is_empty() {
                if let Err(e) = send(socket, &commands) {
                    if alive.load(Ordering::SeqCst) {
                        sink.error(format!("rtl_tcp: cannot send command: {e}"));
                    }
                    break;
                }
            }

            let n = match reader.read(&mut buf) {
                Ok(0) => {
                    if alive.load(Ordering::SeqCst) {
                        sink.error("rtl_tcp: server closed the connection");
                    }
                    break;
                }
                Ok(n) => n,
                Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(ref e) if is_timeout(e) => {
                    timeouts += 1;
                    if timeouts >= STALL_TIMEOUTS {
                        sink.error("rtl_tcp: no data from the server");
                        break;
                    }
                    continue;
                }
                Err(e) => {
                    if alive.load(Ordering::SeqCst) {
                        sink.error(format!("rtl_tcp: read failed: {e}"));
                    }
                    break;
                }
            };
            timeouts = 0;

            let mut chunk: &[u8] = &buf[..n];
            loop {
                let frame = engine.push(chunk);
                chunk = &[];

                if !announced {
                    if let Some(message) = handshake_message(engine.handshake()) {
                        sink.log(message);
                        announced = true;
                    }
                }

                match frame {
                    Some(frame) => {
                        if !sink.frame(frame) {
                            break 'run;
                        }
                    }
                    None => break,
                }
            }

            if engine.is_done() {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Web driver
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};
    use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

    use super::{
        handshake_message, EventSink, SourceError, SourceSession, SweepConfig, SweepEngine,
        DEFAULT_PORT,
    };

    /// The JS handles of one run. `JsValue` is neither `Send` nor `Sync`, so
    /// these cannot live in the session object the GUI holds; they stay in a
    /// thread-local instead, which keeps the session a plain integer and needs
    /// no `unsafe impl Send`.
    struct Live {
        socket: WebSocket,
        _on_open: Closure<dyn FnMut()>,
        _on_message: Closure<dyn FnMut(MessageEvent)>,
        _on_error: Closure<dyn FnMut(ErrorEvent)>,
        _on_close: Closure<dyn FnMut(CloseEvent)>,
    }

    impl Live {
        fn shutdown(self) {
            // Detach first: wasm-bindgen traps if JS invokes a closure that has
            // already been dropped, and `close` can fire `onclose`.
            self.socket.set_onopen(None);
            self.socket.set_onmessage(None);
            self.socket.set_onerror(None);
            self.socket.set_onclose(None);
            let _ = self.socket.close();
        }
    }

    thread_local! {
        static SOCKETS: RefCell<Vec<(u64, Live)>> = RefCell::new(Vec::new());
    }

    fn take(id: u64) -> Option<Live> {
        SOCKETS.with(|sockets| {
            let mut sockets = sockets.borrow_mut();
            let at = sockets.iter().position(|(key, _)| *key == id)?;
            Some(sockets.remove(at).1)
        })
    }

    struct WebSession {
        id: u64,
    }

    impl SourceSession for WebSession {
        fn stop(&mut self) {
            // Outside the borrow, because dropping the closures runs JS.
            if let Some(live) = take(self.id) {
                live.shutdown();
            }
        }
    }

    impl Drop for WebSession {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn describe(error: &JsValue) -> String {
        error.as_string().unwrap_or_else(|| format!("{:?}", error))
    }

    /// `ws://host:port`, from either a URL or a bare `host[:port]`.
    fn websocket_url(device: &str) -> Result<String, SourceError> {
        let text = device.trim();
        if text.starts_with("ws://") || text.starts_with("wss://") {
            return Ok(text.to_owned());
        }
        if text.is_empty() {
            return Ok(format!("ws://127.0.0.1:{DEFAULT_PORT}"));
        }
        Ok(format!("ws://{}", super::endpoint(text)?))
    }

    fn flush(socket: &WebSocket, engine: &mut SweepEngine) {
        if socket.ready_state() != WebSocket::OPEN {
            return;
        }
        for command in engine.pending_commands() {
            let _ = socket.send_with_u8_array(&command);
        }
    }

    pub(super) fn start(
        cfg: SweepConfig,
        sink: EventSink,
    ) -> Result<Box<dyn SourceSession>, SourceError> {
        let engine = SweepEngine::new(&cfg)?;
        let url = websocket_url(&cfg.device)?;

        let socket = WebSocket::new(&url)
            .map_err(|e| SourceError::Io(format!("cannot open {url}: {}", describe(&e))))?;
        socket.set_binary_type(BinaryType::Arraybuffer);

        sink.log(format!(
            "rtl_tcp: connecting to {url}; {} hops, {}-point FFT, {} bins per sweep",
            engine.hops(),
            engine.fft_size(),
            engine.bins()
        ));
        sink.log(
            "rtl_tcp: a browser cannot open a TCP socket, so this expects a \
             WebSocket-to-TCP bridge in front of rtl_tcp",
        );
        sink.started(engine.hops());

        let engine = Rc::new(RefCell::new(engine));

        let on_open = {
            let socket = socket.clone();
            let engine = Rc::clone(&engine);
            let sink = sink.clone();
            Closure::wrap(Box::new(move || {
                sink.log("rtl_tcp: bridge connected");
                if let Ok(mut engine) = engine.try_borrow_mut() {
                    flush(&socket, &mut engine);
                }
            }) as Box<dyn FnMut()>)
        };

        let on_message = {
            let socket = socket.clone();
            let engine = Rc::clone(&engine);
            let sink = sink.clone();
            let mut announced = false;
            Closure::wrap(Box::new(move |event: MessageEvent| {
                let data = event.data();
                // Text frames are a bridge talking to itself, not IQ.
                let Some(buffer) = data.dyn_ref::<js_sys::ArrayBuffer>() else {
                    return;
                };
                let bytes = js_sys::Uint8Array::new(buffer).to_vec();

                let Ok(mut engine) = engine.try_borrow_mut() else {
                    return;
                };
                let mut chunk: &[u8] = &bytes;
                loop {
                    let frame = engine.push(chunk);
                    chunk = &[];
                    flush(&socket, &mut engine);

                    if !announced {
                        if let Some(message) = handshake_message(engine.handshake()) {
                            sink.log(message);
                            announced = true;
                        }
                    }

                    match frame {
                        Some(frame) => {
                            if !sink.frame(frame) {
                                break;
                            }
                        }
                        None => break,
                    }
                }

                // `Stopped` is left to `onclose`, which this triggers.
                if engine.is_done() {
                    let _ = socket.close();
                }
            }) as Box<dyn FnMut(MessageEvent)>)
        };

        let on_error = {
            let sink = sink.clone();
            Closure::wrap(Box::new(move |event: ErrorEvent| {
                let message = event.message();
                if message.is_empty() {
                    sink.error("rtl_tcp: WebSocket error; is the bridge running?");
                } else {
                    sink.error(format!("rtl_tcp: WebSocket error: {message}"));
                }
            }) as Box<dyn FnMut(ErrorEvent)>)
        };

        let on_close = {
            let sink = sink.clone();
            Closure::wrap(Box::new(move |event: CloseEvent| {
                if !event.was_clean() {
                    sink.log(format!(
                        "rtl_tcp: connection closed ({}, {})",
                        event.code(),
                        event.reason()
                    ));
                }
                sink.stopped();
            }) as Box<dyn FnMut(CloseEvent)>)
        };

        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let id = next_id();
        SOCKETS.with(|sockets| {
            sockets.borrow_mut().push((
                id,
                Live {
                    socket,
                    _on_open: on_open,
                    _on_message: on_message,
                    _on_error: on_error,
                    _on_close: on_close,
                },
            ));
        });

        Ok(Box::new(WebSession { id }))
    }

    fn next_id() -> u64 {
        use std::cell::Cell;
        thread_local! {
            static NEXT: Cell<u64> = const { Cell::new(0) };
        }
        NEXT.with(|next| {
            let id = next.get().wrapping_add(1);
            next.set(id);
            id
        })
    }
}

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

static INFO: SourceInfo = SourceInfo {
    id: "rtl_tcp",
    label: "rtl_tcp (networked RTL-SDR)",
    kind: SourceKind::Network,
    default_executable: "",
    additional_params: "",
    has_device_help: false,
    limits: Limits {
        // An RTL-SDR's: the gaps in the sample rate range (300-900 kS/s is
        // unusable on real hardware) are the device's problem, not ours.
        sample_rate: Limit::new(225_001.0, 3_200_000.0, 2_048_000.0),
        bandwidth: Limit::new(0.0, 0.0, 0.0),
        gain: Limit::new(-1.0, 49.6, 37.0),
        start_freq: Limit::new(0.0, 1766.0, 87.0),
        stop_freq: Limit::new(0.0, 1766.0, 108.0),
        bin_size: Limit::new(0.0, 2800.0, 10.0),
        interval: Limit::new(0.0, 3600.0, 1.0),
        ppm: Limit::new(-999, 999, 0),
        crop: Limit::new(0, 49, 0),
    },
};

pub struct RtlTcp;

impl SpectrumSource for RtlTcp {
    fn info(&self) -> &'static SourceInfo {
        &INFO
    }

    fn start(
        &self,
        cfg: SweepConfig,
        sink: EventSink,
    ) -> Result<Box<dyn SourceSession>, SourceError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::start(cfg, sink)
        }
        #[cfg(target_arch = "wasm32")]
        {
            web::start(cfg, sink)
        }
    }

    fn params_help(&self, _executable: &str) -> String {
        "rtl_tcp needs no helper process: SpectroScope speaks the protocol \
         itself and computes the spectrum internally.\n\nPut the server in the \
         Device field as host:port (port 1234 by default). On the web build \
         give a WebSocket URL, ws://host:port, pointing at a \
         WebSocket-to-TCP bridge in front of rtl_tcp."
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SweepConfig {
        SweepConfig {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 107.0,
            bin_size_khz: 10.0,
            interval_s: 1.0,
            sample_rate: 2_000_000.0,
            ..Default::default()
        }
    }

    /// Two hops of 250 kHz, 64-point FFTs, the shortest integration the engine
    /// allows -- the cheapest configuration that still exercises stitching.
    fn small_cfg() -> SweepConfig {
        SweepConfig {
            start_freq_mhz: 87.0,
            stop_freq_mhz: 87.5,
            bin_size_khz: 2800.0,
            interval_s: 0.0,
            sample_rate: 250_000.0,
            ..Default::default()
        }
    }

    /// Bytes per hop of `small_cfg`: everything discarded while settling plus
    /// the minimum integration.
    fn small_hop_bytes() -> usize {
        2 * (IN_FLIGHT_SAMPLES + MIN_INTEGRATION_SEGMENTS * MIN_FFT)
    }

    /// A greeting followed by `len` bytes of non-constant IQ, so a real PSD has
    /// something to chew on.
    fn stream(len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(GREETING_LEN + len);
        out.extend_from_slice(&GREETING_MAGIC);
        out.extend_from_slice(&5u32.to_be_bytes());
        out.extend_from_slice(&29u32.to_be_bytes());
        let mut state = 0x1234_5678u32;
        for _ in 0..len {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            out.push((state >> 24) as u8);
        }
        out
    }

    /// Feed `bytes` in `chunk`-sized pieces, draining every frame.
    fn drive(engine: &mut SweepEngine, bytes: &[u8], chunk: usize) -> Vec<Frame> {
        let mut frames = Vec::new();
        for part in bytes.chunks(chunk) {
            let mut slice: &[u8] = part;
            while let Some(frame) = engine.push(slice) {
                frames.push(frame);
                // Everything in `part` is consumed by the first call; the rest
                // only drains sweeps the engine had already completed.
                slice = &[];
            }
        }
        frames
    }

    #[test]
    fn engine_can_move_to_a_worker_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<SweepEngine>();
    }

    #[test]
    fn fft_size_is_a_clamped_power_of_two() {
        assert_eq!(fft_size_for(2_000_000.0, 10_000.0), 256);
        assert_eq!(fft_size_for(2_400_000.0, 10_000.0), 256);
        assert_eq!(fft_size_for(2_048_000.0, 1_000.0), 2048);
        // Coarser than the floor, and finer than the ceiling.
        assert_eq!(fft_size_for(250_000.0, 2_800_000.0), MIN_FFT);
        assert_eq!(fft_size_for(3_200_000.0, 1.0), MAX_FFT);
        assert_eq!(fft_size_for(3_200_000.0, f64::MIN_POSITIVE), MAX_FFT);
        for size in [
            fft_size_for(2_000_000.0, 10_000.0),
            fft_size_for(2_048_000.0, 7_000.0),
            fft_size_for(1_024_000.0, 3_000.0),
        ] {
            assert!(size.is_power_of_two(), "{size} is not a power of two");
            assert!((MIN_FFT..=MAX_FFT).contains(&size));
        }
    }

    #[test]
    fn hop_layout_for_a_span_that_is_an_exact_multiple() {
        let engine = SweepEngine::new(&cfg()).expect("valid config");
        assert_eq!(engine.fft_size(), 256);
        // 20 MHz of span, 2 MHz usable per hop.
        assert_eq!(engine.hops(), 10);
        assert_eq!(engine.bins(), 10 * 256);

        let centres = engine.hop_centres_hz();
        assert!((centres[0] - 88e6).abs() < 1e-6);
        assert!((centres[9] - 106e6).abs() < 1e-6);
        for pair in centres.windows(2) {
            assert!((pair[1] - pair[0] - 2e6).abs() < 1e-6);
        }
    }

    #[test]
    fn hop_layout_rounds_a_partial_hop_up() {
        let partial = SweepConfig {
            stop_freq_mhz: 108.0,
            ..cfg()
        };
        let engine = SweepEngine::new(&partial).expect("valid config");
        // 21 MHz needs eleven 2 MHz hops, the last one overshooting.
        assert_eq!(engine.hops(), 11);
        let axis = engine.axis();
        assert!(axis[axis.len() - 1] >= 108e6);
    }

    #[test]
    fn crop_shrinks_the_usable_bandwidth_and_adds_hops() {
        let cropped = SweepConfig {
            crop: 0.25,
            ..cfg()
        };
        let engine = SweepEngine::new(&cropped).expect("valid config");
        // 64 bins cropped off each edge of 256 leaves 1 MHz usable, so twice
        // the hops of the uncropped sweep and half the bins per hop.
        assert_eq!(engine.hops(), 20);
        assert_eq!(engine.bins(), 20 * 128);
        assert!((engine.hop_centres_hz()[1] - engine.hop_centres_hz()[0] - 1e6).abs() < 1e-6);
    }

    #[test]
    fn axis_is_uniform_ascending_and_covers_the_span() {
        let engine = SweepEngine::new(&cfg()).expect("valid config");
        let axis = engine.axis();
        assert_eq!(axis.len(), engine.bins());
        assert!((axis[0] - 87e6).abs() < 1e-6);
        let step = 2_000_000.0 / 256.0;
        for pair in axis.windows(2) {
            assert!(pair[1] > pair[0]);
            assert!((pair[1] - pair[0] - step).abs() < 1e-6);
        }
        assert!(axis[axis.len() - 1] >= 107e6 - step);
    }

    #[test]
    fn lnb_lo_is_removed_from_the_tuner_and_kept_in_the_axis() {
        let downconverted = SweepConfig {
            start_freq_mhz: 10_000.0,
            stop_freq_mhz: 10_020.0,
            lnb_lo_hz: 9_750e6,
            ..cfg()
        };
        let engine = SweepEngine::new(&downconverted).expect("valid config");
        assert!((engine.hop_centres_hz()[0] - 251e6).abs() < 1.0);
        assert!((engine.axis()[0] - 10_000e6).abs() < 1.0);
    }

    #[test]
    fn command_bytes_are_big_endian() {
        assert_eq!(
            Command::CentreFreq.encode(100_000_000),
            [0x01, 0x05, 0xf5, 0xe1, 0x00]
        );
        assert_eq!(
            Command::SampleRate.encode(2_048_000),
            [0x02, 0x00, 0x1f, 0x40, 0x00]
        );
        assert_eq!(Command::GainMode.encode(1), [0x03, 0, 0, 0, 1]);
        assert_eq!(Command::TunerGain.encode(372), [0x04, 0, 0, 0x01, 0x74]);
        // A negative ppm travels as the two's complement of the i32.
        assert_eq!(
            Command::FreqCorrection.encode(-12i32 as u32),
            [0x05, 0xff, 0xff, 0xff, 0xf4]
        );

        let expected = [
            0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        ];
        for (command, byte) in Command::ALL.iter().zip(expected) {
            assert_eq!(command.encode(0xdead_beef), [byte, 0xde, 0xad, 0xbe, 0xef]);
        }
    }

    #[test]
    fn setup_commands_describe_the_requested_radio_state() {
        let manual = SweepConfig {
            gain_db: 37.2,
            ppm: -12,
            ..cfg()
        };
        let mut engine = SweepEngine::new(&manual).expect("valid config");
        let commands = engine.pending_commands();
        assert_eq!(
            commands,
            vec![
                Command::SampleRate.encode(2_000_000),
                Command::FreqCorrection.encode(-12i32 as u32),
                Command::GainMode.encode(1),
                Command::TunerGain.encode(372),
                Command::CentreFreq.encode(88_000_000),
            ]
        );
        // Taking them clears the queue.
        assert!(engine.pending_commands().is_empty());

        let auto = SweepConfig {
            gain_db: -1.0,
            ..cfg()
        };
        let commands = SweepEngine::new(&auto)
            .expect("valid config")
            .pending_commands();
        assert_eq!(
            commands,
            vec![
                Command::SampleRate.encode(2_000_000),
                Command::GainMode.encode(0),
                Command::CentreFreq.encode(88_000_000),
            ]
        );
    }

    #[test]
    fn greeting_parses_and_survives_being_split() {
        let bytes = stream(0);
        assert_eq!(
            parse_greeting(&bytes),
            Some(Greeting {
                tuner: TunerType::R820T,
                gain_count: 29,
            })
        );
        assert_eq!(parse_greeting(&bytes[..11]), None);
        assert_eq!(parse_greeting(&[]), None);

        let mut engine = SweepEngine::new(&small_cfg()).expect("valid config");
        assert_eq!(engine.handshake(), Handshake::Waiting);
        assert!(engine.push(&bytes[..5]).is_none());
        assert_eq!(engine.handshake(), Handshake::Waiting);
        assert!(engine.push(&bytes[5..]).is_none());
        assert!(matches!(engine.handshake(), Handshake::Greeted(_)));
    }

    #[test]
    fn a_missing_greeting_is_treated_as_raw_iq() {
        let mut engine = SweepEngine::new(&small_cfg()).expect("valid config");
        let hop = small_hop_bytes();
        let frames = drive(&mut engine, &vec![0x7fu8; 2 * hop + 64], 4_095);
        assert_eq!(engine.handshake(), Handshake::Raw);
        assert_eq!(frames.len(), 1, "raw stream should still produce a sweep");
    }

    #[test]
    fn degenerate_configs_are_rejected_without_panicking() {
        let bad = [
            SweepConfig {
                stop_freq_mhz: 87.0,
                ..cfg()
            },
            SweepConfig {
                stop_freq_mhz: 50.0,
                ..cfg()
            },
            SweepConfig {
                sample_rate: 0.0,
                ..cfg()
            },
            SweepConfig {
                sample_rate: -1.0,
                ..cfg()
            },
            SweepConfig {
                bin_size_khz: 0.0,
                ..cfg()
            },
            SweepConfig { crop: 0.5, ..cfg() },
            SweepConfig { crop: 1.0, ..cfg() },
            SweepConfig {
                start_freq_mhz: f64::NAN,
                ..cfg()
            },
            SweepConfig {
                stop_freq_mhz: f64::NAN,
                ..cfg()
            },
            SweepConfig {
                sample_rate: f64::NAN,
                ..cfg()
            },
            SweepConfig {
                bin_size_khz: f64::NAN,
                ..cfg()
            },
            SweepConfig {
                lnb_lo_hz: f64::INFINITY,
                ..cfg()
            },
            // 1766 MHz at the narrowest usable bandwidth: legal per control,
            // impossible as a sweep.
            SweepConfig {
                start_freq_mhz: 0.0,
                stop_freq_mhz: 1766.0,
                sample_rate: 225_001.0,
                bin_size_khz: 2800.0,
                crop: 0.49,
                ..cfg()
            },
        ];
        for config in bad {
            assert!(
                matches!(
                    SweepEngine::new(&config),
                    Err(SourceError::InvalidConfig(_))
                ),
                "expected InvalidConfig for {config:?}"
            );
        }

        // A NaN crop is a GUI bug, not a reason to refuse: treat it as none.
        let nan_crop = SweepConfig {
            crop: f64::NAN,
            ..cfg()
        };
        assert_eq!(
            SweepEngine::new(&nan_crop)
                .expect("NaN crop means no crop")
                .hops(),
            10
        );
    }

    #[test]
    fn odd_and_split_chunks_are_safe() {
        let hop = small_hop_bytes();
        let bytes = stream(2 * hop + 1);

        // One byte at a time, then odd chunks, then one huge chunk: all three
        // must agree on how many sweeps the same stream contains.
        let mut engine = SweepEngine::new(&small_cfg()).expect("valid config");
        let frames = drive(&mut engine, &bytes[..64], 1);
        assert!(frames.is_empty());

        let mut by_odd = SweepEngine::new(&small_cfg()).expect("valid config");
        let odd = drive(&mut by_odd, &bytes, 4_095);
        let mut whole = SweepEngine::new(&small_cfg()).expect("valid config");
        let big = drive(&mut whole, &bytes, bytes.len());

        assert_eq!(odd.len(), 1);
        assert_eq!(big.len(), odd.len());
        assert_eq!(odd[0].y.len(), by_odd.bins());
        assert_eq!(big[0].y.len(), whole.bins());
        assert_eq!(odd[0].y.len(), 128);
    }

    #[test]
    fn consecutive_frames_share_one_axis_allocation() {
        let hop = small_hop_bytes();
        let mut engine = SweepEngine::new(&small_cfg()).expect("valid config");
        let frames = drive(&mut engine, &stream(5 * hop), 8_191);

        assert!(
            frames.len() >= 2,
            "expected two sweeps, got {}",
            frames.len()
        );
        assert!(
            Arc::ptr_eq(&frames[0].x, &frames[1].x),
            "the axis must be cloned, not rebuilt"
        );
        assert!(Arc::ptr_eq(&frames[0].x, engine.axis()));
        assert_eq!(frames[0].x.len(), frames[0].y.len());
        for pair in frames[0].x.windows(2) {
            assert!(pair[1] > pair[0], "axis must ascend");
        }
        assert!(!engine.is_done());
    }

    #[test]
    fn single_shot_stops_after_one_sweep() {
        let hop = small_hop_bytes();
        let config = SweepConfig {
            single_shot: true,
            ..small_cfg()
        };
        let mut engine = SweepEngine::new(&config).expect("valid config");
        let frames = drive(&mut engine, &stream(6 * hop), 8_191);
        assert_eq!(frames.len(), 1);
        assert!(engine.is_done());
        // Further bytes are swallowed rather than queued up forever.
        assert!(engine.push(&[0u8; 64]).is_none());
    }

    #[test]
    fn a_single_hop_run_does_not_retune_between_sweeps() {
        let one_hop = SweepConfig {
            stop_freq_mhz: 87.2,
            ..small_cfg()
        };
        let mut engine = SweepEngine::new(&one_hop).expect("valid config");
        assert_eq!(engine.hops(), 1);
        let setup = engine.pending_commands();
        assert_eq!(setup.len(), 3);

        let hop = small_hop_bytes();
        let frames = drive(&mut engine, &stream(3 * hop), 8_191);
        assert!(frames.len() >= 2);
        assert!(
            engine.pending_commands().is_empty(),
            "a single-hop run has nothing to retune"
        );
    }

    #[test]
    fn empty_input_does_nothing() {
        let mut engine = SweepEngine::new(&cfg()).expect("valid config");
        assert!(engine.push(&[]).is_none());
        assert!(engine.push(&[]).is_none());
        assert_eq!(engine.handshake(), Handshake::Waiting);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn device_strings_normalise_to_host_and_port() {
        let ok = |device: &str| endpoint(device).expect(device);
        assert_eq!(ok(""), "127.0.0.1:1234");
        assert_eq!(ok("   "), "127.0.0.1:1234");
        assert_eq!(ok("localhost"), "localhost:1234");
        assert_eq!(ok("192.168.1.10"), "192.168.1.10:1234");
        assert_eq!(ok("192.168.1.10:1235"), "192.168.1.10:1235");
        assert_eq!(ok("rtl.lan:7890"), "rtl.lan:7890");
        assert_eq!(ok("rtl.lan:"), "rtl.lan:1234");
        assert_eq!(ok("tcp://rtl.lan:7890"), "rtl.lan:7890");
        assert_eq!(ok("[::1]"), "[::1]:1234");
        assert_eq!(ok("[::1]:1235"), "[::1]:1235");
        assert_eq!(ok("[fe80::1%25eth0]:1234"), "[fe80::1%25eth0]:1234");
        assert_eq!(ok("::1"), "[::1]:1234");
        assert_eq!(ok("fe80::1"), "[fe80::1]:1234");

        for bad in [
            ":1234",
            "rtl.lan:0",
            "rtl.lan:99999",
            "rtl.lan:kilohertz",
            "[::1",
        ] {
            assert!(
                matches!(endpoint(bad), Err(SourceError::InvalidConfig(_))),
                "expected '{bad}' to be rejected"
            );
        }
    }

    #[test]
    fn info_limits_are_an_rtl_sdr() {
        let limits = RtlTcp.info().limits;
        assert_eq!(RtlTcp.info().id, "rtl_tcp");
        assert_eq!(RtlTcp.info().kind, SourceKind::Network);
        assert!(RtlTcp.info().label.contains("rtl_tcp"));
        assert!(RtlTcp.info().default_executable.is_empty());
        assert!(!RtlTcp.info().has_device_help);
        assert!(limits.bandwidth.is_fixed());
        assert_eq!(limits.sample_rate.default, 2_048_000.0);
        assert_eq!(limits.gain.min, -1.0);
        assert_eq!(limits.crop.max, 49);
        assert_eq!(limits.stop_freq.max, 1766.0);

        // Every default must build an engine, or selecting the backend in the
        // GUI and pressing Start fails outright.
        let defaults = SweepConfig {
            start_freq_mhz: limits.start_freq.default,
            stop_freq_mhz: limits.stop_freq.default,
            bin_size_khz: limits.bin_size.default,
            interval_s: limits.interval.default,
            gain_db: limits.gain.default,
            sample_rate: limits.sample_rate.default,
            crop: limits.crop.default as f64 / 100.0,
            ..Default::default()
        };
        assert!(SweepEngine::new(&defaults).is_ok());
    }
}
