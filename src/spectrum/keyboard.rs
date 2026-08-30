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

/// A position in the matrix: which half-row, and which of its five keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub half_row: usize,
    pub column: usize,
}

impl Key {
    pub const CAPS_SHIFT: Key = Key {
        half_row: 0,
        column: 0,
    };
    pub const SYM_SHIFT: Key = Key {
        half_row: 7,
        column: 1,
    };
    pub const ENTER: Key = Key {
        half_row: 6,
        column: 0,
    };
    pub const SPACE: Key = Key {
        half_row: 7,
        column: 0,
    };

    /// Look a key up by the name it has in [`HALF_ROWS`], ignoring case.
    pub fn named(name: &str) -> Option<Key> {
        HALF_ROWS.iter().enumerate().find_map(|(half_row, keys)| {
            keys.iter()
                .position(|k| k.eq_ignore_ascii_case(name))
                .map(|column| Key { half_row, column })
        })
    }
}

/// A key, and the shift held down with it. Everything the Spectrum's forty keys can say
/// is one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    pub shift: Option<Key>,
}

impl Chord {
    pub fn plain(key: Key) -> Chord {
        Chord { key, shift: None }
    }

    pub fn symbol(key: Key) -> Chord {
        Chord {
            key,
            shift: Some(Key::SYM_SHIFT),
        }
    }

    pub fn caps(key: Key) -> Chord {
        Chord {
            key,
            shift: Some(Key::CAPS_SHIFT),
        }
    }

    /// The keys to press to get `c` at a BASIC prompt in **L** mode.
    ///
    /// Letters map to their key whatever their case, because the ROM's cursor mode decides
    /// what a letter key means: the same `P` is the `PRINT` keyword in **K** mode and a
    /// `p` in **L** mode, and synthesising CAPS SHIFT would change neither for the better.
    pub fn for_char(c: char) -> Option<Chord> {
        let symbol = |name: &str| Key::named(name).map(Chord::symbol);
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => {
                Key::named(&c.to_ascii_uppercase().to_string()).map(Chord::plain)
            }
            ' ' => Some(Chord::plain(Key::SPACE)),
            '\n' | '\r' => Some(Chord::plain(Key::ENTER)),
            '!' => symbol("1"),
            '@' => symbol("2"),
            '#' => symbol("3"),
            '$' => symbol("4"),
            '%' => symbol("5"),
            '&' => symbol("6"),
            '\'' => symbol("7"),
            '(' => symbol("8"),
            ')' => symbol("9"),
            '_' => symbol("0"),
            '<' => symbol("R"),
            '>' => symbol("T"),
            ';' => symbol("O"),
            '"' => symbol("P"),
            '-' => symbol("J"),
            '+' => symbol("K"),
            '=' => symbol("L"),
            ':' => symbol("Z"),
            '£' => symbol("X"),
            '?' => symbol("C"),
            '^' => symbol("H"),
            '/' => symbol("V"),
            '*' => symbol("B"),
            ',' => symbol("N"),
            '.' => symbol("M"),
            _ => None,
        }
    }
}

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

    /// Hold a key and its shift, if it has one.
    pub fn press(&mut self, chord: Chord) {
        self.set(chord.key.half_row, chord.key.column, true);
        if let Some(shift) = chord.shift {
            self.set(shift.half_row, shift.column, true);
        }
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
