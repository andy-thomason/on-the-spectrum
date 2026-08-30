# ZX Spectrum video: the display file, attributes and ULA timing

The Spectrum has no video RAM and no video registers. The ULA reads the bottom
6912 bytes of the 16K RAM bank directly and clocks them out to the TV. Everything
the emulator needs is: the address arithmetic, the attribute format, the palette, and
the frame timing.

Sources: [ref/48kreference.htm](ref/48kreference.htm) §48K ZX Spectrum and
§Contended Memory.

## The display file

| Region | Address | Size |
|---|---|---|
| Pixel data | `0x4000`–`0x57FF` | 6144 bytes |
| Attributes | `0x5800`–`0x5AFF` | 768 bytes |
| **Total** | `0x4000`–`0x5AFF` | **6912 bytes** |

The display is 256 × 192 pixels, one bit per pixel, **MSB is the leftmost pixel** of
each 8-pixel group. Colour resolution is one 8 × 8 character cell, 32 × 24 cells.

### Why the layout looks strange

Screen rows are **not** stored consecutively. The address is formed by splicing the
bits of the y coordinate:

```
                 15 14 13 12 11 10  9  8   7  6  5  4  3  2  1  0
pixel address:    0  1  0  T  T  S  S  S   R  R  R  C  C  C  C  C
                  └──┬──┘  └─┬─┘ └──┬──┘   └──┬──┘ └─────┬─────┘
      base 0x4000 ───┘       │      │         │          └── C = x >> 3   (column, 0..31)
                             │      │         └───────────── R = y bits 5-3 (char row in third, 0..7)
                             │      └─────────────────────── S = y bits 2-0 (scanline in char, 0..7)
                             └────────────────────────────── T = y bits 7-6 (third of screen, 0..2)

attribute addr:   0  1  0  1  1  0  T  T   R  R  R  C  C  C  C  C     base 0x5800
```

So the file is: the eight top scanlines of every character in the top third, then the
eight second scanlines, and so on. This let Sinclair drive the character generator
with a simple counter — the low three bits of `H` step through a character's
scanlines, which is why `INC H` walks down one pixel row within a character cell.

```rust
/// Address of the byte containing pixel (x, y). x: 0..256, y: 0..192.
#[inline]
pub fn pixel_addr(x: u16, y: u16) -> u16 {
    0x4000 | ((y & 0xC0) << 5) | ((y & 0x07) << 8) | ((y & 0x38) << 2) | (x >> 3)
}

/// Address of the attribute byte for pixel (x, y).
#[inline]
pub fn attr_addr(x: u16, y: u16) -> u16 {
    0x5800 | ((y & 0xF8) << 2) | (x >> 3)
}
```

Sanity checks (make these unit tests): `pixel_addr(0,0) == 0x4000`,
`pixel_addr(0,1) == 0x4100`, `pixel_addr(0,8) == 0x4020`,
`pixel_addr(0,64) == 0x4800`, `pixel_addr(255,191) == 0x57FF`,
`attr_addr(0,0) == 0x5800`, `attr_addr(255,191) == 0x5AFF`.

For a renderer it is usually easier to iterate the display file linearly and invert the
mapping, which touches every byte exactly once and is cache-friendly:

```rust
for addr in 0x4000u16..0x5800 {
    let t = (addr >> 11) & 0x03;   // third
    let s = (addr >>  8) & 0x07;   // scanline within char
    let r = (addr >>  5) & 0x07;   // char row within third
    let c =  addr        & 0x1F;   // column
    let y = (t << 6) | (r << 3) | s;
    let x = c << 3;
    // ...
}
```

## Attribute byte

One byte per 8 × 8 cell, 32 per row, 24 rows, stored linearly at `0x5800`.

```
 bit  7     6      5   4   3    2   1   0
    ┌─────┬──────┬──────────┬──────────┐
    │FLASH│BRIGHT│  PAPER   │   INK    │
    └─────┴──────┴──────────┴──────────┘
```

- **INK** (bits 0–2): colour of set pixels.
- **PAPER** (bits 3–5): colour of clear pixels.
- **BRIGHT** (bit 6): selects the bright palette for both ink and paper in this cell.
- **FLASH** (bit 7): the ULA swaps ink and paper every 16 frames.

## Palette

Eight colours, each with a normal and a bright variant. Bit 0 of the colour index is
blue, bit 1 red, bit 2 green.

