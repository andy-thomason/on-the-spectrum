//! Writing characters into the display file, using the ROM's own font.
//!
//! The emulator can read the screen back through that font already
//! ([`super::screen::cell_char`]); this is the other direction, and it is how anything the
//! *emulator* wants to say — a menu, a message, a debug overlay — gets said in the
//! machine's own voice, with no second font, no text renderer and nothing for the UI layer
//! to do but display the frame it was going to display anyway.

use super::memory::Memory;
use super::screen::{CHARSET, COLUMNS, ROWS, attribute_address, pixel_address};

/// Black ink on white paper, which is what the ROM itself uses.
pub const DEFAULT_ATTRIBUTE: u8 = 0x38;

/// Fill the display file with `attribute` and no ink.
pub fn clear(memory: &mut Memory, attribute: u8) {
    for row in 0..ROWS {
        for col in 0..COLUMNS {
            for line in 0..8 {
                memory.poke(pixel_address(col, row, line), 0);
            }
            memory.poke(attribute_address(col, row), attribute);
        }
    }
}

/// Write `text` at a character position, one cell per character, clipped at the right edge.
///
/// Characters the ROM's set does not have become spaces — the set covers codes 32 to 127,
/// with `£` at 96 and `©` at 127.
pub fn print_at(memory: &mut Memory, col: usize, row: usize, text: &str, attribute: u8) {
    if row >= ROWS {
        return;
    }
    for (offset, c) in text.chars().enumerate() {
        let col = col + offset;
        if col >= COLUMNS {
            return;
        }
        let code = match c {
            '£' => 96,
            '©' => 127,
            c if (c as u32) >= 32 && (c as u32) < 128 => c as u8,
            _ => b' ',
        };
        let glyph = CHARSET + (code as u16 - 32) * 8;
        for line in 0..8 {
            let bits = memory.peek(glyph + line as u16);
            memory.poke(pixel_address(col, row, line), bits);
        }
        memory.poke(attribute_address(col, row), attribute);
    }
}

/// Write `text` centred on a row.
pub fn print_centred(memory: &mut Memory, row: usize, text: &str, attribute: u8) {
    let width = text.chars().count().min(COLUMNS);
    print_at(memory, (COLUMNS - width) / 2, row, text, attribute);
}

/// Set the attribute of a whole row without touching its pixels — how a menu highlights
/// the line under the cursor.
pub fn highlight_row(memory: &mut Memory, row: usize, attribute: u8) {
    if row >= ROWS {
        return;
    }
    for col in 0..COLUMNS {
        memory.poke(attribute_address(col, row), attribute);
    }
}
