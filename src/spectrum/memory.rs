//! 64K of address space: 16K of ROM that ignores writes, and 48K of RAM that does not.

/// The 48K machine's ROM occupies the bottom 16K.
pub const ROM_SIZE: u16 = 0x4000;

pub struct Memory {
    bytes: Box<[u8]>,
}

impl Default for Memory {
    fn default() -> Self {
        Memory {
            bytes: vec![0; 0x10000].into_boxed_slice(),
        }
    }
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a ROM image at address zero. Anything shorter than 16K leaves the rest as it
    /// was; anything longer is refused, since it would be a different machine.
    pub fn load_rom(&mut self, rom: &[u8]) {
        assert!(
            rom.len() <= ROM_SIZE as usize,
            "{} byte ROM does not fit in {ROM_SIZE} bytes",
            rom.len()
        );
        self.bytes[..rom.len()].copy_from_slice(rom);
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.bytes[addr as usize]
    }

    /// Writes below [`ROM_SIZE`] are **discarded, not faulted** — a program that stores
    /// into ROM on a real Spectrum simply finds the value unchanged.
    pub fn write(&mut self, addr: u16, val: u8) {
        if addr >= ROM_SIZE {
            self.bytes[addr as usize] = val;
        }
    }

    /// Read a byte without going through the bus — for tests and debuggers, which must not
    /// disturb the machine's timing.
    pub fn peek(&self, addr: u16) -> u8 {
        self.read(addr)
    }

    /// Read a little-endian word, as the ROM stores its system variables.
    pub fn peek16(&self, addr: u16) -> u16 {
        u16::from_le_bytes([self.read(addr), self.read(addr.wrapping_add(1))])
    }

    /// Write straight through the ROM protection, for loading snapshots and for tests.
    pub fn poke(&mut self, addr: u16, val: u8) {
        self.bytes[addr as usize] = val;
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
