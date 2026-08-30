//! The other half of §9 stage A: `zxdis roms/48.rom` must agree with the annotated
//! disassembly.
//!
//! `doc/ref/Spectrum48-disassembly.asm` names ~1100 ROM addresses, and prints the
//! instruction at each one. Straight-line disassembly of the ROM reaches most of them, so
//! this is a thousand hand-checked samples for free — every one an instruction the real
//! machine executes, in the mix the real machine executes it.

use on_the_spectrum::z80::disasm;

const LISTING: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/doc/ref/Spectrum48-disassembly.asm"
);
const ROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/roms/48.rom");

/// Lines where the listing writes an assembler expression rather than the value the
/// assembler computes. Ours is the arithmetic result, so both are right:
/// `$022C-$41 == $01EB`, `($1ADF + 1) % 256 == $E0`, and so on.
const EXPRESSIONS: &[(u16, &str)] = &[
    (0x0341, "LD HL,$01EB"),
    (0x034F, "LD HL,$0229"),
    (0x0389, "LD HL,$0230"),
    (0x0609, "SUB $E0"),
    (0x1CA5, "SUB $13"),
];

#[test]
fn the_rom_disassembles_as_the_annotated_listing_reads() {
    let rom = std::fs::read(ROM).expect("roms/48.rom");
    let listing = std::fs::read(LISTING).expect("doc/ref/Spectrum48-disassembly.asm");
    let listing = String::from_utf8_lossy(&listing);

    let ours: std::collections::HashMap<u16, String> = disasm::walk(&rom, 0, 0, rom.len())
        .map(|i| {
            let (text, _) = disasm::disassemble_parts(&i.decoded, i.addr, None);
            // The `*` marks an undocumented opcode; the listing has no such notion.
            (i.addr, text.trim_start_matches('*').to_string())
        })
        .collect();

    let mut compared = 0;
    let mut unreached = 0;
    let mut differences = Vec::new();

    for (addr, expected) in labelled_instructions(&listing) {
        let Some(actual) = ours.get(&addr) else {
            // The ROM embeds data in the instruction stream — a `DEFB` after a `RST 28`,
            // a byte jumped over to reach a different entry point. Straight-line decoding
            // loses phase there until it resynchronises, which is what the listing's own
            // `DEFB` directives are for.
            unreached += 1;
            continue;
        };
        compared += 1;
        let expected = match EXPRESSIONS.iter().find(|&&(a, _)| a == addr) {
            Some(&(_, value)) => value.to_string(),
            None => expected,
        };
        if *actual != expected {
            differences.push(format!(
                "  {addr:04X}  ours: {actual:<24} listing: {expected}"
            ));
        }
    }

    assert!(
        differences.is_empty(),
        "{} of {compared} labelled instructions disagree with the listing:\n{}",
        differences.len(),
        differences.join("\n")
    );
    // Guard against the parsing above quietly matching nothing at all.
    assert!(compared > 950, "only {compared} instructions compared");
    assert!(
        unreached < 40,
        "{unreached} labels missed: decoding lost phase"
    );
}

/// `L11CB:  LD      B,A             ; Save the flag...` → `(0x11CB, "LD B,A")`, in the
/// form the disassembler produces: single-spaced, and with `Lxxxx` label references and
/// `xxH` hex written the way we write them.
fn labelled_instructions(listing: &str) -> Vec<(u16, String)> {
    let mut out = Vec::new();
    for line in listing.lines() {
        let Some((label, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(hex) = label.strip_prefix('L') else {
            continue;
        };
        if hex.len() != 4 {
            continue;
        }
        let Ok(addr) = u16::from_str_radix(hex, 16) else {
            continue;
        };

        let code = rest.split(';').next().unwrap_or("").trim();
        // Data and assembler directives have no disassembly to compare against.
        if code.is_empty()
            || code.starts_with("DEFB")
            || code.starts_with("DEFW")
            || code.starts_with("DEFM")
            || code.starts_with('#')
        {
            continue;
        }
        out.push((addr, normalise(code)));
    }
    out
}

fn normalise(code: &str) -> String {
    let mut out = String::new();
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // `LD      B,A` → `LD B,A`
            ' ' | '\t' => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                out.push(' ');
            }
            // `L11CB` → `$11CB`
            'L' if chars.clone().take(4).all(|c| c.is_ascii_hexdigit())
                && chars.clone().take(4).count() == 4
                && chars
                    .clone()
                    .nth(4)
                    .is_none_or(|c| !c.is_ascii_alphanumeric()) =>
            {
                out.push('$');
                for _ in 0..4 {
                    out.push(chars.next().unwrap());
                }
            }
            _ => out.push(c),
        }
    }
    // `RST 28H` → `RST $28`
    if let Some(hex) = out.strip_prefix("RST ").and_then(|r| r.strip_suffix('H')) {
        return format!("RST ${hex}");
    }
    out
}
