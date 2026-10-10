//! Writing sweeps to disk as they arrive.
//!
//! Both formats here are **append-only and self-recovering**, because the case
//! that matters is the one where a long capture is interrupted: a power cut, a
//! closed laptop, a killed process. Neither format has a trailer, an index or a
//! length field that is written at the end, so a file that simply stops
//! mid-sweep is still a valid file containing every sweep before that point.
//! Each sweep is flushed as it is written, so at most the sweep in flight is
//! lost rather than a buffer's worth.
//!
//! [`Format::SoapyPowerBin`] is the default and the better choice: it is the
//! same container `soapy_power -F soapy_power_bin` produces, so a recording can
//! be loaded straight back as a *Baseline*, it keeps full `f32` precision, and
//! it is about six times smaller than the text form. Its records are
//! magic-delimited, so a truncated tail is detected rather than misparsed.
//! [`Format::Csv`] exists for reading in other tools; being line-based, a
//! partial final line is equally harmless.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::sources::Frame;

use super::soapy_bin;

/// How a recording is laid out on disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Format {
    /// `soapy_power_bin`: compact, full precision, reloadable as a baseline.
    #[default]
    SoapyPowerBin,
    /// `rtl_power`-style CSV: one line per sweep, readable anywhere.
    Csv,
}

impl Format {
    pub const ALL: [Self; 2] = [Self::SoapyPowerBin, Self::Csv];

    pub fn label(self) -> &'static str {
        match self {
            Self::SoapyPowerBin => "soapy_power binary (.bin)",
            Self::Csv => "CSV (.csv)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::SoapyPowerBin => "bin",
            Self::Csv => "csv",
        }
    }

    /// Guess the format a path is asking for, falling back to the default.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("csv") => Self::Csv,
            _ => Self::SoapyPowerBin,
        }
    }
}

/// An open recording.
pub struct Recorder {
    writer: BufWriter<File>,
    format: Format,
    path: PathBuf,
    sweeps: u64,
    bytes: u64,
    /// Unix time of the previous sweep, for the record's `time_start`.
    prev_timestamp: Option<f64>,
}

