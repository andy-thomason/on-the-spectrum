//! The machine-cycle primitives. **Every T-state the CPU spends is spent through one of
//! these**, which is what makes contention placeable and a beam renderer a change of
//! [`Bus::tick`] rather than a rewrite. See
//! [`doc/boot-and-test.md`](../../../doc/boot-and-test.md) §5.
//!
//! Each primitive also records the bus state of every T-state it consumes, when
//! [`Cpu::cycle_log`] is switched on. That is what the per-opcode vectors check against:
//! not just how long an instruction took, but what the address, data and control pins were
//! doing throughout.

use super::{Bus, Cpu};

/// The control pins the tests care about, in the order they are written: `r`, `w`, `m`,
/// `i` — read, write, memory request, I/O request.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Pins(u8);

impl Pins {
    pub const NONE: Pins = Pins(0);
    pub const READ: Pins = Pins(1);
    pub const WRITE: Pins = Pins(2);
    pub const MREQ: Pins = Pins(4);
    pub const IOREQ: Pins = Pins(8);

    pub const fn or(self, other: Pins) -> Pins {
        Pins(self.0 | other.0)
    }

    pub fn has(self, other: Pins) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::fmt::Display for Pins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (bit, c) in [(1, 'r'), (2, 'w'), (4, 'm'), (8, 'i')] {
            f.write_str(if self.0 & bit != 0 {
                match c {
                    'r' => "r",
                    'w' => "w",
                    'm' => "m",
                    _ => "i",
                }
            } else {
                "-"
            })?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Pins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

/// The state of the bus for one T-state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusCycle {
    pub addr: u16,
    /// The data pins, when the CPU or the memory is driving them.
    pub data: Option<u8>,
    pub pins: Pins,
}

impl Cpu {
    #[inline]
    fn log(&mut self, addr: u16, data: Option<u8>, pins: Pins) {
        if let Some(log) = &mut self.cycle_log {
            log.push(BusCycle { addr, data, pins });
        }
    }

    /// Charge `n` T-states to the clock.
    #[inline]
    fn spend<B: Bus>(&mut self, bus: &mut B, n: u32) {
        bus.tick(n);
        self.total_t += n as u64;
    }

    /// Ask the bus how long it wants to stall this access, and stall.
    #[inline]
    fn contend<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let delay = bus.contention(addr);
        for _ in 0..delay {
            self.log(addr, None, Pins::NONE);
        }
        if delay > 0 {
            self.spend(bus, delay);
        }
    }

    /// Only the low seven bits of `R` count: bit 7 is whatever was last written to it.
    #[inline]
    fn bump_r(&mut self) {
        self.regs.r = (self.regs.r & 0x80) | (self.regs.r.wrapping_add(1) & 0x7F);
    }

