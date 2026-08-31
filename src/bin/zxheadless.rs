//! `zxheadless` — boot the machine with no window attached and look at what it did.
//!
//! ```text
//! zxheadless                          # 100 frames, then print the screen
//! zxheadless --frames 2 --state       # the first two frames, with the CPU state
//! zxheadless --until 12A9             # run until PC reaches MAIN-1
//! zxheadless --type 'P2+2\n'           # boot, then type PRINT 2+2 and ENTER
//! ```
//!
//! Everything the Bevy front end will do rests on this: if the machine does not boot here,
//! a window will not help.

use std::process::ExitCode;

use on_the_spectrum::spectrum::{Machine, screen, snapshot};
use on_the_spectrum::symbols::RomSymbols;
use on_the_spectrum::z80::Symbols;

const USAGE: &str = "\
usage: zxheadless [--rom PATH] [--load PATH] [--frames N] [--until ADDR] [--state] [--raw]

  --rom PATH     ROM image to boot (default roms/48.rom)
  --load PATH    load a .sna or .z80 snapshot instead of booting from cold
  --save-sna P   write the machine out as a .sna when the run finishes
  --frames N     frames to run (default 100; one frame is 69888 T-states)
  --until ADDR   run until PC reaches this hex address, then stop
  --type TEXT    boot to the BASIC prompt, then type this and run on
  --state        print the CPU state as well as the screen
  --raw          print all 24 screen lines, blanks included
  --ppm PATH     also write the rendered frame as a binary PPM image

At the K prompt a letter key is a whole keyword, so --type 'P2+2\n' types
PRINT 2+2 and presses ENTER. \n in the text is ENTER.

Addresses are hex, with or without a leading $ or 0x.";

/// `MAIN-1`, where the ROM sits waiting for something to be typed.
const MAIN_1: u16 = 0x12A9;

struct Args {
    rom: String,
    frames: u64,
    until: Option<u16>,
    text: Option<String>,
    state: bool,
    raw: bool,
    ppm: Option<String>,
    sna: Option<String>,
    save_sna: Option<String>,
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

    if let Some(path) = &args.sna {
        match snapshot::load_path(&mut machine, std::path::Path::new(path)) {
            Ok(()) => println!("loaded {path}"),
            Err(e) => {
                eprintln!("zxheadless: {path}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    let mut arrived = match args.until {
        // A generous budget: booting to the main loop takes about 5.9 million T-states.
        Some(addr) => machine.run_until_pc(addr, args.frames.max(1) * 69888),
        None if args.text.is_some() && args.sna.is_none() => {
            machine.run_until_pc(MAIN_1, 12_000_000)
        }
        None => {
            machine.run_frames(args.frames);
            true
        }
    };

    if let Some(text) = &args.text {
        if !arrived {
            eprintln!("zxheadless: never reached the BASIC prompt");
        }
        arrived &= machine.type_text(text);
        machine.run_frames(args.frames.clamp(10, 50));
    }

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

    if let Some(path) = &args.save_sna {
        match snapshot::save_sna(&machine) {
            Ok(data) => {
                if let Err(e) = std::fs::write(path, &data) {
                    eprintln!("zxheadless: {path}: {e}");
                    return ExitCode::FAILURE;
                }
                println!("wrote {path}  {} bytes", data.len());
            }
            Err(e) => {
                eprintln!("zxheadless: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    if let Some(path) = &args.ppm {
        if let Err(e) = write_ppm(&machine, path) {
            eprintln!("zxheadless: {path}: {e}");
            return ExitCode::FAILURE;
        }
        println!("wrote {path}  {}x{}", screen::WIDTH, screen::HEIGHT);
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

/// The rendered frame as a binary PPM: three bytes a pixel behind a nine-byte header, and
/// every image viewer on the machine can open it.
fn write_ppm(machine: &Machine, path: &str) -> std::io::Result<()> {
    let mut frame = vec![0; screen::FRAME_BYTES];
    machine.render_into(&mut frame);

    let mut out = format!("P6\n{} {}\n255\n", screen::WIDTH, screen::HEIGHT).into_bytes();
    out.extend(frame.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
    std::fs::write(path, out)
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        rom: "roms/48.rom".to_string(),
        frames: 100,
        until: None,
        text: None,
        state: false,
        raw: false,
        ppm: None,
        sna: None,
        save_sna: None,
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
            "--type" => args.text = Some(value()?.replace("\\n", "\n")),
            "--state" => args.state = true,
            "--raw" => args.raw = true,
            "--ppm" => args.ppm = Some(value()?),
            "--load" => args.sna = Some(value()?),
            "--save-sna" => args.save_sna = Some(value()?),
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
