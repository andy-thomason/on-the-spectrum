//! The loop-and-match interpreter.
//!
//! One `match` over [`Op`], flat, with every T-state spent through the machine-cycle
//! primitives in [`cycles`](super::cycles) — never by adding a per-instruction total at the
//! end. The timings in [`doc/z80-instruction-set.md`](../../../doc/z80-instruction-set.md)
//! are a test oracle for this file, not its implementation.

use super::alu;
use super::decode::{
    AluOp, BlockOp, ByteSource, Cond, Decoded, ExOp, FetchKind, ImMode, JumpTarget, MemAddr, Op,
    PortSrc, Prefix, Reg8, Reg16, RotOp, Src8, Src16, decode,
};
use super::{Bus, CF, Cpu, F53, HF, NF, PF, SF, ZF};

/// Feeds [`decode`] from memory through the CPU's own machine cycles, so the opcode fetches
/// are charged, contended and refresh-counted in the order they really happen.
struct Stream<'a, B: Bus> {
    cpu: &'a mut Cpu,
    bus: &'a mut B,
}

impl<B: Bus> ByteSource for Stream<'_, B> {
    fn read(&mut self, kind: FetchKind) -> u8 {
        match kind {
            FetchKind::Opcode => self.cpu.fetch_opcode(self.bus),
            FetchKind::Operand => {
                let pc = self.cpu.regs.pc;
                self.cpu.regs.pc = pc.wrapping_add(1);
                self.cpu.read_byte(self.bus, pc)
            }
            FetchKind::DdcbOpcode => {
                let pc = self.cpu.regs.pc;
                self.cpu.regs.pc = pc.wrapping_add(1);
                let v = self.cpu.read_byte(self.bus, pc);
                self.cpu.internal(self.bus, pc, 2);
                v
            }
        }
    }

    fn extend_opcode(&mut self) {
        self.cpu.extend_fetch(self.bus);
    }
}

impl Cpu {
    /// Run one instruction — or accept an interrupt, or spin one `HALT` cycle. Returns the
    /// T-states it took.
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u32 {
        let start = self.total_t;

        if self.try_take_interrupt(bus) {
            return (self.total_t - start) as u32;
        }
        // The shadow cast by EI lasts exactly one instruction, and the interrupt check
        // above is the only thing that reads it.
        self.ei_pending = false;

        if self.halted {
            // A halted Z80 executes NOPs: the clock runs, refresh runs, PC does not.
            self.fetch_opcode(bus);
            self.regs.pc = self.regs.pc.wrapping_sub(1);
            return (self.total_t - start) as u32;
        }

        let dec = {
            let mut stream = Stream {
                cpu: self,
                bus: &mut *bus,
            };
            decode(&mut stream)
        };
        self.execute(bus, &dec);
        (self.total_t - start) as u32
    }

    /// Accept an interrupt if one is pending and allowed. IM 1 takes 13 T-states, IM 2
    /// takes 19; `R` counts the acknowledge as a refresh.
    fn try_take_interrupt<B: Bus>(&mut self, bus: &mut B) -> bool {
        if !self.int_pending || !self.iff1 || self.ei_pending {
            return false;
        }
        self.halted = false;
        if self.p {
            // The acknowledge clears IFF2 before the P/V flag that `LD A,I` and `LD A,R`
            // load from it can be read, so an interrupt landing here shows P/V reset. It
            // is the reason those two instructions cannot be trusted to read the interrupt
            // state on an NMOS Z80.
            self.regs.f &= !PF;
        }
        self.iff1 = false;
        self.iff2 = false;
        self.q = 0;
        self.p = false;

        // The acknowledge cycle is an M1 stretched by two wait states, with IORQ rather
        // than MREQ; nothing is fetched on a Spectrum, where the bus floats to 0xFF.
        self.refresh = self.regs.ir();
        self.internal(bus, self.refresh, 7);
        self.bump_r_public();

        let sp = self.regs.sp.wrapping_sub(2);
        self.regs.sp = sp;
        self.write_byte(bus, sp.wrapping_add(1), (self.regs.pc >> 8) as u8);
        self.write_byte(bus, sp, self.regs.pc as u8);

        let target = match self.im {
            2 => {
                let vector = u16::from_be_bytes([self.regs.i, 0xFF]);
                self.read_word(bus, vector)
            }
            // IM 1 always, and IM 0 with it: nothing on a Spectrum drives the data bus
            // during the acknowledge, so it floats to 0xFF, which is `RST 38h` — the same
            // place IM 1 goes, one T-state slower.
            _ => 0x0038,
        };
        self.regs.pc = target;
        self.regs.wz = target;
        true
    }

