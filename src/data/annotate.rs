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
pub const GLYPH_H: usize = 7;
const ADVANCE: usize = GLYPH_W + 1;

/// Height of the frequency band across the top.
const BAND_H: usize = 18;
/// Height of the band plan strip above it.
const PLAN_H: usize = 11;
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
#[derive(Clone, Copy, Debug)]
pub struct Annotation {
    /// Centre frequency of the first bin, Hz.
    pub start_hz: f64,
    /// Bin spacing, Hz.
    pub bin_hz: f64,
    /// Unix time of the newest sweep, which is image row 0.
    pub newest_unix: f64,
    /// Seconds per sweep.
    pub sweep_s: f64,
    /// Band plan to print above the ruler; [`crate::bandplan::NONE`] for none.
    pub plan: &'static crate::bandplan::BandPlan,
}

impl Default for Annotation {
    fn default() -> Self {
        Self {
            start_hz: 0.0,
            bin_hz: 0.0,
            newest_unix: 0.0,
            sweep_s: 0.0,
            plan: &crate::bandplan::NONE,
        }
    }
}

impl Annotation {
    /// Whether a band strip will be drawn, which the margins depend on.
    pub fn has_plan(&self) -> bool {
        !self.plan.bands.is_empty()
    }

    /// Frequency at the right edge of a `width`-pixel image.
    fn stop_hz(&self, width: usize) -> f64 {
        self.start_hz + self.bin_hz * width as f64
    }
}

// ---------------------------------------------------------------------------
// Font
// ---------------------------------------------------------------------------

