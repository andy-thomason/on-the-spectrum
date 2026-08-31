//! The machine: a Z80 wired to 64K of memory, a ULA and a keyboard.
//!
//! The CPU and the things it talks to are separate fields, because the CPU borrows the bus
//! for the length of an instruction — `Machine` is the pair, and [`SpectrumBus`] is what
//! implements [`Bus`].

pub mod keyboard;
pub mod memory;
pub mod screen;
pub mod snapshot;
pub mod ula;

use crate::z80::{Bus, Cpu};
use keyboard::{Chord, Keyboard};
use memory::Memory;
use ula::{FRAME_T, INT_ACTIVE_T, Ula};

/// Frames a synthesised keypress is held down, and the gap left after it.
pub const KEY_HOLD_FRAMES: u64 = 3;
pub const KEY_GAP_FRAMES: u64 = 3;

/// `E_LINE`: the address of the line being typed.
pub const E_LINE: u16 = 0x5C59;
/// `LAST_K`: the code of the last key the ROM decoded.
pub const LAST_K: u16 = 0x5C08;
/// `MODE`: the cursor mode — K, L, C, E or G.
pub const MODE: u16 = 0x5C41;

/// Everything the CPU can reach.
pub struct SpectrumBus {
    pub memory: Memory,
    pub ula: Ula,
    pub keyboard: Keyboard,
    /// T-states since the start of the current frame. Contention and the interrupt both
    /// depend on this, which is why it lives here and not in the CPU.
    pub frame_t: u32,
    pub frames: u64,
    /// Off until phase H. The accounting above is identical either way: switching
    /// contention on is one function and this flag, not a rewrite.
    pub contention_enabled: bool,
}

impl Bus for SpectrumBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.memory.read(addr)
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.memory.write(addr, val);
    }

    /// The ULA answers every port with A0 low. Nothing else is attached, so an odd port
    /// reads the floating bus — `0xFF` here, until the beam renderer can say what the ULA
    /// was fetching at the time.
    fn in_port(&mut self, port: u16) -> u8 {
        if port & 1 == 0 {
            self.ula.read_port(port, &self.keyboard)
        } else {
            0xFF
        }
    }

    fn out_port(&mut self, port: u16, val: u8) {
        if port & 1 == 0 {
            self.ula.write_port(val);
        }
    }

    /// The frame wraps here rather than in [`Machine::run_frame`], so that *any* way of
    /// driving the machine — a frame at a time, an instruction at a time, or running to a
    /// breakpoint — sees the interrupt arrive on time.
    ///
    /// The overshoot is carried, not discarded: an instruction almost never lands exactly
    /// on 69888, and resetting to zero would slip the clock a few T-states every frame.
    fn tick(&mut self, cycles: u32) {
        self.frame_t += cycles;
        if self.frame_t >= FRAME_T {
            self.frame_t -= FRAME_T;
            self.frames += 1;
        }
        // A scanline or beam renderer hooks in here, and nowhere else.
    }

    fn contention(&mut self, addr: u16) -> u32 {
        if !self.contention_enabled || !(0x4000..0x8000).contains(&addr) {
            return 0;
        }
        // Phase H: the delay table in doc/spectrum-video.md, indexed by self.frame_t.
        0
    }
}

pub struct Machine {
    pub cpu: Cpu,
    pub bus: SpectrumBus,
}

impl Machine {
    /// A machine with `rom` loaded and everything else at power-on.
    pub fn new(rom: &[u8]) -> Self {
        let mut memory = Memory::new();
        memory.load_rom(rom);
        Machine {
            cpu: Cpu::new(),
            bus: SpectrumBus {
                memory,
                ula: Ula::new(),
                keyboard: Keyboard::new(),
                frame_t: 0,
                frames: 0,
                contention_enabled: false,
            },
        }
    }

    /// Load the bundled 48K ROM.
    pub fn with_rom_file(path: &std::path::Path) -> std::io::Result<Self> {
        Ok(Machine::new(&std::fs::read(path)?))
    }