| Index | Name | Normal RGB | Bright RGB |
|---|---|---|---|
| 0 | Black | `#000000` | `#000000` |
| 1 | Blue | `#0000D7` | `#0000FF` |
| 2 | Red | `#D70000` | `#FF0000` |
| 3 | Magenta | `#D700D7` | `#FF00FF` |
| 4 | Green | `#00D700` | `#00FF00` |
| 5 | Cyan | `#00D7D7` | `#00FFFF` |
| 6 | Yellow | `#D7D700` | `#FFFF00` |
| 7 | White | `#D7D7D7` | `#FFFFFF` |

There is no single "correct" set of values — real machines vary and different
emulators pick `0xCD`, `0xD7` or `0xD8` for the non-bright level. `0xD7`/`0xFF` is the
common convention (Fuse uses it) and is what we will use. Note that bright black is
still black: there are 15 distinct colours, not 16.

The **border** colour is written to bits 0–2 of port `0xFE` and is always non-bright.

## Frame and line timing

At 3.5 MHz with 312 lines of 224 T-states:

```
frame = 312 × 224 = 69888 T-states  →  3_500_000 / 69888 = 50.08 Hz
```

Not exactly 50 Hz — a clock program runs about 6 seconds fast over an hour.

### Frame structure (T-states from the falling edge of `/INT`)

| T-states | Lines | Contents |
|---|---|---|
| 0 – 14335 | 64 | Vertical retrace and top border. `/INT` is held low for the first 32 T |
| 14336 – 57343 | 192 | Display lines. The first display byte (address `0x4000`) is emitted at T = 14336 |
| 57344 – 69887 | 56 | Bottom border |

### Line structure (224 T-states)

| T-states | Contents |
|---|---|
| 0 – 127 | 256 pixels of display (32 columns; the ULA fetches a pixel byte and an attribute byte every 4 T) |
| 128 – 151 | 24 T right border (48 pixels) |
| 152 – 199 | 48 T horizontal retrace |
| 200 – 223 | 24 T left border (48 pixels) |

Two pixels are emitted per T-state, so an 8-pixel chunk takes 4 T-states — **that is
the granularity at which the border can change**. An `OUT` to `0xFE` that *completes*
at T = 14339…14342 changes the border at the position of screen byte `0x4000`.

### FLASH

Every 16 frames the ULA inverts ink and paper for every cell whose attribute bit 7 is
set; the full cycle is 32 frames ≈ 0.64 s. Track a frame counter and derive
`flash_phase = (frame_count >> 4) & 1`.

## Contention

While drawing the 192 display lines the ULA has priority on `0x4000`–`0x7FFF`. A CPU
access in that window is stalled. The delay depends on the T-state within the frame,
repeating with period 8 across the 128 display T-states of each line:

```
T-state (mod 8, from 14335):   0  1  2  3  4  5  6  7
delay in T-states:             6  5  4  3  2  1  0  0
```

and zero during the 96 T of border/retrace per line, and zero outside the 192 display
lines. The full per-instruction breakdown of *where* in each instruction the delay
applies is in [spectrum-memory-map.md](spectrum-memory-map.md) §Instruction contention
table.

Contention is not needed to boot the ROM. Stage it: get the machine running with flat
timing, then add contention behind a flag and validate against timing test ROMs.

## Rendering strategies

**1. Frame-at-a-time (start here).** Run 69888 T-states, fire the interrupt, then
convert the whole display file to a 256×192 texture and draw the border as a solid
colour. Simple, fast, correct for the ROM and for the large majority of software.
Cannot show mid-frame effects.

**2. Scanline.** Render at each line boundary. Handles per-line border effects
(Aquaplane's horizon) and most raster tricks.

**3. Cycle-accurate "beam" renderer.** Emit pixels as T-states are consumed, so the
CPU and the beam interleave exactly. Needed for full-screen colour effects, floating
bus tricks and multicolour demos.

Design the ULA so the display fetch is a function of "T-states elapsed this frame" from
the start; then moving from strategy 1 to 3 is a change of when you call it, not a
rewrite.

## Things to note but not implement first

- **Floating bus.** Reading an unused port during the display period returns the byte
  the ULA just fetched, otherwise `0xFF`. Used by Arkanoid, Cobra and others.
- **Snow.** If the `I` register is in `0x40..0x7F`, the Z80's refresh addresses look to
  the ULA like frantic reads of contended RAM; it misses fetches and repeats the
  previous byte, filling the screen with speckle. Vectron uses this deliberately.
- **Border pixel effects** require the beam renderer.
