//! Reader for `soapy_power_bin`, the framed binary container soapy_power
//! writes and QSpectrumAnalyzer consumed through `soapypower.writer`.
//!
//! Replaces the `formatter.read()` and `read_from_file()` halves of
//! `backends/soapy_power.py`: [`read_record`] decodes one frequency hop,
//! [`read_frames`] stitches the hops of a saved file back into whole sweeps,
//! and [`average_frames`] is the running mean the baseline loader needs.

use std::io::Read;
use std::sync::Arc;

use crate::sources::{linspace, Frame};

/// One record's header: Python's `struct` format `'<BdddddQQ2x'`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header {
    pub version: u8,
    pub time_start: f64,
    pub time_stop: f64,
    pub start: f64,
    pub stop: f64,
    pub step: f64,
    pub samples: u64,
    /// Payload length in *bytes*, i.e. four times the number of power values.
    pub size: u64,
}

pub const MAGIC: &[u8; 5] = b"SDRFF";
/// Header bytes following [`MAGIC`], including the two trailing pad bytes.
pub const HEADER_LEN: usize = 59;
/// [`MAGIC`] plus [`HEADER_LEN`]: soapy_power keeps this at a round 64 bytes.
pub const RECORD_HEADER_LEN: usize = 64;

/// The container version this reader was written against. Later versions are
/// read anyway, because every bump so far kept the layout.
pub const SUPPORTED_VERSION: u8 = 2;

/// A hop claiming more than this is a desynchronised stream, not a
/// measurement: 64 MiB is 16M bins, far beyond any usable FFT size. Checked
/// before allocating, so a corrupt length cannot exhaust memory.
const MAX_PAYLOAD_BYTES: u64 = 64 << 20;

#[derive(Debug)]
pub enum BinError {
    BadMagic,
    Truncated,
    Io(String),
}

impl std::fmt::Display for BinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "invalid soapy_power_bin magic"),
            Self::Truncated => write!(f, "truncated soapy_power_bin record"),
            Self::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for BinError {}

/// Read into `buf` until it is full or the stream ends, returning how many
/// bytes arrived. A short count is the caller's cue that the record is cut off.
fn fill<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize, BinError> {
    let mut done = 0;
    while done < buf.len() {
        match r.read(&mut buf[done..]) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(BinError::Io(e.to_string())),
        }
    }
    Ok(done)
}

fn le_f64(b: &[u8; HEADER_LEN], at: usize) -> f64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&b[at..at + 8]);
    f64::from_le_bytes(raw)
}

fn le_u64(b: &[u8; HEADER_LEN], at: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(raw)
}

impl Header {
    /// `'<'` means no alignment padding, so every field sits at a fixed offset.
    fn decode(b: &[u8; HEADER_LEN]) -> Self {
        Self {
            version: b[0],
            time_start: le_f64(b, 1),
            time_stop: le_f64(b, 9),
            start: le_f64(b, 17),
            stop: le_f64(b, 25),
            step: le_f64(b, 33),
            samples: le_u64(b, 41),
            size: le_u64(b, 49),
        }
    }
}

/// Decode the next record, or `Ok(None)` at a clean end of stream.
pub fn read_record<R: Read>(r: &mut R) -> Result<Option<(Header, Vec<f32>)>, BinError> {
    let mut magic = [0u8; MAGIC.len()];
    match fill(r, &mut magic)? {
        0 => return Ok(None),
        n if n < magic.len() => return Err(BinError::Truncated),
        _ => {}
    }
    if &magic != MAGIC {
        return Err(BinError::BadMagic);
    }

    let mut raw = [0u8; HEADER_LEN];
    if fill(r, &mut raw)? < HEADER_LEN {
        return Err(BinError::Truncated);
    }
    let header = Header::decode(&raw);

    if header.size > MAX_PAYLOAD_BYTES {
        return Err(BinError::Io(format!(
            "soapy_power_bin record claims {} payload bytes",
            header.size
        )));
    }

    let mut payload = vec![0u8; header.size as usize];
    if fill(r, &mut payload)? < payload.len() {
        return Err(BinError::Truncated);
    }

    // A size that is not a whole number of f32s drops its tail here and the
    // record then fails the x/y length check, which is the desired outcome.
    let values = payload
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    Ok(Some((header, values)))
}

