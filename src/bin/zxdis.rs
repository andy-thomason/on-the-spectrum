//! `zxdis` — disassemble a binary with the emulator's own decoder.
//!
//! ```text
//! zxdis roms/48.rom                          # the whole ROM at $0000
//! zxdis --start 11CB --end 1200 roms/48.rom  # one routine
//! zxdis --org 8000 --no-symbols game.bin     # a loaded block, no ROM names
//! ```
//!
//! Output is laid out to be read beside `doc/ref/Spectrum48-disassembly.asm`: the harvested
//! label goes above the instruction it names, exactly as the listing has it.

use std::process::ExitCode;

use on_the_spectrum::symbols::RomSymbols;
use on_the_spectrum::z80::{Symbols, disasm};

struct Args {
    path: String,
    org: u16,
    start: Option<u16>,
    end: Option<u16>,
    symbols: bool,
}

const USAGE: &str = "\
usage: zxdis [--org ADDR] [--start ADDR] [--end ADDR] [--no-symbols] FILE

  --org ADDR      address the file is loaded at (default 0000)
  --start ADDR    first address to disassemble (default: org)
  --end ADDR      one past the last address (default: end of file)
  --no-symbols    do not annotate ROM entry points

Addresses are hex, with or without a leading $ or 0x.";

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("zxdis: {message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    let mem = match std::fs::read(&args.path) {
        Ok(mem) => mem,
        Err(e) => {
            eprintln!("zxdis: {}: {e}", args.path);
            return ExitCode::FAILURE;
        }
    };

    let syms: Option<&dyn Symbols> = args.symbols.then_some(&RomSymbols);
    let start = args.start.unwrap_or(args.org).wrapping_sub(args.org) as usize;
    let end = match args.end {
        Some(end) => end.wrapping_sub(args.org) as usize,
        None => mem.len(),
    };

    println!(
        "; {} — ${:04X}..${:04X}",
        args.path,
        args.org.wrapping_add(start as u16),
        args.org.wrapping_add(end as u16)
    );
    for insn in disasm::walk(&mem, args.org, start, end) {
        if let Some(name) = syms.and_then(|s| s.name(insn.addr)) {
            println!(";; {name}");
        }
        let (text, comment) = disasm::disassemble_parts(&insn.decoded, insn.addr, syms);
        let bytes = insn
            .bytes
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        match comment {
            Some(comment) => println!("{:04X}  {bytes:<11}  {text:<22} ; {comment}", insn.addr),
            None => println!("{:04X}  {bytes:<11}  {text}", insn.addr),
        }
    }
    ExitCode::SUCCESS
}

fn parse_args() -> Result<Args, String> {
    let mut path = None;
    let mut org = 0u16;
    let mut start = None;
    let mut end = None;
    let mut symbols = true;

    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let mut value = || {
            argv.next()
                .ok_or_else(|| format!("{arg} needs an address"))
                .and_then(|v| parse_addr(&v))
        };
        match arg.as_str() {
            "--org" => org = value()?,
            "--start" => start = Some(value()?),
            "--end" => end = Some(value()?),
            "--no-symbols" => symbols = false,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => path = Some(other.to_string()),
        }
    }

    Ok(Args {
        path: path.ok_or("no input file")?,
        org,
        start,
        end,
        symbols,
    })
}

fn parse_addr(s: &str) -> Result<u16, String> {
    let hex = s.trim_start_matches('$').trim_start_matches("0x");
    u16::from_str_radix(hex, 16).map_err(|_| format!("bad address {s:?}"))
}
