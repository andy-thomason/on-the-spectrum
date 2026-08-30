//! The T-state column of the generated tables in `doc/z80-instruction-set.md`, used as the
//! oracle the plan says it is.
//!
//! Nothing in the interpreter looks up an instruction's length in T-states: time is spent
//! through the machine-cycle primitives, one cycle at a time, and the total is whatever
//! falls out. This test checks that what falls out is what the spec says, for every opcode
//! in every prefix bank — including both arms of every conditional.
//!
//! Unlike `z80_json.rs` this needs no downloaded vectors, so it still guards the timing
//! model on a fresh clone.

use on_the_spectrum::z80::{Bus, Cpu};

mod fixture {
    include!("fixtures/opcodes.rs");
}

/// Flat RAM, no contention, ports that read as an unconnected bus.
struct FlatBus {
    ram: Vec<u8>,
}

impl Bus for FlatBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }
    fn write(&mut self, addr: u16, val: u8) {
        self.ram[addr as usize] = val;
    }
    fn in_port(&mut self, _port: u16) -> u8 {
        0xFF
    }
    fn out_port(&mut self, _port: u16, _val: u8) {}
    fn tick(&mut self, _cycles: u32) {}
}

const ORG: u16 = 0x0100;

/// Two starting states, between them exercising both arms of everything conditional:
/// every flag set against every flag clear, `B` counting down to zero against `B` still
/// going, and `BC` exhausted against `BC` with more to do.
const SETUPS: [(u8, u16); 2] = [(0xFF, 0x0001), (0x00, 0x0100)];

fn time_of(bytes: &[u8]) -> Vec<u32> {
    SETUPS
        .iter()
        .map(|&(f, bc)| {
            let mut bus = FlatBus {
                ram: vec![0; 0x10000],
            };
            bus.ram[ORG as usize..ORG as usize + bytes.len()].copy_from_slice(bytes);

            let mut cpu = Cpu::new();
            cpu.regs.pc = ORG;
            cpu.regs.f = f;
            cpu.regs.set_bc(bc);
            // A differs from what CPI and friends will find in memory, so the compare-and-
            // repeat instructions do repeat when BC allows.
            cpu.regs.a = 0x55;
            cpu.regs.set_hl(0x8000);
            cpu.regs.set_de(0x9000);
            cpu.regs.ix = 0x8000;
            cpu.regs.iy = 0x8000;
            cpu.regs.sp = 0xC000;
            cpu.step(&mut bus)
        })
        .collect()
}

/// `"13/8"` → `{13, 8}`; `"4"` → `{4}`.
fn expected(tstates: &str) -> Vec<u32> {
    tstates.split('/').map(|t| t.parse().unwrap()).collect()
}

/// The two runs may produce the taken and not-taken times in either order, and an
/// unconditional instruction produces the same time twice.
fn agrees(got: &[u32], want: &[u32]) -> bool {
    let mut got = got.to_vec();
    let mut want = want.to_vec();
    got.sort_unstable();
    got.dedup();
    want.sort_unstable();
    want.dedup();
    got == want
}

fn check(bytes: &[u8], tstates: &str, what: &str) {
    let got = time_of(bytes);
    let want = expected(tstates);
    assert!(
        agrees(&got, &want),
        "{what}: took {got:?} T-states, spec says {tstates}"
    );
}

const N1: u8 = 0x34;
const N2: u8 = 0x12;

#[test]
fn unprefixed_timings_match_the_spec() {
    for op in 0..=255u8 {
        if matches!(op, 0xCB | 0xDD | 0xED | 0xFD) {
            continue;
        }
        let row = fixture::UNPREFIXED[op as usize].unwrap();
        check(&[op, N1, N2], row.tstates, &format!("{op:02X}"));
    }
}

#[test]
fn cb_timings_match_the_spec() {
    for op in 0..=255u8 {
        let row = fixture::CB[op as usize].unwrap();
        check(&[0xCB, op], row.tstates, &format!("CB {op:02X}"));
    }
}

#[test]
fn ed_timings_match_the_spec() {
    for op in 0..=255u8 {
        let row = fixture::ED[op as usize].unwrap();
        check(&[0xED, op, N1, N2], row.tstates, &format!("ED {op:02X}"));
    }
}

