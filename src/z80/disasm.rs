//! [`Decoded`] → text.
//!
//! Two renderings, one formatter:
//!
//! * [`spec_mnemonic`] reproduces the tables in
//!   [`doc/z80-instruction-set.md`](../../../doc/z80-instruction-set.md) exactly, with `n`,
//!   `nn` and `d` left as placeholders. The round-trip test diffs the decoder against the
//!   spec document through this, so docs and code cannot drift apart silently.
//! * [`disassemble`] renders a concrete instruction at a concrete address: real immediates,
//!   relative jumps resolved to absolute targets, undocumented opcodes marked with `*`, and
//!   symbol names in a trailing comment.

use super::decode::{
    AluOp, BlockOp, Cond, Decoded, ExOp, ImMode, JumpTarget, MemAddr, Op, PortSrc, Reg8, Reg16,
    RotOp, Src8, Src16, decode_bytes,
};

/// A source of names for addresses — ROM entry points, system variables, whatever the
/// caller has. Used only for trailing comments; it never changes the instruction text.
pub trait Symbols {
    fn name(&self, addr: u16) -> Option<&str>;
}

/// No symbols at all.
impl Symbols for () {
    fn name(&self, _addr: u16) -> Option<&str> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    /// Placeholders, no undocumented marker: the form the spec tables use.
    Spec,
    /// Real values at a real address.
    Listing,
}

/// Render an instruction the way the spec tables do — `LD B,n`, `JR NZ,d`, `RST 38h` —
/// so it can be compared against them directly.
pub fn spec_mnemonic(dec: &Decoded) -> String {
    Fmt {
        dec,
        pc: 0,
        style: Style::Spec,
        syms: None,
        comments: Vec::new(),
    }
    .render()
    .0
}

/// Render the instruction at `pc`, plus the trailing comment as a separate string so a
/// caller can align it into its own column.
pub fn disassemble_parts(
    dec: &Decoded,
    pc: u16,
    syms: Option<&dyn Symbols>,
) -> (String, Option<String>) {
    Fmt {
        dec,
        pc,
        style: Style::Listing,
        syms,
        comments: Vec::new(),
    }
    .render()
}

/// Render the instruction at `pc` as a single line, comment included.
pub fn disassemble(dec: &Decoded, pc: u16, syms: Option<&dyn Symbols>) -> String {
    match disassemble_parts(dec, pc, syms) {
        (text, None) => text,
        (text, Some(comment)) => format!("{text} ; {comment}"),
    }
}

// ------------------------------------------------------------------------------ walking

/// One instruction found by [`walk`].
pub struct Instruction<'a> {
    /// Address of the first byte, `org` relative.
    pub addr: u16,
    /// The raw bytes, prefixes included.
    pub bytes: &'a [u8],
    pub decoded: Decoded,
}

/// Decode `mem` from `start` to `end` (byte offsets), as though it were loaded at `org`.
///
/// Straight-line decoding: it does not follow branches, so data areas disassemble as
/// nonsense — which is exactly what a ROM listing shows too.
pub fn walk(mem: &[u8], org: u16, start: usize, end: usize) -> Walk<'_> {
    Walk {
        mem,
        org,
        pos: start.min(mem.len()),
        end: end.min(mem.len()),
    }
}

pub struct Walk<'a> {
    mem: &'a [u8],
    org: u16,
    pos: usize,
    end: usize,
}

impl<'a> Iterator for Walk<'a> {
    type Item = Instruction<'a>;

    fn next(&mut self) -> Option<Instruction<'a>> {
        if self.pos >= self.end {
            return None;
        }
        let decoded = decode_bytes(&self.mem[self.pos..]);
        let addr = self.org.wrapping_add(self.pos as u16);
        // An instruction can run past `end` — a ROM's last byte may be a prefix. Clamp the
        // byte slice; `decoded` already read `0xFF` for anything past the end of `mem`.
        let stop = (self.pos + decoded.len as usize).min(self.mem.len());
        let bytes = &self.mem[self.pos..stop];
        self.pos += decoded.len as usize;
        Some(Instruction {
            addr,
            bytes,
            decoded,
        })
    }
}

