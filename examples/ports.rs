//! Which ports is a program actually reading?
//!
//! ```sh
//! cargo run --example ports -- games/Manic_Miner_1983_Software_Projects.z80 50 '\n'
//! ```
//!
//! The third argument is a key *name* held down for a while before the tally starts, so a
//! game can be got past its title screen — and it has to be held: a game that samples the
//! keyboard once every twenty frames, as this one does between notes of its tune, will
//! never see the three-frame press that is enough for the ROM. Each port's distinct return values are listed too: that is what
//! says whether a program is being told a key is held down.
//!
//! Wraps the machine's bus in one that keeps a tally, which needs nothing added to the
//! library: `Cpu::step` is generic over `Bus`, so anything that delegates will do.

use std::collections::BTreeMap;

use on_the_spectrum::spectrum::snapshot;
use on_the_spectrum::spectrum::ula::INT_ACTIVE_T;
use on_the_spectrum::spectrum::{Machine, SpectrumBus};
use on_the_spectrum::z80::Bus;

struct Spy<'a> {
    inner: &'a mut SpectrumBus,
    reads: BTreeMap<u16, (u32, std::collections::BTreeSet<u8>)>,
    writes: BTreeMap<u16, u32>,
}

impl Bus for Spy<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        self.inner.read(addr)
    }
    fn write(&mut self, addr: u16, val: u8) {
        self.inner.write(addr, val);
    }
    fn in_port(&mut self, port: u16) -> u8 {
        let value = self.inner.in_port(port);
        let entry = self.reads.entry(port).or_default();
        entry.0 += 1;
        entry.1.insert(value);
        value
    }
    fn out_port(&mut self, port: u16, val: u8) {
        *self.writes.entry(port).or_default() += 1;
        self.inner.out_port(port, val);
    }
    fn tick(&mut self, cycles: u32) {
        self.inner.tick(cycles);
    }
    fn contention(&mut self, addr: u16) -> u32 {
        self.inner.contention(addr)
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| {
        eprintln!("usage: ports <snapshot> [frames]");
        std::process::exit(1);
    });
    let frames: u64 = args.next().and_then(|n| n.parse().ok()).unwrap_or(50);
    let hold = args.next().unwrap_or_default();

    let rom = std::fs::read("roms/48.rom").expect("roms/48.rom");
    let mut machine = Machine::new(&rom);
    snapshot::load_path(&mut machine, std::path::Path::new(&path)).expect("load");
    if !hold.is_empty() {
        // Two presses: the first wakes the title sequence, the second starts the game.
        for _ in 0..2 {
            assert!(machine.tap_named(&hold), "no key called {hold:?}");
            machine.bus.keyboard.set_named(&hold, true);
            machine.run_frames(60);
            machine.bus.keyboard.release_all();
            machine.run_frames(30);
        }
    }

    println!(
        "after the warm-up: PC ${:04X}, border {}",
        machine.cpu.regs.pc, machine.bus.ula.border
    );
    let mut spy = Spy {
        inner: &mut machine.bus,
        reads: BTreeMap::new(),
        writes: BTreeMap::new(),
    };
    let target = spy.inner.frames + frames;
    while spy.inner.frames < target {
        machine.cpu.int_pending = spy.inner.frame_t < INT_ACTIVE_T;
        machine.cpu.step(&mut spy);
    }

    println!("{path}, {frames} frames");
    println!("  IN:");
    for (port, (count, values)) in &spy.reads {
        let seen: Vec<String> = values.iter().map(|v| format!("{v:02X}")).collect();
        println!(
            "    ${port:04X}  {count:>7}   saw {:<20} {}",
            seen.join(" "),
            describe(*port)
        );
    }
    println!("  OUT:");
    for (port, count) in &spy.writes {
        println!("    ${port:04X}  {count:>8}   {}", describe(*port));
    }
}

fn describe(port: u16) -> &'static str {
    match (port & 0xFF, port & 1) {
        (0x1F, _) => "Kempston joystick",
        (0x7F, _) => "Fuller joystick",
        (_, 0) => "ULA: keyboard, border, tape, beeper",
        _ => "nothing is attached here",
    }
}
