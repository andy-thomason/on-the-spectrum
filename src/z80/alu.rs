//! 8- and 16-bit arithmetic, and the flags that come out of it.
//!
//! Every function here is pure: value in, value and flags out. The undocumented flags 5
//! and 3 are computed everywhere, because real software reads them — see
//! [`doc/z80-instruction-set.md`](../../../doc/z80-instruction-set.md) §1.

use super::{CF, F3, F5, F53, HF, NF, PF, SF, ZF};

/// Parity of a byte, in the position of the P/V flag.
pub fn parity(v: u8) -> u8 {
    if v.count_ones().is_multiple_of(2) {
        PF
    } else {
        0
    }
}

fn sz53(v: u8) -> u8 {
    (v & (SF | F53)) | if v == 0 { ZF } else { 0 }
}

/// `ADD A,x` and `ADC A,x`.
pub fn add8(a: u8, b: u8, carry: bool) -> (u8, u8) {
    let full = a as u16 + b as u16 + carry as u16;
    let r = full as u8;
    let f = sz53(r)
        | ((a ^ b ^ r) & HF)
        | if (a ^ b) & 0x80 == 0 && (a ^ r) & 0x80 != 0 {
            PF
        } else {
            0
        }
        | if full > 0xFF { CF } else { 0 };
    (r, f)
}

/// `SUB x` and `SBC A,x`.
pub fn sub8(a: u8, b: u8, carry: bool) -> (u8, u8) {
    let full = (a as i16) - (b as i16) - (carry as i16);
    let r = full as u8;
    let f = sz53(r)
        | NF
        | ((a ^ b ^ r) & HF)
        | if (a ^ b) & 0x80 != 0 && (a ^ r) & 0x80 != 0 {
            PF
        } else {
            0
        }
        | if full < 0 { CF } else { 0 };
    (r, f)
}

/// `CP x`. The result is discarded, so flags 5 and 3 come from *the operand*.
pub fn cp8(a: u8, b: u8) -> u8 {
    let (_, f) = sub8(a, b, false);
    (f & !F53) | (b & F53)
}

pub fn and8(a: u8, b: u8) -> (u8, u8) {
    let r = a & b;
    (r, sz53(r) | HF | parity(r))
}

pub fn or8(a: u8, b: u8) -> (u8, u8) {
    let r = a | b;
    (r, sz53(r) | parity(r))
}

pub fn xor8(a: u8, b: u8) -> (u8, u8) {
    let r = a ^ b;
    (r, sz53(r) | parity(r))
}

/// `INC r`. Carry is preserved, so the caller passes the current flags in.
pub fn inc8(v: u8, f: u8) -> (u8, u8) {
    let r = v.wrapping_add(1);
    let f =
        (f & CF) | sz53(r) | if r & 0x0F == 0 { HF } else { 0 } | if v == 0x7F { PF } else { 0 };
    (r, f)
}

/// `DEC r`. Carry is preserved.
pub fn dec8(v: u8, f: u8) -> (u8, u8) {
    let r = v.wrapping_sub(1);
    let f = (f & CF)
        | NF
        | sz53(r)
        | if v & 0x0F == 0 { HF } else { 0 }
        | if v == 0x80 { PF } else { 0 };
    (r, f)
}

pub fn neg(a: u8) -> (u8, u8) {
    sub8(0, a, false)
}

// ------------------------------------------------------------------------ rotate/shift

/// The `CB`-prefixed rotates and shifts, which set the full flag set.
pub fn rlc(v: u8, _f: u8) -> (u8, u8) {
    let r = v.rotate_left(1);
    (r, sz53(r) | parity(r) | (v >> 7))
}

pub fn rrc(v: u8, _f: u8) -> (u8, u8) {
    let r = v.rotate_right(1);
    (r, sz53(r) | parity(r) | (v & CF))
}

pub fn rl(v: u8, f: u8) -> (u8, u8) {
    let r = (v << 1) | (f & CF);
    (r, sz53(r) | parity(r) | (v >> 7))
}

pub fn rr(v: u8, f: u8) -> (u8, u8) {
    let r = (v >> 1) | ((f & CF) << 7);
    (r, sz53(r) | parity(r) | (v & CF))
}