// ---------------------------------------------------------------------------- formatting

struct Fmt<'a> {
    dec: &'a Decoded,
    pc: u16,
    style: Style,
    syms: Option<&'a dyn Symbols>,
    comments: Vec<String>,
}

impl Fmt<'_> {
    fn render(mut self) -> (String, Option<String>) {
        let mut text = self.op_text();
        if self.style == Style::Listing && self.dec.undocumented {
            text.insert(0, '*');
        }
        let comment = if self.comments.is_empty() {
            None
        } else {
            Some(self.comments.join(", "))
        };
        (text, comment)
    }

    // -- operands ---------------------------------------------------------------------

    /// An immediate byte: `n`, or `$3F`.
    fn imm8(&self, n: u8) -> String {
        match self.style {
            Style::Spec => "n".to_string(),
            Style::Listing => format!("${n:02X}"),
        }
    }

    /// An immediate word: `nn`, or `$11CB`.
    fn imm16(&mut self, nn: u16) -> String {
        match self.style {
            Style::Spec => "nn".to_string(),
            Style::Listing => {
                self.note_symbol(nn);
                format!("${nn:04X}")
            }
        }
    }

    /// A direct address: `(nn)`, or `($5C78)`.
    fn addr16(&mut self, nn: u16) -> String {
        let inner = self.imm16(nn);
        format!("({inner})")
    }

    /// `(IX+d)`, or `(IX+$05)` / `(IX-$03)`.
    fn idx_mem(&self) -> String {
        let ix = match self.dec.index() {
            Some(Reg16::Iy) => "IY",
            _ => "IX",
        };
        match self.style {
            Style::Spec => format!("({ix}+d)"),
            Style::Listing => {
                let d = self.dec.disp;
                let sign = if d < 0 { '-' } else { '+' };
                format!("({ix}{sign}${:02X})", d.unsigned_abs())
            }
        }
    }

    /// A relative branch: `d`, or the absolute target it reaches.
    fn rel(&mut self, disp: i8) -> String {
        match self.style {
            Style::Spec => "d".to_string(),
            Style::Listing => {
                let target = self
                    .pc
                    .wrapping_add(self.dec.len as u16)
                    .wrapping_add(disp as i16 as u16);
                self.note_symbol(target);
                self.comments.push(format!(
                    "d={}${:02X}",
                    if disp < 0 { '-' } else { '+' },
                    disp.unsigned_abs()
                ));
                format!("${target:04X}")
            }
        }
    }

    fn note_symbol(&mut self, addr: u16) {
        if let Some(name) = self.syms.and_then(|s| s.name(addr)) {
            self.comments.push(name.to_string());
        }
    }

    fn reg8(&self, r: Reg8) -> String {
        match r {
            Reg8::B => "B",
            Reg8::C => "C",
            Reg8::D => "D",
            Reg8::E => "E",
            Reg8::H => "H",
            Reg8::L => "L",
            Reg8::A => "A",
            Reg8::IxH => "IXH",
            Reg8::IxL => "IXL",
            Reg8::IyH => "IYH",
            Reg8::IyL => "IYL",
            Reg8::I => "I",
            Reg8::R => "R",
            Reg8::MemHl => "(HL)",
            Reg8::MemIdx => return self.idx_mem(),
        }
        .to_string()
    }

    fn src8(&self, s: Src8) -> String {
        match s {
            Src8::Reg(r) => self.reg8(r),
            Src8::Imm(n) => self.imm8(n),
        }
    }

    fn src16(&mut self, s: Src16) -> String {
        match s {
            Src16::Reg(r) => reg16(r).to_string(),
            Src16::Imm(nn) => self.imm16(nn),
            Src16::Mem(nn) => self.addr16(nn),
        }
    }

    fn mem_addr(&mut self, a: MemAddr) -> String {
        match a {
            MemAddr::Bc => "(BC)".to_string(),
            MemAddr::De => "(DE)".to_string(),
            MemAddr::Imm(nn) => self.addr16(nn),
        }
    }

    fn port(&mut self, p: PortSrc) -> String {
        match p {
            PortSrc::Imm(n) => format!("({})", self.imm8(n)),
            PortSrc::Bc => "(C)".to_string(),
        }
    }

    /// `RST 38h` in the spec tables, `RST $38` in a listing.
    fn rst(&self, n: u8) -> String {
        match self.style {
            Style::Spec => format!("RST {n:02X}h"),
            Style::Listing => format!("RST ${n:02X}"),
        }
    }

    // -- the instruction --------------------------------------------------------------

    fn op_text(&mut self) -> String {
        match self.dec.op {
            Op::Nop => "NOP".to_string(),
            Op::Load8 { dst, src } => format!("LD {},{}", self.reg8(dst), self.src8(src)),
            Op::LdAMem(a) => format!("LD A,{}", self.mem_addr(a)),
            Op::LdMemA(a) => format!("LD {},A", self.mem_addr(a)),
            Op::Load16 { dst, src } => format!("LD {},{}", reg16(dst), self.src16(src)),
            Op::Store16 { addr, src } => format!("LD {},{}", self.addr16(addr), reg16(src)),
            Op::Alu { op, src } => format!("{}{}", alu(op), self.src8(src)),
            Op::Inc8(r) => format!("INC {}", self.reg8(r)),
            Op::Dec8(r) => format!("DEC {}", self.reg8(r)),
            Op::Inc16(r) => format!("INC {}", reg16(r)),
            Op::Dec16(r) => format!("DEC {}", reg16(r)),
            Op::AddHl { hl, src } => format!("ADD {},{}", reg16(hl), reg16(src)),
            Op::AdcHl { src } => format!("ADC HL,{}", reg16(src)),
            Op::SbcHl { src } => format!("SBC HL,{}", reg16(src)),
            Op::Rot {
                op,
                target,
                copy_to,
            } => self.with_copy(copy_to, format!("{} {}", rot(op), self.reg8(target))),
            Op::Bit { bit, target } => format!("BIT {bit},{}", self.reg8(target)),
            Op::Res {
                bit,
                target,
                copy_to,
            } => self.with_copy(copy_to, format!("RES {bit},{}", self.reg8(target))),
            Op::Set {
                bit,
                target,
                copy_to,
            } => self.with_copy(copy_to, format!("SET {bit},{}", self.reg8(target))),
            Op::Jp { cond, target } => {
                let target = match target {
                    JumpTarget::Imm(nn) => self.imm16(nn),
                    JumpTarget::Reg(r) => format!("({})", reg16(r)),
                };
                match cond {
                    Cond::Always => format!("JP {target}"),
                    _ => format!("JP {},{target}", cc(cond)),
                }
            }
            Op::Jr { cond, disp } => {
                let target = self.rel(disp);
                match cond {
                    Cond::Always => format!("JR {target}"),
                    _ => format!("JR {},{target}", cc(cond)),
                }
            }
            Op::Djnz { disp } => format!("DJNZ {}", self.rel(disp)),
            Op::Call { cond, addr } => {
                let addr = self.imm16(addr);
                match cond {
                    Cond::Always => format!("CALL {addr}"),
                    _ => format!("CALL {},{addr}", cc(cond)),
                }
            }
            Op::Ret { cond } => match cond {
                Cond::Always => "RET".to_string(),
                _ => format!("RET {}", cc(cond)),
            },
            Op::Retn => "RETN".to_string(),
            Op::Reti => "RETI".to_string(),
            Op::Rst(n) => self.rst(n),
            Op::Push(r) => format!("PUSH {}", reg16(r)),
            Op::Pop(r) => format!("POP {}", reg16(r)),
            Op::Ex(ExOp::AfAf) => "EX AF,AF'".to_string(),
            Op::Ex(ExOp::DeHl) => "EX DE,HL".to_string(),
            Op::Ex(ExOp::SpReg(r)) => format!("EX (SP),{}", reg16(r)),
            Op::Exx => "EXX".to_string(),
            Op::In { dst, src } => {
                let port = self.port(src);
                match dst {
                    Some(r) => format!("IN {},{port}", self.reg8(r)),
                    None => format!("IN {port}"),
                }
            }
            Op::Out { src, dst } => {
                let port = self.port(dst);
                match src {
                    Some(r) => format!("OUT {port},{}", self.reg8(r)),
                    None => format!("OUT {port},0"),
                }
            }
            Op::Block(b) => block(b).to_string(),
            Op::Rlca => "RLCA".to_string(),
            Op::Rrca => "RRCA".to_string(),
            Op::Rla => "RLA".to_string(),
            Op::Rra => "RRA".to_string(),
            Op::Daa => "DAA".to_string(),
            Op::Cpl => "CPL".to_string(),
            Op::Scf => "SCF".to_string(),
            Op::Ccf => "CCF".to_string(),
            Op::Neg => "NEG".to_string(),
            Op::Rld => "RLD".to_string(),
            Op::Rrd => "RRD".to_string(),
            Op::Di => "DI".to_string(),
            Op::Ei => "EI".to_string(),
            Op::Im(m) => format!(
                "IM {}",
                match m {
                    ImMode::Im0 => "0",
                    ImMode::Im01 => "0/1",
                    ImMode::Im1 => "1",
                    ImMode::Im2 => "2",
                }
            ),
            Op::Halt => "HALT".to_string(),
            Op::Invalid => "NONI, NOP".to_string(),
        }
    }

    /// The `DDCB` register-copy quirk: `LD B,RES 0,(IX+d)`.
    fn with_copy(&self, copy_to: Option<Reg8>, body: String) -> String {
        match copy_to {
            Some(r) => format!("LD {},{body}", self.reg8(r)),
            None => body,
        }
    }
}

