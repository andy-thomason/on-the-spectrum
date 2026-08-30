//! The 8 × 5 key matrix.
//!
//! Modelled as eight half-rows of five bits, exactly as the hardware is wired, so that
//! ghosting falls out on its own: pressing CAPS, B and V really does make the machine see
//! SPACE as well, and a "which key is down" variable would not reproduce that. See
//! [`doc/spectrum-keyboard.md`](../../../doc/spectrum-keyboard.md).

/// The eight half-rows, in the order the address lines select them.
pub const HALF_ROWS: [[&str; 5]; 8] = [
    ["CAPS SHIFT", "Z", "X", "C", "V"],
    ["A", "S", "D", "F", "G"],
    ["Q", "W", "E", "R", "T"],
    ["1", "2", "3", "4", "5"],
    ["0", "9", "8", "7", "6"],
    ["P", "O", "I", "U", "Y"],
    ["ENTER", "L", "K", "J", "H"],
    ["SPACE", "SYM SHIFT", "M", "N", "B"],
];

/// One byte per half-row; a set bit is a key held down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keyboard {
    rows: [u8; 8],
}

impl Keyboard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hold or release the key in `half_row` (0..8) at `column` (0..5).
    pub fn set(&mut self, half_row: usize, column: usize, pressed: bool) {
        let bit = 1 << column;
        if pressed {
            self.rows[half_row] |= bit;
        } else {
            self.rows[half_row] &= !bit;
        }
    }

    /// Hold or release a key by the name it has in [`HALF_ROWS`].
    pub fn set_named(&mut self, name: &str, pressed: bool) -> bool {
        for (row, keys) in HALF_ROWS.iter().enumerate() {
            if let Some(col) = keys.iter().position(|k| k.eq_ignore_ascii_case(name)) {
                self.set(row, col, pressed);
                return true;
            }
        }
        false
    }

    pub fn release_all(&mut self) {
        self.rows = [0; 8];
    }

    /// Bits 0–4 of an `IN` from port `0xFE`: **0 means pressed**.
    ///
    /// A zero in bit *n* of the port's high byte selects half-row *n*, and several
    /// selected at once are ANDed together.
    pub fn read(&self, port: u16) -> u8 {
        let select = (port >> 8) as u8;
        let mut result = 0x1F;
        for (n, row) in self.rows.iter().enumerate() {
            if select & (1 << n) == 0 {
                result &= !row & 0x1F;
            }
        }
        result
    }
}