    /// **M1**: opcode fetch. Four T-states, contended at `PC`, and `R` increments.
    ///
    /// The refresh address — `I` paired with `R` *before* the increment — stays on the
    /// address bus afterwards, which is where the internal T-states of instructions like
    /// `INC BC` and `ADD HL,rp` are spent.
    pub fn fetch_opcode<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let pc = self.regs.pc;
        self.contend(bus, pc);
        let v = bus.read(pc);
        let refresh = self.regs.ir();
        self.refresh = refresh;
        self.log(pc, None, Pins::NONE);
        self.log(pc, None, Pins::READ.or(Pins::MREQ));
        self.log(refresh, Some(v), Pins::NONE);
        self.log(refresh, None, Pins::NONE);
        self.spend(bus, 4);
        self.regs.pc = pc.wrapping_add(1);
        self.bump_r();
        v
    }

    /// One more T-state on the opcode fetch just made, with the refresh address still on
    /// the bus: `DJNZ`, `RET cc`, `PUSH`, `RST`, `LD A,I` and the block I/O instructions.
    pub fn extend_fetch<B: Bus>(&mut self, bus: &mut B) {
        self.log(self.refresh, None, Pins::NONE);
        self.spend(bus, 1);
    }

    /// Memory read: three T-states, contended at `addr`.
    pub fn read_byte<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        self.contend(bus, addr);
        let v = bus.read(addr);
        self.log(addr, None, Pins::NONE);
        self.log(addr, None, Pins::READ.or(Pins::MREQ));
        self.log(addr, Some(v), Pins::NONE);
        self.spend(bus, 3);
        v
    }

    /// Memory write: three T-states, contended at `addr`.
    pub fn write_byte<B: Bus>(&mut self, bus: &mut B, addr: u16, val: u8) {
        self.contend(bus, addr);
        bus.write(addr, val);
        self.log(addr, None, Pins::NONE);
        self.log(addr, Some(val), Pins::WRITE.or(Pins::MREQ));
        self.log(addr, None, Pins::NONE);
        self.spend(bus, 3);
    }

    /// Read a little-endian word: two memory reads, in address order.
    pub fn read_word<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u16 {
        let lo = self.read_byte(bus, addr);
        let hi = self.read_byte(bus, addr.wrapping_add(1));
        u16::from_le_bytes([lo, hi])
    }

    /// Write a little-endian word: low byte first.
    pub fn write_word<B: Bus>(&mut self, bus: &mut B, addr: u16, val: u16) {
        let [lo, hi] = val.to_le_bytes();
        self.write_byte(bus, addr, lo);
        self.write_byte(bus, addr.wrapping_add(1), hi);
    }

    /// Internal CPU operation: `n` T-states with `addr` on the address bus.
    ///
    /// Each is contended *individually*, because the ULA can stall the CPU at any one of
    /// them — hence `n × 1` and not `1 × n`.
    pub fn internal<B: Bus>(&mut self, bus: &mut B, addr: u16, n: u32) {
        for _ in 0..n {
            self.contend(bus, addr);
            self.log(addr, None, Pins::NONE);
            self.spend(bus, 1);
        }
    }

    /// Internal T-states with the refresh address still on the bus, as left by the last
    /// opcode fetch.
    pub fn internal_at_refresh<B: Bus>(&mut self, bus: &mut B, n: u32) {
        let addr = self.refresh;
        self.internal(bus, addr, n);
    }

    /// `IN` from a port: four T-states.
    pub fn in_port<B: Bus>(&mut self, bus: &mut B, port: u16) -> u8 {
        self.io_prologue(bus, port);
        let v = bus.in_port(port);
        self.log(port, None, Pins::READ.or(Pins::IOREQ));
        self.spend(bus, 1);
        self.log(port, Some(v), Pins::NONE);
        self.spend(bus, 1);
        v
    }

    /// `OUT` to a port: four T-states. Unlike a read, the data is on the bus in the same
    /// T-state as the request.
    pub fn out_port<B: Bus>(&mut self, bus: &mut B, port: u16, val: u8) {
        self.io_prologue(bus, port);
        bus.out_port(port, val);
        self.log(port, Some(val), Pins::WRITE.or(Pins::IOREQ));
        self.spend(bus, 1);
        self.log(port, None, Pins::NONE);
        self.spend(bus, 1);
    }

    /// The first two T-states of an I/O cycle, where the ULA's contention for an I/O
    /// access lands. The rules depend on the *port* address, not a memory address — see
    /// [`doc/spectrum-memory-map.md`](../../../doc/spectrum-memory-map.md) §Contended I/O.
    fn io_prologue<B: Bus>(&mut self, bus: &mut B, port: u16) {
        let high_contended = (0x40..=0x7f).contains(&(port >> 8));
        let ula = port & 1 == 0;
        match (high_contended, ula) {
            // N:4 — uncontended throughout.
            (false, false) => {}
            // N:1, C:3 — the ULA is asked to answer, so the wait lands after one T-state.
            (false, true) => {
                self.log(port, None, Pins::NONE);
                self.spend(bus, 1);
                self.contend(bus, port);
                self.log(port, None, Pins::NONE);
                self.spend(bus, 1);
                return;
            }
            // C:1 × 4 and C:1, C:3 — contended memory page, so every T-state may stall.
            (true, _) => {
                self.contend(bus, port);
                self.log(port, None, Pins::NONE);
                self.spend(bus, 1);
                self.contend(bus, port);
                self.log(port, None, Pins::NONE);
                self.spend(bus, 1);
                return;
            }
        }
        self.log(port, None, Pins::NONE);
        self.spend(bus, 1);
        self.log(port, None, Pins::NONE);
        self.spend(bus, 1);
    }
}