/// The frequency axis of one record, or `None` when it is unusable.
///
/// `round((stop - start) / step)` is the bin count soapy_power's own writer
/// derives, and a record whose power values do not match it is corrupt.
pub fn record_axis(header: &Header, y_len: usize) -> Option<Vec<f64>> {
    let bins = ((header.stop - header.start) / header.step).round();
    if !bins.is_finite() || bins < 1.0 || bins as usize != y_len {
        return None;
    }
    Some(linspace(header.start, header.stop, y_len))
}

/// Hops accumulated into one sweep.
#[derive(Default)]
struct Sweep {
    /// The lowest hop start seen; a record at that frequency begins a sweep.
    min_start: Option<f64>,
    timestamp: f64,
    x: Vec<f64>,
    y: Vec<f32>,
}

impl Sweep {
    /// Add one record, returning the sweep it completed, if any.
    fn push(&mut self, header: &Header, y: Vec<f32>) -> Option<Frame> {
        let x = record_axis(header, y.len())?;

        if self.min_start.is_some_and(|m| header.start > m) {
            self.x.extend_from_slice(&x);
            self.y.extend_from_slice(&y);
            return None;
        }

        let finished = self.take();
        self.min_start = Some(match self.min_start {
            Some(m) => m.min(header.start),
            None => header.start,
        });
        // The Python version stamps a sweep with its *first* hop's stop time.
        self.timestamp = header.time_stop;
        self.x = x;
        self.y = y;
        finished
    }

    fn take(&mut self) -> Option<Frame> {
        if self.x.is_empty() {
            return None;
        }
        Some(Frame {
            timestamp: self.timestamp,
            x: Arc::new(std::mem::take(&mut self.x)),
            y: std::mem::take(&mut self.y),
        })
    }
}

/// Read a whole `soapy_power_bin` stream as complete sweeps.
///
/// Corrupt records end the read, like a short file would: whatever sweeps were
/// already complete are returned, plus the one in progress.
pub fn read_frames<R: Read>(r: &mut R) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut sweep = Sweep::default();

    while let Ok(Some((header, y))) = read_record(r) {
        if let Some(frame) = sweep.push(&header, y) {
            frames.push(frame);
        }
    }
    frames.extend(sweep.take());
    frames
}

/// Running mean of several sweeps, as `set_subtract_baseline` computes it when
/// a baseline file holds more than one measurement.
///
/// Sweeps whose length differs from the first are skipped: they come from a
/// different configuration and averaging them would silently misalign bins.
pub fn average_frames(frames: &[Frame]) -> Option<(Arc<Vec<f64>>, Vec<f32>)> {
    let first = frames.first()?;
    let len = first.y.len();

    let mut mean: Vec<f64> = Vec::new();
    let mut count = 0u32;
    for frame in frames.iter().filter(|f| f.y.len() == len) {
        count += 1;
        if count == 1 {
            mean = frame.y.iter().map(|&v| f64::from(v)).collect();
            continue;
        }
        let n = f64::from(count);
        for (m, &v) in mean.iter_mut().zip(frame.y.iter()) {
            *m = (*m * (n - 1.0) + f64::from(v)) / n;
        }
    }

    Some((
        Arc::clone(&first.x),
        mean.into_iter().map(|v| v as f32).collect(),
    ))
}

/// Append one record to `w`.
///
/// Records are self-delimiting -- magic, fixed header, then exactly
/// `header.size` bytes -- which is what makes this format safe to record into:
/// a file cut short mid-sweep still reads back cleanly up to its last complete
/// record, and [`read_record`] reports the remainder as [`BinError::Truncated`]
/// rather than returning garbage.
pub fn write_record<W: std::io::Write>(
    w: &mut W,
    header: &Header,
    y: &[f32],
) -> std::io::Result<()> {
    w.write_all(&encode_record(header, y))
}