pub fn sla(v: u8, _f: u8) -> (u8, u8) {
    let r = v << 1;
    (r, sz53(r) | parity(r) | (v >> 7))
}

pub fn sra(v: u8, _f: u8) -> (u8, u8) {
    let r = (v >> 1) | (v & 0x80);
    (r, sz53(r) | parity(r) | (v & CF))
}

/// Undocumented: shift left, and bit 0 becomes 1.
pub fn sll(v: u8, _f: u8) -> (u8, u8) {
    let r = (v << 1) | 1;
    (r, sz53(r) | parity(r) | (v >> 7))
}

pub fn srl(v: u8, _f: u8) -> (u8, u8) {
    let r = v >> 1;
    (r, sz53(r) | parity(r) | (v & CF))
}

/// The accumulator rotates `RLCA`/`RRCA`/`RLA`/`RRA`, which leave S, Z and P/V alone and
/// take flags 5 and 3 from the result.
pub fn rlca(a: u8, f: u8) -> (u8, u8) {
    let r = a.rotate_left(1);
    (r, (f & (SF | ZF | PF)) | (r & F53) | (a >> 7))
}

pub fn rrca(a: u8, f: u8) -> (u8, u8) {
    let r = a.rotate_right(1);
    (r, (f & (SF | ZF | PF)) | (r & F53) | (a & CF))
}

pub fn rla(a: u8, f: u8) -> (u8, u8) {
    let r = (a << 1) | (f & CF);
    (r, (f & (SF | ZF | PF)) | (r & F53) | (a >> 7))
}

pub fn rra(a: u8, f: u8) -> (u8, u8) {
    let r = (a >> 1) | ((f & CF) << 7);
    (r, (f & (SF | ZF | PF)) | (r & F53) | (a & CF))
}

// ------------------------------------------------------------------------------- bit ops

/// `BIT n,x`. `f53` supplies flags 5 and 3: the operand for a register, the high byte of
/// MEMPTR for `(HL)` and `(IX+d)`.
pub fn bit(bit: u8, v: u8, f: u8, f53: u8) -> u8 {
    let set = v & (1 << bit) != 0;
    (f & CF)
        | HF
        | (f53 & F53)
        | if set { 0 } else { ZF | PF }
        | if set && bit == 7 { SF } else { 0 }
}

// ---------------------------------------------------------------------------- 16-bit

/// `ADD HL,rp`. S, Z and P/V are left alone; 5, 3 and H come from the high byte.
pub fn add16(a: u16, b: u16, f: u8) -> (u16, u8) {
    let full = a as u32 + b as u32;
    let r = full as u16;
    let f = (f & (SF | ZF | PF))
        | ((((a ^ b ^ r) >> 8) as u8) & HF)
        | (((r >> 8) as u8) & F53)
        | if full > 0xFFFF { CF } else { 0 };
    (r, f)
}

/// `ADC HL,rp`. Unlike `ADD`, this one sets the whole flag set.
pub fn adc16(a: u16, b: u16, carry: bool) -> (u16, u8) {
    let full = a as u32 + b as u32 + carry as u32;
    let r = full as u16;
    let f = (((r >> 8) as u8) & (SF | F53))
        | if r == 0 { ZF } else { 0 }
        | ((((a ^ b ^ r) >> 8) as u8) & HF)
        | if (a ^ b) & 0x8000 == 0 && (a ^ r) & 0x8000 != 0 {
            PF
        } else {
            0
        }
        | if full > 0xFFFF { CF } else { 0 };
    (r, f)
}

/// `SBC HL,rp`.
pub fn sbc16(a: u16, b: u16, carry: bool) -> (u16, u8) {
    let full = (a as i32) - (b as i32) - (carry as i32);
    let r = full as u16;
    let f = (((r >> 8) as u8) & (SF | F53))
        | if r == 0 { ZF } else { 0 }
        | NF
        | ((((a ^ b ^ r) >> 8) as u8) & HF)
        | if (a ^ b) & 0x8000 != 0 && (a ^ r) & 0x8000 != 0 {
            PF
        } else {
            0
        }
        | if full < 0 { CF } else { 0 };
    (r, f)
}

