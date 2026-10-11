//! Render the bitmap font to a PNG so the glyphs can be checked by eye.
//!
//! `cargo run --example font_sheet -- sheet.png`

use spectroscope::data::annotate::{self, Annotation};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sheet.png".to_owned());

    let lines = [
        "ABCDEFGHIJKLM",
        "NOPQRSTUVWXYZ",
        "abcdefghijklm",
        "nopqrstuvwxyz",
        "0123456789 .:-",
        "FM Broadcast",
        "2m Ham Band (144-148)",
        "Air Band VOR/ILS",
        "PMR446 / ISM 2.4GHz",
    ];

    let scale = 3usize;
    let pad = 4usize;
    let w = lines
        .iter()
        .map(|l| annotate::text_width(l))
        .max()
        .unwrap_or(0)
        + pad * 2;
    let h = lines.len() * (annotate::GLYPH_H + 2) + pad * 2;

    let mut rgb = vec![255u8; w * h * 3];
    for (row, line) in lines.iter().enumerate() {
        annotate::draw_text_rgb(
            &mut rgb,
            w,
            h,
            pad,
            pad + row * (annotate::GLYPH_H + 2),
            line,
            [0, 0, 0],
        );
    }

    // Nearest-neighbour upscale, so individual pixels are legible.
    let (bw, bh) = (w * scale, h * scale);
    let mut big = vec![0u8; bw * bh * 3];
    for y in 0..bh {
        for x in 0..bw {
            let src = ((y / scale) * w + (x / scale)) * 3;
            let dst = (y * bw + x) * 3;
            big[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
        }
    }

    let _ = Annotation::default();
    match image::RgbImage::from_raw(bw as u32, bh as u32, big) {
        Some(img) => match img.save(&path) {
            Ok(()) => println!("wrote {path}: {bw} x {bh}"),
            Err(e) => eprintln!("save failed: {e}"),
        },
        None => eprintln!("buffer size mismatch"),
    }
}