const fn reg16(r: Reg16) -> &'static str {
    match r {
        Reg16::Bc => "BC",
        Reg16::De => "DE",
        Reg16::Hl => "HL",
        Reg16::Sp => "SP",
        Reg16::Af => "AF",
        Reg16::Ix => "IX",
        Reg16::Iy => "IY",
        Reg16::Pc => "PC",
    }
}

const fn cc(c: Cond) -> &'static str {
    match c {
        Cond::Always => "",
        Cond::Nz => "NZ",
        Cond::Z => "Z",
        Cond::Nc => "NC",
        Cond::C => "C",
        Cond::Po => "PO",
        Cond::Pe => "PE",
        Cond::P => "P",
        Cond::M => "M",
    }
}

/// Includes the trailing separator, so `SUB B` and `ADD A,B` both fall out of one format.
const fn alu(op: AluOp) -> &'static str {
    match op {
        AluOp::Add => "ADD A,",
        AluOp::Adc => "ADC A,",
        AluOp::Sub => "SUB ",
        AluOp::Sbc => "SBC A,",
        AluOp::And => "AND ",
        AluOp::Xor => "XOR ",
        AluOp::Or => "OR ",
        AluOp::Cp => "CP ",
    }
}

const fn rot(op: RotOp) -> &'static str {
    match op {
        RotOp::Rlc => "RLC",
        RotOp::Rrc => "RRC",
        RotOp::Rl => "RL",
        RotOp::Rr => "RR",
        RotOp::Sla => "SLA",
        RotOp::Sra => "SRA",
        RotOp::Sll => "SLL",
        RotOp::Srl => "SRL",
    }
}

const fn block(b: BlockOp) -> &'static str {
    match b {
        BlockOp::Ldi => "LDI",
        BlockOp::Cpi => "CPI",
        BlockOp::Ini => "INI",
        BlockOp::Outi => "OUTI",
        BlockOp::Ldd => "LDD",
        BlockOp::Cpd => "CPD",
        BlockOp::Ind => "IND",
        BlockOp::Outd => "OUTD",
        BlockOp::Ldir => "LDIR",
        BlockOp::Cpir => "CPIR",
        BlockOp::Inir => "INIR",
        BlockOp::Otir => "OTIR",
        BlockOp::Lddr => "LDDR",
        BlockOp::Cpdr => "CPDR",
        BlockOp::Indr => "INDR",
        BlockOp::Otdr => "OTDR",
    }
}
