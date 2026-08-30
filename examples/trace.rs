//! A bare instruction trace, in the column format of
//! [`doc/boot-and-test.md`](../doc/boot-and-test.md) §7.
//!
//! ```sh
//! cargo run --example trace -- --from 11CB --count 12
//! cargo run --example trace -- --from 0D6B --count 20   # CLS
//! ```
//!
//! The `Tracer` trait of §7 — ring buffer, opcode counts, watchpoints — is not built yet.
//! This is what the same decoder gets you in the meantime: run to an address, then print
//! one line per instruction, decoded exactly as the interpreter is about to execute it.

use on_the_spectrum::spectrum::Machine;
use on_the_spectrum::symbols::RomSymbols;
use on_the_spectrum::z80::{decode_bytes, disassemble};

fn main() {
    let mut from = 0x11CBu16;
    let mut count = 20usize;
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let value = argv.next().unwrap_or_default();
        match arg.as_str() {
            "--from" => from = u16::from_str_radix(value.trim_start_matches('$'), 16).unwrap(),
            "--count" => count = value.parse().unwrap(),
            other => {
                eprintln!("usage: trace [--from ADDR] [--count N]   (got {other})");
                std::process::exit(1);
            }
        }
    }

    let rom = std::fs::read("roms/48.rom").expect("roms/48.rom");
    let mut machine = Machine::new(&rom);
    if !machine.run_until_pc(from, 20_000_000) {
        eprintln!("never reached ${from:04X}");
        std::process::exit(1);
    }

    println!(
        " PC   bytes        mnemonic               AF   BC   DE   HL   IX   IY   SP   IR   T-state"
    );
    for _ in 0..count {
        let pc = machine.cpu.regs.pc;
        // The same decoder the interpreter is about to run, so the trace cannot lie.
        let decoded = decode_bytes(&machine.bus.memory.bytes()[pc as usize..]);
        let bytes: String = (0..decoded.len)
            .map(|i| {
                format!(
                    "{:02X} ",
                    machine.bus.memory.peek(pc.wrapping_add(i as u16))
                )
            })
            .collect();
        let text = disassemble(&decoded, pc, Some(&RomSymbols));

        let r = &machine.cpu.regs;
        println!(
            "{pc:04X}  {bytes:<12} {text:<22} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}  {:010}",
            r.af(),
            r.bc(),
            r.de(),
            r.hl(),
            r.ix,
            r.iy,
            r.sp,
            r.ir(),
            machine.cpu.total_t,
        );
        machine.step();
    }
}
