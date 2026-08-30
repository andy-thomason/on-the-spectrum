//! §8.4 of the plan: boot the real ROM headless and assert on what it did.
//!
//! Milestones 1–9. Each one is a place the ROM passes through on the way to the BASIC
//! prompt, and each is checked on observable state rather than on a trace, so a failure
//! says *what* broke rather than *where*.

use on_the_spectrum::spectrum::{Machine, screen, ula::Ula};

const ROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/roms/48.rom");

/// Booting to the main loop takes about 5.9 million T-states, most of it the RAM test.
const BOOT_BUDGET: u64 = 12_000_000;
/// `MAIN-1`: the ROM is up and waiting for something to be typed.
const MAIN_1: u16 = 0x12A9;

fn machine() -> Machine {
    Machine::new(&std::fs::read(ROM).expect("roms/48.rom"))
}

/// Milestone 1: the reset vector runs.
#[test]
fn the_reset_vector_executes() {
    let mut m = machine();
    for _ in 0..4 {
        m.step();
    }
    assert_eq!(m.cpu.regs.pc, 0x11CB, "should have jumped to START-NEW");
    assert_eq!(
        m.cpu.regs.de(),
        0xFFFF,
        "DE points at the top of possible RAM"
    );
    assert_eq!(m.cpu.regs.a, 0x00, "A signals that this came from START");
    assert!(!m.cpu.iff1, "interrupts are disabled through the RAM test");
}

/// Milestone 2: the RAM test completes, which means writes are landing and reading back.
#[test]
fn the_ram_test_completes() {
    let mut m = machine();
    assert!(
        m.run_until_pc(0x11EF, BOOT_BUDGET),
        "never reached RAM-DONE"
    );
}

/// Milestones 3, 4 and 5: RAMTOP, the system variables, and the stream table.
#[test]
fn the_system_variables_are_initialised() {
    let mut m = machine();
    assert!(m.run_until_pc(MAIN_1, BOOT_BUDGET), "never reached MAIN-1");
    let mem = &m.bus.memory;

    // RAMTOP just below the user-defined graphics, and P-RAMT at the top of 48K.
    assert_eq!(mem.peek16(0x5CB2), 0xFF57, "RAMTOP");
    assert_eq!(mem.peek16(0x5CB4), 0xFFFF, "P-RAMT");

    assert_eq!(mem.peek(0x5C8D), 0x38, "ATTR_P: black ink on white paper");
    assert_eq!(mem.peek(0x5C8F), 0x38, "ATTR_T");
    assert_eq!(mem.peek(0x5C48), 0x38, "BORDCR");
    assert_eq!(mem.peek(0x5C09), 0x23, "REPDEL");
    assert_eq!(mem.peek(0x5C0A), 0x05, "REPPER");

    // The stream table is copied verbatim out of the ROM at L15C6.
    let streams: Vec<u8> = (0..14).map(|i| mem.peek(0x5C10 + i)).collect();
    let rom_table: Vec<u8> = (0..14).map(|i| mem.peek(0x15C6 + i)).collect();
    assert_eq!(streams, rom_table, "STRMS should match the ROM's table");
}

/// Milestone 6: `CLS` has run — the display file is clear and every cell is black on white.
#[test]
fn cls_has_cleared_the_screen() {
    let mut m = machine();
    assert!(m.run_until_pc(MAIN_1, BOOT_BUDGET), "never reached MAIN-1");

    // The copyright message is printed after CLS, so only the top 22 rows stay blank —
    // and "row" means a character row, which the display file's bit-spliced layout does
    // not store anywhere near contiguously.
    for row in 0..22 {
        for col in 0..screen::COLUMNS {
            for line in 0..8 {
                let addr = screen::pixel_address(col, row, line);
                assert_eq!(
                    m.bus.memory.peek(addr),
                    0,
                    "display file at {addr:#06X} (row {row}, col {col}) should be clear"
                );
            }
        }
    }
    let attributes: Vec<u8> = (screen::ATTRIBUTES..screen::DISPLAY_END)
        .map(|a| m.bus.memory.peek(a))
        .collect();
    assert!(
        attributes.iter().all(|&a| a == 0x38),
        "every cell should be black ink on white paper"
    );
}

/// Milestone 7: the copyright message. The classic "it lives" moment.
#[test]
fn the_copyright_message_is_on_screen() {
    let mut m = machine();
    assert!(m.run_until_pc(MAIN_1, BOOT_BUDGET), "never reached MAIN-1");
    let text = m.screen_text();
    assert!(
        text.iter()
            .any(|l| l.contains("© 1982 Sinclair Research Ltd")),
        "screen reads:\n{}",
        text.join("\n")
    );
}