#[test]
fn dd_and_fd_timings_match_the_spec() {
    for prefix in [0xDDu8, 0xFD] {
        for op in 0..=255u8 {
            let what = format!("{prefix:02X} {op:02X}");
            match fixture::DD[op as usize] {
                Some(row) => check(&[prefix, op, N1, N2], row.tstates, &what),
                None => {
                    if matches!(op, 0xCB | 0xDD | 0xED | 0xFD) {
                        continue;
                    }
                    // "behaves as the unprefixed opcode + 4 T", says the spec.
                    let bare = time_of(&[op, N1, N2]);
                    let prefixed = time_of(&[prefix, op, N1, N2]);
                    let want: Vec<u32> = bare.iter().map(|t| t + 4).collect();
                    assert!(
                        agrees(&prefixed, &want),
                        "{what}: took {prefixed:?} T-states, want {want:?} \
                         (the unprefixed {op:02X} plus four)"
                    );
                }
            }
        }
    }
}

#[test]
fn ddcb_and_fdcb_timings_match_the_spec() {
    for prefix in [0xDDu8, 0xFD] {
        for op in 0..=255u8 {
            let row = fixture::DDCB[op as usize].unwrap();
            check(
                &[prefix, 0xCB, 0x05, op],
                row.tstates,
                &format!("{prefix:02X} CB d {op:02X}"),
            );
        }
    }
}

/// A few whole-instruction checks that read as assembly rather than as opcodes, to catch a
/// wiring mistake that a per-opcode sweep would not: a `Bus` that never sees a write, a
/// `PC` that does not advance, an interrupt that cannot be taken.
#[test]
fn a_short_program_runs() {
    let mut bus = FlatBus {
        ram: vec![0; 0x10000],
    };
    let program = [
        0x21, 0x00, 0x90, // LD HL,$9000
        0x3E, 0x2A, //       LD A,$2A
        0x77, //             LD (HL),A
        0x23, //             INC HL
        0x35, //             DEC (HL)
        0xFB, //             EI
        0x76, //             HALT
    ];
    bus.ram[ORG as usize..ORG as usize + program.len()].copy_from_slice(&program);

    let mut cpu = Cpu::new();
    cpu.regs.pc = ORG;
    let mut total = 0;
    for _ in 0..6 {
        total += cpu.step(&mut bus);
    }
    assert_eq!(bus.ram[0x9000], 0x2A);
    assert_eq!(bus.ram[0x9001], 0xFF, "DEC (HL) should wrap 0 to FF");
    assert_eq!(cpu.regs.hl(), 0x9001);
    assert_eq!(total, 10 + 7 + 7 + 6 + 11 + 4);

    // EI leaves a one-instruction shadow, so the HALT cannot be interrupted...
    cpu.int_pending = true;
    let halt = cpu.step(&mut bus);
    assert_eq!(halt, 4);
    assert!(cpu.halted);
    assert_eq!(cpu.regs.pc, ORG + program.len() as u16);

    // ...but the next step is fair game: IM 1 pushes PC and vectors through $0038.
    let taken = cpu.step(&mut bus);
    assert_eq!(taken, 13, "IM 1 acknowledge takes 13 T-states");
    assert!(!cpu.halted);
    assert!(!cpu.iff1);
    assert_eq!(cpu.regs.pc, 0x0038);
    assert_eq!(cpu.regs.sp, 0xFFFE);
    assert_eq!(
        u16::from_le_bytes([bus.ram[0xFFFE], bus.ram[0xFFFF]]),
        ORG + program.len() as u16,
        "the return address is the instruction after the HALT"
    );
}

/// `R` counts opcode fetches, seven bits wide, and the eighth is whatever was put there.
#[test]
fn the_refresh_register_counts_m1_cycles_only() {
    let cases: &[(&[u8], u8)] = &[
        (&[0x00], 1),                   // NOP
        (&[0x01, 0x34, 0x12], 1),       // LD BC,nn — the operands are not M1 cycles
        (&[0xCB, 0x00], 2),             // RLC B
        (&[0xED, 0x44], 2),             // NEG
        (&[0xDD, 0x23], 2),             // INC IX
        (&[0xDD, 0xCB, 0x05, 0x06], 2), // RLC (IX+d) — the CB here is not an M1
    ];
    for &(bytes, bump) in cases {
        let mut bus = FlatBus {
            ram: vec![0; 0x10000],
        };
        bus.ram[ORG as usize..ORG as usize + bytes.len()].copy_from_slice(bytes);
        let mut cpu = Cpu::new();
        cpu.regs.pc = ORG;
        cpu.regs.r = 0xFE;
        cpu.step(&mut bus);
        // 0xFE + bump, with bit 7 held: 0xFE -> 0xFF -> 0x80.
        let want = 0x80 | (0xFE_u8.wrapping_add(bump) & 0x7F);
        assert_eq!(cpu.regs.r, want, "{bytes:02X?}: R should advance by {bump}");
    }
}
