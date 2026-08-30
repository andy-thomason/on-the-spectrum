//! The Z80 core.
//!
//! It knows nothing about the Spectrum: it talks to a bus, which is what lets it be tested
//! in isolation against the standard exercisers. See
//! [`doc/boot-and-test.md`](../../doc/boot-and-test.md).

pub mod decode;
pub mod disasm;

pub use decode::{
    AluOp, BlockOp, ByteSource, Bytes, Cond, Decoded, ExOp, ImMode, JumpTarget, MemAddr, Op,
    PortSrc, Prefix, Reg8, Reg16, RotOp, Src8, Src16, decode, decode_bytes,
};
pub use disasm::{Symbols, disassemble, disassemble_parts, spec_mnemonic, walk};
