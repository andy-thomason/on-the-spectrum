//! The Z80 core.
//!
//! It knows nothing about the Spectrum: it talks to a [`Bus`], which is what lets it be
//! tested in isolation against the per-opcode vectors and the standard exercisers. See
//! [`doc/boot-and-test.md`](../../doc/boot-and-test.md).

pub mod alu;
pub mod cycles;
pub mod decode;
pub mod disasm;
pub mod exec;

pub use cycles::{BusCycle, Pins};
pub use decode::{
    AluOp, BlockOp, ByteSource, Bytes, Cond, Decoded, ExOp, FetchKind, ImMode, JumpTarget, MemAddr,
    Op, PortSrc, Prefix, Reg8, Reg16, RotOp, Src8, Src16, decode, decode_bytes,
};
pub use disasm::{Symbols, disassemble, disassemble_parts, spec_mnemonic, walk};

/// Everything the CPU can do to the outside world.
///
/// `tick` is called by the CPU as each machine cycle is consumed, *not* once per
/// instruction: that is what makes ULA contention and a beam renderer possible without
/// restructuring anything. See [`doc/boot-and-test.md`](../../doc/boot-and-test.md) §5.
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
    fn in_port(&mut self, port: u16) -> u8;
    fn out_port(&mut self, port: u16, val: u8);

    /// Advance the machine clock by `cycles` T-states.
    fn tick(&mut self, cycles: u32);

    /// The bus's delay for an access to `addr` at the current T-state, in extra T-states
    /// to burn *before* the access. Zero until the ULA's contention is switched on.
    fn contention(&mut self, _addr: u16) -> u32 {
        0
    }
}

pub const SF: u8 = 0x80;
pub const ZF: u8 = 0x40;
/// Undocumented flag 5, often called YF. A copy of bit 5 of the result.
pub const F5: u8 = 0x20;
pub const HF: u8 = 0x10;
/// Undocumented flag 3, often called XF. A copy of bit 3 of the result.
pub const F3: u8 = 0x08;
pub const PF: u8 = 0x04;
pub const NF: u8 = 0x02;
pub const CF: u8 = 0x01;

/// The undocumented pair, which travel together everywhere.
pub const F53: u8 = F5 | F3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    /// The alternate set, only ever exchanged wholesale.
    pub af_: u16,
    pub bc_: u16,
    pub de_: u16,
    pub hl_: u16,
    pub ix: u16,
    pub iy: u16,
    pub sp: u16,
    pub pc: u16,
    pub i: u8,
    pub r: u8,
    /// MEMPTR: internal, not directly readable, but it leaks into flags 5 and 3 on
    /// `BIT n,(HL)`.
    pub wz: u16,
}

macro_rules! pair {
    ($get:ident, $set:ident, $hi:ident, $lo:ident) => {
        pub fn $get(&self) -> u16 {
            u16::from_be_bytes([self.$hi, self.$lo])
        }
        pub fn $set(&mut self, v: u16) {
            let [hi, lo] = v.to_be_bytes();
            self.$hi = hi;
            self.$lo = lo;
        }
    };
}

impl Registers {
    pair!(af, set_af, a, f);
    pair!(bc, set_bc, b, c);
    pair!(de, set_de, d, e);
    pair!(hl, set_hl, h, l);

    /// The address the Z80 puts on the bus during a refresh cycle.
    pub fn ir(&self) -> u16 {
        u16::from_be_bytes([self.i, self.r])
    }
}

/// Interrupt mode.
pub type InterruptMode = u8;

#[derive(Clone, Debug, Default)]
pub struct Cpu {
    pub regs: Registers,
    pub iff1: bool,
    pub iff2: bool,
    pub im: InterruptMode,
    /// `HALT` has been executed and no interrupt has arrived yet.
    pub halted: bool,
    /// Set for exactly one instruction after `EI`, during which no interrupt may be taken.
    pub ei_pending: bool,
    /// `F` as it was left by the last instruction that wrote flags, or zero if the last
    /// instruction did not touch them. `SCF` and `CCF` need it to decide where flags 5 and
    /// 3 come from.
    pub q: u8,
    /// The last instruction executed was `LD A,I` or `LD A,R`.
    pub p: bool,
    /// Monotonic T-state count. The *machine* keeps the position within the frame; this is
    /// for traces and for the per-opcode vectors.
    pub total_t: u64,
    /// The address the last opcode fetch left on the bus: `I` paired with the `R` it put
    /// out for the refresh. Internal T-states that follow a fetch are spent there. Not
    /// architectural state — it is where the pins happen to be pointing.
    pub refresh: u16,
    /// The machine is asserting `/INT`. Level-triggered: the machine holds it true for as
    /// long as the line is low.
    pub int_pending: bool,
    /// When `Some`, every T-state's bus state is recorded. Off in normal running: one
    /// predictable branch per machine cycle.
    pub cycle_log: Option<Vec<BusCycle>>,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Power-on state: everything clear, `PC` at the reset vector.
    pub fn reset(&mut self) {
        let log = self.cycle_log.take();
        *self = Cpu {
            cycle_log: log,
            ..Cpu::default()
        };
    }

    /// Write `F`, recording it in `Q` — see [`Cpu::q`]. Every flag write goes through here,
    /// so `Q` cannot drift out of step with what actually happened.
    #[inline]
    pub fn set_f(&mut self, f: u8) {
        self.regs.f = f;
        self.q = f;
    }
}