    fn bump_r_public(&mut self) {
        self.regs.r = (self.regs.r & 0x80) | (self.regs.r.wrapping_add(1) & 0x7F);
    }

    fn execute<B: Bus>(&mut self, bus: &mut B, dec: &Decoded) {
        // Q is what the *previous* instruction left in F, and only SCF and CCF read it.
        // A DD or FD prefix is an instruction in its own right — a NONI that writes no
        // flags — so a prefixed SCF or CCF always sees a cleared Q.
        let q = if dec.prefix == Prefix::None {
            self.q
        } else {
            0
        };
        self.q = 0;
        // P remembers `LD A,I` / `LD A,R` for exactly one instruction.
        self.p = false;

        // For an indexed operand the address is computed now, before any access, and the
        // T-states that compute it are spent now too — five of them, or two when an
        // immediate has already been read past the displacement.
        let ea = if dec.indexed {
            let base = match dec.prefix.index() {
                Some(Reg16::Iy) => self.regs.iy,
                _ => self.regs.ix,
            };
            let ea = base.wrapping_add(dec.disp as i16 as u16);
            if matches!(dec.prefix, Prefix::Dd | Prefix::Fd) {
                let n = if matches!(
                    dec.op,
                    Op::Load8 {
                        src: Src8::Imm(_),
                        ..
                    }
                ) {
                    2
                } else {
                    5
                };
                let at = self.regs.pc.wrapping_sub(1);
                self.internal(bus, at, n);
            }
            self.regs.wz = ea;
            ea
        } else {
            self.regs.hl()
        };

        match dec.op {
            Op::Nop => {}

            Op::Load8 { dst, src } => {
                // LD A,I / LD A,R / LD I,A / LD R,A each stretch the ED fetch by a T-state.
                let special =
                    matches!(dst, Reg8::I | Reg8::R) || matches!(src, Src8::Reg(Reg8::I | Reg8::R));
                if special {
                    self.extend_fetch(bus);
                }
                let v = self.src8(bus, src, ea);
                self.set_reg8(bus, dst, ea, v);
                if let Src8::Reg(Reg8::I | Reg8::R) = src {
                    // Flags come from the byte moved, and P/V shows IFF2 — which is how a
                    // program reads the interrupt state.
                    let f = (self.regs.f & CF)
                        | (v & (SF | F53))
                        | if v == 0 { ZF } else { 0 }
                        | if self.iff2 { PF } else { 0 };
                    self.set_f(f);
                    self.p = true;
                }
            }

            Op::LdAMem(addr) => {
                let addr = self.mem_addr(addr);
                self.regs.wz = addr.wrapping_add(1);
                self.regs.a = self.read_byte(bus, addr);
            }
            Op::LdMemA(addr) => {
                let addr = self.mem_addr(addr);
                // The high byte of MEMPTR takes A, not the address — a real oddity, and
                // one that ZEXALL-style tests check.
                self.regs.wz = u16::from_be_bytes([self.regs.a, addr.wrapping_add(1) as u8]);
                self.write_byte(bus, addr, self.regs.a);
            }

            Op::Load16 { dst, src } => {
                let v = match src {
                    Src16::Imm(nn) => nn,
                    Src16::Reg(r) => {
                        // LD SP,HL: two internal T-states while the value crosses.
                        self.internal_at_refresh(bus, 2);
                        self.reg16(r)
                    }
                    Src16::Mem(nn) => {
                        self.regs.wz = nn.wrapping_add(1);
                        self.read_word(bus, nn)
                    }
                };
                self.set_reg16(dst, v);
            }
            Op::Store16 { addr, src } => {
                self.regs.wz = addr.wrapping_add(1);
                let v = self.reg16(src);
                self.write_word(bus, addr, v);
            }

            Op::Alu { op, src } => {
                let v = self.src8(bus, src, ea);
                let a = self.regs.a;
                let carry = self.regs.f & CF != 0;
                let (r, f) = match op {
                    AluOp::Add => alu::add8(a, v, false),
                    AluOp::Adc => alu::add8(a, v, carry),
                    AluOp::Sub => alu::sub8(a, v, false),
                    AluOp::Sbc => alu::sub8(a, v, carry),
                    AluOp::And => alu::and8(a, v),
                    AluOp::Xor => alu::xor8(a, v),
                    AluOp::Or => alu::or8(a, v),
                    AluOp::Cp => (a, alu::cp8(a, v)),
                };
                self.regs.a = r;
                self.set_f(f);
            }

            Op::Inc8(r) => {
                let v = self.reg8(bus, r, ea);
                let (res, f) = alu::inc8(v, self.regs.f);
                self.rmw_pause(bus, r, ea);
                self.set_reg8(bus, r, ea, res);
                self.set_f(f);
            }
            Op::Dec8(r) => {
                let v = self.reg8(bus, r, ea);
                let (res, f) = alu::dec8(v, self.regs.f);
                self.rmw_pause(bus, r, ea);
                self.set_reg8(bus, r, ea, res);
                self.set_f(f);
            }

            Op::Inc16(r) => {
                self.internal_at_refresh(bus, 2);
                let v = self.reg16(r).wrapping_add(1);
                self.set_reg16(r, v);
            }
            Op::Dec16(r) => {
                self.internal_at_refresh(bus, 2);
                let v = self.reg16(r).wrapping_sub(1);
                self.set_reg16(r, v);
            }

            Op::AddHl { hl, src } => {
                self.internal_at_refresh(bus, 7);
                let a = self.reg16(hl);
                let b = self.reg16(src);
                self.regs.wz = a.wrapping_add(1);
                let (r, f) = alu::add16(a, b, self.regs.f);
                self.set_reg16(hl, r);
                self.set_f(f);
            }
            Op::AdcHl { src } => {
                self.internal_at_refresh(bus, 7);
                let a = self.regs.hl();
                let b = self.reg16(src);
                self.regs.wz = a.wrapping_add(1);
                let (r, f) = alu::adc16(a, b, self.regs.f & CF != 0);
                self.regs.set_hl(r);
                self.set_f(f);
            }
            Op::SbcHl { src } => {
                self.internal_at_refresh(bus, 7);
                let a = self.regs.hl();
                let b = self.reg16(src);
                self.regs.wz = a.wrapping_add(1);
                let (r, f) = alu::sbc16(a, b, self.regs.f & CF != 0);
                self.regs.set_hl(r);
                self.set_f(f);
            }

            Op::Rot {
                op,
                target,
                copy_to,
            } => {
                let v = self.reg8(bus, target, ea);
                let (r, f) = match op {
                    RotOp::Rlc => alu::rlc(v, self.regs.f),
                    RotOp::Rrc => alu::rrc(v, self.regs.f),
                    RotOp::Rl => alu::rl(v, self.regs.f),
                    RotOp::Rr => alu::rr(v, self.regs.f),
                    RotOp::Sla => alu::sla(v, self.regs.f),
                    RotOp::Sra => alu::sra(v, self.regs.f),
                    RotOp::Sll => alu::sll(v, self.regs.f),
                    RotOp::Srl => alu::srl(v, self.regs.f),
                };
                self.rmw_pause(bus, target, ea);
                self.set_reg8(bus, target, ea, r);
                self.copy_result(bus, copy_to, ea, r);
                self.set_f(f);
            }
            Op::Bit { bit, target } => {
                let v = self.reg8(bus, target, ea);
                if matches!(target, Reg8::MemHl | Reg8::MemIdx) {
                    self.internal(bus, ea, 1);
                }
                // Flags 5 and 3 come from the operand for a register, and from the high
                // byte of MEMPTR for memory — the one place MEMPTR is observable.
                let f53 = match target {
                    Reg8::MemHl | Reg8::MemIdx => (self.regs.wz >> 8) as u8,
                    _ => v,
                };
                let f = alu::bit(bit, v, self.regs.f, f53);
                self.set_f(f);
            }
            Op::Res {
                bit,
                target,
                copy_to,
            } => {
                let v = self.reg8(bus, target, ea) & !(1 << bit);
                self.rmw_pause(bus, target, ea);
                self.set_reg8(bus, target, ea, v);
                self.copy_result(bus, copy_to, ea, v);
            }
            Op::Set {
                bit,
                target,
                copy_to,
            } => {
                let v = self.reg8(bus, target, ea) | (1 << bit);
                self.rmw_pause(bus, target, ea);
                self.set_reg8(bus, target, ea, v);
                self.copy_result(bus, copy_to, ea, v);
            }

            Op::Jp { cond, target } => match target {
                JumpTarget::Imm(nn) => {
                    self.regs.wz = nn;
                    if self.cond(cond) {
                        self.regs.pc = nn;
                    }
                }
                JumpTarget::Reg(r) => self.regs.pc = self.reg16(r),
            },
            Op::Jr { cond, disp } => {
                if self.cond(cond) {
                    let at = self.regs.pc.wrapping_sub(1);
                    self.internal(bus, at, 5);
                    let target = self.regs.pc.wrapping_add(disp as i16 as u16);
                    self.regs.pc = target;
                    self.regs.wz = target;
                }
            }
            Op::Djnz { disp } => {
                let b = self.regs.b.wrapping_sub(1);
                self.regs.b = b;
                if b != 0 {
                    let at = self.regs.pc.wrapping_sub(1);
                    self.internal(bus, at, 5);
                    let target = self.regs.pc.wrapping_add(disp as i16 as u16);
                    self.regs.pc = target;
                    self.regs.wz = target;
                }
            }
            Op::Call { cond, addr } => {
                self.regs.wz = addr;
                if self.cond(cond) {
                    let at = self.regs.pc.wrapping_sub(1);
                    self.internal(bus, at, 1);
                    self.push(bus, self.regs.pc);
                    self.regs.pc = addr;
                }
            }
            Op::Ret { cond } => {
                if cond != Cond::Always {
                    self.extend_fetch(bus);
                }
                if self.cond(cond) {
                    let target = self.pop(bus);
                    self.regs.pc = target;
                    self.regs.wz = target;
                }
            }
            Op::Retn | Op::Reti => {
                let target = self.pop(bus);
                self.regs.pc = target;
                self.regs.wz = target;
                self.iff1 = self.iff2;
            }
            Op::Rst(n) => {
                self.extend_fetch(bus);
                self.push(bus, self.regs.pc);
                self.regs.pc = n as u16;
                self.regs.wz = n as u16;
            }

            Op::Push(r) => {
                self.extend_fetch(bus);
                let v = self.reg16(r);
                self.push(bus, v);
            }
            Op::Pop(r) => {
                let v = self.pop(bus);
                self.set_reg16(r, v);
            }

            Op::Ex(ExOp::AfAf) => {
                let af = self.regs.af();
                self.regs.set_af(self.regs.af_);
                self.regs.af_ = af;
            }
            Op::Ex(ExOp::DeHl) => {
                let de = self.regs.de();
                self.regs.set_de(self.regs.hl());
                self.regs.set_hl(de);
            }
            Op::Ex(ExOp::SpReg(r)) => {
                let sp = self.regs.sp;
                let lo = self.read_byte(bus, sp);
                let hi = self.read_byte(bus, sp.wrapping_add(1));
                self.internal(bus, sp.wrapping_add(1), 1);
                let v = self.reg16(r);
                self.write_byte(bus, sp.wrapping_add(1), (v >> 8) as u8);
                self.write_byte(bus, sp, v as u8);
                self.internal(bus, sp, 2);
                let new = u16::from_le_bytes([lo, hi]);
                self.set_reg16(r, new);
                self.regs.wz = new;
            }
            Op::Exx => {
                let (bc, de, hl) = (self.regs.bc(), self.regs.de(), self.regs.hl());
                self.regs.set_bc(self.regs.bc_);
                self.regs.set_de(self.regs.de_);
                self.regs.set_hl(self.regs.hl_);
                self.regs.bc_ = bc;
                self.regs.de_ = de;
                self.regs.hl_ = hl;
            }

            Op::In { dst, src } => {
                let port = match src {
                    PortSrc::Imm(n) => {
                        let port = u16::from_be_bytes([self.regs.a, n]);
                        self.regs.wz = port.wrapping_add(1);
                        port
                    }
                    PortSrc::Bc => {
                        let port = self.regs.bc();
                        self.regs.wz = port.wrapping_add(1);
                        port
                    }
                };
                let v = self.in_port(bus, port);
                match (dst, src) {
                    // IN A,(n) is the one that leaves the flags alone.
                    (Some(r), PortSrc::Imm(_)) => self.set_reg8(bus, r, ea, v),
                    (dst, _) => {
                        if let Some(r) = dst {
                            self.set_reg8(bus, r, ea, v);
                        }
                        let f = (self.regs.f & CF)
                            | (v & (SF | F53))
                            | if v == 0 { ZF } else { 0 }
                            | alu::parity(v);
                        self.set_f(f);
                    }
                }
            }
            Op::Out { src, dst } => {
                let port = match dst {
                    PortSrc::Imm(n) => {
                        let port = u16::from_be_bytes([self.regs.a, n]);
                        // Only the low byte advances; the high byte is A.
                        self.regs.wz = u16::from_be_bytes([self.regs.a, n.wrapping_add(1)]);
                        port
                    }
                    PortSrc::Bc => {
                        let port = self.regs.bc();
                        self.regs.wz = port.wrapping_add(1);
                        port
                    }
                };
                let v = match src {
                    Some(r) => self.reg8(bus, r, ea),
                    None => 0,
                };
                self.out_port(bus, port, v);
            }

            Op::Block(op) => self.block(bus, op),

            Op::Rlca => {
                let (a, f) = alu::rlca(self.regs.a, self.regs.f);
                self.regs.a = a;
                self.set_f(f);
            }
            Op::Rrca => {
                let (a, f) = alu::rrca(self.regs.a, self.regs.f);
                self.regs.a = a;
                self.set_f(f);
            }
            Op::Rla => {
                let (a, f) = alu::rla(self.regs.a, self.regs.f);
                self.regs.a = a;
                self.set_f(f);
            }
            Op::Rra => {
                let (a, f) = alu::rra(self.regs.a, self.regs.f);
                self.regs.a = a;
                self.set_f(f);
            }
            Op::Daa => {
                let (a, f) = alu::daa(self.regs.a, self.regs.f);
                self.regs.a = a;
                self.set_f(f);
            }
            Op::Cpl => {
                self.regs.a = !self.regs.a;
                let f = (self.regs.f & (SF | ZF | PF | CF)) | HF | NF | (self.regs.a & F53);
                self.set_f(f);
            }
            Op::Scf => {
                let f = (self.regs.f & (SF | ZF | PF))
                    | CF
                    | alu::scf_ccf_53(self.regs.a, self.regs.f, q);
                self.set_f(f);
            }
            Op::Ccf => {
                let carry = self.regs.f & CF != 0;
                let f = (self.regs.f & (SF | ZF | PF))
                    | if carry { HF } else { CF }
                    | alu::scf_ccf_53(self.regs.a, self.regs.f, q);
                self.set_f(f);
            }
            Op::Neg => {
                let (a, f) = alu::neg(self.regs.a);
                self.regs.a = a;
                self.set_f(f);
            }

            Op::Rld => {
                let hl = self.regs.hl();
                let v = self.read_byte(bus, hl);
                self.internal(bus, hl, 4);
                let a = self.regs.a;
                self.write_byte(bus, hl, (v << 4) | (a & 0x0F));
                self.regs.a = (a & 0xF0) | (v >> 4);
                self.regs.wz = hl.wrapping_add(1);
                self.set_nibble_flags();
            }
            Op::Rrd => {
                let hl = self.regs.hl();
                let v = self.read_byte(bus, hl);
                self.internal(bus, hl, 4);
                let a = self.regs.a;
                self.write_byte(bus, hl, (a << 4) | (v >> 4));
                self.regs.a = (a & 0xF0) | (v & 0x0F);
                self.regs.wz = hl.wrapping_add(1);
                self.set_nibble_flags();
            }

            Op::Di => {
                self.iff1 = false;
                self.iff2 = false;
            }
            Op::Ei => {
                self.iff1 = true;
                self.iff2 = true;
                self.ei_pending = true;
            }
            Op::Im(mode) => {
                self.im = match mode {
                    ImMode::Im0 | ImMode::Im01 => 0,
                    ImMode::Im1 => 1,
                    ImMode::Im2 => 2,
                }
            }
            Op::Halt => self.halted = true,

            // NONI: no operation, and no interrupt either — the following instruction is
            // shielded exactly as it is after EI.
            Op::Invalid => self.ei_pending = true,
        }
    }

