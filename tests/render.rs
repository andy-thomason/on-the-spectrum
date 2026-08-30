//! The frame renderer, checked without a window.
//!
//! The Bevy layer adds no emulator capability: it uploads what
//! [`spectrum::screen::render`] produces and nothing more. So the pixels are checked here,
//! headlessly, against a machine that has really booted — and the UI is left with only its
//! own plumbing to get wrong.

use on_the_spectrum::spectrum::{Machine, screen};

const ROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/roms/48.rom");
const MAIN_1: u16 = 0x12A9;

fn booted() -> Machine {
    let mut m = Machine::new(&std::fs::read(ROM).expect("roms/48.rom"));
    assert!(m.run_until_pc(MAIN_1, 12_000_000), "never reached MAIN-1");
    m
}

fn pixel(frame: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * screen::WIDTH + x) * 4;
    [frame[i], frame[i + 1], frame[i + 2]]
}

const WHITE: [u8; 3] = [0xD7, 0xD7, 0xD7];
const BLACK: [u8; 3] = [0, 0, 0];

#[test]
fn the_palette_has_fifteen_colours() {
    // Bit 0 blue, bit 1 red, bit 2 green; bright black is still black.
    assert_eq!(screen::colour(0, false), BLACK);
    assert_eq!(screen::colour(0, true), BLACK);
    assert_eq!(screen::colour(1, false), [0, 0, 0xD7]);
    assert_eq!(screen::colour(2, false), [0xD7, 0, 0]);
    assert_eq!(screen::colour(4, false), [0, 0xD7, 0]);
    assert_eq!(screen::colour(7, false), WHITE);
    assert_eq!(screen::colour(7, true), [0xFF, 0xFF, 0xFF]);

    let distinct: std::collections::HashSet<[u8; 3]> = (0..8)
        .flat_map(|i| [screen::colour(i, false), screen::colour(i, true)])
        .collect();
    assert_eq!(distinct.len(), 15, "bright black is not a sixteenth colour");
}

#[test]
fn the_booted_screen_renders_black_on_white() {
    let m = booted();
    let mut frame = vec![0; screen::FRAME_BYTES];
    m.render_into(&mut frame);

    // The ROM sets a white border, and CLS leaves black ink on white paper everywhere.
    assert_eq!(pixel(&frame, 0, 0), WHITE, "top left of the border");
    assert_eq!(
        pixel(&frame, screen::WIDTH - 1, screen::HEIGHT - 1),
        WHITE,
        "bottom right of the border"
    );
    assert_eq!(
        pixel(&frame, screen::BORDER_LEFT, screen::BORDER_TOP),
        WHITE,
        "the first pixel of the display, which CLS cleared to paper"
    );

    // The copyright message is the only ink on the screen, so exactly one character row
    // has any black in it — and it is the row the font reader finds the message on.
    let inked: Vec<usize> = (0..screen::ROWS)
        .filter(|row| {
            (0..screen::COLUMNS * 8).any(|x| {
                (0..8).any(|dy| {
                    pixel(
                        &frame,
                        screen::BORDER_LEFT + x,
                        screen::BORDER_TOP + row * 8 + dy,
                    ) == BLACK
                })
            })
        })
        .collect();
    assert_eq!(
        inked.len(),
        1,
        "expected ink on one row only, got {inked:?}"
    );
    assert!(
        m.screen_text()[inked[0]].contains("1982 Sinclair Research Ltd"),
        "the inked row should be the copyright message, and reads {:?}",
        m.screen_text()[inked[0]]
    );
}

#[test]
fn flash_swaps_ink_and_paper_in_flashing_cells_only() {
    let mut m = booted();
    m.type_text("2"); // any key starts an edit line, and the cursor cell flashes

    let mut steady = vec![0; screen::FRAME_BYTES];
    let mut inverted = vec![0; screen::FRAME_BYTES];
    screen::render_into(&m.bus.memory, m.bus.ula.border, false, &mut steady);
    screen::render_into(&m.bus.memory, m.bus.ula.border, true, &mut inverted);

    let changed = steady
        .chunks_exact(4)
        .zip(inverted.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    // Exactly one 8 × 8 cell flashes: the cursor.
    assert_eq!(changed, 64, "only the cursor cell should change");
}

#[test]
fn a_cell_renders_its_attribute() {
    let mut m = Machine::new(&std::fs::read(ROM).expect("roms/48.rom"));
    // Bright red ink on blue paper, and a pixel row of alternating bits.
    m.bus
        .memory
        .poke(screen::pixel_address(0, 0, 0), 0b1010_1010);
    m.bus
        .memory
        .poke(screen::attribute_address(0, 0), 0x40 | (1 << 3) | 2);

    let mut frame = vec![0; screen::FRAME_BYTES];
    m.render_into(&mut frame);

    let (x, y) = (screen::BORDER_LEFT, screen::BORDER_TOP);
    assert_eq!(
        pixel(&frame, x, y),
        [0xFF, 0, 0],
        "bit 7 is the leftmost pixel"
    );
    assert_eq!(
        pixel(&frame, x + 1, y),
        [0, 0, 0xFF],
        "and it is bright blue paper"
    );
    assert_eq!(pixel(&frame, x + 2, y), [0xFF, 0, 0]);
}
