//! Saving the waterfall as an image.
//!
//! The export is the stored history at its native resolution -- one pixel per
//! bin per sweep -- not a screenshot of the widget. What is on screen has been
//! decimated to the pane's width and vertical scale; a saved waterfall is
//! usually wanted as evidence, so it keeps every bin.
//!
//! The two formats are deliberately different things:
//!
//! * **PNG** is a picture. The colour map is baked into 8-bit RGB, which is
//!   what you want for a report or a bug thread.
//! * **TIFF** is the measurement. Pixels are the raw `f32` dB values, and the
//!   colour map travels beside them as an ImageJ LUT together with the display
//!   window, so ImageJ opens it looking exactly like the screen while every
//!   pixel still reads back in dB. Baking the colours in would quantise 32-bit
//!   data to 8 bits per channel and make the file unmeasurable.
//!
//! Both are lossless, which matters when the image is evidence: JPEG ringing
//! around a carrier is indistinguishable from a spur.

use std::path::Path;

use crate::data::HistoryBuffer;

/// Image container to write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFormat {
    #[default]
    Png,
    Tiff,
}

impl ImageFormat {
    /// Pick from the path's extension, defaulting to PNG.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff") => {
                Self::Tiff
            }
            _ => Self::Png,
        }
    }
}

#[derive(Debug)]
pub enum ExportError {
    /// Nothing has been captured yet.
    Empty,
    Io(String),
    Encode(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "no sweeps have been captured yet"),
            Self::Io(m) => write!(f, "{m}"),
            Self::Encode(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// What the frequency and time axes mean, recorded alongside the pixels.
///
/// Without this a raw TIFF is a grid of numbers with no idea which bin or which
/// sweep any of them came from.
#[derive(Clone, Copy, Debug, Default)]
pub struct Axes {
    /// Centre frequency of the first bin, Hz.
    pub start_hz: f64,
    /// Bin spacing, Hz.
    pub bin_hz: f64,
    /// Seconds per sweep; `0` when it was never measured.
    pub sweep_s: f64,
    /// Unix time of the newest sweep, which is image row 0.
    pub newest_unix: f64,
}

impl Axes {
    // Only the PNG path annotates, and that is desktop-only.
    #[cfg(not(target_arch = "wasm32"))]
    fn annotation(&self) -> super::annotate::Annotation {
        super::annotate::Annotation {
            start_hz: self.start_hz,
            bin_hz: self.bin_hz,
            newest_unix: self.newest_unix,
            sweep_s: self.sweep_s,
        }
    }
}

/// Raw `f32` dB values, row 0 the newest sweep, and the image dimensions.
///
/// This is the TIFF's pixel data, and also what the PNG path colours.
fn raw_rows(history: &HistoryBuffer) -> Option<(u32, u32, Vec<f32>)> {
    let width = history.bins();
    let height = history.len();
    if width == 0 || height == 0 {
        return None;
    }

    let mut out = Vec::with_capacity(width * height);
    for age in 0..height {
        match history.row_from_newest(age) {
            Some(row) => {
                let n = row.len().min(width);
                out.extend_from_slice(&row[..n]);
                out.resize(out.len() + (width - n), f32::NAN);
            }
            None => out.resize(out.len() + width, f32::NAN),
        }
    }

    Some((width as u32, height as u32, out))
}

/// Render the history to an RGB buffer, newest sweep on the first row.
///
/// Returns `(width, height, rgb)` with `rgb` three bytes per pixel. Separated
/// from the file writing so the mapping can be tested without touching a disk.
pub fn render_rgb(
    history: &HistoryBuffer,
    lut: &[[u8; 3]; 256],
    low: f32,
    high: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    let (width, height, raw) = raw_rows(history)?;

    let span = high - low;
    let inv = if span > 0.0 && span.is_finite() {
        1.0 / span
    } else {
        0.0
    };

    let mut rgb = Vec::with_capacity(raw.len() * 3);
    for v in raw {
        let t = (v - low) * inv;
        // NaN must land at the bottom of the map rather than wrapping the
        // index, the same rule the shader and the CPU preview follow.
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        rgb.extend_from_slice(&lut[(t * 255.0).round() as usize]);
    }

    Some((width, height, rgb))
}

/// Write the history to `path`, choosing the container from its extension.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(
    path: &Path,
    history: &HistoryBuffer,
    lut: &[[u8; 3]; 256],
    low: f32,
    high: f32,
    axes: Axes,
) -> Result<(u32, u32), ExportError> {
    match ImageFormat::from_path(path) {
        ImageFormat::Png => save_png(path, history, lut, low, high, axes),
        ImageFormat::Tiff => save_tiff(path, history, lut, low, high, axes),
    }
}

/// Colour-mapped 8-bit RGB, inside a labelled frequency/time frame.
#[cfg(not(target_arch = "wasm32"))]
fn save_png(
    path: &Path,
    history: &HistoryBuffer,
    lut: &[[u8; 3]; 256],
    low: f32,
    high: f32,
    axes: Axes,
) -> Result<(u32, u32), ExportError> {
    let (w, h, rgb) = render_rgb(history, lut, low, high).ok_or(ExportError::Empty)?;
    // A picture is read by eye, so it gets the ruler; the TIFF stays raw.
    let (width, height, rgb) = super::annotate::with_axes(w, h, &rgb, &axes.annotation());

    let buffer = image::RgbImage::from_raw(width, height, rgb)
        .ok_or_else(|| ExportError::Encode("buffer does not match its dimensions".to_owned()))?;

    let file = std::fs::File::create(path).map_err(|e| ExportError::Io(e.to_string()))?;
    let mut writer = std::io::BufWriter::new(file);
    buffer
        .write_to(&mut writer, image::ImageFormat::Png)
        .map_err(|e| ExportError::Encode(e.to_string()))?;

    Ok((width, height))
}

/// 32-bit float TIFF carrying the dB values, with the colour map and the
/// display window as ImageJ metadata.
///
/// ImageJ opens this as a 32-bit image, applies the stored LUT and contrast
/// window, and shows what the waterfall showed -- while *Analyze > Measure* and
/// every plugin still see dB. Deflate keeps it to a reasonable size and is one
/// of the two compressions ImageJ's own TIFF reader understands.
#[cfg(not(target_arch = "wasm32"))]
fn save_tiff(
    path: &Path,
    history: &HistoryBuffer,
    lut: &[[u8; 3]; 256],
    low: f32,
    high: f32,
    axes: Axes,
) -> Result<(u32, u32), ExportError> {
    use fast_tiff_lib::{
        Compression, DisplayMode, MetadataFormat, SampleType, StackMetaWrite, TiffWriter,
        WriterOptions,
    };

    let (width, height, raw) = raw_rows(history).ok_or(ExportError::Empty)?;

    // `Color` is the mode that tells ImageJ to run the single channel through
    // the LUT below instead of showing it grey.
    let mut meta = StackMetaWrite::new(1, 1)
        .mode(DisplayMode::Color)
        .channel_lut(*lut)
        .range(low as f64, high as f64)
        // The axes carry different units, which ImageJ's single spatial
        // calibration cannot express, so they go in as plain description lines
        // rather than as a pixel size that would mislabel one of them.
        .extra("spectroscope.row0", "newest sweep")
        .extra("spectroscope.x", "frequency")
        .extra("spectroscope.y", "time, newest first")
        .extra("spectroscope.values", "dBm");

    if axes.bin_hz.is_finite() && axes.bin_hz > 0.0 {
        meta = meta
            .extra("spectroscope.start_hz", format!("{:.6}", axes.start_hz))
            .extra("spectroscope.bin_hz", format!("{:.6}", axes.bin_hz));
    }
    if axes.sweep_s.is_finite() && axes.sweep_s > 0.0 {
        meta = meta
            .frame_interval_s(axes.sweep_s)
            .extra("spectroscope.sweep_s", format!("{:.6}", axes.sweep_s));
    }

    let options = WriterOptions::new(width, height, SampleType::F32)
        .compression(Compression::Deflate)
        .metadata_format(MetadataFormat::ImageJ)
        .metadata(meta);

    let mut writer =
        TiffWriter::create(path, options).map_err(|e| ExportError::Io(e.to_string()))?;
    writer
        .write_frame_f32(&raw)
        .map_err(|e| ExportError::Encode(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| ExportError::Io(e.to_string()))?;

    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREY: [[u8; 3]; 256] = {
        let mut lut = [[0u8; 3]; 256];
        let mut i = 0;
        while i < 256 {
            lut[i] = [i as u8, i as u8, i as u8];
            i += 1;
        }
        lut
    };

    fn history(rows: &[&[f32]]) -> HistoryBuffer {
        let bins = rows.first().map_or(0, |r| r.len());
        let mut h = HistoryBuffer::new(bins, rows.len().max(1));
        for r in rows {
            h.append(r);
        }
        h
    }

    #[test]
    fn empty_history_exports_nothing() {
        assert!(render_rgb(&HistoryBuffer::default(), &GREY, -100.0, 0.0).is_none());
        assert!(render_rgb(&HistoryBuffer::new(8, 4), &GREY, -100.0, 0.0).is_none());
        assert!(raw_rows(&HistoryBuffer::new(8, 4)).is_none());
    }

    #[test]
    fn newest_sweep_is_the_first_row() {
        let h = history(&[&[-100.0, -100.0], &[0.0, 0.0]]);
        let (w, ht, rgb) = render_rgb(&h, &GREY, -100.0, 0.0).expect("render");
        assert_eq!((w, ht), (2, 2));
        assert_eq!(&rgb[0..3], &[255, 255, 255]);
        assert_eq!(&rgb[6..9], &[0, 0, 0]);

        // The raw buffer agrees with the coloured one about the order.
        let (_, _, raw) = raw_rows(&h).expect("raw");
        assert_eq!(raw, vec![0.0, 0.0, -100.0, -100.0]);
    }

    #[test]
    fn levels_window_the_mapping() {
        let h = history(&[&[-50.0]]);
        let (_, _, rgb) = render_rgb(&h, &GREY, -100.0, 0.0).expect("render");
        assert_eq!(rgb[0], 128);
        let (_, _, rgb) = render_rgb(&h, &GREY, -40.0, 0.0).expect("render");
        assert_eq!(rgb[0], 0);
        let (_, _, rgb) = render_rgb(&h, &GREY, -100.0, -60.0).expect("render");
        assert_eq!(rgb[0], 255);
    }

    #[test]
    fn a_degenerate_window_does_not_divide_by_zero() {
        let h = history(&[&[-50.0]]);
        let (_, _, rgb) = render_rgb(&h, &GREY, -50.0, -50.0).expect("render");
        assert_eq!(rgb[0], 0);
    }

    #[test]
    fn non_finite_samples_do_not_panic() {
        let h = history(&[&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY]]);
        let (_, _, rgb) = render_rgb(&h, &GREY, -100.0, 0.0).expect("render");
        assert_eq!(rgb[0], 0, "NaN must sit at the bottom of the map");
        assert_eq!(rgb[3], 255, "+inf clamps to the top");
        assert_eq!(rgb[6], 0, "-inf clamps to the bottom");
    }

    #[test]
    fn buffer_is_exactly_three_bytes_per_pixel() {
        let h = history(&[&[-90.0; 7], &[-80.0; 7], &[-70.0; 7]]);
        let (w, ht, rgb) = render_rgb(&h, &GREY, -100.0, 0.0).expect("render");
        assert_eq!(rgb.len(), (w * ht * 3) as usize);
    }

    #[test]
    fn a_partly_filled_ring_exports_only_its_live_rows() {
        let mut h = HistoryBuffer::new(4, 10);
        h.append(&[-90.0; 4]);
        h.append(&[-80.0; 4]);
        let (w, ht, _) = render_rgb(&h, &GREY, -100.0, 0.0).expect("render");
        assert_eq!((w, ht), (4, 2), "unwritten rows must not be exported");
    }

    #[test]
    fn format_follows_the_extension() {
        assert_eq!(ImageFormat::from_path(Path::new("a.png")), ImageFormat::Png);
        assert_eq!(
            ImageFormat::from_path(Path::new("a.tif")),
            ImageFormat::Tiff
        );
        assert_eq!(
            ImageFormat::from_path(Path::new("a.TIFF")),
            ImageFormat::Tiff
        );
        assert_eq!(ImageFormat::from_path(Path::new("a")), ImageFormat::Png);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn temp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("spectroscope-export-{}-{name}", std::process::id()));
        p
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn png_is_a_real_colour_image() {
        let h = history(&[&[-90.0, -40.0], &[-70.0, -20.0]]);
        let path = temp("pic.png");
        // A PNG is a picture, so it comes out inside its labelled margins.
        let (w, ht) = save(&path, &h, &GREY, -100.0, 0.0, Axes::default()).expect("save");
        assert_eq!((w, ht), super::super::annotate::outer_size(2, 2));

        let decoded = image::ImageReader::open(&path)
            .expect("open")
            .with_guessed_format()
            .expect("guess")
            .decode()
            .expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (w, ht));

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tiff_keeps_the_raw_db_values() {
        use fast_tiff_lib::{read_frame_f32, SampleFormat, TiffStack};

        // Values that would be destroyed by an 8-bit colour quantisation.
        let h = history(&[&[-93.25, -41.5], &[-72.125, -20.0625]]);
        let path = temp("raw.tif");
        let axes = Axes {
            start_hz: 87e6,
            bin_hz: 10e3,
            sweep_s: 1.5,
            newest_unix: 1e9,
        };
        let (w, ht) = save(&path, &h, &GREY, -100.0, 0.0, axes).expect("save");
        assert_eq!((w, ht), (2, 2));

        let bytes = std::fs::read(&path).expect("read");
        let stack = TiffStack::from_bytes(bytes).expect("parse");
        assert_eq!(stack.frames.len(), 1);

        let frame = &stack.frames[0];
        assert_eq!((frame.width, frame.height), (2, 2));
        assert_eq!(frame.bits_per_sample, 32);
        assert_eq!(frame.sample_format, SampleFormat::Float);

        let pixels = read_frame_f32(&stack.data, frame, stack.byte_order).expect("decode");
        // Newest row first, bit-for-bit.
        assert_eq!(
            pixels.as_ref(),
            &[-72.125f32, -20.0625, -93.25, -41.5],
            "dB values must survive exactly"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tiff_carries_the_colormap_and_display_window() {
        use fast_tiff_lib::TiffStack;

        // A map with a recognisable shape, so a grey default cannot pass.
        let lut = crate::colormap::LUTS[4]; // Turbo
        let h = history(&[&[-90.0, -30.0]]);
        let path = temp("lut.tif");
        save(&path, &h, &lut, -95.0, -25.0, Axes::default()).expect("save");

        let bytes = std::fs::read(&path).expect("read");
        let stack = TiffStack::from_bytes(bytes).expect("parse");

        assert!(
            stack.meta.has_explicit_luts,
            "ImageJ must find a real LUT, not fall back to grey"
        );
        let channel = stack.meta.channel_display.first().expect("one channel");
        assert_eq!(channel.lut, lut, "the active colour map must round-trip");

        let (lo, hi) = channel.range.expect("display window");
        assert!(
            (lo - -95.0).abs() < 1e-6 && (hi - -25.0).abs() < 1e-6,
            "{lo} {hi}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tiff_describes_its_axes() {
        use fast_tiff_lib::TiffStack;

        let h = history(&[&[-90.0, -30.0]]);
        let path = temp("axes.tif");
        let axes = Axes {
            start_hz: 87e6,
            bin_hz: 10e3,
            sweep_s: 2.0,
            newest_unix: 1e9,
        };
        save(&path, &h, &GREY, -100.0, 0.0, axes).expect("save");

        let bytes = std::fs::read(&path).expect("read");
        let stack = TiffStack::from_bytes(bytes).expect("parse");

        let text = stack.description.expect("ImageJ description");
        assert!(text.starts_with("ImageJ="), "{text}");
        assert!(text.contains("spectroscope.bin_hz=10000"), "{text}");
        assert!(text.contains("spectroscope.start_hz=87000000"), "{text}");
        assert!(text.contains("spectroscope.values=dBm"), "{text}");
        assert_eq!(stack.meta.frame_interval_s, Some(2.0));

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_waterfall_sized_tiff_round_trips() {
        use fast_tiff_lib::{read_frame_f32, TiffStack};

        // Realistic shape, and values that compress unevenly.
        let rows: Vec<Vec<f32>> = (0..64)
            .map(|r| {
                (0..512)
                    .map(|c| -95.0 + ((r * 7 + c * 13) % 61) as f32 * 0.5)
                    .collect()
            })
            .collect();
        let refs: Vec<&[f32]> = rows.iter().map(|r| r.as_slice()).collect();
        let h = history(&refs);

        let path = temp("big.tif");
        let (w, ht) = save(&path, &h, &GREY, -100.0, -60.0, Axes::default()).expect("save");
        assert_eq!((w, ht), (512, 64));

        let bytes = std::fs::read(&path).expect("read");
        let stack = TiffStack::from_bytes(bytes).expect("parse");
        let pixels =
            read_frame_f32(&stack.data, &stack.frames[0], stack.byte_order).expect("decode");
        assert_eq!(pixels.len(), 512 * 64);
        // Row 0 of the file is the last row appended.
        assert_eq!(&pixels[..4], &rows[63][..4]);

        let _ = std::fs::remove_file(&path);
    }
}
