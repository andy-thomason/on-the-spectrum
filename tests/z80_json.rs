//! §8.2 of the plan: the [SingleStepTests/z80](https://github.com/SingleStepTests/z80)
//! per-opcode vectors — about 1000 cases for each of 1604 instruction sequences.
//!
//! Each case gives the complete initial state, the complete final state, and **the bus
//! activity of every T-state in between**. All three are checked here, because the point of
//! spending T-states through machine-cycle primitives is that the cycles come out in the
//! right order as well as in the right number: a per-instruction cycle table would pass the
//! register comparison and fail this one.
//!
//! The vectors are 1.3 GB expanded, so they are not in the repository. Run
//! `tests/vectors/fetch.sh` to get them; without them this test skips.
//!
//! It is worth running these in release mode — `cargo test --release --test z80_json` —
//! since the debug build spends most of its time in `serde_json`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use on_the_spectrum::z80::{Bus, BusCycle, Cpu};
use serde_json::Value;

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/z80/v1");

/// 64K of RAM and a port queue: everything the CPU can see, and nothing else.
#[derive(Default)]
struct TestBus {
    ram: Vec<u8>,
    /// Values the vector says the ports will return, in order.
    port_reads: VecDeque<u8>,
    /// What actually happened, to be compared with the vector's `ports` array.
    port_log: Vec<(u16, u8, char)>,
}

impl Bus for TestBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }
    fn write(&mut self, addr: u16, val: u8) {
        self.ram[addr as usize] = val;
    }
    fn in_port(&mut self, port: u16) -> u8 {
        let v = self.port_reads.pop_front().unwrap_or(0xFF);
        self.port_log.push((port, v, 'r'));
        v
    }
    fn out_port(&mut self, port: u16, val: u8) {
        self.port_log.push((port, val, 'w'));
    }
    fn tick(&mut self, _cycles: u32) {}
}

fn byte(state: &Value, key: &str) -> u8 {
    state[key].as_u64().unwrap_or(0) as u8
}

fn word(state: &Value, key: &str) -> u16 {
    state[key].as_u64().unwrap_or(0) as u16
}

fn load(cpu: &mut Cpu, bus: &mut TestBus, initial: &Value) {
    let r = &mut cpu.regs;
    r.a = byte(initial, "a");
    r.f = byte(initial, "f");
    r.b = byte(initial, "b");
    r.c = byte(initial, "c");
    r.d = byte(initial, "d");
    r.e = byte(initial, "e");
    r.h = byte(initial, "h");
    r.l = byte(initial, "l");
    r.af_ = word(initial, "af_");
    r.bc_ = word(initial, "bc_");
    r.de_ = word(initial, "de_");
    r.hl_ = word(initial, "hl_");
    r.ix = word(initial, "ix");
    r.iy = word(initial, "iy");
    r.sp = word(initial, "sp");
    r.pc = word(initial, "pc");
    r.i = byte(initial, "i");
    r.r = byte(initial, "r");
    r.wz = word(initial, "wz");
    cpu.iff1 = byte(initial, "iff1") != 0;
    cpu.iff2 = byte(initial, "iff2") != 0;
    cpu.im = byte(initial, "im");
    cpu.ei_pending = byte(initial, "ei") != 0;
    cpu.q = byte(initial, "q");
    cpu.p = byte(initial, "p") != 0;
    cpu.halted = false;
    cpu.int_pending = false;
    cpu.total_t = 0;

    bus.ram.clear();
    bus.ram.resize(0x10000, 0);
    for cell in initial["ram"].as_array().unwrap() {
        let addr = cell[0].as_u64().unwrap() as usize;
        bus.ram[addr] = cell[1].as_u64().unwrap() as u8;
    }
    bus.port_reads.clear();
    bus.port_log.clear();
}