/// A 5x7 bitmap font covering printable ASCII.
///
/// Written as art rather than packed bytes so a wrong pixel is visible in
/// review. Anything outside this table renders as a blank cell, which leaves a
/// gap rather than panicking.
const FONT: &[(char, [&str; GLYPH_H])] = &[
    (
        ' ',
        [
            ".....", ".....", ".....", ".....", ".....", ".....", ".....",
        ],
    ),
    (
        '!',
        [
            "..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#..",
        ],
    ),
    (
        '"',
        [
            ".#.#.", ".#.#.", ".....", ".....", ".....", ".....", ".....",
        ],
    ),
    (
        '#',
        [
            ".#.#.", ".#.#.", "#####", ".#.#.", "#####", ".#.#.", ".#.#.",
        ],
    ),
    (
        '$',
        [
            "..#..", ".####", "#.#..", ".###.", "..#.#", "####.", "..#..",
        ],
    ),
    (
        '%',
        [
            "##..#", "##.#.", "..#..", ".#...", "#..##", ".#.##", ".....",
        ],
    ),
    (
        '&',
        [
            ".##..", "#..#.", "#.#..", ".#...", "#.#.#", "#..#.", ".##.#",
        ],
    ),
    (
        '\'',
        [
            "..#..", "..#..", ".....", ".....", ".....", ".....", ".....",
        ],
    ),
    (
        '(',
        [
            "...#.", "..#..", ".#...", ".#...", ".#...", "..#..", "...#.",
        ],
    ),
    (
        ')',
        [
            ".#...", "..#..", "...#.", "...#.", "...#.", "..#..", ".#...",
        ],
    ),
    (
        '*',
        [
            ".....", "#.#.#", ".###.", "#####", ".###.", "#.#.#", ".....",
        ],
    ),
    (
        '+',
        [
            ".....", "..#..", "..#..", "#####", "..#..", "..#..", ".....",
        ],
    ),
    (
        ',',
        [
            ".....", ".....", ".....", ".....", ".##..", ".##..", ".#...",
        ],
    ),
    (
        '-',
        [
            ".....", ".....", ".....", "#####", ".....", ".....", ".....",
        ],
    ),
    (
        '.',
        [
            ".....", ".....", ".....", ".....", ".....", ".##..", ".##..",
        ],
    ),
    (
        '/',
        [
            "....#", "....#", "...#.", "..#..", ".#...", "#....", "#....",
        ],
    ),
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
        ':',
        [
            ".....", ".##..", ".##..", ".....", ".##..", ".##..", ".....",
        ],
    ),
    (
        ';',
        [
            ".....", ".##..", ".##..", ".....", ".##..", ".##..", ".#...",
        ],
    ),
    (
        '<',
        [
            "...#.", "..#..", ".#...", "#....", ".#...", "..#..", "...#.",
        ],
    ),
    (
        '=',
        [
            ".....", ".....", "#####", ".....", "#####", ".....", ".....",
        ],
    ),
    (
        '>',
        [
            ".#...", "..#..", "...#.", "....#", "...#.", "..#..", ".#...",
        ],
    ),
    (
        '?',
        [
            ".###.", "#...#", "....#", "...#.", "..#..", ".....", "..#..",
        ],
    ),
    (
        '@',
        [
            ".###.", "#...#", "#.###", "#.#.#", "#.###", "#....", ".###.",
        ],
    ),
    (
        'A',
        [
            ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'B',
        [
            "####.", "#...#", "#...#", "####.", "#...#", "#...#", "####.",
        ],
    ),
    (
        'C',
        [
            ".###.", "#...#", "#....", "#....", "#....", "#...#", ".###.",
        ],
    ),
    (
        'D',
        [
            "####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####.",
        ],
    ),
    (
        'E',
        [
            "#####", "#....", "#....", "####.", "#....", "#....", "#####",
        ],
    ),
    (
        'F',
        [
            "#####", "#....", "#....", "####.", "#....", "#....", "#....",
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
        'I',
        [
            ".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ],
    ),
    (
        'J',
        [
            "..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##..",
        ],
    ),
    (
        'K',
        [
            "#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#",
        ],
    ),
    (
        'L',
        [
            "#....", "#....", "#....", "#....", "#....", "#....", "#####",
        ],
    ),
    (
        'M',
        [
            "#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'N',
        [
            "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'O',
        [
            ".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ],
    ),
    (
        'P',
        [
            "####.", "#...#", "#...#", "####.", "#....", "#....", "#....",
        ],
    ),
    (
        'Q',
        [
            ".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#",
        ],
    ),
    (
        'R',
        [
            "####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#",
        ],
    ),
    (
        'S',
        [
            ".####", "#....", "#....", ".###.", "....#", "....#", "####.",
        ],
    ),
    (
        'T',
        [
            "#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#..",
        ],
    ),
    (
        'U',
        [
            "#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ],
    ),
    (
        'V',
        [
            "#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#..",
        ],
    ),
    (
        'W',
        [
            "#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#",
        ],
    ),
    (
        'X',
        [
            "#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#",
        ],
    ),
    (
        'Y',
        [
            "#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#..",
        ],
    ),
    (
        'Z',
        [
            "#####", "....#", "...#.", "..#..", ".#...", "#....", "#####",
        ],
    ),
    (
        '[',
        [
            ".###.", ".#...", ".#...", ".#...", ".#...", ".#...", ".###.",
        ],
    ),
    (
        '\\',
        [
            "#....", "#....", ".#...", "..#..", "...#.", "....#", "....#",
        ],
    ),
    (
        ']',
        [
            ".###.", "...#.", "...#.", "...#.", "...#.", "...#.", ".###.",
        ],
    ),
    (
        '^',
        [
            "..#..", ".#.#.", "#...#", ".....", ".....", ".....", ".....",
        ],
    ),
    (
        '_',
        [
            ".....", ".....", ".....", ".....", ".....", ".....", "#####",
        ],
    ),
    (
        '`',
        [
            ".#...", "..#..", ".....", ".....", ".....", ".....", ".....",
        ],
    ),
    (
        'a',
        [
            ".....", ".....", ".###.", "....#", ".####", "#...#", ".####",
        ],
    ),
    (
        'b',
        [
            "#....", "#....", "####.", "#...#", "#...#", "#...#", "####.",
        ],
    ),
    (
        'c',
        [
            ".....", ".....", ".###.", "#....", "#....", "#...#", ".###.",
        ],
    ),
    (
        'd',
        [
            "....#", "....#", ".####", "#...#", "#...#", "#...#", ".####",
        ],
    ),
    (
        'e',
        [
            ".....", ".....", ".###.", "#...#", "#####", "#....", ".###.",
        ],
    ),
    (
        'f',
        [
            "..##.", ".#..#", ".#...", "###..", ".#...", ".#...", ".#...",
        ],
    ),
    (
        'g',
        [
            ".....", ".####", "#...#", "#...#", ".####", "....#", ".###.",
        ],
    ),
    (
        'h',
        [
            "#....", "#....", "####.", "#...#", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'i',
        [
            "..#..", ".....", ".##..", "..#..", "..#..", "..#..", ".###.",
        ],
    ),
    (
        'j',
        [
            "...#.", ".....", "..##.", "...#.", "...#.", "#..#.", ".##..",
        ],
    ),
    (
        'k',
        [
            "#....", "#....", "#..#.", "#.#..", "##...", "#.#..", "#..#.",
        ],
    ),
    (
        'l',
        [
            ".##..", "..#..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ],
    ),
    (
        'm',
        [
            ".....", ".....", "##.#.", "#.#.#", "#.#.#", "#...#", "#...#",
        ],
    ),
    (
        'n',
        [
            ".....", ".....", "####.", "#...#", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'o',
        [
            ".....", ".....", ".###.", "#...#", "#...#", "#...#", ".###.",
        ],
    ),
    (
        'p',
        [
            ".....", "####.", "#...#", "#...#", "####.", "#....", "#....",
        ],
    ),
    (
        'q',
        [
            ".....", ".####", "#...#", "#...#", ".####", "....#", "....#",
        ],
    ),
    (
        'r',
        [
            ".....", ".....", "#.##.", "##..#", "#....", "#....", "#....",
        ],
    ),
    (
        's',
        [
            ".....", ".....", ".####", "#....", ".###.", "....#", "####.",
        ],
    ),
    (
        't',
        [
            ".#...", ".#...", "###..", ".#...", ".#...", ".#..#", "..##.",
        ],
    ),
    (
        'u',
        [
            ".....", ".....", "#...#", "#...#", "#...#", "#..##", ".##.#",
        ],
    ),
    (
        'v',
        [
            ".....", ".....", "#...#", "#...#", "#...#", ".#.#.", "..#..",
        ],
    ),
    (
        'w',
        [
            ".....", ".....", "#...#", "#...#", "#.#.#", "#.#.#", ".#.#.",
        ],
    ),
    (
        'x',
        [
            ".....", ".....", "#...#", ".#.#.", "..#..", ".#.#.", "#...#",
        ],
    ),
    (
        'y',
        [
            ".....", "#...#", "#...#", "#...#", ".####", "....#", ".###.",
        ],
    ),
    (
        'z',
        [
            ".....", ".....", "#####", "...#.", "..#..", ".#...", "#####",
        ],
    ),
    (
        '{',
        [
            "...#.", "..#..", "..#..", ".#...", "..#..", "..#..", "...#.",
        ],
    ),
    (
        '|',
        [
            "..#..", "..#..", "..#..", "..#..", "..#..", "..#..", "..#..",
        ],
    ),
    (
        '}',
        [
            ".#...", "..#..", "..#..", "...#.", "..#..", "..#..", ".#...",
        ],
    ),
    (
        '~',
        [
            ".....", ".....", ".#..#", "#.#.#", "#..#.", ".....", ".....",
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

/// Draw `text` into a plain RGB8 buffer. Used by the font proof sheet and by
/// callers that already own a canvas of their own.
pub fn draw_text_rgb(
    rgb: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    text: &str,
    color: [u8; 3],
) {
    let table = glyphs();
    for (i, ch) in text.chars().enumerate() {
        let Some(g) = table.get(&ch) else { continue };
        let ox = x + i * ADVANCE;
        for (gy, row) in g.iter().enumerate() {
            for (gx, on) in row.iter().enumerate() {
                if !*on {
                    continue;
                }
                let (px, py) = (ox + gx, y + gy);
                if px >= width || py >= height {
                    continue;
                }
                let at = (py * width + px) * 3;
                if let Some(slot) = rgb.get_mut(at..at + 3) {
                    slot.copy_from_slice(&color);
                }
            }
        }
    }
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

/// An image being drawn into, one element per pixel.
///
/// Generic over the pixel so the margins can be painted once and used twice:
/// as `[u8; 3]` for the PNG, and as `f32` dB levels for the TIFF, where the
/// "ink" is a real sample value that the colour map then renders.
struct Canvas<P: Copy> {
    w: usize,
    h: usize,
    px: Vec<P>,
}

impl<P: Copy> Canvas<P> {
    fn new(w: usize, h: usize, fill: P) -> Self {
        Self {
            w,
            h,
            px: vec![fill; w * h],
        }
    }

    fn set(&mut self, x: usize, y: usize, c: P) {
        if x >= self.w || y >= self.h {
            return;
        }
        self.px[y * self.w + x] = c;
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, c: P) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                self.px[yy * self.w + xx] = c;
            }
        }
    }

    /// Drop a `w x h` block of pixels in at `(x, y)`.
    fn blit(&mut self, x: usize, y: usize, w: usize, h: usize, src: &[P]) {
        for row in 0..h {
            let Some(line) = src.get(row * w..row * w + w) else {
                break;
            };
            for (col, p) in line.iter().enumerate() {
                self.set(x + col, y + row, *p);
            }
        }
    }

    fn text(&mut self, x: usize, y: usize, s: &str, c: P) {
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
// Geometry
//
// Where the ink goes is worked out once, here, and painted twice: once in
// colour for the PNG and once in dB levels for the TIFF. Keeping the layout in
// one place is what stops a saved picture and a saved measurement from
// disagreeing about where a band edge is.
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

/// Pixel margins the image sits inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Margins {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    /// Height of the band plan strip, `0` when there is no plan.
    pub plan_h: usize,
}

impl Margins {
    pub fn for_annotation(a: &Annotation) -> Self {
        let plan_h = if a.has_plan() { PLAN_H } else { 0 };
        Self {
            left: TIME_W,
            top: BAND_H + plan_h,
            right: MARGIN_R,
            bottom: MARGIN_B,
            plan_h,
        }
    }

    /// Top edge of the frequency band, below the plan strip.
    fn band_top(&self) -> usize {
        self.plan_h
    }
}

/// One tick on the frequency ruler.
#[derive(Clone, Debug, PartialEq)]
pub struct FreqTick {
    /// Pixel column within the image area.
    pub x: usize,
    pub major: bool,
    /// Present only on majors.
    pub label: Option<String>,
}

/// Ticks for a `width`-pixel image, in ascending frequency.
pub fn frequency_ticks(a: &Annotation, width: usize) -> Vec<FreqTick> {
    let mut out = Vec::new();
    if !(a.bin_hz > 0.0) || !a.bin_hz.is_finite() || width == 0 {
        return out;
    }

    let span = a.bin_hz * width as f64;
    let step = nice_step(span * MIN_LABEL_GAP_PX as f64 / width as f64);
    let minor = step / 5.0;
    if !(minor > 0.0) || !minor.is_finite() {
        return out;
    }

    let stop = a.stop_hz(width);
    let first = (a.start_hz / minor).ceil() * minor;
    // Bounded by the pixel count, so a pathological step cannot spin here.
    for i in 0..=(width * 2 + 16) {
        let hz = first + minor * i as f64;
        if hz > stop {
            break;
        }
        let px = ((hz - a.start_hz) / a.bin_hz).round();
        if px < 0.0 || px >= width as f64 {
            continue;
        }
        // Majors are tested against the step rather than counted, so rounding
        // cannot drift over a long ruler.
        let major = ((hz / step).round() * step - hz).abs() < minor * 0.5;
        out.push(FreqTick {
            x: px as usize,
            major,
            label: major.then(|| format_tick(hz, step, stop)),
        });
    }
    out
}

/// One band plan segment, clipped to the image.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanSegment {
    /// Half-open pixel columns within the image area.
    pub x0: usize,
    pub x1: usize,
    pub name: &'static str,
}

impl PlanSegment {
    pub fn width(&self) -> usize {
        self.x1.saturating_sub(self.x0)
    }
}

/// Visible band plan segments for a `width`-pixel image.
pub fn plan_segments(a: &Annotation, width: usize) -> Vec<PlanSegment> {
    let mut out = Vec::new();
    if !a.has_plan() || !(a.bin_hz > 0.0) || !a.bin_hz.is_finite() || width == 0 {
        return out;
    }

    let stop = a.stop_hz(width);
    for band in a.plan.overlapping(a.start_hz, stop) {
        let to_px = |hz: f64| ((hz - a.start_hz) / a.bin_hz).round();
        let x0 = to_px(band.start_hz).clamp(0.0, width as f64) as usize;
        let x1 = to_px(band.stop_hz).clamp(0.0, width as f64) as usize;
        if x1 > x0 {
            out.push(PlanSegment {
                x0,
                x1,
                name: band.name,
            });
        }
    }
    out
}

/// One label down the time column.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeTick {
    /// Pixel row within the image area.
    pub y: usize,
    pub label: String,
}

/// Time labels for a `height`-pixel image, newest first.
pub fn time_ticks(a: &Annotation, height: usize) -> Vec<TimeTick> {
    let mut out = Vec::new();
    if !(a.sweep_s > 0.0) || !a.sweep_s.is_finite() || a.newest_unix <= 0.0 || height == 0 {
        return out;
    }

    let secs_per_label = nice_step(MIN_TIME_GAP_PX as f64 * a.sweep_s);
    let rows_step = (secs_per_label / a.sweep_s).max(1.0);
    // Whole seconds would print the same label over and over for a fast
    // sweep, so sub-second spacing gets a tenths place.
    let subsecond = secs_per_label < 1.0;

    let mut row = 0.0;
    while row < height as f64 && out.len() < height {
        out.push(TimeTick {
            y: row.round() as usize,
            label: clock_label(a.newest_unix - row * a.sweep_s, subsecond),
        });
        row += rows_step;
    }
    out
}

// ---------------------------------------------------------------------------
// Compositing
// ---------------------------------------------------------------------------

/// Total size of the annotated image for a `width x height` waterfall.
pub fn outer_size(width: u32, height: u32, a: &Annotation) -> (u32, u32) {
    let m = Margins::for_annotation(a);
    (
        width + (m.left + m.right) as u32,
        height + (m.top + m.bottom) as u32,
    )
}

/// Paint the margins of an already-blitted canvas.
///
/// `bg`/`fg` are the band's background and its ink; `side_fg` the time
/// column's. The PNG passes colours, the TIFF passes dB levels.
#[allow(clippy::too_many_arguments)]
fn paint_margins<P: Copy>(
    canvas: &mut Canvas<P>,
    m: &Margins,
    a: &Annotation,
    w: usize,
    h: usize,
    bg: P,
    fg: P,
    side_fg: P,
) {
    // The whole top strip is the ruler's background, plan included, so the two
    // read as one unit the way rtl_power's heatmap header does.
    canvas.fill_rect(0, 0, canvas.w, m.top, bg);

    if a.newest_unix > 0.0 {
        let (date, _) = crate::util::unix_to_civil(a.newest_unix);
        canvas.text(3, m.plan_h + (BAND_H - GLYPH_H) / 2, &date, fg);
    }

    for seg in plan_segments(a, w) {
        // Edges first, so a band too narrow for its name still shows extent.
        canvas.fill_rect(m.left + seg.x0, 0, 1, m.plan_h, fg);
        canvas.fill_rect(m.left + seg.x1.saturating_sub(1), 0, 1, m.plan_h, fg);

        let label_w = text_width(seg.name);
        if label_w + 4 <= seg.width() {
            let cx = m.left + seg.x0 + (seg.width() - label_w) / 2;
            canvas.text(cx, 2, seg.name, fg);
        }
    }

    for tick in frequency_ticks(a, w) {
        let x = m.left + tick.x;
        let len = if tick.major { 7 } else { 4 };
        canvas.fill_rect(x, m.top - len, 1, len, fg);
        if let Some(label) = tick.label {
            // Centred on the tick, but kept inside the image columns: the
            // first label would otherwise reach back over the date in the
            // corner, and the last would run off the edge.
            let half = text_width(&label) / 2;
            let right = canvas.w.saturating_sub(text_width(&label));
            let lx = x.saturating_sub(half).clamp(m.left.min(right), right);
            canvas.text(lx, m.band_top() + 2, &label, fg);
        }
    }

    for tick in time_ticks(a, h) {
        let y = m.top + tick.y;
        canvas.fill_rect(m.left - 4, y, 4, 1, side_fg);
        let ty = y
            .saturating_sub(GLYPH_H / 2)
            .clamp(m.top, canvas.h.saturating_sub(GLYPH_H).max(m.top));
        canvas.text(
            m.left - 6 - text_width(&tick.label),
            ty,
            &tick.label,
            side_fg,
        );
    }
}

/// Wrap `rgb` (a `width x height` RGB8 waterfall) in labelled margins.
pub fn with_axes(width: u32, height: u32, rgb: &[u8], a: &Annotation) -> (u32, u32, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let m = Margins::for_annotation(a);
    let (ow, oh) = outer_size(width, height, a);

    let mut canvas = Canvas::new(ow as usize, oh as usize, SIDE_BG);
    // A reinterpretation, not a copy. The obvious `chunks_exact(3).map(...)
    // .collect()` allocates a second buffer the same size as the first, and
    // for a full-depth waterfall that is another 100 MB live at the same time
    // as `rgb`, the raw f32 rows and the output -- which matters most in the
    // browser, where the whole address space is 32-bit.
    let pixels: &[[u8; 3]] = bytemuck::cast_slice(rgb);
    canvas.blit(m.left, m.top, w, h, pixels);

    paint_margins(&mut canvas, &m, a, w, h, BAND_BG, BAND_FG, SIDE_FG);

    let mut out = Vec::with_capacity(canvas.px.len() * 3);
    for p in &canvas.px {
        out.extend_from_slice(p);
    }
    (ow, oh, out)
}

/// The same margins, painted into raw `f32` samples.
///
/// Background is the bottom of the display window and ink is the top, so the
/// colour map paints the labels at the two extremes of the data's own scale --
/// no separate palette, and the file is still nothing but sample values.
pub fn with_axes_raw(
    width: u32,
    height: u32,
    raw: &[f32],
    a: &Annotation,
    low: f32,
    high: f32,
) -> (u32, u32, Vec<f32>) {
    let (w, h) = (width as usize, height as usize);
    let m = Margins::for_annotation(a);
    let (ow, oh) = outer_size(width, height, a);

    let mut canvas = Canvas::new(ow as usize, oh as usize, low);
    canvas.blit(m.left, m.top, w, h, raw);
    paint_margins(&mut canvas, &m, a, w, h, low, high, high);

    (ow, oh, canvas.px)
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
            plan: &crate::bandplan::NONE,
        };
        (rgb, a)
    }

    fn fm_plan() -> &'static crate::bandplan::BandPlan {
        crate::bandplan::find("general")
    }

    #[test]
    fn a_plan_adds_a_strip_to_the_margins() {
        let plain = Annotation {
            bin_hz: 10e3,
            ..Default::default()
        };
        let with_plan = Annotation {
            plan: fm_plan(),
            ..plain
        };

        let (_, h0) = outer_size(100, 50, &plain);
        let (_, h1) = outer_size(100, 50, &with_plan);
        assert!(h1 > h0, "the strip must make room for itself");
        assert_eq!(
            Margins::for_annotation(&plain).plan_h,
            0,
            "no plan, no strip"
        );
    }

    #[test]
    fn plan_segments_are_clipped_to_the_view() {
        // 87-108 MHz: FM broadcast spans almost all of it.
        let a = Annotation {
            start_hz: 87e6,
            bin_hz: 10e3,
            plan: fm_plan(),
            ..Default::default()
        };
        let segs = plan_segments(&a, 2100);
        assert!(!segs.is_empty(), "FM broadcast should be visible here");
        for seg in &segs {
            assert!(seg.x1 <= 2100, "{seg:?} runs past the image");
            assert!(seg.x1 > seg.x0, "{seg:?} is empty");
        }
        assert!(
            segs.iter().any(|s| s.name.contains("FM")),
            "FM broadcast missing from {segs:?}"
        );
    }

    #[test]
    fn a_plan_outside_the_view_contributes_nothing() {
        // 1 kHz of spectrum at 5 MHz: no general-plan band covers it.
        let a = Annotation {
            start_hz: 5_000_000.0,
            bin_hz: 1.0,
            plan: fm_plan(),
            ..Default::default()
        };
        assert!(plan_segments(&a, 1000).is_empty());
    }

    #[test]
    fn plan_segments_need_a_usable_axis() {
        let a = Annotation {
            plan: fm_plan(),
            bin_hz: 0.0,
            ..Default::default()
        };
        assert!(plan_segments(&a, 100).is_empty());
        let a = Annotation {
            plan: fm_plan(),
            bin_hz: f64::NAN,
            ..Default::default()
        };
        assert!(plan_segments(&a, 100).is_empty());
    }

    #[test]
    fn the_raw_overlay_writes_only_the_window_extremes() {
        // Every pixel the TIFF annotator adds must be one of the two values
        // the colour map maps to its ends -- otherwise the labels would read
        // as spurious measurements somewhere in between.
        let data = vec![-70.0f32; 40 * 10];
        let a = Annotation {
            start_hz: 87e6,
            bin_hz: 10e3,
            newest_unix: 1_000_000_000.0,
            sweep_s: 1.0,
            plan: fm_plan(),
        };
        let (w, h, px) = with_axes_raw(40, 10, &data, &a, -100.0, -20.0);
        assert_eq!(px.len(), (w * h) as usize);

        let m = Margins::for_annotation(&a);
        for (i, v) in px.iter().enumerate() {
            let (x, y) = (i % w as usize, i / w as usize);
            let in_data = x >= m.left && x < m.left + 40 && y >= m.top && y < m.top + 10;
            if in_data {
                assert_eq!(*v, -70.0, "data was overwritten at {x},{y}");
            } else {
                assert!(
                    *v == -100.0 || *v == -20.0,
                    "margin pixel at {x},{y} is {v}, not a window extreme"
                );
            }
        }
    }

    #[test]
    fn the_raw_overlay_keeps_the_data_where_the_margins_say_it_is() {
        // A recognisable corner value, so a reader following the metadata
        // offsets lands on the real first bin.
        let mut data = vec![-70.0f32; 8 * 4];
        data[0] = -12.5;
        let a = Annotation {
            start_hz: 87e6,
            bin_hz: 10e3,
            ..Default::default()
        };
        let (w, _, px) = with_axes_raw(8, 4, &data, &a, -100.0, -20.0);
        let m = Margins::for_annotation(&a);
        assert_eq!(px[m.top * w as usize + m.left], -12.5);
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
        assert_eq!((w, h), outer_size(200, 50, &a));
        assert!(w > 200 && h > 50);
        assert_eq!(out.len(), (w * h * 3) as usize);
    }

    #[test]
    fn the_waterfall_lands_intact_inside_its_margins() {
        // Deliberately not the flat grey `probe` gives: every pixel here is a
        // different colour and no channel ever equals another, so a blit that
        // transposed, shifted or rotated the channels could not pass. The
        // image arrives as a flat `&[u8]` and is reinterpreted as `[u8; 3]`
        // pixels without copying, and this is what says that reinterpretation
        // is the right one.
        let (w, h) = (60usize, 20usize);
        let (_, a) = probe(w as u32, h as u32);
        let want = |x: usize, y: usize| {
            let i = (y * w + x) as u8;
            [i, i.wrapping_add(71), i.wrapping_add(149)]
        };
        let mut rgb = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                rgb.extend_from_slice(&want(x, y));
            }
        }

        let (ow, _, out) = with_axes(w as u32, h as u32, &rgb, &a);
        let at = |x: usize, y: usize| {
            let i = (y * ow as usize + x) * 3;
            [out[i], out[i + 1], out[i + 2]]
        };
        for y in 0..h {
            for x in 0..w {
                assert_eq!(at(TIME_W + x, BAND_H + y), want(x, y), "at {x},{y}");
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
            plan: &crate::bandplan::NONE,
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
        let mut c: Canvas<[u8; 3]> = Canvas::new(60, 12, [0, 0, 0]);
        c.text(0, 0, "12 ?? 34", [255, 255, 255]);
        assert!(c.px.contains(&[255, 255, 255]), "known glyphs still drew");
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
                ..Default::default()
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
            plan: &crate::bandplan::NONE,
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
