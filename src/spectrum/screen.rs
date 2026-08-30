//! Reading the display file back as text.
//!
//! Every cell of the screen is matched against the ROM's own character set, which turns
//! "does it look right?" into an exact string comparison — for the boot milestones, for
//! typing at the BASIC prompt, and for looking at a headless machine at all. See
//! [`doc/spectrum-video.md`](../../../doc/spectrum-video.md).

use super::memory::Memory;

pub const DISPLAY_FILE: u16 = 0x4000;
pub const ATTRIBUTES: u16 = 0x5800;
pub const DISPLAY_END: u16 = 0x5B00;
/// The ROM's character set: 96 characters of 8 bytes, for codes 32 to 127.
pub const CHARSET: u16 = 0x3D00;

pub const COLUMNS: usize = 32;
pub const ROWS: usize = 24;

/// Address of the pixel byte for character cell `(col, row)`, scanline `line` within it.
///
/// The bit-splicing that makes the display file look so strange: the third of the screen
/// and the scanline within the character both sit above the row.
pub fn pixel_address(col: usize, row: usize, line: usize) -> u16 {
    let third = (row / 8) as u16;
    let row_in_third = (row % 8) as u16;
    DISPLAY_FILE | (third << 11) | ((line as u16) << 8) | (row_in_third << 5) | col as u16
}

/// Address of the attribute byte for character cell `(col, row)`.
pub fn attribute_address(col: usize, row: usize) -> u16 {
    ATTRIBUTES + (row * COLUMNS + col) as u16
}

/// The character in a cell, and whether it is drawn in inverse video — which is how the
/// ROM draws the flashing `K` cursor.
///
/// Returns `None` if the cell matches no character in the ROM's set.
pub fn cell_char(memory: &Memory, col: usize, row: usize) -> Option<(char, bool)> {
    let mut cell = [0u8; 8];
    for (line, byte) in cell.iter_mut().enumerate() {
        *byte = memory.peek(pixel_address(col, row, line));
    }

    for code in 32u8..128 {
        let base = CHARSET + (code as u16 - 32) * 8;
        let glyph: [u8; 8] = std::array::from_fn(|i| memory.peek(base + i as u16));
        if glyph == cell {
            return Some((spectrum_char(code), false));
        }
        if glyph.iter().zip(&cell).all(|(g, c)| !g == *c) {
            return Some((spectrum_char(code), true));
        }
    }
    None
}

/// The Spectrum's character set is ASCII with two substitutions, and both of them show up
/// on the boot screen: the copyright sign in the message, and the pound sign the moment
/// anyone types a price.
pub fn spectrum_char(code: u8) -> char {
    match code {
        96 => '£',
        127 => '©',
        _ => code as char,
    }
}

/// The whole screen as 24 lines of 32 characters. A cell that matches nothing in the
/// character set becomes `?`.
pub fn screen_to_text(memory: &Memory) -> Vec<String> {
    (0..ROWS)
        .map(|row| {
            (0..COLUMNS)
                .map(|col| cell_char(memory, col, row).map_or('?', |(c, _)| c))
                .collect()
        })
        .collect()
}

/// The screen as text with trailing spaces trimmed and blank lines dropped — what you
/// actually want to look at, or to assert on.
pub fn screen_lines(memory: &Memory) -> Vec<String> {
    screen_to_text(memory)
        .into_iter()
        .map(|line| line.trim_end().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

// ------------------------------------------------------------------------- rendering

/// The visible frame the ULA emits: the 256 × 192 display with a border around it.
pub const BORDER_LEFT: usize = 48;
pub const BORDER_TOP: usize = 48;
pub const BORDER_RIGHT: usize = 48;
pub const BORDER_BOTTOM: usize = 56;
pub const WIDTH: usize = BORDER_LEFT + COLUMNS * 8 + BORDER_RIGHT;
pub const HEIGHT: usize = BORDER_TOP + ROWS * 8 + BORDER_BOTTOM;
/// Bytes in a rendered frame, RGBA.
pub const FRAME_BYTES: usize = WIDTH * HEIGHT * 4;

/// The colour level used for the non-bright palette. Real machines vary; `0xD7` is the
/// common convention and what Fuse uses.
pub const NORMAL_LEVEL: u8 = 0xD7;

/// One of the eight colours, bright or not.
///
/// Bit 0 of the index is blue, bit 1 red, bit 2 green — so bright black is still black,
/// and there are fifteen distinct colours rather than sixteen.
pub fn colour(index: u8, bright: bool) -> [u8; 3] {
    let level = if bright { 0xFF } else { NORMAL_LEVEL };
    let on = |bit: u8| if index & bit != 0 { level } else { 0 };
    [on(2), on(4), on(1)]
}

/// Ink and paper for an attribute byte, swapped if this cell is flashing and the ULA is
/// currently in its inverted phase.
pub fn ink_paper(attribute: u8, flash_inverted: bool) -> ([u8; 3], [u8; 3]) {
    let bright = attribute & 0x40 != 0;
    let ink = colour(attribute & 0x07, bright);
    let paper = colour((attribute >> 3) & 0x07, bright);
    if flash_inverted && attribute & 0x80 != 0 {
        (paper, ink)
    } else {
        (ink, paper)
    }
}

/// Render the whole visible frame into `out` as RGBA, top row first.
///
/// A whole-frame renderer: it reads the display file as it stands now, so a program that
/// changes the border or the display part-way down a frame is drawn as though it had not.
/// That is what the beam renderer in phase H replaces, and it is the only thing here that
/// phase H changes.
pub fn render_into(memory: &Memory, border: u8, flash_inverted: bool, out: &mut [u8]) {
    assert_eq!(out.len(), FRAME_BYTES, "frame buffer is the wrong size");

    let border_rgb = colour(border & 0x07, false);
    for pixel in out.chunks_exact_mut(4) {
        pixel[0] = border_rgb[0];
        pixel[1] = border_rgb[1];
        pixel[2] = border_rgb[2];
        pixel[3] = 0xFF;
    }

    for row in 0..ROWS {
        for col in 0..COLUMNS {
            let (ink, paper) = ink_paper(memory.peek(attribute_address(col, row)), flash_inverted);
            for line in 0..8 {
                let bits = memory.peek(pixel_address(col, row, line));
                let y = BORDER_TOP + row * 8 + line;
                let x = BORDER_LEFT + col * 8;
                let start = (y * WIDTH + x) * 4;
                for bit in 0..8 {
                    // The most significant bit is the leftmost pixel.
                    let rgb = if bits & (0x80 >> bit) != 0 {
                        ink
                    } else {
                        paper
                    };
                    let p = start + bit * 4;
                    out[p] = rgb[0];
                    out[p + 1] = rgb[1];
                    out[p + 2] = rgb[2];
                    out[p + 3] = 0xFF;
                }
            }
        }
    }
}
