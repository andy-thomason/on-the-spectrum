//! `.sna` snapshots: 27 bytes of registers and then the whole of RAM.
//!
//! The format has **no `PC` field**. A snapshot is taken from inside an interrupt, so the
//! return address is sitting on the program's own stack and the loader finishes by doing
//! what the `RETN` would have done: pop `PC`, and copy `IFF2` into `IFF1`. Saving has to
//! push `PC` back the same way, which is why `save_sna` can fail — a stack pointing into
//! ROM has nowhere to put it.
//!
//! Layout, from the World of Spectrum format reference. 16-bit values are little-endian,
//! as everything on a Z80 is:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 1 | `I` |
//! | 1 | 8 | `HL'`, `DE'`, `BC'`, `AF'` |
//! | 9 | 10 | `HL`, `DE`, `BC`, `IY`, `IX` |
//! | 19 | 1 | bit 2 holds `IFF2` |
//! | 20 | 1 | `R` |
//! | 21 | 4 | `AF`, `SP` |
//! | 25 | 1 | interrupt mode |
//! | 26 | 1 | border |
//! | 27 | 49152 | RAM, `0x4000`–`0xFFFF` |

use std::path::Path;

use super::Machine;
use super::memory::ROM_SIZE;

/// A 48K `.sna` is exactly this long. Anything else is a different machine or a
/// different format.
pub const SNA_48K_LEN: usize = 49179;