    // ------------------------------------------------------------------- operand access

    fn reg8<B: Bus>(&mut self, bus: &mut B, r: Reg8, ea: u16) -> u8 {
        match r {
            Reg8::B => self.regs.b,
            Reg8::C => self.regs.c,
            Reg8::D => self.regs.d,
            Reg8::E => self.regs.e,
            Reg8::H => self.regs.h,
            Reg8::L => self.regs.l,
            Reg8::A => self.regs.a,
            Reg8::IxH => (self.regs.ix >> 8) as u8,
            Reg8::IxL => self.regs.ix as u8,
            Reg8::IyH => (self.regs.iy >> 8) as u8,
            Reg8::IyL => self.regs.iy as u8,
            Reg8::I => self.regs.i,
            Reg8::R => self.regs.r,
            Reg8::MemHl | Reg8::MemIdx => self.read_byte(bus, ea),
        }
    }

    fn set_reg8<B: Bus>(&mut self, bus: &mut B, r: Reg8, ea: u16, v: u8) {
        match r {
            Reg8::B => self.regs.b = v,
            Reg8::C => self.regs.c = v,
            Reg8::D => self.regs.d = v,
            Reg8::E => self.regs.e = v,
            Reg8::H => self.regs.h = v,
            Reg8::L => self.regs.l = v,
            Reg8::A => self.regs.a = v,
            Reg8::IxH => self.regs.ix = u16::from_be_bytes([v, self.regs.ix as u8]),
            Reg8::IxL => self.regs.ix = u16::from_be_bytes([(self.regs.ix >> 8) as u8, v]),
            Reg8::IyH => self.regs.iy = u16::from_be_bytes([v, self.regs.iy as u8]),
            Reg8::IyL => self.regs.iy = u16::from_be_bytes([(self.regs.iy >> 8) as u8, v]),
            Reg8::I => self.regs.i = v,
            Reg8::R => self.regs.r = v,
            Reg8::MemHl | Reg8::MemIdx => self.write_byte(bus, ea, v),
        }
    }

