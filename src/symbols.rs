//! Names for ROM addresses, harvested from the annotated disassembly by `build.rs`.

use crate::z80::Symbols;

include!(concat!(env!("OUT_DIR"), "/rom_symbols.rs"));

/// The 48K ROM's entry points, as named by the annotated disassembly.
pub struct RomSymbols;

impl Symbols for RomSymbols {
    fn name(&self, addr: u16) -> Option<&str> {
        ROM_SYMBOLS
            .binary_search_by_key(&addr, |&(a, _)| a)
            .ok()
            .map(|i| ROM_SYMBOLS[i].1)
    }
}
