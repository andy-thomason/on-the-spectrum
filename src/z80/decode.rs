//! Byte stream → [`Decoded`]: the single source of truth for what an opcode means.
//!
//! The structure mirrors the octal decomposition in
//! [`doc/z80-instruction-set.md`](../../../doc/z80-instruction-set.md) §3 exactly, so the
//! tables there can be read as a specification of this file. Both the interpreter and the
//! disassembler consume the result, which is what stops a trace from disagreeing with what
//! was actually executed.
//!
//! Decoding is a *stream* operation, not a slice operation: bytes arrive through
//! [`ByteSource`]. The interpreter will implement that over the bus so the opcode-fetch
//! machine cycles are charged in order; [`decode_bytes`] implements it over a slice for the
//! disassembler and for tests.

/// Which prefix bank the instruction came from.
///
/// This also names the index register a [`Reg8::MemIdx`] operand refers to — see
/// [`Prefix::index`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefix {
    None,
    Cb,
    Ed,
    Dd,
    Fd,
    DdCb,
    FdCb,
}

impl Prefix {
    /// The index register `(IX+d)` / `(IY+d)` operands refer to, if any.
    pub fn index(self) -> Option<Reg16> {
        match self {
            Prefix::Dd | Prefix::DdCb => Some(Reg16::Ix),
            Prefix::Fd | Prefix::FdCb => Some(Reg16::Iy),
            _ => None,
        }
    }
}

/// An 8-bit operand location. `MemHl` and `MemIdx` are memory, not registers, but they
/// occupy slot 6 of the `r` table and travel with the register operands everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reg8 {
    B,
    C,
    D,
    E,
    H,
    L,
    A,
    IxH,
    IxL,
    IyH,
    IyL,
    I,
    R,
    /// `(HL)`
    MemHl,
    /// `(IX+d)` or `(IY+d)`, per [`Decoded::prefix`], displaced by [`Decoded::disp`].
    MemIdx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reg16 {
    Bc,
    De,
    Hl,
    Sp,
    Af,
    Ix,
    Iy,
    Pc,
}

