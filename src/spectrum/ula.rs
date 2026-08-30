//! The ULA, as far as stage D needs it: one port, the border, and the 50 Hz interrupt.
//!
//! Everything the ULA does is reached through port `0xFE` — or rather through *every* even
//! port, because address decoding is partial and the ULA answers whenever A0 is low.

use super::keyboard::Keyboard;

/// T-states in a 48K frame: 312 lines of 224.
pub const FRAME_T: u32 = 69888;
/// `/INT` is held low for the first 32 T-states of a frame. An instruction that straddles
/// the window misses the interrupt entirely, which is real behaviour and not a bug.
pub const INT_ACTIVE_T: u32 = 32;
/// The ULA inverts every FLASH cell every 16 frames.
pub const FLASH_FRAMES: u64 = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct Ula {
    /// Border colour, 0–7, never bright.
    pub border: u8,
    /// The last value written to bits 3 and 4 — MIC and EAR share a pin, and what was
    /// written comes back on bit 6 of a read.
    ear_mic: u8,
}

impl Ula {
    pub fn new() -> Self {
        Self::default()
    }

    /// `OUT (0xFE),A`: border in bits 0–2, MIC in bit 3, EAR and the speaker in bit 4.
    pub fn write_port(&mut self, val: u8) {
        self.border = val & 0x07;
        self.ear_mic = val & 0x18;
    }

    /// `IN A,(0xFE)`: the selected keyboard half-rows in bits 0–4, bits 5 and 7 always set,
    /// and bit 6 reading back the last EAR level written — which is what an issue 3 machine
    /// does, and enough for everything short of loading tape.
    pub fn read_port(&self, port: u16, keyboard: &Keyboard) -> u8 {
        let ear = if self.ear_mic & 0x10 != 0 { 0x40 } else { 0 };
        0xA0 | ear | keyboard.read(port)
    }
}
