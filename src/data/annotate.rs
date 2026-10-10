//! Labelled margins for an exported waterfall picture: a frequency ruler
//! across the top and wall-clock times down the left.
//!
//! The layout follows `rtl_power`'s `heatmap.py` output, which is what people
//! compare these captures against: a yellow band carrying tick marks and
//! frequency labels, with the image proper below it.
//!
//! This is for the **PNG** export only. The TIFF holds raw dB values, and
//! painting labels into it would overwrite measurements with decoration; its
//! axes travel as metadata instead.
//!
//! The font is a 5x7 bitmap written out below rather than a real typeface,
//! because the alternative is a font crate plus an embedded TTF to render a
//! dozen distinct characters at one fixed size.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Glyph cell size, and the advance between cells.
const GLYPH_W: usize = 5;
const GLYPH_H: usize = 7;
const ADVANCE: usize = GLYPH_W + 1;

/// Height of the frequency band across the top.
const BAND_H: usize = 18;
/// Width of the time column down the left.
const TIME_W: usize = 70;
const MARGIN_R: usize = 6;
const MARGIN_B: usize = 6;

/// Shortest gap between frequency labels, so they cannot run together.
const MIN_LABEL_GAP_PX: usize = 62;
/// Shortest gap between time labels.
const MIN_TIME_GAP_PX: usize = 18;

const BAND_BG: [u8; 3] = [255, 255, 0];
const BAND_FG: [u8; 3] = [0, 0, 0];
const SIDE_BG: [u8; 3] = [16, 16, 18];
const SIDE_FG: [u8; 3] = [200, 200, 205];

/// What the two axes mean.
#[derive(Clone, Copy, Debug, Default)]
pub struct Annotation {
    /// Centre frequency of the first bin, Hz.
    pub start_hz: f64,
    /// Bin spacing, Hz.
    pub bin_hz: f64,
    /// Unix time of the newest sweep, which is image row 0.
    pub newest_unix: f64,
    /// Seconds per sweep.
    pub sweep_s: f64,
}

// ---------------------------------------------------------------------------
// Font
// ---------------------------------------------------------------------------