/// A branch condition. `Always` covers the unconditional forms, so `JP` and `JP cc` share
/// one arm in the interpreter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Always,
    Nz,
    Z,
    Nc,
    C,
    Po,
    Pe,
    P,
    M,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AluOp {
    Add,
    Adc,
    Sub,
    Sbc,
    And,
    Xor,
    Or,
    Cp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotOp {
    Rlc,
    Rrc,
    Rl,
    Rr,
    Sla,
    Sra,
    /// Undocumented: shift left, bit 0 becomes 1.
    Sll,
    Srl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockOp {
    Ldi,
    Cpi,
    Ini,
    Outi,
    Ldd,
    Cpd,
    Ind,
    Outd,
    Ldir,
    Cpir,
    Inir,
    Otir,
    Lddr,
    Cpdr,
    Indr,
    Otdr,
}

/// Interrupt mode. `Im01` is the undocumented `IM 0/1` setting, which behaves as mode 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImMode {
    Im0,
    Im01,
    Im1,
    Im2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExOp {
    /// `EX AF,AF'`
    AfAf,
    /// `EX DE,HL` — never affected by a `DD`/`FD` prefix.
    DeHl,
    /// `EX (SP),HL` / `EX (SP),IX` / `EX (SP),IY`
    SpReg(Reg16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src8 {
    Reg(Reg8),
    Imm(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src16 {
    Reg(Reg16),
    Imm(u16),
    /// `(nn)`
    Mem(u16),
}

/// The addressing modes of the accumulator loads — the only 8-bit loads that reach memory
/// other than through `(HL)` / `(IX+d)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemAddr {
    Bc,
    De,
    Imm(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpTarget {
    Imm(u16),
    /// `JP (HL)` / `JP (IX)` / `JP (IY)` — despite the notation, no memory is read.
    Reg(Reg16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortSrc {
    /// `(n)`, with the high byte of the port taken from `A`.
    Imm(u8),
    /// `(C)`, with the whole of `BC` on the address bus.
    Bc,
}

/// What an instruction does. One variant per distinct operation, with the operands as data
/// — the interpreter matches on this, the disassembler formats it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Nop,
    Load8 {
        dst: Reg8,
        src: Src8,
    },
    /// `LD A,(BC)` / `LD A,(DE)` / `LD A,(nn)`
    LdAMem(MemAddr),
    /// `LD (BC),A` / `LD (DE),A` / `LD (nn),A`
    LdMemA(MemAddr),
    Load16 {
        dst: Reg16,
        src: Src16,
    },
    /// `LD (nn),rp`
    Store16 {
        addr: u16,
        src: Reg16,
    },
    Alu {
        op: AluOp,
        src: Src8,
    },
    Inc8(Reg8),
    Dec8(Reg8),
    Inc16(Reg16),
    Dec16(Reg16),
    /// `hl` is `HL`, `IX` or `IY`; `src` is already substituted to match.
    AddHl {
        hl: Reg16,
        src: Reg16,
    },
    AdcHl {
        src: Reg16,
    },
    SbcHl {
        src: Reg16,
    },
    /// `copy_to` is the `DDCB` quirk: the result is written back to `(IX+d)` *and* copied
    /// into a register.
    Rot {
        op: RotOp,
        target: Reg8,
        copy_to: Option<Reg8>,
    },
    Bit {
        bit: u8,
        target: Reg8,
    },
    Res {
        bit: u8,
        target: Reg8,
        copy_to: Option<Reg8>,
    },
    Set {
        bit: u8,
        target: Reg8,
        copy_to: Option<Reg8>,
    },
    Jp {
        cond: Cond,
        target: JumpTarget,
    },
    Jr {
        cond: Cond,
        disp: i8,
    },
    Djnz {
        disp: i8,
    },
    Call {
        cond: Cond,
        addr: u16,
    },
    Ret {
        cond: Cond,
    },
    Retn,
    Reti,
    Rst(u8),
    Push(Reg16),
    Pop(Reg16),
    Ex(ExOp),
    Exx,
    /// `dst: None` is the undocumented `IN (C)`, which sets the flags and discards the byte.
    In {
        dst: Option<Reg8>,
        src: PortSrc,
    },
    /// `src: None` is the undocumented `OUT (C),0`.
    Out {
        src: Option<Reg8>,
        dst: PortSrc,
    },
    Block(BlockOp),
    Rlca,
    Rrca,
    Rla,
    Rra,
    Daa,
    Cpl,
    Scf,
    Ccf,
    Neg,
    Rld,
    Rrd,
    Di,
    Ei,
    Im(ImMode),
    Halt,
    /// An `ED` opcode with no effect: NONI followed by NOP. Two bytes, 8 T-states, and it
    /// inhibits the following interrupt. Not an error, and never a panic.
    Invalid,
}

/// One decoded instruction, including everything needed to execute it, print it, or step
/// over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub op: Op,
    /// Total length in bytes, prefixes included.
    pub len: u8,
    pub prefix: Prefix,
    /// The `(IX+d)` displacement; meaningless unless an operand is [`Reg8::MemIdx`].
    pub disp: i8,
    /// True for opcodes Zilog never published: `SLL`, the `IXH`/`IXL` halves, `IN (C)`,
    /// the `ED` aliases, the `DDCB` register-copy forms.
    pub undocumented: bool,
}

impl Decoded {
    /// The index register `MemIdx` operands refer to, if any.
    pub fn index(&self) -> Option<Reg16> {
        self.prefix.index()
    }
}

/// A source of instruction bytes.
///
/// The interpreter implements this over the bus, where each call is a machine cycle that
/// costs T-states and bumps `R`; the disassembler implements it over a slice.
pub trait ByteSource {
    fn next_byte(&mut self) -> u8;
}

/// Reads from a slice, returning `0xFF` past the end — what a real Spectrum's floating bus
/// gives you, and harmless for a disassembler walking off the end of a ROM.
pub struct Bytes<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bytes<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Bytes { data, pos: 0 }
    }

    /// Bytes consumed so far.
    pub fn pos(&self) -> usize {
        self.pos
    }
}

impl ByteSource for Bytes<'_> {
    fn next_byte(&mut self) -> u8 {
        let b = self.data.get(self.pos).copied().unwrap_or(0xFF);
        self.pos += 1;
        b
    }
}

/// A run of `DD`/`FD` prefixes longer than this decodes as a NONI-NOP of that length, and
/// the rest of the run is decoded as the next instruction.
///
/// On real hardware a prefix run is unbounded and uninterruptible; bounding it here keeps
/// [`Decoded::len`] in a `u8` and costs nothing but interrupt latency after four
/// consecutive prefixes, which no real program has.
const MAX_PREFIX_RUN: u8 = 4;

/// Decode one instruction from `src`.
pub fn decode<S: ByteSource>(src: &mut S) -> Decoded {
    let mut f = Fetcher {
        src,
        len: 0,
        disp: 0,
        have_disp: false,
        undoc: false,
    };
    let mut idx: Option<Reg16> = None;

    loop {
        let op = f.byte();
        return match op {
            // A DD or FD is a 4 T-state NONI that arms the index substitution for whatever
            // follows. A later prefix overrides an earlier one.
            0xDD | 0xFD if f.len < MAX_PREFIX_RUN => {
                idx = Some(if op == 0xDD { Reg16::Ix } else { Reg16::Iy });
                continue;
            }
            0xDD => f.finish(Op::Nop, Prefix::Dd),
            0xFD => f.finish(Op::Nop, Prefix::Fd),
            // A DD/FD before an ED is discarded: ED never uses an index register.
            0xED => {
                let op = decode_ed(&mut f);
                f.finish(op, Prefix::Ed)
            }
            0xCB => match idx {
                Some(ix) => {
                    let op = decode_ddcb(&mut f, ix);
                    f.finish(
                        op,
                        if ix == Reg16::Ix {
                            Prefix::DdCb
                        } else {
                            Prefix::FdCb
                        },
                    )
                }
                None => {
                    let op = decode_cb(&mut f);
                    f.finish(op, Prefix::Cb)
                }
            },
            _ => {
                let decoded = decode_base(&mut f, op, idx);
                let prefix = match idx {
                    None => Prefix::None,
                    Some(Reg16::Ix) => Prefix::Dd,
                    _ => Prefix::Fd,
                };
                f.finish(decoded, prefix)
            }
        };
    }
}

/// Decode one instruction from a byte slice. Bytes past the end read as `0xFF`.
pub fn decode_bytes(bytes: &[u8]) -> Decoded {
    decode(&mut Bytes::new(bytes))
}

// ---------------------------------------------------------------------------- the fetcher

struct Fetcher<'a, S: ByteSource> {
    src: &'a mut S,
    len: u8,
    disp: i8,
    have_disp: bool,
    undoc: bool,
}

impl<S: ByteSource> Fetcher<'_, S> {
    fn byte(&mut self) -> u8 {
        self.len += 1;
        self.src.next_byte()
    }

    fn word(&mut self) -> u16 {
        let lo = self.byte();
        let hi = self.byte();
        u16::from_le_bytes([lo, hi])
    }

    fn signed(&mut self) -> i8 {
        self.byte() as i8
    }

    /// Fetch the `(IX+d)` displacement, at most once per instruction. Called at the point
    /// the operand is resolved, which is what puts `d` before any immediate `n` — the byte
    /// order `LD (IX+d),n` requires.
    fn fetch_disp(&mut self) {
        if !self.have_disp {
            self.disp = self.signed();
            self.have_disp = true;
        }
    }

    fn finish(&self, op: Op, prefix: Prefix) -> Decoded {
        Decoded {
            op,
            len: self.len,
            prefix,
            disp: self.disp,
            undocumented: self.undoc,
        }
    }

    /// `r[y]`, with `DD`/`FD` substitution applied: `H`/`L` become the index halves and
    /// `(HL)` becomes `(IX+d)`.
    ///
    /// Only for instructions with a single `r` operand. Two-operand `LD r,r'` goes through
    /// [`Fetcher::ld_pair`], which has an exception this does not.
    fn reg(&mut self, y: u8, idx: Option<Reg16>) -> Reg8 {
        let Some(ix) = idx else { return plain_r(y) };
        match y {
            4 | 5 => {
                self.undoc = true;
                match (ix, y) {
                    (Reg16::Ix, 4) => Reg8::IxH,
                    (Reg16::Ix, _) => Reg8::IxL,
                    (_, 4) => Reg8::IyH,
                    (_, _) => Reg8::IyL,
                }
            }
            6 => {
                self.fetch_disp();
                Reg8::MemIdx
            }
            _ => plain_r(y),
        }
    }

    /// The `LD r[y],r[z]` operand pair.
    ///
    /// The exception that makes this its own function: when one operand becomes `(IX+d)`,
    /// the *other* keeps `H` and `L` as `H` and `L`. `LD H,(IX+d)` exists;
    /// `LD IXH,(IX+d)` does not.
    fn ld_pair(&mut self, y: u8, z: u8, idx: Option<Reg16>) -> (Reg8, Reg8) {
        match idx {
            Some(_) if y == 6 => (self.reg(6, idx), plain_r(z)),
            Some(_) if z == 6 => (plain_r(y), self.reg(6, idx)),
            _ => (self.reg(y, idx), self.reg(z, idx)),
        }
    }
}

// ------------------------------------------------------------------------- decode tables

const fn plain_r(y: u8) -> Reg8 {
    match y {
        0 => Reg8::B,
        1 => Reg8::C,
        2 => Reg8::D,
        3 => Reg8::E,
        4 => Reg8::H,
        5 => Reg8::L,
        6 => Reg8::MemHl,
        _ => Reg8::A,
    }
}

/// `HL`, or the index register a `DD`/`FD` prefix substitutes for it.
const fn hl(idx: Option<Reg16>) -> Reg16 {
    match idx {
        Some(ix) => ix,
        None => Reg16::Hl,
    }
}

/// `rp[p]`
const fn rp(p: u8, idx: Option<Reg16>) -> Reg16 {
    match p {
        0 => Reg16::Bc,
        1 => Reg16::De,
        2 => hl(idx),
        _ => Reg16::Sp,
    }
}

/// `rp2[p]`
const fn rp2(p: u8, idx: Option<Reg16>) -> Reg16 {
    match p {
        0 => Reg16::Bc,
        1 => Reg16::De,
        2 => hl(idx),
        _ => Reg16::Af,
    }
}

const fn cc(y: u8) -> Cond {
    match y {
        0 => Cond::Nz,
        1 => Cond::Z,
        2 => Cond::Nc,
        3 => Cond::C,
        4 => Cond::Po,
        5 => Cond::Pe,
        6 => Cond::P,
        _ => Cond::M,
    }
}

const fn alu(y: u8) -> AluOp {
    match y {
        0 => AluOp::Add,
        1 => AluOp::Adc,
        2 => AluOp::Sub,
        3 => AluOp::Sbc,
        4 => AluOp::And,
        5 => AluOp::Xor,
        6 => AluOp::Or,
        _ => AluOp::Cp,
    }
}

const fn rot(y: u8) -> RotOp {
    match y {
        0 => RotOp::Rlc,
        1 => RotOp::Rrc,
        2 => RotOp::Rl,
        3 => RotOp::Rr,
        4 => RotOp::Sla,
        5 => RotOp::Sra,
        6 => RotOp::Sll,
        _ => RotOp::Srl,
    }
}

const fn im(y: u8) -> ImMode {
    match y & 3 {
        0 => ImMode::Im0,
        1 => ImMode::Im01,
        2 => ImMode::Im1,
        _ => ImMode::Im2,
    }
}

/// `bli[y][z]`
const fn bli(y: u8, z: u8) -> BlockOp {
    match (y, z) {
        (4, 0) => BlockOp::Ldi,
        (4, 1) => BlockOp::Cpi,
        (4, 2) => BlockOp::Ini,
        (4, _) => BlockOp::Outi,
        (5, 0) => BlockOp::Ldd,
        (5, 1) => BlockOp::Cpd,
        (5, 2) => BlockOp::Ind,
        (5, _) => BlockOp::Outd,
        (6, 0) => BlockOp::Ldir,
        (6, 1) => BlockOp::Cpir,
        (6, 2) => BlockOp::Inir,
        (6, _) => BlockOp::Otir,
        (_, 0) => BlockOp::Lddr,
        (_, 1) => BlockOp::Cpdr,
        (_, 2) => BlockOp::Indr,
        (_, _) => BlockOp::Otdr,
    }
}

const fn xyzpq(op: u8) -> (u8, u8, u8, u8, u8) {
    (op >> 6, (op >> 3) & 7, op & 7, (op >> 4) & 3, (op >> 3) & 1)
}

// ------------------------------------------------------------------------ the four banks

fn decode_base<S: ByteSource>(f: &mut Fetcher<S>, op: u8, idx: Option<Reg16>) -> Op {
    let (x, y, z, p, q) = xyzpq(op);
    match (x, z) {
        (0, 0) => match y {
            0 => Op::Nop,
            1 => Op::Ex(ExOp::AfAf),
            2 => Op::Djnz { disp: f.signed() },
            3 => Op::Jr {
                cond: Cond::Always,
                disp: f.signed(),
            },
            _ => Op::Jr {
                cond: cc(y - 4),
                disp: f.signed(),
            },
        },
        (0, 1) if q == 0 => Op::Load16 {
            dst: rp(p, idx),
            src: Src16::Imm(f.word()),
        },
        (0, 1) => Op::AddHl {
            hl: hl(idx),
            src: rp(p, idx),
        },
        (0, 2) => match (q, p) {
            (0, 0) => Op::LdMemA(MemAddr::Bc),
            (0, 1) => Op::LdMemA(MemAddr::De),
            (0, 2) => Op::Store16 {
                addr: f.word(),
                src: hl(idx),
            },
            (0, _) => Op::LdMemA(MemAddr::Imm(f.word())),
            (_, 0) => Op::LdAMem(MemAddr::Bc),
            (_, 1) => Op::LdAMem(MemAddr::De),
            (_, 2) => Op::Load16 {
                dst: hl(idx),
                src: Src16::Mem(f.word()),
            },
            (_, _) => Op::LdAMem(MemAddr::Imm(f.word())),
        },
        (0, 3) if q == 0 => Op::Inc16(rp(p, idx)),
        (0, 3) => Op::Dec16(rp(p, idx)),
        (0, 4) => Op::Inc8(f.reg(y, idx)),
        (0, 5) => Op::Dec8(f.reg(y, idx)),
        (0, 6) => {
            // The displacement is fetched here, before the immediate: `DD 36 d n`.
            let dst = f.reg(y, idx);
            Op::Load8 {
                dst,
                src: Src8::Imm(f.byte()),
            }
        }
        (0, _) => match y {
            0 => Op::Rlca,
            1 => Op::Rrca,
            2 => Op::Rla,
            3 => Op::Rra,
            4 => Op::Daa,
            5 => Op::Cpl,
            6 => Op::Scf,
            _ => Op::Ccf,
        },

        (1, _) if y == 6 && z == 6 => Op::Halt,
        (1, _) => {
            let (dst, src) = f.ld_pair(y, z, idx);
            Op::Load8 {
                dst,
                src: Src8::Reg(src),
            }
        }

        (2, _) => Op::Alu {
            op: alu(y),
            src: Src8::Reg(f.reg(z, idx)),
        },

        (3, 0) => Op::Ret { cond: cc(y) },
        (3, 1) if q == 0 => Op::Pop(rp2(p, idx)),
        (3, 1) => match p {
            0 => Op::Ret { cond: Cond::Always },
            1 => Op::Exx,
            2 => Op::Jp {
                cond: Cond::Always,
                target: JumpTarget::Reg(hl(idx)),
            },
            _ => Op::Load16 {
                dst: Reg16::Sp,
                src: Src16::Reg(hl(idx)),
            },
        },
        (3, 2) => Op::Jp {
            cond: cc(y),
            target: JumpTarget::Imm(f.word()),
        },
        (3, 3) => match y {
            0 => Op::Jp {
                cond: Cond::Always,
                target: JumpTarget::Imm(f.word()),
            },
            // y == 1 is the CB prefix, consumed by `decode` before it gets here.
            2 => Op::Out {
                src: Some(Reg8::A),
                dst: PortSrc::Imm(f.byte()),
            },
            3 => Op::In {
                dst: Some(Reg8::A),
                src: PortSrc::Imm(f.byte()),
            },
            4 => Op::Ex(ExOp::SpReg(hl(idx))),
            5 => Op::Ex(ExOp::DeHl),
            6 => Op::Di,
            _ => Op::Ei,
        },
        (3, 4) => Op::Call {
            cond: cc(y),
            addr: f.word(),
        },
        (3, 5) if q == 0 => Op::Push(rp2(p, idx)),
        // p == 1, 2, 3 are the DD, ED and FD prefixes, consumed by `decode`.
        (3, 5) => Op::Call {
            cond: Cond::Always,
            addr: f.word(),
        },
        (3, 6) => Op::Alu {
            op: alu(y),
            src: Src8::Imm(f.byte()),
        },
        (_, _) => Op::Rst(y * 8),
    }
}

fn decode_cb<S: ByteSource>(f: &mut Fetcher<S>) -> Op {
    let (x, y, z, ..) = xyzpq(f.byte());
    let target = plain_r(z);
    match x {
        0 => {
            let op = rot(y);
            f.undoc = op == RotOp::Sll;
            Op::Rot {
                op,
                target,
                copy_to: None,
            }
        }
        1 => Op::Bit { bit: y, target },
        2 => Op::Res {
            bit: y,
            target,
            copy_to: None,
        },
        _ => Op::Set {
            bit: y,
            target,
            copy_to: None,
        },
    }
}

fn decode_ed<S: ByteSource>(f: &mut Fetcher<S>) -> Op {
    let (x, y, z, p, q) = xyzpq(f.byte());
    if x == 0 || x == 3 {
        f.undoc = true;
        return Op::Invalid;
    }
    if x == 2 {
        if z <= 3 && y >= 4 {
            return Op::Block(bli(y, z));
        }
        f.undoc = true;
        return Op::Invalid;
    }
    match z {
        0 if y == 6 => {
            f.undoc = true;
            Op::In {
                dst: None,
                src: PortSrc::Bc,
            }
        }
        0 => Op::In {
            dst: Some(plain_r(y)),
            src: PortSrc::Bc,
        },
        1 if y == 6 => {
            f.undoc = true;
            Op::Out {
                src: None,
                dst: PortSrc::Bc,
            }
        }
        1 => Op::Out {
            src: Some(plain_r(y)),
            dst: PortSrc::Bc,
        },
        2 if q == 0 => Op::SbcHl { src: rp(p, None) },
        2 => Op::AdcHl { src: rp(p, None) },
        3 if q == 0 => Op::Store16 {
            addr: f.word(),
            src: rp(p, None),
        },
        3 => Op::Load16 {
            dst: rp(p, None),
            src: Src16::Mem(f.word()),
        },
        4 => {
            f.undoc = y != 0;
            Op::Neg
        }
        5 => {
            f.undoc = y > 1;
            if y == 1 { Op::Reti } else { Op::Retn }
        }
        6 => {
            f.undoc = !matches!(y, 0 | 2 | 3);
            Op::Im(im(y))
        }
        _ => match y {
            0 => Op::Load8 {
                dst: Reg8::I,
                src: Src8::Reg(Reg8::A),
            },
            1 => Op::Load8 {
                dst: Reg8::R,
                src: Src8::Reg(Reg8::A),
            },
            2 => Op::Load8 {
                dst: Reg8::A,
                src: Src8::Reg(Reg8::I),
            },
            3 => Op::Load8 {
                dst: Reg8::A,
                src: Src8::Reg(Reg8::R),
            },
            4 => Op::Rrd,
            5 => Op::Rld,
            _ => {
                f.undoc = true;
                Op::Nop
            }
        },
    }
}

/// `DD`/`FD`, `CB`, **`d`**, opcode — the displacement comes *before* the opcode byte.
fn decode_ddcb<S: ByteSource>(f: &mut Fetcher<S>, _ix: Reg16) -> Op {
    f.fetch_disp();
    let (x, y, z, ..) = xyzpq(f.byte());
    let target = Reg8::MemIdx;
    // Every form that copies the result to a register is undocumented, as is `SLL`, as are
    // the seven `BIT` aliases.
    let copy_to = if z == 6 {
        None
    } else {
        f.undoc = true;
        Some(plain_r(z))
    };
    match x {
        0 => {
            let op = rot(y);
            f.undoc |= op == RotOp::Sll;
            Op::Rot {
                op,
                target,
                copy_to,
            }
        }
        1 => Op::Bit { bit: y, target },
        2 => Op::Res {
            bit: y,
            target,
            copy_to,
        },
        _ => Op::Set {
            bit: y,
            target,
            copy_to,
        },
    }
}