    fn src8<B: Bus>(&mut self, bus: &mut B, src: Src8, ea: u16) -> u8 {
        match src {
            Src8::Reg(r) => self.reg8(bus, r, ea),
            Src8::Imm(n) => n,
        }
    }

    fn reg16(&self, r: Reg16) -> u16 {
        match r {
            Reg16::Bc => self.regs.bc(),
            Reg16::De => self.regs.de(),
            Reg16::Hl => self.regs.hl(),
            Reg16::Sp => self.regs.sp,
            Reg16::Af => self.regs.af(),
            Reg16::Ix => self.regs.ix,
            Reg16::Iy => self.regs.iy,
            Reg16::Pc => self.regs.pc,
        }
    }

    fn set_reg16(&mut self, r: Reg16, v: u16) {
        match r {
            Reg16::Bc => self.regs.set_bc(v),
            Reg16::De => self.regs.set_de(v),
            Reg16::Hl => self.regs.set_hl(v),
            Reg16::Sp => self.regs.sp = v,
            Reg16::Af => {
                self.regs.set_af(v);
                // POP AF loads F wholesale; it is not a flag computation, so Q stays clear.
            }
            Reg16::Ix => self.regs.ix = v,
            Reg16::Iy => self.regs.iy = v,
            Reg16::Pc => self.regs.pc = v,
        }
    }