/// `header.size` is written verbatim, so a test can craft a mismatch.
pub(crate) fn encode_record(header: &Header, y: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RECORD_HEADER_LEN + y.len() * 4);
    out.extend_from_slice(MAGIC);
    out.push(header.version);
    for v in [
        header.time_start,
        header.time_stop,
        header.start,
        header.stop,
        header.step,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&header.samples.to_le_bytes());
    out.extend_from_slice(&header.size.to_le_bytes());
    out.extend_from_slice(&[0u8; 2]);
    for v in y {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Header describing one complete sweep, for recording.
pub fn sweep_header(start: f64, stop: f64, bins: usize, time_start: f64, time_stop: f64) -> Header {
    Header {
        version: SUPPORTED_VERSION,
        time_start,
        time_stop,
        start,
        stop,
        // The reader reconstructs the axis from `step`, so it has to be the
        // spacing that `bins` points across `start..stop` really have.
        step: if bins > 1 {
            (stop - start) / bins as f64
        } else {
            0.0
        },
        samples: 0,
        size: (bins * std::mem::size_of::<f32>()) as u64,
    }
}

/// A plausible header for `bins` values spanning `start..stop`.
#[cfg(test)]
pub(crate) fn test_header(start: f64, stop: f64, bins: usize, time_stop: f64) -> Header {
    Header {
        version: SUPPORTED_VERSION,
        time_start: time_stop - 0.5,
        time_stop,
        start,
        stop,
        step: (stop - start) / bins as f64,
        samples: 1024,
        size: (bins * 4) as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn frame(y: &[f32]) -> Frame {
        Frame {
            timestamp: 0.0,
            x: Arc::new(linspace(0.0, 1.0, y.len())),
            y: y.to_vec(),
        }
    }

    #[test]
    fn record_header_is_sixty_four_bytes() {
        assert_eq!(MAGIC.len() + HEADER_LEN, RECORD_HEADER_LEN);
        let h = test_header(1e6, 2e6, 4, 10.0);
        let bytes = encode_record(&h, &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(bytes.len(), RECORD_HEADER_LEN + 16);
        assert_eq!(&bytes[..5], MAGIC);
    }

    #[test]
    fn round_trips_one_record() {
        let h = test_header(1e6, 2e6, 4, 1234.5);
        let y = [-10.5f32, -20.0, 0.0, 3.25];
        let mut cur = Cursor::new(encode_record(&h, &y));

        let (got, values) = read_record(&mut cur).expect("decode").expect("one record");
        assert_eq!(got, h);
        assert_eq!(values, y);
        // The stream is exhausted exactly at the record boundary.
        assert!(read_record(&mut cur).expect("eof").is_none());
    }

    #[test]
    fn empty_stream_is_a_clean_eof() {
        let mut cur = Cursor::new(Vec::new());
        assert!(read_record(&mut cur).expect("eof").is_none());
        assert!(read_frames(&mut Cursor::new(Vec::new())).is_empty());
    }

    #[test]
    fn partial_records_are_truncated() {
        let h = test_header(1e6, 2e6, 4, 1.0);
        let full = encode_record(&h, &[1.0, 2.0, 3.0, 4.0]);

        for cut in [3, 10, RECORD_HEADER_LEN, RECORD_HEADER_LEN + 7] {
            let mut cur = Cursor::new(full[..cut].to_vec());
            assert!(
                matches!(read_record(&mut cur), Err(BinError::Truncated)),
                "cut at {cut} should be truncated"
            );
        }
    }

    #[test]
    fn wrong_magic_is_rejected() {
        let h = test_header(1e6, 2e6, 2, 1.0);
        let mut bytes = encode_record(&h, &[1.0, 2.0]);
        bytes[2] = b'X';
        let mut cur = Cursor::new(bytes);
        assert!(matches!(read_record(&mut cur), Err(BinError::BadMagic)));
    }

    #[test]
    fn implausible_size_is_not_allocated() {
        let mut h = test_header(1e6, 2e6, 2, 1.0);
        h.size = u64::MAX - 7;
        let mut cur = Cursor::new(encode_record(&h, &[1.0, 2.0]));
        match read_record(&mut cur) {
            Err(BinError::Io(m)) => assert!(m.contains("payload bytes"), "{m}"),
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn multi_hop_sweeps_are_stitched_and_split() {
        // Two hops per sweep, two sweeps, hops in the order soapy_power emits.
        let a = test_header(1e6, 2e6, 4, 100.0);
        let b = test_header(2e6, 3e6, 4, 100.4);
        let a2 = test_header(1e6, 2e6, 4, 101.0);
        let b2 = test_header(2e6, 3e6, 4, 101.4);

        let mut stream = Vec::new();
        stream.extend(encode_record(&a, &[1.0, 2.0, 3.0, 4.0]));
        stream.extend(encode_record(&b, &[5.0, 6.0, 7.0, 8.0]));
        stream.extend(encode_record(&a2, &[9.0, 10.0, 11.0, 12.0]));
        stream.extend(encode_record(&b2, &[13.0, 14.0, 15.0, 16.0]));

        let frames = read_frames(&mut Cursor::new(stream));
        assert_eq!(frames.len(), 2);

        assert_eq!(frames[0].timestamp, 100.0);
        assert_eq!(frames[0].y, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(frames[0].x.len(), 8);
        assert_eq!(frames[0].x[0], 1e6);
        assert_eq!(frames[0].x[4], 2e6);
        assert_eq!(frames[0].x[7], 3e6);

        assert_eq!(frames[1].timestamp, 101.0);
        assert_eq!(frames[1].y.first().copied(), Some(9.0));
        assert_eq!(frames[1].y.last().copied(), Some(16.0));
    }

    #[test]
    fn single_hop_file_yields_one_frame() {
        let h = test_header(88e6, 108e6, 5, 7.0);
        let frames = read_frames(&mut Cursor::new(encode_record(
            &h,
            &[-1.0, -2.0, -3.0, -4.0, -5.0],
        )));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].y.len(), 5);
    }

    #[test]
    fn length_mismatch_skips_the_record_only() {
        let good = test_header(1e6, 2e6, 4, 5.0);
        let mut bad = test_header(2e6, 3e6, 4, 5.4);
        // Claim four values but ship two: the axis no longer matches.
        bad.size = 8;

        let mut stream = encode_record(&good, &[1.0, 2.0, 3.0, 4.0]);
        stream.extend(encode_record(&bad, &[5.0, 6.0]));

        let frames = read_frames(&mut Cursor::new(stream));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].y, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn degenerate_spans_are_skipped() {
        let zero_span = Header {
            version: SUPPORTED_VERSION,
            time_start: 0.0,
            time_stop: 1.0,
            start: 1e6,
            stop: 1e6,
            step: 1e5,
            samples: 16,
            size: 8,
        };
        let zero_step = Header {
            step: 0.0,
            stop: 2e6,
            ..zero_span
        };
        let nan_step = Header {
            step: f64::NAN,
            ..zero_step
        };

        for h in [zero_span, zero_step, nan_step] {
            assert!(record_axis(&h, 2).is_none());
            assert!(read_frames(&mut Cursor::new(encode_record(&h, &[1.0, 2.0]))).is_empty());
        }
    }

    #[test]
    fn truncated_tail_still_returns_earlier_sweeps() {
        let a = test_header(1e6, 2e6, 4, 1.0);
        let b = test_header(1e6, 2e6, 4, 2.0);
        let mut stream = encode_record(&a, &[1.0, 2.0, 3.0, 4.0]);
        let partial = encode_record(&b, &[5.0, 6.0, 7.0, 8.0]);
        stream.extend_from_slice(&partial[..RECORD_HEADER_LEN + 4]);

        let frames = read_frames(&mut Cursor::new(stream));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].timestamp, 1.0);
    }

    #[test]
    fn average_is_the_running_mean() {
        let frames = [
            frame(&[0.0, 10.0]),
            frame(&[2.0, 20.0]),
            frame(&[4.0, 30.0]),
        ];
        let (x, y) = average_frames(&frames).expect("average");
        assert_eq!(x.len(), 2);
        assert!((y[0] - 2.0).abs() < 1e-6, "{y:?}");
        assert!((y[1] - 20.0).abs() < 1e-6, "{y:?}");
    }

    #[test]
    fn average_ignores_mismatched_lengths() {
        let frames = [frame(&[0.0, 10.0]), frame(&[100.0; 7]), frame(&[2.0, 20.0])];
        let (_, y) = average_frames(&frames).expect("average");
        assert_eq!(y.len(), 2);
        assert!((y[0] - 1.0).abs() < 1e-6, "{y:?}");
        assert!((y[1] - 15.0).abs() < 1e-6, "{y:?}");
    }

    #[test]
    fn average_of_one_frame_is_that_frame() {
        let frames = [frame(&[-3.5, -4.5, -5.5])];
        let (_, y) = average_frames(&frames).expect("average");
        assert_eq!(y, vec![-3.5, -4.5, -5.5]);
    }

    #[test]
    fn average_of_nothing_is_none() {
        assert!(average_frames(&[]).is_none());
        // An empty sweep is still a sweep; it just has nothing in it.
        let (_, y) = average_frames(&[frame(&[])]).expect("average");
        assert!(y.is_empty());
    }

    #[test]
    fn average_propagates_nan_like_numpy() {
        let frames = [frame(&[1.0, 1.0]), frame(&[f32::NAN, 3.0])];
        let (_, y) = average_frames(&frames).expect("average");
        assert!(y[0].is_nan());
        assert!((y[1] - 2.0).abs() < 1e-6);
    }
}