/// Only the characters the two axes can produce. Anything else renders blank,
/// which is a gap rather than a panic.
const FONT: &[(char, [&str; GLYPH_H])] = &[
    (
        '0',
        [
            ".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###.",
        ],
    ),
    (
        '1',
        [
            "..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ],
    ),
    (
        '2',
        [
            ".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####",
        ],
    ),
    (
        '3',
        [
            "#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###.",
        ],
    ),
    (
        '4',
        [
            "...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#.",
        ],
    ),
    (
        '5',
        [
            "#####", "#....", "####.", "....#", "....#", "#...#", ".###.",
        ],
    ),
    (
        '6',
        [
            "..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###.",
        ],
    ),
    (
        '7',
        [
            "#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#...",
        ],
    ),
    (
        '8',
        [
            ".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###.",
        ],
    ),
    (
        '9',
        [
            ".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##..",
        ],
    ),
    (
        '.',
        [
            ".....", ".....", ".....", ".....", ".....", ".##..", ".##..",
        ],
    ),
    (
        ':',
        [
            ".....", ".##..", ".##..", ".....", ".##..", ".##..", ".....",
        ],
    ),
    (
        '-',
        [
            ".....", ".....", ".....", "#####", ".....", ".....", ".....",
        ],
    ),
    (
        ' ',
        [
            ".....", ".....", ".....", ".....", ".....", ".....", ".....",
        ],
    ),
    (
        'k',
        [
            "#....", "#....", "#..#.", "#.#..", "##...", "#.#..", "#..#.",
        ],
    ),
    (
        'M',
        [
            "#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'G',
        [
            ".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".###.",
        ],
    ),
    (
        'H',
        [
            "#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'z',
        [
            ".....", ".....", "#####", "...#.", "..#..", ".#...", "#####",
        ],
    ),
];

type Glyph = [[bool; GLYPH_W]; GLYPH_H];

fn glyphs() -> &'static HashMap<char, Glyph> {
    static TABLE: OnceLock<HashMap<char, Glyph>> = OnceLock::new();
    TABLE.get_or_init(|| {
        FONT.iter()
            .map(|(ch, rows)| {
                let mut g: Glyph = [[false; GLYPH_W]; GLYPH_H];
                for (y, row) in rows.iter().enumerate() {
                    for (x, c) in row.chars().take(GLYPH_W).enumerate() {
                        g[y][x] = c == '#';
                    }
                }
                (*ch, g)
            })
            .collect()
    })
}

/// Rendered width of `text`, in pixels.
pub fn text_width(text: &str) -> usize {
    match text.chars().count() {
        0 => 0,
        n => n * ADVANCE - 1,
    }
}

// ---------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------

/// A plain RGB8 image being drawn into.
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize, fill: [u8; 3]) -> Self {
        let mut px = Vec::with_capacity(w * h * 3);
        for _ in 0..w * h {
            px.extend_from_slice(&fill);
        }
        Self { w, h, px }
    }

    fn set(&mut self, x: usize, y: usize, c: [u8; 3]) {
        if x >= self.w || y >= self.h {
            return;
        }
        let i = (y * self.w + x) * 3;
        self.px[i..i + 3].copy_from_slice(&c);
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, c: [u8; 3]) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                self.set(xx, yy, c);
            }
        }
    }

    /// Blit an RGB block, used to drop the waterfall into its margins.
    fn blit(&mut self, x: usize, y: usize, w: usize, h: usize, rgb: &[u8]) {
        for row in 0..h {
            let src = row * w * 3;
            let Some(line) = rgb.get(src..src + w * 3) else {
                break;
            };
            for col in 0..w {
                let c = [line[col * 3], line[col * 3 + 1], line[col * 3 + 2]];
                self.set(x + col, y + row, c);
            }
        }
    }

    fn text(&mut self, x: usize, y: usize, s: &str, c: [u8; 3]) {
        let table = glyphs();
        for (i, ch) in s.chars().enumerate() {
            let Some(g) = table.get(&ch) else { continue };
            let ox = x + i * ADVANCE;
            for (gy, row) in g.iter().enumerate() {
                for (gx, on) in row.iter().enumerate() {
                    if *on {
                        self.set(ox + gx, y + gy, c);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tick selection
// ---------------------------------------------------------------------------

/// A 1-2-5 step at least `min` large.
pub fn nice_step(min: f64) -> f64 {
    if !(min > 0.0) || !min.is_finite() {
        return 1.0;
    }
    let decade = 10f64.powf(min.log10().floor());
    for mult in [1.0, 2.0, 5.0, 10.0] {
        let step = decade * mult;
        if step >= min {
            return step;
        }
    }
    decade * 10.0
}

/// SI scale for a ruler whose largest label is `max_hz`.
///
/// The smallest prefix that still keeps that label under five digits, which is
/// what makes a ruler read `100M 200M ... 1600M` like `rtl_power`'s heatmap
/// rather than flipping to `1.6G` partway along -- while a narrow span stays
/// out of `87500k` territory.
fn tick_scale(max_hz: f64) -> (f64, &'static str) {
    let max = max_hz.abs();
    for (scale, suffix) in [(1.0, ""), (1e3, "k"), (1e6, "M"), (1e9, "G")] {
        if max / scale < 10_000.0 {
            return (scale, suffix);
        }
    }
    (1e9, "G")
}

/// A tick label: the value at `hz`, scaled for a ruler topping out at
/// `max_hz`, with just enough decimals to tell neighbouring ticks apart.
pub fn format_tick(hz: f64, step: f64, max_hz: f64) -> String {
    let (scale, suffix) = tick_scale(max_hz);
    let stepped = step / scale;
    let decimals = if stepped >= 1.0 {
        0
    } else if stepped >= 0.1 {
        1
    } else {
        2
    };
    format!("{:.*}{suffix}", decimals, hz / scale)
}

// ---------------------------------------------------------------------------
// Compositing
// ---------------------------------------------------------------------------

/// Total size of the annotated image for a `width x height` waterfall.
pub fn outer_size(width: u32, height: u32) -> (u32, u32) {
    (
        width + (TIME_W + MARGIN_R) as u32,
        height + (BAND_H + MARGIN_B) as u32,
    )
}

/// Wrap `rgb` (a `width x height` RGB8 waterfall) in labelled margins.
pub fn with_axes(width: u32, height: u32, rgb: &[u8], a: &Annotation) -> (u32, u32, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (ow, oh) = outer_size(width, height);
    let mut canvas = Canvas::new(ow as usize, oh as usize, SIDE_BG);

    canvas.blit(TIME_W, BAND_H, w, h, rgb);

    draw_frequency_band(&mut canvas, w, a);
    draw_time_column(&mut canvas, h, a);

    (ow, oh, canvas.px)
}

fn draw_frequency_band(canvas: &mut Canvas, w: usize, a: &Annotation) {
    canvas.fill_rect(0, 0, canvas.w, BAND_H, BAND_BG);

    // The date sits in the corner cell, where it labels the time column
    // without stealing a row from it.
    if a.newest_unix > 0.0 {
        let (date, _) = crate::util::unix_to_civil(a.newest_unix);
        canvas.text(3, (BAND_H - GLYPH_H) / 2, &date, BAND_FG);
    }

    if !(a.bin_hz > 0.0) || !a.bin_hz.is_finite() || w == 0 {
        return;
    }

    let span = a.bin_hz * w as f64;
    let step = nice_step(span * MIN_LABEL_GAP_PX as f64 / w as f64);
    let minor = step / 5.0;

    let first = (a.start_hz / minor).ceil() * minor;
    let mut hz = first;
    // The guard is on the pixel position, not the value, so a pathological
    // step cannot spin here.
    while hz <= a.start_hz + span {
        let px = ((hz - a.start_hz) / a.bin_hz).round();
        if px < 0.0 || px >= w as f64 {
            hz += minor;
            continue;
        }
        let x = TIME_W + px as usize;

        // Whether this is a labelled tick, tested against the step rather
        // than counted, so rounding cannot drift over a long ruler.
        let on_major = ((hz / step).round() * step - hz).abs() < minor * 0.5;
        let tick = if on_major { 7 } else { 4 };
        canvas.fill_rect(x, BAND_H - tick, 1, tick, BAND_FG);

        if on_major {
            let label = format_tick(hz, step, a.start_hz + span);
            let lx = x.saturating_sub(text_width(&label) / 2);
            canvas.text(lx, 2, &label, BAND_FG);
        }

        hz += minor;
    }
}

fn draw_time_column(canvas: &mut Canvas, h: usize, a: &Annotation) {
    if !(a.sweep_s > 0.0) || !a.sweep_s.is_finite() || a.newest_unix <= 0.0 || h == 0 {
        return;
    }

    // Label on whole seconds, at a spacing that keeps the text from touching.
    let rows_per_label = (MIN_TIME_GAP_PX as f64 / 1.0).max(1.0);
    let secs_per_label = nice_step(rows_per_label * a.sweep_s);
    let rows_step = (secs_per_label / a.sweep_s).max(1.0);

    // Whole seconds would print the same label over and over for a fast
    // sweep, so sub-second spacing gets a tenths place.
    let subsecond = secs_per_label < 1.0;

    let mut row = 0.0;
    while row < h as f64 {
        let y = BAND_H + row.round() as usize;
        canvas.fill_rect(TIME_W - 4, y, 4, 1, SIDE_FG);

        let time = clock_label(a.newest_unix - row * a.sweep_s, subsecond);
        // Centre the text on its tick, but never let the topmost one ride up
        // into the frequency band or the bottom one off the image.
        let ty = y
            .saturating_sub(GLYPH_H / 2)
            .clamp(BAND_H, (canvas.h.saturating_sub(GLYPH_H)).max(BAND_H));
        canvas.text(TIME_W - 6 - text_width(&time), ty, &time, SIDE_FG);

        row += rows_step;
    }
}

/// `HH:MM:SS`, or `HH:MM:SS.t` when ticks are closer together than a second.
fn clock_label(unix: f64, subsecond: bool) -> String {
    let (_, time) = crate::util::unix_to_civil(unix);
    if !subsecond {
        return time;
    }
    // `unix_to_civil` floors, so the remainder is the fraction it dropped.
    let tenths = ((unix - unix.floor()) * 10.0).floor().clamp(0.0, 9.0) as u8;
    format!("{time}.{tenths}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(w: u32, h: u32) -> (Vec<u8>, Annotation) {
        // A flat mid-grey block, so anything drawn over it is obvious.
        let rgb = vec![128u8; (w * h * 3) as usize];
        let a = Annotation {
            start_hz: 87e6,
            bin_hz: 10e3,
            // 2001-09-09 01:46:40 UTC.
            newest_unix: 1_000_000_000.0,
            sweep_s: 1.0,
        };
        (rgb, a)
    }

    #[test]
    fn nice_step_climbs_the_1_2_5_ladder() {
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(1.5), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(6.0), 10.0);
        assert_eq!(nice_step(150_000.0), 200_000.0);
        // Nonsense in, something usable out.
        assert_eq!(nice_step(0.0), 1.0);
        assert_eq!(nice_step(-5.0), 1.0);
        assert_eq!(nice_step(f64::NAN), 1.0);
    }

    #[test]
    fn ticks_read_like_the_rtl_power_ruler() {
        // The 24M-1701M sweep in rtl_power's own heatmap: labelled in M all
        // the way up, not flipped to G once past 1000.
        assert_eq!(format_tick(100e6, 100e6, 1701e6), "100M");
        assert_eq!(format_tick(1.6e9, 100e6, 1701e6), "1600M");
        // A narrow FM-band ruler stays in M rather than reading "87500k".
        assert_eq!(format_tick(87.5e6, 500e3, 108e6), "87.5M");
        assert_eq!(format_tick(500e3, 100e3, 900e3), "500k");
        // Only once M itself would need five digits does G take over.
        assert_eq!(format_tick(12e9, 1e9, 24e9), "12G");
    }

    #[test]
    fn the_tick_scale_keeps_labels_short() {
        assert_eq!(tick_scale(900.0), (1.0, ""));
        assert_eq!(tick_scale(900e3), (1e3, "k"));
        assert_eq!(tick_scale(1701e6), (1e6, "M"));
        assert_eq!(tick_scale(24e9), (1e9, "G"));
        // Absurd inputs still pick something.
        assert_eq!(tick_scale(0.0), (1.0, ""));
        assert_eq!(tick_scale(1e30), (1e9, "G"));
    }

    #[test]
    fn margins_are_added_around_the_image() {
        let (rgb, a) = probe(200, 50);
        let (w, h, out) = with_axes(200, 50, &rgb, &a);
        assert_eq!((w, h), outer_size(200, 50));
        assert!(w > 200 && h > 50);
        assert_eq!(out.len(), (w * h * 3) as usize);
    }

    #[test]
    fn the_waterfall_lands_intact_inside_its_margins() {
        let (rgb, a) = probe(60, 20);
        let (w, _, out) = with_axes(60, 20, &rgb, &a);
        let at = |x: usize, y: usize| {
            let i = (y * w as usize + x) * 3;
            [out[i], out[i + 1], out[i + 2]]
        };
        // Every pixel of the original is where it should be, unmodified.
        for y in 0..20 {
            for x in 0..60 {
                assert_eq!(at(TIME_W + x, BAND_H + y), [128, 128, 128], "at {x},{y}");
            }
        }
    }

    #[test]
    fn the_band_is_yellow_and_carries_ink() {
        let (rgb, a) = probe(400, 40);
        let (w, _, out) = with_axes(400, 40, &rgb, &a);
        let at = |x: usize, y: usize| {
            let i = (y * w as usize + x) * 3;
            [out[i], out[i + 1], out[i + 2]]
        };

        // Background of the band.
        assert_eq!(at(TIME_W + 200, 0), BAND_BG);
        // Ticks and labels are drawn in black somewhere along it.
        let band_ink = (0..w as usize)
            .flat_map(|x| (0..BAND_H).map(move |y| (x, y)))
            .filter(|(x, y)| at(*x, *y) == BAND_FG)
            .count();
        assert!(band_ink > 50, "band looks empty: {band_ink} dark pixels");
    }

    #[test]
    fn clock_labels_gain_a_tenths_place_only_when_needed() {
        // 2001-09-09 01:46:40.25 UTC.
        assert_eq!(clock_label(1_000_000_000.25, false), "01:46:40");
        assert_eq!(clock_label(1_000_000_000.25, true), "01:46:40.2");
        assert_eq!(clock_label(1_000_000_000.0, true), "01:46:40.0");
        // The tenths place can never carry into the second.
        assert_eq!(clock_label(1_000_000_000.999, true), "01:46:40.9");
    }

    #[test]
    fn a_fast_sweep_does_not_repeat_the_same_timestamp() {
        // 100 sweeps at 10 ms is 1 s total: whole-second labels would all read
        // the same, which is what the tenths place is for.
        let rgb = vec![0u8; 50 * 100 * 3];
        let a = Annotation {
            start_hz: 87e6,
            bin_hz: 10e3,
            newest_unix: 1_000_000_000.0,
            sweep_s: 0.01,
        };
        let (w, h, out) = with_axes(50, 100, &rgb, &a);
        assert_eq!(out.len(), (w * h * 3) as usize);

        // Two labels a row-step apart must differ somewhere in the column.
        let column_ink = |y0: usize, y1: usize| {
            (0..TIME_W)
                .flat_map(|x| (y0..y1).map(move |y| (x, y)))
                .filter(|(x, y)| {
                    let i = (y * w as usize + x) * 3;
                    [out[i], out[i + 1], out[i + 2]] == SIDE_FG
                })
                .count()
        };
        assert!(column_ink(BAND_H, BAND_H + 40) > 20, "no time labels drawn");
    }

    #[test]
    fn the_time_column_carries_ink() {
        let (rgb, a) = probe(100, 200);
        let (w, h, out) = with_axes(100, 200, &rgb, &a);
        let at = |x: usize, y: usize| {
            let i = (y * w as usize + x) * 3;
            [out[i], out[i + 1], out[i + 2]]
        };
        let side_ink = (0..TIME_W)
            .flat_map(|x| (BAND_H..h as usize).map(move |y| (x, y)))
            .filter(|(x, y)| at(*x, *y) == SIDE_FG)
            .count();
        assert!(
            side_ink > 50,
            "time column looks empty: {side_ink} lit pixels"
        );
    }

    #[test]
    fn an_unknown_character_leaves_a_gap_rather_than_panicking() {
        let mut c = Canvas::new(60, 12, [0, 0, 0]);
        c.text(0, 0, "12 ?? 34", [255, 255, 255]);
        assert!(c.px.contains(&255), "known glyphs still drew");
    }

    #[test]
    fn degenerate_axes_still_produce_a_valid_image() {
        let rgb = vec![10u8; 30 * 10 * 3];
        for a in [
            Annotation::default(),
            Annotation {
                bin_hz: 0.0,
                sweep_s: 1.0,
                ..Default::default()
            },
            Annotation {
                bin_hz: f64::NAN,
                ..Default::default()
            },
            Annotation {
                bin_hz: 1e3,
                sweep_s: -1.0,
                newest_unix: 1.0,
                ..Default::default()
            },
            Annotation {
                bin_hz: 1e3,
                sweep_s: f64::INFINITY,
                newest_unix: 1e9,
                start_hz: 0.0,
            },
        ] {
            let (w, h, out) = with_axes(30, 10, &rgb, &a);
            assert_eq!(out.len(), (w * h * 3) as usize, "{a:?}");
        }
    }

    #[test]
    fn a_very_wide_ruler_terminates() {
        // A tiny bin size over a wide image is the case where a naive minor
        // tick loop runs for a long time; the pixel guard has to bound it.
        let rgb = vec![0u8; 4000 * 4 * 3];
        let a = Annotation {
            start_hz: 0.0,
            bin_hz: 1.0,
            newest_unix: 1e9,
            sweep_s: 0.001,
        };
        let (w, h, out) = with_axes(4000, 4, &rgb, &a);
        assert_eq!(out.len(), (w * h * 3) as usize);
    }

    #[test]
    fn text_width_matches_what_is_drawn() {
        assert_eq!(text_width(""), 0);
        assert_eq!(text_width("1"), GLYPH_W);
        assert_eq!(text_width("12"), GLYPH_W * 2 + 1);
        assert_eq!(text_width("100M"), ADVANCE * 4 - 1);
    }

    #[test]
    fn every_font_row_is_the_declared_width() {
        for (ch, rows) in FONT {
            for (i, row) in rows.iter().enumerate() {
                assert_eq!(
                    row.chars().count(),
                    GLYPH_W,
                    "glyph {ch:?} row {i} is not {GLYPH_W} wide"
                );
            }
        }
        assert_eq!(
            glyphs().len(),
            FONT.len(),
            "duplicate character in the font"
        );
    }
}