/// Compare final state, memory, ports and per-T-state bus activity. Returns every
/// difference found, so one failing case reports everything that is wrong with it rather
/// than only the first thing.
fn compare(cpu: &Cpu, bus: &TestBus, test: &Value) -> Vec<String> {
    let mut bad = Vec::new();
    let f = &test["final"];
    let r = &cpu.regs;

    let mut check = |name: &str, got: u64, want: u64| {
        if got != want {
            bad.push(format!("{name}: got {got:#X}, want {want:#X}"));
        }
    };
    check("a", r.a as u64, byte(f, "a") as u64);
    check("f", r.f as u64, byte(f, "f") as u64);
    check("b", r.b as u64, byte(f, "b") as u64);
    check("c", r.c as u64, byte(f, "c") as u64);
    check("d", r.d as u64, byte(f, "d") as u64);
    check("e", r.e as u64, byte(f, "e") as u64);
    check("h", r.h as u64, byte(f, "h") as u64);
    check("l", r.l as u64, byte(f, "l") as u64);
    check("af_", r.af_ as u64, word(f, "af_") as u64);
    check("bc_", r.bc_ as u64, word(f, "bc_") as u64);
    check("de_", r.de_ as u64, word(f, "de_") as u64);
    check("hl_", r.hl_ as u64, word(f, "hl_") as u64);
    check("ix", r.ix as u64, word(f, "ix") as u64);
    check("iy", r.iy as u64, word(f, "iy") as u64);
    check("sp", r.sp as u64, word(f, "sp") as u64);
    check("pc", r.pc as u64, word(f, "pc") as u64);
    check("i", r.i as u64, byte(f, "i") as u64);
    check("r", r.r as u64, byte(f, "r") as u64);
    check("wz", r.wz as u64, word(f, "wz") as u64);
    check("iff1", cpu.iff1 as u64, byte(f, "iff1") as u64);
    check("iff2", cpu.iff2 as u64, byte(f, "iff2") as u64);
    check("im", cpu.im as u64, byte(f, "im") as u64);
    check("ei", cpu.ei_pending as u64, byte(f, "ei") as u64);
    check("q", cpu.q as u64, byte(f, "q") as u64);
    check("p", cpu.p as u64, byte(f, "p") as u64);

    for cell in f["ram"].as_array().unwrap() {
        let addr = cell[0].as_u64().unwrap() as usize;
        let want = cell[1].as_u64().unwrap() as u8;
        if bus.ram[addr] != want {
            bad.push(format!(
                "ram[{addr:#06X}]: got {:#04X}, want {want:#04X}",
                bus.ram[addr]
            ));
        }
    }

    let want_ports: Vec<(u16, u8, char)> = test["ports"]
        .as_array()
        .map(|ports| {
            ports
                .iter()
                .map(|p| {
                    (
                        p[0].as_u64().unwrap() as u16,
                        p[1].as_u64().unwrap() as u8,
                        p[2].as_str().unwrap().chars().next().unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if bus.port_log != want_ports {
        bad.push(format!(
            "ports: got {:X?}, want {want_ports:X?}",
            bus.port_log
        ));
    }

    let log = cpu.cycle_log.as_deref().unwrap_or(&[]);
    let want_cycles = test["cycles"].as_array().unwrap();
    if log.len() != want_cycles.len() {
        bad.push(format!(
            "{} T-states, want {} ({})",
            log.len(),
            want_cycles.len(),
            show_cycles(log)
        ));
    } else {
        for (i, (got, want)) in log.iter().zip(want_cycles).enumerate() {
            if !cycle_matches(got, want) {
                bad.push(format!(
                    "T{}: got [{:#06X}, {}, {}], want {want}\n     ours: {}",
                    i + 1,
                    got.addr,
                    match got.data {
                        Some(d) => format!("{d}"),
                        None => "null".into(),
                    },
                    got.pins,
                    show_cycles(log)
                ));
                break;
            }
        }
    }
    bad
}

fn cycle_matches(got: &BusCycle, want: &Value) -> bool {
    if want[0].as_u64().is_some_and(|addr| addr as u16 != got.addr) {
        return false;
    }
    let want_data = want[1].as_u64().map(|d| d as u8);
    if want_data != got.data {
        return false;
    }
    want[2].as_str() == Some(&got.pins.to_string())
}

fn show_cycles(log: &[BusCycle]) -> String {
    log.iter()
        .map(|c| {
            format!(
                "{:04X}/{}/{}",
                c.addr,
                match c.data {
                    Some(d) => format!("{d:02X}"),
                    None => "--".into(),
                },
                c.pins
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct Failure {
    file: String,
    name: String,
    problems: Vec<String>,
}

fn run_file(path: &Path) -> (usize, Vec<Failure>) {
    let text = std::fs::read_to_string(path).expect("read vector file");
    let tests: Value = serde_json::from_str(&text).expect("parse vector file");
    let file = path.file_stem().unwrap().to_string_lossy().to_string();

    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    let mut failures = Vec::new();
    let mut count = 0;

    let limit = std::env::var("Z80_VECTORS_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);

    for test in tests.as_array().unwrap().iter().take(limit) {
        count += 1;
        load(&mut cpu, &mut bus, &test["initial"]);
        // Prime the ports with what the vector says the outside world will answer.
        for p in test["ports"].as_array().into_iter().flatten() {
            if p[2].as_str() == Some("r") {
                bus.port_reads.push_back(p[1].as_u64().unwrap() as u8);
            }
        }
        cpu.cycle_log = Some(Vec::with_capacity(32));

        cpu.step(&mut bus);

        let problems = compare(&cpu, &bus, test);
        if !problems.is_empty() {
            failures.push(Failure {
                file: file.clone(),
                name: test["name"].as_str().unwrap_or("?").to_string(),
                problems,
            });
        }
    }
    (count, failures)
}

#[test]
fn single_step_tests() {
    let dir = PathBuf::from(VECTORS);
    if !dir.is_dir() {
        eprintln!(
            "skipping: no vectors at {VECTORS}\n\
             run tests/vectors/fetch.sh to download them (about 280 MB)"
        );
        return;
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read vector directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(files.len() > 1500, "only {} vector files", files.len());

    // Iterating on one instruction is much quicker than sweeping all 1604:
    //     Z80_VECTORS_FILTER="dd cb" Z80_VECTORS_CASES=50 cargo test --test z80_json
    if let Ok(filter) = std::env::var("Z80_VECTORS_FILTER") {
        files.retain(|p| {
            p.file_stem()
                .unwrap()
                .to_string_lossy()
                .starts_with(&filter)
        });
        eprintln!("filtered to {} files matching {filter:?}", files.len());
    }

    let next = std::sync::atomic::AtomicUsize::new(0);
    let totals = Mutex::new((0usize, Vec::<Failure>::new()));
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let (count, failures) = run_file(path);
                    let mut totals = totals.lock().unwrap();
                    totals.0 += count;
                    // Keep the report bounded: enough to debug, not a wall of text.
                    if totals.1.len() < 40 {
                        totals.1.extend(failures.into_iter().take(3));
                    } else if !failures.is_empty() {
                        totals.1.push(Failure {
                            file: failures[0].file.clone(),
                            name: String::new(),
                            problems: vec![format!("+{} more failing cases", failures.len())],
                        });
                    }
                }
            });
        }
    });

    let (count, failures) = totals.into_inner().unwrap();
    if !failures.is_empty() {
        let mut report = String::new();
        for f in failures.iter().take(40) {
            report.push_str(&format!("\n{} [{}]\n", f.file, f.name));
            for p in f.problems.iter().take(8) {
                report.push_str(&format!("    {p}\n"));
            }
        }
        panic!(
            "{} of {count} cases failed (showing the first {}):\n{report}",
            failures.len(),
            failures.len().min(40)
        );
    }
    eprintln!("{count} cases passed across {} opcodes", files.len());
}
