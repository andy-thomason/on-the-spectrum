//! Writing text into the display file, checked by reading it back through the same font.

use on_the_spectrum::spectrum::{Machine, screen, text};

const ROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/roms/48.rom");

fn machine() -> Machine {
    Machine::new(&std::fs::read(ROM).expect("roms/48.rom"))
}

#[test]
fn what_goes_in_reads_back_out() {
    let mut m = machine();
    text::clear(&mut m.bus.memory, text::DEFAULT_ATTRIBUTE);
    text::print_at(&mut m.bus.memory, 0, 0, "HELLO", text::DEFAULT_ATTRIBUTE);
    text::print_at(
        &mut m.bus.memory,
        4,
        5,
        "world 42!",
        text::DEFAULT_ATTRIBUTE,
    );
    text::print_centred(&mut m.bus.memory, 23, "© 1982", text::DEFAULT_ATTRIBUTE);

    let lines = m.screen_text();
    assert_eq!(lines[0], "HELLO                           ");
    assert_eq!(&lines[5][4..13], "world 42!");
    assert!(lines[23].contains("© 1982"), "got {:?}", lines[23]);
    assert_eq!(lines[1].trim(), "", "nothing should leak onto other rows");
}

#[test]
fn text_is_clipped_at_the_right_edge() {
    let mut m = machine();
    text::clear(&mut m.bus.memory, text::DEFAULT_ATTRIBUTE);
    text::print_at(
        &mut m.bus.memory,
        28,
        3,
        "0123456789",
        text::DEFAULT_ATTRIBUTE,
    );
    assert_eq!(m.screen_text()[3], "                            0123");

    // Off the bottom is a no-op rather than a panic.
    text::print_at(&mut m.bus.memory, 0, 24, "nowhere", text::DEFAULT_ATTRIBUTE);
    text::highlight_row(&mut m.bus.memory, 99, 0x38);
}

#[test]
fn a_highlight_changes_the_colour_and_not_the_characters() {
    let mut m = machine();
    text::clear(&mut m.bus.memory, text::DEFAULT_ATTRIBUTE);
    text::print_at(&mut m.bus.memory, 1, 7, "selected", text::DEFAULT_ATTRIBUTE);
    let before = m.screen_text();

    text::highlight_row(&mut m.bus.memory, 7, 0x47); // bright, black on white
    assert_eq!(m.screen_text(), before, "the pixels are untouched");
    for col in 0..screen::COLUMNS {
        assert_eq!(
            m.bus.memory.peek(screen::attribute_address(col, 7)),
            0x47,
            "attribute at column {col}"
        );
    }
}

#[test]
fn clearing_sets_every_attribute_and_no_pixels() {
    let mut m = machine();
    text::print_at(&mut m.bus.memory, 0, 0, "gone", text::DEFAULT_ATTRIBUTE);
    text::clear(&mut m.bus.memory, 0x0F);

    for row in 0..screen::ROWS {
        assert_eq!(m.screen_text()[row].trim(), "");
        for col in 0..screen::COLUMNS {
            assert_eq!(m.bus.memory.peek(screen::attribute_address(col, row)), 0x0F);
        }
    }
}