impl Recorder {
    /// Create (or truncate) `path` and prepare to append sweeps.
    pub fn create(path: impl Into<PathBuf>, format: Format) -> std::io::Result<Self> {
        let path = path.into();
        let file = File::create(&path)?;
        Ok(Self {
            writer: BufWriter::new(file),
            format,
            path,
            sweeps: 0,
            bytes: 0,
            prev_timestamp: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn sweeps(&self) -> u64 {
        self.sweeps
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Append one sweep and flush it.
    ///
    /// Flushing every sweep is what bounds the loss from an interrupted
    /// recording to the sweep in flight. At a few kilobytes per sweep and a
    /// sweep a second this costs nothing; a backend fast enough for it to
    /// matter is already producing more data than a disk wants.
    pub fn write(&mut self, frame: &Frame) -> std::io::Result<()> {
        if frame.y.is_empty() || frame.x.len() != frame.y.len() {
            return Ok(());
        }

        let before = self.bytes;
        match self.format {
            Format::SoapyPowerBin => self.write_bin(frame)?,
            Format::Csv => self.write_csv(frame)?,
        }
        self.writer.flush()?;
        self.sweeps += 1;
        self.prev_timestamp = Some(frame.timestamp);
        debug_assert!(self.bytes >= before);
        Ok(())
    }

    fn write_bin(&mut self, frame: &Frame) -> std::io::Result<()> {
        let start = frame.x[0];
        let stop = frame.x[frame.x.len() - 1];
        let header = soapy_bin::sweep_header(
            start,
            stop,
            frame.y.len(),
            self.prev_timestamp.unwrap_or(frame.timestamp),
            frame.timestamp,
        );
        soapy_bin::write_record(&mut self.writer, &header, &frame.y)?;
        self.bytes += (soapy_bin::RECORD_HEADER_LEN + frame.y.len() * 4) as u64;
        Ok(())
    }

    fn write_csv(&mut self, frame: &Frame) -> std::io::Result<()> {
        // `rtl_power`'s column order, so the usual heatmap scripts accept it.
        // The date and time columns are what those scripts group rows by.
        let (date, time) = crate::util::unix_to_civil(frame.timestamp);
        let start = frame.x[0];
        let stop = frame.x[frame.x.len() - 1];
        let step = if frame.y.len() > 1 {
            (stop - start) / frame.y.len() as f64
        } else {
            0.0
        };

        let mut line = String::with_capacity(16 + frame.y.len() * 8);
        line.push_str(&format!(
            "{date}, {time}, {start:.0}, {stop:.0}, {step:.2}, 0"
        ));
        for v in &frame.y {
            line.push_str(&format!(", {v:.2}"));
        }
        line.push('\n');

        self.writer.write_all(line.as_bytes())?;
        self.bytes += line.len() as u64;
        Ok(())
    }

    /// Flush and close. Dropping the recorder does the same, minus the error.
    pub fn finish(mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn frame(ts: f64, y: &[f32]) -> Frame {
        Frame {
            timestamp: ts,
            x: Arc::new(
                (0..y.len())
                    .map(|i| 87e6 + i as f64 * 10e3)
                    .collect::<Vec<_>>(),
            ),
            y: y.to_vec(),
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("spectroscope-test-{name}-{}", std::process::id()));
        p
    }

    #[test]
    fn civil_date_matches_known_timestamps() {
        assert_eq!(
            crate::util::unix_to_civil(0.0),
            ("1970-01-01".to_owned(), "00:00:00".to_owned())
        );
        // 2001-09-09T01:46:40Z, the classic billennium check.
        assert_eq!(
            crate::util::unix_to_civil(1_000_000_000.0),
            ("2001-09-09".to_owned(), "01:46:40".to_owned())
        );
        // A leap day, which is where a naive conversion goes wrong.
        assert_eq!(
            crate::util::unix_to_civil(1_709_164_800.0),
            ("2024-02-29".to_owned(), "00:00:00".to_owned())
        );
        // Before the epoch, and not a finite number at all.
        assert_eq!(
            crate::util::unix_to_civil(-1.0),
            ("1969-12-31".to_owned(), "23:59:59".to_owned())
        );
        assert_eq!(
            crate::util::unix_to_civil(f64::NAN),
            ("1970-01-01".to_owned(), "00:00:00".to_owned())
        );
    }

    #[test]
    fn format_is_guessed_from_the_extension() {
        assert_eq!(Format::from_path(Path::new("a.csv")), Format::Csv);
        assert_eq!(Format::from_path(Path::new("a.CSV")), Format::Csv);
        assert_eq!(Format::from_path(Path::new("a.bin")), Format::SoapyPowerBin);
        assert_eq!(Format::from_path(Path::new("a")), Format::SoapyPowerBin);
    }

    #[test]
    fn a_binary_recording_reads_back_as_the_same_sweeps() {
        let path = temp_path("bin-roundtrip");
        let mut rec = Recorder::create(&path, Format::SoapyPowerBin).expect("create");
        rec.write(&frame(1000.0, &[-90.0, -80.0, -70.0, -60.0]))
            .unwrap();
        rec.write(&frame(1001.0, &[-91.0, -81.0, -71.0, -61.0]))
            .unwrap();
        assert_eq!(rec.sweeps(), 2);
        rec.finish().unwrap();

        let mut f = File::open(&path).expect("open");
        let frames = soapy_bin::read_frames(&mut f);
        assert_eq!(frames.len(), 2, "both sweeps must come back");
        assert_eq!(frames[0].y, vec![-90.0, -80.0, -70.0, -60.0]);
        assert_eq!(frames[1].y, vec![-91.0, -81.0, -71.0, -61.0]);
        assert_eq!(frames[0].x.len(), 4);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_interrupted_binary_recording_keeps_its_complete_sweeps() {
        // The whole reason for this format: cut the file mid-record and the
        // sweeps before it must still load.
        let path = temp_path("bin-truncated");
        let mut rec = Recorder::create(&path, Format::SoapyPowerBin).expect("create");
        for i in 0..3 {
            rec.write(&frame(1000.0 + i as f64, &[-90.0; 8])).unwrap();
        }
        rec.finish().unwrap();

        let full = std::fs::read(&path).expect("read");
        let record_len = soapy_bin::RECORD_HEADER_LEN + 8 * 4;
        assert_eq!(full.len(), record_len * 3);

        // Chop the last record in half, as a killed process would.
        std::fs::write(&path, &full[..record_len * 2 + record_len / 2]).unwrap();

        let mut f = File::open(&path).expect("open");
        let frames = soapy_bin::read_frames(&mut f);
        assert_eq!(frames.len(), 2, "complete sweeps must survive truncation");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn flushing_per_sweep_means_the_file_is_readable_while_recording() {
        let path = temp_path("bin-live");
        let mut rec = Recorder::create(&path, Format::SoapyPowerBin).expect("create");
        rec.write(&frame(1000.0, &[-90.0; 4])).unwrap();

        // Still open and recording, but the sweep is already on disk.
        let mut f = File::open(&path).expect("open");
        assert_eq!(soapy_bin::read_frames(&mut f).len(), 1);

        rec.write(&frame(1001.0, &[-80.0; 4])).unwrap();
        let mut f = File::open(&path).expect("open");
        assert_eq!(soapy_bin::read_frames(&mut f).len(), 2);

        rec.finish().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn csv_has_one_line_per_sweep_in_rtl_power_column_order() {
        let path = temp_path("csv");
        let mut rec = Recorder::create(&path, Format::Csv).expect("create");
        rec.write(&frame(1_000_000_000.0, &[-90.0, -80.5])).unwrap();
        rec.write(&frame(1_000_000_001.0, &[-70.0, -60.0])).unwrap();
        rec.finish().unwrap();

        let text = std::fs::read_to_string(&path).expect("read");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);

        let cols: Vec<&str> = lines[0].split(',').map(|c| c.trim()).collect();
        assert_eq!(cols[0], "2001-09-09");
        assert_eq!(cols[1], "01:46:40");
        assert_eq!(cols[2], "87000000");
        assert_eq!(cols[3], "87010000");
        assert_eq!(cols[6], "-90.00");
        assert_eq!(cols[7], "-80.50");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_and_mismatched_sweeps_are_skipped_rather_than_written() {
        let path = temp_path("skip");
        let mut rec = Recorder::create(&path, Format::SoapyPowerBin).expect("create");
        rec.write(&frame(1.0, &[])).unwrap();

        let mut bad = frame(1.0, &[-90.0, -80.0]);
        bad.x = Arc::new(vec![1.0, 2.0, 3.0]);
        rec.write(&bad).unwrap();

        assert_eq!(rec.sweeps(), 0);
        assert_eq!(rec.bytes(), 0);
        rec.finish().unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn creating_in_a_missing_directory_is_an_error_not_a_panic() {
        let mut p = temp_path("nope");
        p.push("deeper");
        p.push("still.bin");
        assert!(Recorder::create(&p, Format::SoapyPowerBin).is_err());
    }
}
