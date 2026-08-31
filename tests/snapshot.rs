//! `.sna` snapshots.
//!
//! A round trip on its own would pass just as happily with the header fields in the wrong
//! order, so the first test here reads a hand-built snapshot with a different value in
//! every field. The round trip then covers everything a field-by-field test cannot: all
//! 48K of RAM, the border, and the screen that comes out of it.

use on_the_spectrum::spectrum::snapshot::{self, Error, SNA_48K_LEN};
use on_the_spectrum::spectrum::{Machine, screen};

const ROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/roms/48.rom");
const MAIN_1: u16 = 0x12A9;

fn machine() -> Machine {
    Machine::new(&std::fs::read(ROM).expect("roms/48.rom"))
}

fn booted() -> Machine {
    let mut m = machine();
    assert!(m.run_until_pc(MAIN_1, 12_000_000), "never reached MAIN-1");
    m
}

/// Every field, at its documented offset, with a value that could not be mistaken for any
/// other field's.
#[test]
fn the_header_fields_are_where_the_format_says() {
    let mut sna = vec![0u8; SNA_48K_LEN];
    let mut put = |offset: usize, value: u16| {
        sna[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    };
    put(1, 0x1122); // HL'
    put(3, 0x3344); // DE'
    put(5, 0x5566); // BC'
    put(7, 0x7788); // AF'
    put(9, 0x99AA); // HL
    put(11, 0xBBCC); // DE
    put(13, 0xDDEE); // BC
    put(15, 0x0F1E); // IY
    put(17, 0x2D3C); // IX
    put(21, 0x4B5A); // AF
    put(23, 0x8000); // SP
    sna[0] = 0x3F; // I
    sna[19] = 0x04; // IFF2 set
    sna[20] = 0x69; // R
    sna[25] = 2; // IM 2
    sna[26] = 5; // cyan border

    // PC waits on the stack at SP.
    let sp_index = 27 + (0x8000 - 0x4000);
    sna[sp_index] = 0xCD;
    sna[sp_index + 1] = 0xAB;

    let mut m = machine();
    snapshot::load_sna(&mut m, &sna).unwrap();
    let r = &m.cpu.regs;

    assert_eq!(r.i, 0x3F, "I");
    assert_eq!(r.hl_, 0x1122, "HL'");
    assert_eq!(r.de_, 0x3344, "DE'");
    assert_eq!(r.bc_, 0x5566, "BC'");
    assert_eq!(r.af_, 0x7788, "AF'");
    assert_eq!(r.hl(), 0x99AA, "HL");
    assert_eq!(r.de(), 0xBBCC, "DE");
    assert_eq!(r.bc(), 0xDDEE, "BC");
    assert_eq!(r.iy, 0x0F1E, "IY");
    assert_eq!(r.ix, 0x2D3C, "IX");
    assert_eq!(r.af(), 0x4B5A, "AF");
    assert_eq!(r.r, 0x69, "R");
    assert_eq!(m.cpu.im, 2, "interrupt mode");
    assert_eq!(m.bus.ula.border, 5, "border");

    // The RETN: IFF2 into IFF1, then PC off the stack and SP up by two.
    assert!(m.cpu.iff2 && m.cpu.iff1, "IFF1 should follow IFF2");
    assert_eq!(r.pc, 0xABCD, "PC comes off the stack");
    assert_eq!(r.sp, 0x8002, "SP past the popped PC");
}

#[test]
fn interrupts_disabled_stay_disabled() {
    let mut sna = vec![0u8; SNA_48K_LEN];
    sna[23..25].copy_from_slice(&0x8000u16.to_le_bytes());
    sna[19] = 0x00;

    let mut m = machine();
    snapshot::load_sna(&mut m, &sna).unwrap();
    assert!(!m.cpu.iff1 && !m.cpu.iff2);
}

#[test]
fn a_round_trip_preserves_the_whole_machine() {
    let mut original = booted();
    original.type_text("P2+2");
    original.run_frames(5);

    let sna = snapshot::save_sna(&original).expect("save");
    assert_eq!(sna.len(), SNA_48K_LEN);

    let mut restored = machine();
    snapshot::load_sna(&mut restored, &sna).expect("load");

    let (a, b) = (&original.cpu, &restored.cpu);
    assert_eq!(a.regs.pc, b.regs.pc, "PC");
    assert_eq!(a.regs.sp, b.regs.sp, "SP");
    assert_eq!(a.regs.af(), b.regs.af(), "AF");
    assert_eq!(a.regs.bc(), b.regs.bc(), "BC");
    assert_eq!(a.regs.de(), b.regs.de(), "DE");
    assert_eq!(a.regs.hl(), b.regs.hl(), "HL");
    assert_eq!(
        (a.regs.af_, a.regs.bc_),
        (b.regs.af_, b.regs.bc_),
        "shadows"
    );
    assert_eq!(
        (a.regs.de_, a.regs.hl_),
        (b.regs.de_, b.regs.hl_),
        "shadows"
    );
    assert_eq!((a.regs.ix, a.regs.iy), (b.regs.ix, b.regs.iy), "index");
    assert_eq!((a.regs.i, a.regs.r), (b.regs.i, b.regs.r), "I and R");
    assert_eq!((a.iff1, a.iff2, a.im), (b.iff1, b.iff2, b.im), "interrupts");
    assert_eq!(original.bus.ula.border, restored.bus.ula.border, "border");

    // All of RAM matches except the two bytes below SP, where the format put PC so a
    // RETN could find it. They are free stack, and the restored machine has already
    // popped them — but the bytes are still there, and they are the PC.
    let pushed = original.cpu.regs.sp.wrapping_sub(2);
    for addr in 0x4000..=0xFFFFu32 {
        let addr = addr as u16;
        if addr == pushed || addr == pushed.wrapping_add(1) {
            continue;
        }
        assert_eq!(
            original.bus.memory.peek(addr),
            restored.bus.memory.peek(addr),
            "RAM at {addr:#06X}"
        );
    }
    assert_eq!(
        restored.bus.memory.peek16(pushed),
        original.cpu.regs.pc,
        "the two bytes below SP hold the pushed PC"
    );
    assert_eq!(original.screen_text(), restored.screen_text(), "the screen");
}

/// The point of a snapshot: it carries on from where it was.
#[test]
fn a_restored_machine_carries_on_running() {
    let mut original = booted();
    original.type_text("P2+2");
    let sna = snapshot::save_sna(&original).expect("save");

    let mut restored = machine();
    snapshot::load_sna(&mut restored, &sna).expect("load");

    // Both machines press ENTER; both should print 4.
    original.type_text("\n");
    restored.type_text("\n");
    original.run_frames(10);
    restored.run_frames(10);

    assert_eq!(original.screen_text()[0].trim_end(), "4");
    assert_eq!(
        restored.screen_text()[0].trim_end(),
        "4",
        "the restored machine should finish the sum:\n{}",
        restored.screen_text().join("\n")
    );
}

#[test]
fn saving_does_not_disturb_the_machine_being_saved() {
    let mut m = booted();
    let before_sp = m.cpu.regs.sp;
    let before_screen = m.screen_text();
    let before_stack = m.bus.memory.peek16(before_sp.wrapping_sub(2));

    let _ = snapshot::save_sna(&m).expect("save");

    assert_eq!(m.cpu.regs.sp, before_sp, "SP");
    assert_eq!(
        m.bus.memory.peek16(before_sp.wrapping_sub(2)),
        before_stack,
        "PC was pushed into the image, not into the machine"
    );
    assert_eq!(m.screen_text(), before_screen);
    m.run_frames(2);
}

#[test]
fn the_wrong_length_is_refused() {
    let mut m = machine();
    assert_eq!(snapshot::load_sna(&mut m, &[0; 10]), Err(Error::Length(10)));
    assert_eq!(
        snapshot::load_sna(&mut m, &vec![0; SNA_48K_LEN + 1]),
        Err(Error::Length(SNA_48K_LEN + 1))
    );
}

#[test]
fn a_stack_in_rom_cannot_be_saved() {
    let mut m = booted();
    m.cpu.regs.sp = 0x4001; // pushing would write to $3FFF
    assert_eq!(snapshot::save_sna(&m), Err(Error::StackInRom(0x4001)));

    m.cpu.regs.sp = 0x0001; // and the high byte would wrap into ROM
    assert_eq!(snapshot::save_sna(&m), Err(Error::StackInRom(0x0001)));

    m.cpu.regs.sp = 0x4002; // just enough room
    assert!(snapshot::save_sna(&m).is_ok());
}

/// A snapshot holds the screen, so the renderer sees it too.
#[test]
fn the_screen_survives_a_snapshot() {
    let original = booted();
    let sna = snapshot::save_sna(&original).expect("save");
    let mut restored = machine();
    snapshot::load_sna(&mut restored, &sna).expect("load");

    let mut a = vec![0; screen::FRAME_BYTES];
    let mut b = vec![0; screen::FRAME_BYTES];
    original.render_into(&mut a);
    restored.render_into(&mut b);
    assert!(a == b, "the rendered frame should be identical");
    assert!(
        restored
            .screen_text()
            .iter()
            .any(|l| l.contains("© 1982 Sinclair Research Ltd")),
        "restored screen:\n{}",
        restored.screen_text().join("\n")
    );
}

// -------------------------------------------------------------------------------- .z80

/// A version 1 header with a different value in every field, so a transposition shows up
/// as a wrong register rather than as nothing at all.
fn z80_v1_header(compressed: bool) -> Vec<u8> {
    let mut h = vec![0u8; 30];
    let put = |h: &mut Vec<u8>, offset: usize, value: u16| {
        h[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    };
    h[0] = 0x12; // A
    h[1] = 0x34; // F
    put(&mut h, 2, 0x5678); // BC
    put(&mut h, 4, 0x9ABC); // HL
    put(&mut h, 6, 0x4321); // PC — non-zero, so this is a version 1 file
    put(&mut h, 8, 0x8765); // SP
    h[10] = 0x3F; // I
    h[11] = 0x69; // R, low seven bits
    // bit 0 is R's top bit, bits 1-3 the border, bit 5 compression
    h[12] = 0x01 | (5 << 1) | if compressed { 0x20 } else { 0x00 };
    put(&mut h, 13, 0xDEF0); // DE
    put(&mut h, 15, 0x0102); // BC'
    put(&mut h, 17, 0x0304); // DE'
    put(&mut h, 19, 0x0506); // HL'
    h[21] = 0x07; // A'
    h[22] = 0x08; // F'
    put(&mut h, 23, 0x1357); // IY
    put(&mut h, 25, 0x2468); // IX
    h[27] = 1; // IFF1
    h[28] = 0; // IFF2
    h[29] = 2; // IM 2
    h
}

fn assert_v1_registers(m: &Machine) {
    let r = &m.cpu.regs;
    assert_eq!(r.af(), 0x1234, "AF");
    assert_eq!(r.bc(), 0x5678, "BC");
    assert_eq!(r.hl(), 0x9ABC, "HL");
    assert_eq!(r.pc, 0x4321, "PC");
    assert_eq!(r.sp, 0x8765, "SP");
    assert_eq!(r.i, 0x3F, "I");
    assert_eq!(r.r, 0xE9, "R, with bit 7 out of the flags byte");
    assert_eq!(r.de(), 0xDEF0, "DE");
    assert_eq!(r.bc_, 0x0102, "BC'");
    assert_eq!(r.de_, 0x0304, "DE'");
    assert_eq!(r.hl_, 0x0506, "HL'");
    assert_eq!(r.af_, 0x0708, "AF'");
    assert_eq!(r.iy, 0x1357, "IY");
    assert_eq!(r.ix, 0x2468, "IX");
    assert!(m.cpu.iff1 && !m.cpu.iff2, "IFF1 set, IFF2 clear");
    assert_eq!(m.cpu.im, 2, "interrupt mode");
    assert_eq!(m.bus.ula.border, 5, "border");
}

/// The `.z80` run-length encoding, written out here so the loader is tested against an
/// encoder rather than against itself.
///
/// Runs of five or more are encoded, and runs of `ED` from two upwards. The rule that is
/// easy to miss — and that this got wrong first time — is that **a literal `ED` must
/// always be followed by a literal byte**. Otherwise the `ED` sits next to the `ED ED` of
/// the following run and no decoder can tell which of the three EDs begins the marker.
/// Since a run of two or more EDs is always encoded, the byte after a literal `ED` is
/// never another `ED`, so copying it out literally is always safe.
fn z80_compress(ram: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < ram.len() {
        let byte = ram[at];
        let mut run = 1;
        while at + run < ram.len() && ram[at + run] == byte && run < 255 {
            run += 1;
        }
        if run >= 5 || (byte == 0xED && run >= 2) {
            out.extend_from_slice(&[0xED, 0xED, run as u8, byte]);
            at += run;
        } else {
            // `byte` differs from the one after it, or is a run of two to four ordinary
            // bytes: either way, literals.
            out.push(byte);
            at += 1;
            if byte == 0xED && at < ram.len() {
                out.push(ram[at]);
                at += 1;
            }
        }
    }
    out
}

#[test]
fn the_z80_v1_header_fields_are_where_the_format_says() {
    let mut file = z80_v1_header(false);
    let ram: Vec<u8> = (0..49152).map(|i| (i % 251) as u8).collect();
    file.extend_from_slice(&ram);

    let mut m = machine();
    snapshot::load_z80(&mut m, &file).unwrap();
    assert_v1_registers(&m);
    for addr in [0x4000u16, 0x4001, 0x8000, 0xC000, 0xFFFF] {
        let expected = ram[(addr - 0x4000) as usize];
        assert_eq!(m.bus.memory.peek(addr), expected, "RAM at {addr:#06X}");
    }
}

#[test]
fn a_compressed_v1_file_decompresses() {
    // Every case the encoding has: a short run left alone, a run worth encoding, a lone
    // ED, a pair of EDs that must be encoded, and a run longer than one count can hold.
    let mut ram = vec![0u8; 49152];
    ram[0..4].copy_from_slice(&[1, 1, 1, 1]);
    ram[4..12].copy_from_slice(&[0xAA; 8]);
    ram[12] = 0xED;
    ram[13] = 0x01;
    ram[14] = 0xED;
    ram[15] = 0xED;
    ram[16] = 0x02;
    for byte in ram[17..317].iter_mut() {
        *byte = 0xFF;
    }
    ram[49151] = 0x5A;

    let mut file = z80_v1_header(true);
    file.extend_from_slice(&z80_compress(&ram));
    file.extend_from_slice(&[0x00, 0xED, 0xED, 0x00]); // the version 1 end marker

    let mut m = machine();
    snapshot::load_z80(&mut m, &file).unwrap();
    assert_v1_registers(&m);
    for (i, &expected) in ram.iter().enumerate() {
        let addr = 0x4000 + i as u16;
        assert_eq!(
            m.bus.memory.peek(addr),
            expected,
            "RAM at {addr:#06X} after decompression"
        );
    }
    // The marker's leading zero must not have been taken for data.
    assert_eq!(m.bus.memory.peek(0xFFFF), 0x5A, "the last byte of RAM");
}

/// A version 2 or 3 header, and the memory blocks that follow it.
fn z80_paged(extra: usize, mode: u8, blocks: &[(u8, u8)]) -> Vec<u8> {
    let mut file = z80_v1_header(false);
    file[6..8].copy_from_slice(&0u16.to_le_bytes()); // PC = 0 means "read further on"
    file.extend_from_slice(&(extra as u16).to_le_bytes());
    let mut tail = vec![0u8; extra];
    tail[0..2].copy_from_slice(&0xBEEFu16.to_le_bytes()); // the real PC
    tail[2] = mode;
    file.extend_from_slice(&tail);

    for &(page, fill) in blocks {
        file.extend_from_slice(&0xFFFFu16.to_le_bytes()); // uncompressed
        file.push(page);
        file.extend_from_slice(&vec![fill; 0x4000]);
    }
    file
}

#[test]
fn v2_pages_land_where_they_belong() {
    // Page 11 is a Multiface ROM: not ours to place, and not an error either.
    let file = z80_paged(23, 0, &[(8, 0x11), (4, 0x22), (5, 0x33), (11, 0x44)]);

    let mut m = machine();
    snapshot::load_z80(&mut m, &file).unwrap();
    assert_eq!(m.cpu.regs.pc, 0xBEEF, "PC comes from the extra header");
    assert_eq!(m.bus.memory.peek(0x4000), 0x11, "page 8 at 0x4000");
    assert_eq!(m.bus.memory.peek(0x7FFF), 0x11);
    assert_eq!(m.bus.memory.peek(0x8000), 0x22, "page 4 at 0x8000");
    assert_eq!(m.bus.memory.peek(0xBFFF), 0x22);
    assert_eq!(m.bus.memory.peek(0xC000), 0x33, "page 5 at 0xC000");
    assert_eq!(m.bus.memory.peek(0xFFFF), 0x33);
}

#[test]
fn a_compressed_page_decompresses() {
    let mut page = vec![0u8; 0x4000];
    page[0..10].copy_from_slice(&[0x77; 10]);
    page[10] = 0xED;
    page[0x3FFF] = 0x99;

    let mut file = z80_paged(23, 0, &[]);
    let compressed = z80_compress(&page);
    file.extend_from_slice(&(compressed.len() as u16).to_le_bytes());
    file.push(8);
    file.extend_from_slice(&compressed);

    let mut m = machine();
    snapshot::load_z80(&mut m, &file).unwrap();
    assert_eq!(m.bus.memory.peek(0x4000), 0x77);
    assert_eq!(m.bus.memory.peek(0x400A), 0xED);
    assert_eq!(m.bus.memory.peek(0x7FFF), 0x99, "the last byte of the page");
}

#[test]
fn the_hardware_mode_decides_what_we_will_load() {
    let mut m = machine();
    // Mode 3 is 128K in a version 2 file...
    assert_eq!(
        snapshot::load_z80(&mut m, &z80_paged(23, 3, &[(8, 0)])),
        Err(Error::UnsupportedHardware(3))
    );
    // ...and 48K with a disk interface in a version 3 one.
    assert!(snapshot::load_z80(&mut m, &z80_paged(54, 3, &[(8, 0x5C)])).is_ok());
    assert_eq!(m.bus.memory.peek(0x4000), 0x5C);

    for mode in [4u8, 5, 6, 2] {
        assert_eq!(
            snapshot::load_z80(&mut m, &z80_paged(54, mode, &[(8, 0)])),
            Err(Error::UnsupportedHardware(mode)),
            "mode {mode}"
        );
    }
}

#[test]
fn a_short_z80_is_refused() {
    let mut m = machine();
    assert_eq!(snapshot::load_z80(&mut m, &[0; 12]), Err(Error::Truncated));

    // A version 1 file whose RAM stops early.
    let mut file = z80_v1_header(false);
    file.extend_from_slice(&[0; 100]);
    assert_eq!(snapshot::load_z80(&mut m, &file), Err(Error::Truncated));

    // A compressed file that never reaches 48K.
    let mut file = z80_v1_header(true);
    file.extend_from_slice(&[0xED, 0xED, 10, 0x00, 0x00, 0xED, 0xED, 0x00]);
    assert_eq!(snapshot::load_z80(&mut m, &file), Err(Error::Truncated));
}

/// A real machine out through `.z80` and back, which checks the loader against live state
/// rather than against a fixture.
#[test]
fn a_real_machine_survives_a_v1_round_trip() {
    let mut original = booted();
    original.type_text("P2+2");
    original.run_frames(3);

    // Write a version 1 file by hand — the loader has never seen this encoder.
    let r = &original.cpu.regs;
    let mut file = vec![0u8; 30];
    file[0] = r.a;
    file[1] = r.f;
    file[2..4].copy_from_slice(&r.bc().to_le_bytes());
    file[4..6].copy_from_slice(&r.hl().to_le_bytes());
    file[6..8].copy_from_slice(&r.pc.to_le_bytes());
    file[8..10].copy_from_slice(&r.sp.to_le_bytes());
    file[10] = r.i;
    file[11] = r.r & 0x7F;
    file[12] = (r.r >> 7) | (original.bus.ula.border << 1) | 0x20;
    file[13..15].copy_from_slice(&r.de().to_le_bytes());
    file[15..17].copy_from_slice(&r.bc_.to_le_bytes());
    file[17..19].copy_from_slice(&r.de_.to_le_bytes());
    file[19..21].copy_from_slice(&r.hl_.to_le_bytes());
    file[21..23].copy_from_slice(&r.af_.to_be_bytes());
    file[23..25].copy_from_slice(&r.iy.to_le_bytes());
    file[25..27].copy_from_slice(&r.ix.to_le_bytes());
    file[27] = original.cpu.iff1 as u8;
    file[28] = original.cpu.iff2 as u8;
    file[29] = original.cpu.im;

    let ram: Vec<u8> = (0x4000..=0xFFFFu32)
        .map(|a| original.bus.memory.peek(a as u16))
        .collect();
    file.extend_from_slice(&z80_compress(&ram));
    file.extend_from_slice(&[0x00, 0xED, 0xED, 0x00]);

    let mut restored = machine();
    snapshot::load_z80(&mut restored, &file).expect("load");

    let (a, b) = (&original.cpu, &restored.cpu);
    assert_eq!(
        (a.regs.pc, a.regs.sp, a.regs.af(), a.regs.bc()),
        (b.regs.pc, b.regs.sp, b.regs.af(), b.regs.bc())
    );
    assert_eq!((a.regs.i, a.regs.r), (b.regs.i, b.regs.r), "I and R");
    assert_eq!((a.iff1, a.iff2, a.im), (b.iff1, b.iff2, b.im));
    for addr in 0x4000..=0xFFFFu32 {
        let addr = addr as u16;
        assert_eq!(
            original.bus.memory.peek(addr),
            restored.bus.memory.peek(addr),
            "RAM at {addr:#06X}"
        );
    }
    assert_eq!(original.screen_text(), restored.screen_text());

    // And it carries on: ENTER finishes the sum.
    restored.type_text("\n");
    restored.run_frames(10);
    assert_eq!(restored.screen_text()[0].trim_end(), "4");
}