    /// Pull `/INT` low for the first 32 T-states of each frame and run one instruction.
    ///
    /// Because the line is only low for that long, an instruction that starts inside the
    /// window takes the interrupt and one that straddles it misses — which is what the
    /// hardware does.
    pub fn step(&mut self) -> u32 {
        self.cpu.int_pending = self.bus.frame_t < INT_ACTIVE_T;
        self.cpu.step(&mut self.bus)
    }

    /// Run until the frame counter advances.
    pub fn run_frame(&mut self) {
        let target = self.bus.frames + 1;
        while self.bus.frames < target {
            self.step();
        }
    }

    pub fn run_frames(&mut self, frames: u64) {
        for _ in 0..frames {
            self.run_frame();
        }
    }

    /// Run until `PC` reaches `addr`, giving up after `budget` T-states. Returns whether it
    /// arrived — the shape every boot milestone is checked with.
    pub fn run_until_pc(&mut self, addr: u16, budget: u64) -> bool {
        let deadline = self.cpu.total_t + budget;
        while self.cpu.total_t < deadline {
            if self.cpu.regs.pc == addr {
                return true;
            }
            self.step();
        }
        self.cpu.regs.pc == addr
    }

    /// Run until `PC` reaches any of `addrs`, giving up after `budget` T-states. Returns
    /// the address it stopped at.
    pub fn run_until_any_pc(&mut self, addrs: &[u16], budget: u64) -> Option<u16> {
        let deadline = self.cpu.total_t + budget;
        while self.cpu.total_t < deadline {
            if addrs.contains(&self.cpu.regs.pc) {
                return Some(self.cpu.regs.pc);
            }
            self.step();
        }
        None
    }

    /// The screen as 24 lines of 32 characters, read back through the ROM's own font.
    pub fn screen_text(&self) -> Vec<String> {
        screen::screen_to_text(&self.bus.memory)
    }

    /// Render the visible frame — display and border — into `out` as RGBA.
    pub fn render_into(&self, out: &mut [u8]) {
        screen::render_into(
            &self.bus.memory,
            self.bus.ula.border,
            Ula::flash_inverted(self.bus.frames),
            out,
        );
    }

    // ------------------------------------------------------------------------- typing

    /// Press a chord, hold it, release it, and let the machine settle.
    ///
    /// The ROM debounces and decodes keys in its 50 Hz interrupt handler, so a keypress
    /// has to last frames to exist at all — pulsing the matrix for an instruction is
    /// invisible. Holding for [`KEY_HOLD_FRAMES`] and pausing for [`KEY_GAP_FRAMES`] is
    /// comfortably inside `REPDEL`, so nothing auto-repeats.
    pub fn tap(&mut self, chord: Chord) {
        self.bus.keyboard.press(chord);
        self.run_frames(KEY_HOLD_FRAMES);
        self.bus.keyboard.release_all();
        self.run_frames(KEY_GAP_FRAMES);
    }

    /// Type a key by its name in [`keyboard::HALF_ROWS`] — `"ENTER"`, `"SPACE"`, `"P"`.
    pub fn tap_named(&mut self, name: &str) -> bool {
        match keyboard::Key::named(name) {
            Some(key) => {
                self.tap(Chord::plain(key));
                true
            }
            None => false,
        }
    }

    /// Type text at the BASIC prompt, one character at a time.
    ///
    /// Returns false if some character has no key. Remember that the *machine* decides
    /// what a key means: at the `K` prompt, typing `P` gets you the whole `PRINT` keyword,
    /// which is exactly how you type `PRINT 2+2` on a Spectrum.
    pub fn type_text(&mut self, text: &str) -> bool {
        text.chars().all(|c| match Chord::for_char(c) {
            Some(chord) => {
                self.tap(chord);
                true
            }
            None => false,
        })
    }

    /// The line being typed, as the ROM holds it: from `E_LINE` up to its `0x0D`
    /// terminator, keywords still as single token bytes.
    pub fn edit_line(&self) -> Vec<u8> {
        let start = self.bus.memory.peek16(E_LINE);
        let mut line = Vec::new();
        for offset in 0..0x200 {
            let byte = self.bus.memory.peek(start.wrapping_add(offset));
            if byte == 0x0D {
                break;
            }
            line.push(byte);
        }
        line
    }
}