    fn mem_addr(&self, a: MemAddr) -> u16 {
        match a {
            MemAddr::Bc => self.regs.bc(),
            MemAddr::De => self.regs.de(),
            MemAddr::Imm(nn) => nn,
        }
    }

    /// The one internal T-state a read-modify-write instruction spends between reading its
    /// operand and writing it back.
    fn rmw_pause<B: Bus>(&mut self, bus: &mut B, r: Reg8, ea: u16) {
        if matches!(r, Reg8::MemHl | Reg8::MemIdx) {
            self.internal(bus, ea, 1);
        }
    }

    /// The `DDCB` quirk: the result also lands in a register, and it does so whether or not
    /// the write to `(IX+d)` went anywhere.
    fn copy_result<B: Bus>(&mut self, bus: &mut B, copy_to: Option<Reg8>, ea: u16, v: u8) {
        if let Some(r) = copy_to {
            self.set_reg8(bus, r, ea, v);
        }
    }

    fn cond(&self, cond: Cond) -> bool {
        let f = self.regs.f;
        match cond {
            Cond::Always => true,
            Cond::Nz => f & ZF == 0,
            Cond::Z => f & ZF != 0,
            Cond::Nc => f & CF == 0,
            Cond::C => f & CF != 0,
            Cond::Po => f & PF == 0,
            Cond::Pe => f & PF != 0,
            Cond::P => f & SF == 0,
            Cond::M => f & SF != 0,
        }
    }

