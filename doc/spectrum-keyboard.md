# ZX Spectrum keyboard

40 keys in an **8 × 5 matrix**, read through port `0xFE`. There is no dedicated
keyboard controller: the ULA simply gates five data lines onto the bus according to
which of the eight high address lines are low.

Sources: [ref/48kreference.htm](ref/48kreference.htm) §Port 0xfe, and the
`KEY-SCAN` / `K-DECODE` routines at `L028E` and `L0333` in
[ref/Spectrum48-disassembly.asm](ref/Spectrum48-disassembly.asm).

## Physical layout

```
┌────┬────┬────┬────┬────┬────┬────┬────┬────┬────┐
│ 1  │ 2  │ 3  │ 4  │ 5  │ 6  │ 7  │ 8  │ 9  │ 0  │
├────┼────┼────┼────┼────┼────┼────┼────┼────┼────┤
│ Q  │ W  │ E  │ R  │ T  │ Y  │ U  │ I  │ O  │ P  │
├────┼────┼────┼────┼────┼────┼────┼────┼────┼────┤
│ A  │ S  │ D  │ F  │ G  │ H  │ J  │ K  │ L  │ENTR│
├────┼────┼────┼────┼────┼────┼────┼────┼────┼────┤
│CAPS│ Z  │ X  │ C  │ V  │ B  │ N  │ M  │SYM │SPC │
│SHFT│    │    │    │    │    │    │    │SHFT│BRK │
└────┴────┴────┴────┴────┴────┴────┴────┴────┴────┘
```

The two shift keys are the modifiers: **CAPS SHIFT** (bottom left) and **SYMBOL
SHIFT** (bottom right, red legend). `CAPS SHIFT` + `SPACE` is BREAK.

## The matrix

Reading port `0xFE` with a **0 in bit *n* of the high address byte** selects half-row
*n*. Bits 0–4 of the result are the five keys, **0 meaning pressed**. Bits 5 and 7
always read 1; bit 6 is the EAR input.

| Port | High byte | Bit 0 | Bit 1 | Bit 2 | Bit 3 | Bit 4 |
|---|---|---|---|---|---|---|
| `0xFEFE` | `11111110` | **CAPS SHIFT** | Z | X | C | V |
| `0xFDFE` | `11111101` | A | S | D | F | G |
| `0xFBFE` | `11111011` | Q | W | E | R | T |
| `0xF7FE` | `11110111` | 1 | 2 | 3 | 4 | 5 |
| `0xEFFE` | `11101111` | 0 | 9 | 8 | 7 | 6 |
| `0xDFFE` | `11011111` | P | O | I | U | Y |
| `0xBFFE` | `10111111` | **ENTER** | L | K | J | H |
| `0x7FFE` | `01111111` | **SPACE** | **SYM SHIFT** | M | N | B |

Note the mirror symmetry: rows 0–3 read left-to-right from the outside in, rows 4–7
read right-to-left. Bit 0 is always the outermost column.

If several address lines are low, the result is the **logical AND** of the individual
half-rows — a 0 in a bit means at least one of the corresponding keys is down. So
`IN A,(0x00FE)` (e.g. `XOR A` / `IN A,(0xFE)`) returns `0x1F` in the low bits only if
no key at all is pressed.

Because it is a plain diode-less matrix, **three or more simultaneous keys can ghost**.
Pressing CAPS, B and V makes the machine also see SPACE, producing a spurious BREAK.
Emulating the matrix as an 8-byte array of bit masks reproduces this automatically —
do not shortcut it with a "current key" variable.

### Emulator representation

```rust
/// One byte per half-row, bit set = key pressed.
pub struct Keyboard { rows: [u8; 8] }

impl Keyboard {
    /// Value returned in bits 0..4 of an IN from port 0xFE.
    pub fn read(&self, port: u16) -> u8 {
        let sel = (port >> 8) as u8;
        let mut r = 0x1F;
        for n in 0..8 {
            if sel & (1 << n) == 0 { r &= !self.rows[n] & 0x1F; }
        }
        r
    }
}
```

Then `IN A,(0xFE)` returns `0xA0 | ear_bit | keyboard.read(port)` — bits 5 and 7 set.

## ROM key codes

`KEY-SCAN` (`L028E`) returns up to two key values in `D` and `E`, most-significant
shift first, or `0xFF` for none. The value for half-row *r* (0–7 as tabled above),
bit *b* (0–4) is:

```
key_value = 39 - 8*b - r          (range 0..39)
```

`K-DECODE` (`L0333`) indexes the tables at `L0205` (MAIN-KEYS) and `L022C`
(E-UNSHIFT etc.) to turn that into a character code, using `MODE` (`0x5C41`) to pick
between the **K**, **L**, **C**, **E** and **G** cursor modes. The result lands in
`LAST_K` (`0x5C08`) with bit 5 of `FLAGS` (`0x5C3B`) set to signal "a key is
available".

Auto-repeat is driven by `KSTATE` (`0x5C00`, two 4-byte debounce sets), `REPDEL`
(`0x5C09`, default 35 frames ≈ 0.7 s) and `REPPER` (`0x5C0A`, default 5 frames).
All of this runs in the ROM's 50 Hz interrupt handler — **the emulator only has to
supply the eight matrix bytes**; the ROM does debounce, repeat and decoding itself.

## Suggested host-key mapping

| Host key | Spectrum |
|---|---|
| `A`–`Z`, `0`–`9` | direct |
| Enter | ENTER |
| Space | SPACE |
| Left Shift | CAPS SHIFT |
| Right Shift / Ctrl / Alt | SYMBOL SHIFT |
| Backspace | CAPS SHIFT + `0` (DELETE) |
| Arrow keys | CAPS SHIFT + `5`/`6`/`7`/`8` (← ↓ ↑ →) |
| Escape | CAPS SHIFT + SPACE (BREAK) |
| Caps Lock | CAPS SHIFT + `2` |
| `,` `.` `;` `"` etc. | SYMBOL SHIFT + the appropriate key |

For the punctuation shortcuts, press the synthesised combination for at least one
full 50 Hz frame so the ROM's scan sees it — the UI layer should hold a synthesised
chord for ~2 frames rather than pulsing it for one emulated instruction.