const HEADER_LEN: usize = 27;
const RAM_LEN: usize = 0x1_0000 - ROM_SIZE as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a 48K `.sna`.
    Length(usize),
    /// `SP` is low enough that pushing `PC` would land in ROM, which no snapshot can
    /// represent.
    StackInRom(u16),
    /// The file ends in the middle of a header, a memory page or a compressed run.
    Truncated,
    /// A `.z80` for hardware this is not: 128K, SamRam, and the rest.
    UnsupportedHardware(u8),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Length(n) => {
                write!(f, "{n} bytes is not a 48K .sna, which is {SNA_48K_LEN}")
            }
            Error::StackInRom(sp) => write!(
                f,
                "SP is ${sp:04X}: pushing PC would write to ROM, which a .sna cannot hold"
            ),
            Error::Truncated => write!(f, "the file ends part-way through"),
            Error::UnsupportedHardware(mode) => {
                write!(f, "hardware mode {mode} is not a 48K Spectrum")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Restore a machine from a 48K `.sna`.
pub fn load_sna(machine: &mut Machine, data: &[u8]) -> Result<(), Error> {
    if data.len() != SNA_48K_LEN {
        return Err(Error::Length(data.len()));
    }
    let word = |offset: usize| u16::from_le_bytes([data[offset], data[offset + 1]]);

    // RAM first: `PC` is read back out of it.
    for (i, &byte) in data[HEADER_LEN..].iter().enumerate() {
        machine.bus.memory.poke(ROM_SIZE + i as u16, byte);
    }
    machine.bus.ula.write_port(data[26] & 0x07);

    let cpu = &mut machine.cpu;
    cpu.regs.i = data[0];
    cpu.regs.hl_ = word(1);
    cpu.regs.de_ = word(3);
    cpu.regs.bc_ = word(5);
    cpu.regs.af_ = word(7);
    cpu.regs.set_hl(word(9));
    cpu.regs.set_de(word(11));
    cpu.regs.set_bc(word(13));
    cpu.regs.iy = word(15);
    cpu.regs.ix = word(17);
    cpu.regs.r = data[20];
    cpu.regs.set_af(word(21));
    cpu.regs.sp = word(23);
    cpu.im = data[25];
    // The `RETN` the snapshot was taken for: IFF2 back into IFF1, then pop.
    cpu.iff2 = data[19] & 0x04 != 0;
    cpu.iff1 = cpu.iff2;
    cpu.halted = false;
    cpu.ei_pending = false;

    let sp = cpu.regs.sp;
    machine.cpu.regs.pc = machine.bus.memory.peek16(sp);
    machine.cpu.regs.sp = sp.wrapping_add(2);
    Ok(())
}

/// Write the machine out as a 48K `.sna`, `PC` pushed onto its own stack.
pub fn save_sna(machine: &Machine) -> Result<Vec<u8>, Error> {
    let regs = &machine.cpu.regs;
    // Where the push would leave SP, and where PC's two bytes would land.
    let sp = regs.sp.wrapping_sub(2);
    let pc_high = sp.wrapping_add(1);
    if sp < ROM_SIZE || pc_high < ROM_SIZE {
        return Err(Error::StackInRom(regs.sp));
    }

    let mut out = vec![0u8; SNA_48K_LEN];
    let mut put = |offset: usize, value: u16| {
        out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    };
    put(1, regs.hl_);
    put(3, regs.de_);
    put(5, regs.bc_);
    put(7, regs.af_);
    put(9, regs.hl());
    put(11, regs.de());
    put(13, regs.bc());
    put(15, regs.iy);
    put(17, regs.ix);
    put(21, regs.af());
    put(23, sp);

    out[0] = regs.i;
    out[19] = if machine.cpu.iff2 { 0x04 } else { 0x00 };
    out[20] = regs.r;
    out[25] = machine.cpu.im;
    out[26] = machine.bus.ula.border;

    for i in 0..RAM_LEN {
        out[HEADER_LEN + i] = machine.bus.memory.peek(ROM_SIZE + i as u16);
    }

    // Push PC into the RAM image rather than into the live machine, so saving a snapshot
    // does not disturb the machine it is a snapshot of.
    let index = |addr: u16| HEADER_LEN + addr.wrapping_sub(ROM_SIZE) as usize;
    let [low, high] = regs.pc.to_le_bytes();
    out[index(sp)] = low;
    out[index(pc_high)] = high;
    Ok(out)
}

// -------------------------------------------------------------------------------- .z80

/// The `.z80` header is 30 bytes, and its `PC` field is zero to mean "look further on".
const Z80_V1_HEADER: usize = 30;
/// A version 2 file says its extra header is this long. Version 3 says 54 or 55, and the
/// difference matters: hardware mode 3 means 128K in a version 2 file and 48K with a
/// disk interface in a version 3 one.
const Z80_V2_EXTRA: usize = 23;
const PAGE_LEN: usize = 0x4000;

/// Load a 48K `.z80`, version 1, 2 or 3.
///
/// The format has no magic number — a `.z80` is recognised by being a `.z80` — so the
/// only sanity checks available are lengths and the hardware mode.
pub fn load_z80(machine: &mut Machine, data: &[u8]) -> Result<(), Error> {
    if data.len() < Z80_V1_HEADER {
        return Err(Error::Truncated);
    }
    let word = |offset: usize| u16::from_le_bytes([data[offset], data[offset + 1]]);
    let flags1 = data[12];

    let cpu = &mut machine.cpu;
    cpu.regs.a = data[0];
    cpu.regs.f = data[1];
    cpu.regs.set_bc(word(2));
    cpu.regs.set_hl(word(4));
    cpu.regs.sp = word(8);
    cpu.regs.i = data[10];
    // Byte 11 holds the low seven bits of R; its top bit lives in bit 0 of the flags.
    cpu.regs.r = (data[11] & 0x7F) | ((flags1 & 0x01) << 7);
    cpu.regs.set_de(word(13));
    cpu.regs.bc_ = word(15);
    cpu.regs.de_ = word(17);
    cpu.regs.hl_ = word(19);
    cpu.regs.af_ = u16::from_be_bytes([data[21], data[22]]);
    cpu.regs.iy = word(23);
    cpu.regs.ix = word(25);
    cpu.iff1 = data[27] != 0;
    cpu.iff2 = data[28] != 0;
    cpu.im = data[29] & 0x03;
    cpu.halted = false;
    cpu.ei_pending = false;
    machine.bus.ula.write_port((flags1 >> 1) & 0x07);

    let pc = word(6);
    if pc != 0 {
        // Version 1: one 48K block from 0x4000, compressed if bit 5 of the flags says so.
        machine.cpu.regs.pc = pc;
        let body = &data[Z80_V1_HEADER..];
        let ram = if flags1 & 0x20 != 0 {
            decompress(body, RAM_LEN)?
        } else {
            body.to_vec()
        };
        if ram.len() < RAM_LEN {
            return Err(Error::Truncated);
        }
        for (i, &byte) in ram[..RAM_LEN].iter().enumerate() {
            machine.bus.memory.poke(ROM_SIZE + i as u16, byte);
        }
        return Ok(());
    }

    // Versions 2 and 3: a longer header, then memory a page at a time.
    if data.len() < Z80_V1_HEADER + 5 {
        return Err(Error::Truncated);
    }
    let extra = word(30) as usize;
    let header_end = 32 + extra;
    if data.len() < header_end {
        return Err(Error::Truncated);
    }
    machine.cpu.regs.pc = word(32);

    let mode = data[34];
    let is_48k = match extra {
        Z80_V2_EXTRA => matches!(mode, 0 | 1),
        // Version 3 adds mode 3 for 48K with an M.G.T. interface.
        _ => matches!(mode, 0 | 1 | 3),
    };
    if !is_48k {
        return Err(Error::UnsupportedHardware(mode));
    }

    let mut at = header_end;
    while at + 3 <= data.len() {
        let length = u16::from_le_bytes([data[at], data[at + 1]]) as usize;
        let page = data[at + 2];
        at += 3;

        let (bytes, next) = if length == 0xFFFF {
            // Not compressed: exactly one page follows.
            let end = at + PAGE_LEN;
            (data.get(at..end).ok_or(Error::Truncated)?.to_vec(), end)
        } else {
            let end = at + length;
            let block = data.get(at..end).ok_or(Error::Truncated)?;
            (decompress(block, PAGE_LEN)?, end)
        };
        at = next;

        // The three pages a 48K machine has. Anything else — a Multiface ROM, a 128K
        // page in a mislabelled file — is not ours to place, so leave it alone.
        let base = match page {
            8 => 0x4000,
            4 => 0x8000,
            5 => 0xC000,
            _ => continue,
        };
        for (i, &byte) in bytes.iter().enumerate() {
            machine.bus.memory.poke(base + i as u16, byte);
        }
    }
    Ok(())
}

/// Undo the `.z80` run-length encoding, stopping at `target` bytes.
///
/// A run is `ED ED count value`. Only runs of five or more are encoded, except runs of
/// `ED` itself, which are encoded from two upwards — otherwise a literal `ED ED` in the
/// data would read as a marker. A single `ED` is left alone, which is why the marker test
/// has to look at two bytes.
///
/// A version 1 file ends with `00 ED ED 00`. That zero count can never begin a real run,
/// so it doubles as the terminator; in a well-formed file the data has already reached
/// `target` before the marker is reached at all.
///
/// This relies on one invariant of the encoding, which is what makes it decodable at all:
/// a literal `ED` is always followed by a literal byte, never by a run. A file that broke
/// that rule would be ambiguous to every decoder, not just this one.
fn decompress(input: &[u8], target: usize) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(target);
    let mut at = 0;
    while at < input.len() && out.len() < target {
        if input[at] == 0xED && input.get(at + 1) == Some(&0xED) {
            let (Some(&count), Some(&value)) = (input.get(at + 2), input.get(at + 3)) else {
                return Err(Error::Truncated);
            };
            if count == 0 {
                break;
            }
            out.extend(std::iter::repeat_n(value, count as usize));
            at += 4;
        } else {
            out.push(input[at]);
            at += 1;
        }
    }
    if out.len() < target {
        return Err(Error::Truncated);
    }
    // A run may overshoot the last byte of the page.
    out.truncate(target);
    Ok(out)
}

// ------------------------------------------------------------------------ loading a file

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Format(Error),
    /// Neither `.sna` nor `.z80`.
    UnknownExtension(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "{e}"),
            LoadError::Format(e) => write!(f, "{e}"),
            LoadError::UnknownExtension(ext) => {
                write!(
                    f,
                    "do not know how to load a {ext:?} file; .sna and .z80 only"
                )
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Load whichever snapshot format the path names, chosen by extension.
pub fn load_path(machine: &mut Machine, path: &Path) -> Result<(), LoadError> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let data = std::fs::read(path).map_err(LoadError::Io)?;
    match extension.as_str() {
        "sna" => load_sna(machine, &data).map_err(LoadError::Format),
        "z80" => load_z80(machine, &data).map_err(LoadError::Format),
        other => Err(LoadError::UnknownExtension(other.to_string())),
    }
}