    fn push<B: Bus>(&mut self, bus: &mut B, v: u16) {
        let sp = self.regs.sp;
        self.write_byte(bus, sp.wrapping_sub(1), (v >> 8) as u8);
        self.write_byte(bus, sp.wrapping_sub(2), v as u8);
        self.regs.sp = sp.wrapping_sub(2);
    }

    fn pop<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let sp = self.regs.sp;
        let v = self.read_word(bus, sp);
        self.regs.sp = sp.wrapping_add(2);
        v
    }

    /// `RLD` and `RRD` set the flags from the new `A`, leaving carry alone.
    fn set_nibble_flags(&mut self) {
        let a = self.regs.a;
        let f =
            (self.regs.f & CF) | (a & (SF | F53)) | if a == 0 { ZF } else { 0 } | alu::parity(a);
        self.set_f(f);
    }

    // ------------------------------------------------------------------------ block ops

    fn block<B: Bus>(&mut self, bus: &mut B, op: BlockOp) {
        use BlockOp::*;
        let forward = matches!(op, Ldi | Cpi | Ini | Outi | Ldir | Cpir | Inir | Otir);
        let step = if forward { 1u16 } else { 0xFFFFu16 };
        let repeats = matches!(op, Ldir | Lddr | Cpir | Cpdr | Inir | Indr | Otir | Otdr);

        match op {
            Ldi | Ldd | Ldir | Lddr => {
                let hl = self.regs.hl();
                let de = self.regs.de();
                let v = self.read_byte(bus, hl);
                self.write_byte(bus, de, v);
                self.internal(bus, de, 2);
                self.regs.set_hl(hl.wrapping_add(step));
                self.regs.set_de(de.wrapping_add(step));
                let bc = self.regs.bc().wrapping_sub(1);
                self.regs.set_bc(bc);
                let f = alu::block_transfer_flags(self.regs.f, v, self.regs.a, bc != 0);
                self.set_f(f);
                if repeats && bc != 0 {
                    self.internal(bus, de, 5);
                    self.repeat();
                    let f = alu::block_repeat_53(self.regs.f, (self.regs.pc >> 8) as u8);
                    self.set_f(f);
                }
            }
            Cpi | Cpd | Cpir | Cpdr => {
                let hl = self.regs.hl();
                let v = self.read_byte(bus, hl);
                self.internal(bus, hl, 5);
                self.regs.set_hl(hl.wrapping_add(step));
                let bc = self.regs.bc().wrapping_sub(1);
                self.regs.set_bc(bc);
                let f = alu::block_compare_flags(self.regs.f, self.regs.a, v, bc != 0);
                self.set_f(f);
                self.regs.wz = self.regs.wz.wrapping_add(step);
                if repeats && bc != 0 && f & ZF == 0 {
                    self.internal(bus, hl, 5);
                    self.repeat();
                    let f = alu::block_repeat_53(self.regs.f, (self.regs.pc >> 8) as u8);
                    self.set_f(f);
                }
            }
            Ini | Ind | Inir | Indr => {
                self.extend_fetch(bus);
                let port = self.regs.bc();
                self.regs.wz = port.wrapping_add(step);
                let v = self.in_port(bus, port);
                let hl = self.regs.hl();
                self.write_byte(bus, hl, v);
                let b = self.regs.b.wrapping_sub(1);
                self.regs.b = b;
                self.regs.set_hl(hl.wrapping_add(step));
                let k = v as u16 + self.regs.c.wrapping_add(step as u8) as u16;
                self.set_f(alu::block_io_flags(b, v, k));
                if repeats && b != 0 {
                    self.internal(bus, hl, 5);
                    self.repeat();
                    let pch = (self.regs.pc >> 8) as u8;
                    let f = alu::block_io_repeat_flags(self.regs.f, b, v, pch);
                    self.set_f(f);
                }
            }
            Outi | Outd | Otir | Otdr => {
                self.extend_fetch(bus);
                let hl = self.regs.hl();
                let v = self.read_byte(bus, hl);
                let b = self.regs.b.wrapping_sub(1);
                self.regs.b = b;
                let port = self.regs.bc();
                self.regs.wz = port.wrapping_add(step);
                self.out_port(bus, port, v);
                self.regs.set_hl(hl.wrapping_add(step));
                let k = v as u16 + self.regs.l as u16;
                self.set_f(alu::block_io_flags(b, v, k));
                if repeats && b != 0 {
                    self.internal(bus, port, 5);
                    self.repeat();
                    let pch = (self.regs.pc >> 8) as u8;
                    let f = alu::block_io_repeat_flags(self.regs.f, b, v, pch);
                    self.set_f(f);
                }
            }
        }
    }

    /// A repeating block instruction that has more to do rewinds `PC` over its own two
    /// bytes, so the interrupt it can now accept resumes it correctly.
    fn repeat(&mut self) {
        self.regs.pc = self.regs.pc.wrapping_sub(2);
        self.regs.wz = self.regs.pc.wrapping_add(1);
    }
}