// --------------------------------------------------------------------------------- DAA

/// The c.s.s FAQ's formulation, which is exact where the Zilog table is awkward.
pub fn daa(a: u8, f: u8) -> (u8, u8) {
    let mut correction = 0u8;
    let mut carry = false;
    if a & 0x0F > 9 || f & HF != 0 {
        correction |= 0x06;
    }
    if a > 0x99 || f & CF != 0 {
        correction |= 0x60;
        carry = true;
    }
    let r = if f & NF != 0 {
        a.wrapping_sub(correction)
    } else {
        a.wrapping_add(correction)
    };
    let f = (f & NF) | sz53(r) | parity(r) | ((a ^ r) & HF) | if carry { CF } else { 0 };
    (r, f)
}

/// `SCF` and `CCF` take flags 5 and 3 from `A` **or** from the flags themselves, depending
/// on whether the previous instruction wrote flags — which is what `Q` records. If it did,
/// `q == f` and this reduces to `A`; if it did not, `q == 0` and the old 5 and 3 survive.
pub fn scf_ccf_53(a: u8, f: u8, q: u8) -> u8 {
    ((q ^ f) | a) & F53
}

/// `LDI`/`LDD`: flags 5 and 3 come from `A + the byte transferred`, in an unusual
/// arrangement — 3 is bit 3, but 5 is **bit 1**.
pub fn block_transfer_flags(f: u8, value: u8, a: u8, bc_left: bool) -> u8 {
    let v = value.wrapping_add(a);
    (f & (SF | ZF | CF))
        | if bc_left { PF } else { 0 }
        | (v & F3)
        | if v & 0x02 != 0 { F5 } else { 0 }
}

/// `CPI`/`CPD`.
pub fn block_compare_flags(f: u8, a: u8, value: u8, bc_left: bool) -> u8 {
    let r = a.wrapping_sub(value);
    let half = (a ^ value ^ r) & HF;
    let v = r.wrapping_sub((half != 0) as u8);
    (f & CF)
        | NF
        | (r & SF)
        | if r == 0 { ZF } else { 0 }
        | half
        | if bc_left { PF } else { 0 }
        | (v & F3)
        | if v & 0x02 != 0 { F5 } else { 0 }
}

/// When a repeating block instruction goes round again, flags 5 and 3 stop coming from the
/// data and come from the high byte of `PC` — which, `PC` having just been rewound, is the
/// high byte of the instruction's own address.
pub fn block_repeat_53(f: u8, pch: u8) -> u8 {
    (f & !F53) | (pch & F53)
}

/// Repeating disturbs H and P/V as well for the block I/O instructions, in a pattern that
/// depends on the carry and on the low nibble of `B`. Patrik Rak worked this out; the
/// per-opcode vectors confirm it on every one of their 3990 repeating cases.
pub fn block_io_repeat_flags(f: u8, b: u8, value: u8, pch: u8) -> u8 {
    let mut f = block_repeat_53(f, pch);
    let p = if f & CF != 0 {
        f &= !HF;
        if value & 0x80 != 0 {
            if b & 0x0F == 0x00 {
                f |= HF;
            }
            parity(b.wrapping_sub(1) & 7)
        } else {
            if b & 0x0F == 0x0F {
                f |= HF;
            }
            parity(b.wrapping_add(1) & 7)
        }
    } else {
        parity(b & 7)
    };
    // P/V is *flipped* by an odd parity here, not replaced by it.
    f ^ (p ^ PF)
}

/// `INI`/`IND`/`OUTI`/`OUTD` and their repeats. `k` is the byte transferred plus the
/// port-neighbouring value the instruction happens to add: `C ± 1` for the `IN` forms,
/// `L` for the `OUT` forms.
pub fn block_io_flags(b_after: u8, value: u8, k: u16) -> u8 {
    let carry = k > 0xFF;
    (b_after & (SF | F53))
        | if b_after == 0 { ZF } else { 0 }
        | if value & 0x80 != 0 { NF } else { 0 }
        | if carry { HF | CF } else { 0 }
        | parity(((k & 7) as u8) ^ b_after)
}
