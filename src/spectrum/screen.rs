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
