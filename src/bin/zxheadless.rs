//! `zxheadless` — boot the machine with no window attached and look at what it did.
//!
//! ```text
//! zxheadless                          # 100 frames, then print the screen
//! zxheadless --frames 2 --state       # the first two frames, with the CPU state
//! zxheadless --until 12A9             # run until PC reaches MAIN-1
//! ```
//!
//! Everything the Bevy front end will do rests on this: if the machine does not boot here,
//! a window will not help.

use std::process::ExitCode;

use on_the_spectrum::spectrum::Machine;
use on_the_spectrum::symbols::RomSymbols;
use on_the_spectrum::z80::Symbols;

const USAGE: &str = "\
usage: zxheadless [--rom PATH] [--frames N] [--until ADDR] [--state] [--raw]

  --rom PATH     ROM image to boot (default roms/48.rom)
  --frames N     frames to run (default 100; one frame is 69888 T-states)
  --until ADDR   run until PC reaches this hex address, then stop
  --state        print the CPU state as well as the screen
  --raw          print all 24 screen lines, blanks included

Addresses are hex, with or without a leading $ or 0x.";

struct Args {
    rom: String,
    frames: u64,
    until: Option<u16>,
    state: bool,
    raw: bool,
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("zxheadless: {message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    let mut machine = match Machine::with_rom_file(std::path::Path::new(&args.rom)) {
        Ok(machine) => machine,
        Err(e) => {
            eprintln!("zxheadless: {}: {e}", args.rom);
            return ExitCode::FAILURE;
        }
    };

    let arrived = match args.until {
        // A generous budget: booting to the main loop takes about 3.4 million T-states.
        Some(addr) => machine.run_until_pc(addr, args.frames.max(1) * 69888),
        None => {
            machine.run_frames(args.frames);
            true
        }
    };

    let pc = machine.cpu.regs.pc;
    let name = RomSymbols
        .name(pc)
        .map(|n| format!(" ({n})"))
        .unwrap_or_default();
    println!(
        "frames {}  T {}  PC ${pc:04X}{name}  border {}{}",
        machine.bus.frames,
        machine.cpu.total_t,
        machine.bus.ula.border,
        if arrived { "" } else { "  [did not arrive]" }
    );

    if args.state {
        let r = &machine.cpu.regs;
        println!(
            "AF {:04X}  BC {:04X}  DE {:04X}  HL {:04X}  IX {:04X}  IY {:04X}  SP {:04X}  \
             IR {:04X}  IFF{}{} IM{}",
            r.af(),
            r.bc(),
            r.de(),
            r.hl(),
            r.ix,
            r.iy,
            r.sp,
            r.ir(),
            machine.cpu.iff1 as u8,
            machine.cpu.iff2 as u8,
            machine.cpu.im
        );
    }

    println!("┌{}┐", "─".repeat(32));
    for line in machine.screen_text() {
        if args.raw || !line.trim().is_empty() {
            println!("│{line}│");
        }
    }
    println!("└{}┘", "─".repeat(32));

    if arrived {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        rom: "roms/48.rom".to_string(),
        frames: 100,
        until: None,
        state: false,
        raw: false,
    };

    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let mut value = || argv.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--rom" => args.rom = value()?,
            "--frames" => {
                args.frames = value()?
                    .parse()
                    .map_err(|_| "--frames needs a number".to_string())?
            }
            "--until" => args.until = Some(parse_addr(&value()?)?),
            "--state" => args.state = true,
            "--raw" => args.raw = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok(args)
}

fn parse_addr(s: &str) -> Result<u16, String> {
    let hex = s.trim_start_matches('$').trim_start_matches("0x");
    u16::from_str_radix(hex, 16).map_err(|_| format!("bad address {s:?}"))
}