/// Milestone 8: the main loop is reached, with interrupts on and the ROM's own IY.
#[test]
fn the_main_loop_is_reached() {
    let mut m = machine();
    assert!(m.run_until_pc(MAIN_1, BOOT_BUDGET), "never reached MAIN-1");
    assert!(
        m.cpu.iff1,
        "the ROM enables interrupts before the main loop"
    );
    assert_eq!(m.cpu.im, 1, "IM 1");
    assert_eq!(m.cpu.regs.iy, 0x5C3A, "IY addresses the system variables");
}

/// Milestone 9: the interrupt handler runs once per frame and counts frames as it goes.
#[test]
fn the_frame_counter_advances() {
    let mut m = machine();
    assert!(m.run_until_pc(MAIN_1, BOOT_BUDGET), "never reached MAIN-1");

    let frames = |m: &Machine| {
        let mem = &m.bus.memory;
        u32::from_le_bytes([mem.peek(0x5C78), mem.peek(0x5C79), mem.peek(0x5C7A), 0])
    };
    let before = frames(&m);
    m.run_frames(50);
    let after = frames(&m);

    // 49 or 50: `run_frames` returns the moment the fiftieth frame begins, and the
    // interrupt that starts it is not serviced until the next instruction boundary.
    assert!(
        (49..=50).contains(&(after - before)),
        "FRAMES should count one per frame ({before} -> {after})"
    );
}

/// The matrix by hand, without the typing helpers: hold one key down and watch the whole
/// loop turn — interrupt handler, key scan, editor, display.
#[test]
fn a_keypress_reaches_the_editor() {
    let mut m = machine();
    assert!(m.run_until_pc(MAIN_1, BOOT_BUDGET), "never reached MAIN-1");
    m.run_frames(10);

    // "1" is half-row 3, column 0. The ROM scans on the interrupt, so hold it a while.
    m.bus.keyboard.set_named("1", true);
    m.run_frames(5);
    m.bus.keyboard.release_all();
    m.run_frames(10);

    let text = m.screen_text();
    assert!(
        text.iter().any(|l| l.contains('1')),
        "typing 1 should put a 1 on the screen:\n{}",
        text.join("\n")
    );
}

// ------------------------------------------------------------------------ stage E

/// Boot to the prompt, ready to type at.
fn booted() -> Machine {
    let mut m = machine();
    assert!(m.run_until_pc(MAIN_1, BOOT_BUDGET), "never reached MAIN-1");
    m
}

/// Milestone 10: the cursor. It appears as soon as there is a line to edit — as the plain
/// character `K`, in a cell the ROM marks to FLASH. The inversion people remember is the
/// ULA's doing, not the display file's: the pixels never change.
#[test]
fn the_cursor_is_a_flashing_k() {
    let mut m = booted();
    m.type_text("2");

    let cursor = (0..screen::ROWS)
        .flat_map(|row| (0..screen::COLUMNS).map(move |col| (col, row)))
        .find(|&(col, row)| screen::cell_char(&m.bus.memory, col, row) == Some(('K', false)));
    let Some((col, row)) = cursor else {
        panic!("no K cursor on screen:\n{}", m.screen_text().join("\n"));
    };

    let attribute = m.bus.memory.peek(screen::attribute_address(col, row));
    assert_eq!(
        attribute & 0x80,
        0x80,
        "the cursor cell should be set to FLASH"
    );

    // The ULA does the flashing, sixteen frames at a time; the display file never changes.
    assert!(!Ula::flash_inverted(0));
    assert!(Ula::flash_inverted(16));
    assert!(!Ula::flash_inverted(32));
}

/// Milestone 11: keys reach the edit line, and the ROM keeps the line as it goes.
#[test]
fn typing_reaches_the_edit_line() {
    let mut m = booted();
    assert!(m.type_text("12345"));

    assert_eq!(m.edit_line(), b"12345", "E_LINE should hold what was typed");
    let text = m.screen_text();
    assert!(
        text.iter().any(|l| l.starts_with("12345")),
        "screen reads:\n{}",
        text.join("\n")
    );
}

/// Milestone 12: BASIC executes. **This is "the emulator works".**
#[test]
fn basic_runs_print_2_plus_2() {
    let mut m = booted();

    // At the K prompt the P key is the whole PRINT keyword — which is how anyone types
    // this on a real machine — so the edit line holds one token and three characters.
    assert!(m.type_text("P2+2"));
    assert_eq!(
        m.edit_line(),
        [0xF5, b'2', b'+', b'2'],
        "E_LINE should hold the PRINT token and the expression"
    );

    assert!(m.type_text("\n"));
    m.run_frames(10);

    let text = m.screen_text();
    assert_eq!(
        text[0].trim_end(),
        "4",
        "screen reads:\n{}",
        text.join("\n")
    );
    assert!(
        text.iter().any(|l| l.starts_with("0 OK, 0:1")),
        "BASIC should report success:\n{}",
        text.join("\n")
    );
}
