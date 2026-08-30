# The Bevy UI

The front end: the 256×192 display plus border, and a clickable keyboard built from a
photograph of the real machine. Bevy owns the window, the render loop and input;
the emulator core stays a plain library with no Bevy dependency
(see [boot-and-test.md](boot-and-test.md) §2).

**Target: Bevy 0.19** (0.19.1 is current as of August 2026). The code sketches below
are structural — check exact type and system names against the 0.19 migration guide
when you write them, since Bevy's API moves between releases.

## 1. Shape of the app

```
┌──────────────────────────────────────────────────────────┐
│  menu bar:  File  Machine  Debug                         │
├──────────────────────────────────────────────────────────┤
│                                                          │
│              ┌────────────────────────┐                  │
│              │ border                 │                  │
│              │   ┌────────────────┐   │                  │
│              │   │                │   │   352 × 296 px   │
│              │   │  256 × 192     │   │   integer-scaled │
│              │   │                │   │                  │
│              │   └────────────────┘   │                  │
│              └────────────────────────┘                  │
│                                                          │
├──────────────────────────────────────────────────────────┤
│                                                          │
│         [ photo of the ZX Spectrum, keys clickable ]     │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

Two panels stacked vertically. The screen panel keeps a fixed aspect and scales by
integer factors so pixels stay square and crisp. The keyboard panel scales freely to
the window width.

### Plugin structure

| Plugin | Responsibility |
|---|---|
| `EmulatorPlugin` | Owns the `Machine` resource; runs frames; owns emulation-speed state |
| `ScreenPlugin` | The display texture, the border quad, and the blit |
| `KeyboardPlugin` | The photo, the picking mask, click and physical-key handling |
| `DebugPlugin` | Trace window, register view, breakpoints, disassembly pane |

## 2. Driving emulation from the render loop

The Spectrum runs at 50.08 Hz; the host window will typically be 60 Hz or higher and
may be variable. Do **not** run one emulated frame per rendered frame — that plays
BASIC and games 20% fast and makes the beeper wrong.

Accumulate real time and consume it in whole emulated frames:

```rust
#[derive(Resource)]
struct Emulator {
    machine: Machine,
    accumulator: f64,        // seconds of emulated time owed
    speed: f32,              // 1.0 = real time; 0.0 = paused
}

const FRAME_SECONDS: f64 = 69888.0 / 3_500_000.0;   // 0.019968 s

fn run_emulation(mut emu: ResMut<Emulator>, time: Res<Time>) {
    if emu.speed == 0.0 { return; }
    emu.accumulator += (time.delta_secs_f64()) * emu.speed as f64;

    // Cap the catch-up so a stall (window drag, breakpoint) doesn't cause a
    // multi-second fast-forward when we resume.
    emu.accumulator = emu.accumulator.min(FRAME_SECONDS * 4.0);

    while emu.accumulator >= FRAME_SECONDS {
        emu.machine.run_frame();     // 69888 T-states + one /INT
        emu.accumulator -= FRAME_SECONDS;
    }
}
```

`Machine::run_frame` is the whole interface the UI needs. Debug modes add
`step_instruction()`, `run_until(pc)` and `run_scanline()`.

Run this in `Update` before the screen upload. Speed presets: pause, ½×, 1×, 2×, and
"turbo" (uncapped, for tape loading).

## 3. The screen

### Texture

One `Image` of 352 × 296 RGBA8 — 256 × 192 display plus a 48-pixel border on the left
and right and 52/56 above and below (the standard "full frame with border" size; a
32-pixel border is a common smaller alternative). Uploaded once per emulated frame.

```rust
fn upload_screen(emu: Res<Emulator>, mut images: ResMut<Assets<Image>>, tex: Res<ScreenTexture>) {
    let img = images.get_mut(&tex.0).unwrap();
    emu.machine.render_into(img.data.as_mut().unwrap());   // &mut [u8], 352*296*4
}
```

`render_into` lives in `spectrum::screen` — the UI never touches the display file
directly. That keeps the frame-at-a-time renderer swappable for a scanline or beam
renderer later without touching Bevy code (see
[spectrum-video.md](spectrum-video.md) §Rendering strategies).

### Sampling and scaling

Set the sampler to **nearest-neighbour** — the default linear filter turns Spectrum
pixels into mush. Set it on the `Image` at creation:

```rust
image.sampler = ImageSampler::nearest();
```

Then constrain the on-screen size to an **integer multiple** of 352 × 296. At 3× that is
1056 × 888, which suits a 1080p window. Recompute on window resize:

```rust
let scale = ((win.width() / 352.0).min(win.height() * 0.6 / 296.0)).floor().max(1.0);
```

Non-integer scaling produces uneven pixel widths that are very visible on the
Spectrum's 1-pixel-wide character strokes.

Optional and worth having later: a CRT post-process (scanlines, slight barrel
distortion, phosphor bloom) as a Bevy post-processing node, behind a toggle.

### FLASH

The attribute FLASH bit inverts every 16 frames. Handle it in `render_into` from the
machine's own frame counter, not from wall-clock time, so it stays correct when paused
or fast-forwarded.

## 4. The keyboard

### The image

`assets/zx-spectrum-48k.jpg` — 2168 × 1593, **CC BY-SA 2.5**, by Bill Bertram
(Wikimedia Commons, [File:ZXSpectrum48k.jpg](https://commons.wikimedia.org/wiki/File:ZXSpectrum48k.jpg)).

Two obligations that must be met before release:

1. **Attribution** — credit Bill Bertram and name the licence, visibly (an About dialog
   and `assets/CREDITS.md`).
2. **Share-alike** — modified versions of the *image* must be released under CC BY-SA 2.5
   or a compatible licence. This binds the image and derivatives of it, not the
   emulator's source code. Record this in `assets/CREDITS.md` now, while it is cheap.

**The photo is a three-quarter perspective shot, not a flat top-down view.** That is
what makes it look good, but it means key outlines are trapezoids of varying size, and
axis-aligned rectangles will not fit them. Three ways to handle it:

| Approach | How | Verdict |
|---|---|---|
| **Picking mask** | A second PNG, same dimensions, each key filled with a unique flat colour. Click → sample the mask at the corresponding texel → key id | **Recommended.** Works with any image, perspective or not; no geometry maths; the mask doubles as the highlight mask |
| Rectify the photo | Compute a homography from the four corners of the keyboard plate, warp once offline to a flat top-down image, then use plain rects | Loses the pleasing perspective; extra offline step |
| Per-key quads | Hand-author four corner points per key, hit-test with a point-in-quad check | 40 keys × 4 points to author and tune by hand |

### Picking mask in detail

Author `assets/zx-spectrum-48k-keymask.png` once, by hand, in any image editor: load
the photo, add a layer, flood-fill each key cap with `rgb(id, 0, 0)` where `id` is
1..=40, and leave everything else black. Export as **lossless PNG** with no
antialiasing on the fills — a lossy or filtered mask produces stray ids on key edges.

```rust
#[derive(Resource)]
struct KeyMask { image: Handle<Image>, width: u32, height: u32 }

/// Map a cursor position within the keyboard sprite to a Spectrum key.
fn key_at(mask: &Image, uv: Vec2) -> Option<SpecKey> {
    let x = (uv.x * mask.width() as f32) as u32;
    let y = (uv.y * mask.height() as f32) as u32;
    let i = ((y * mask.width() + x) * 4) as usize;
    match mask.data.as_ref()?[i] {          // red channel is the id
        0 => None,
        id => SpecKey::from_id(id),
    }
}
```

Keep the mask image in CPU-readable form (`RenderAssetUsages::MAIN_WORLD` so it is not
discarded after upload) — you need to read pixels on the CPU.

Validate the mask at startup with a test: every id 1..=40 must appear at least, say,
200 times. That catches a missed key or a typo in a fill colour immediately, rather
than as a mysteriously dead key later.

### Key state and the matrix

The emulator wants eight bytes. Sources of "pressed" merge with OR:

```rust
#[derive(Resource, Default)]
struct KeyboardState {
    physical: [u8; 8],     // from the host keyboard
    mouse: Option<SpecKey>,// the key currently held under the pointer
    sticky: [u8; 8],       // click-to-toggle for CAPS/SYMBOL SHIFT
}
```

`SpecKey` carries its `(row, bit)` in the matrix — the table is in
[spectrum-keyboard.md](spectrum-keyboard.md). Feed the OR of all three into
`machine.keyboard.rows` before each `run_frame`.

The **sticky shift** behaviour matters for a clickable keyboard: you cannot click
CAPS SHIFT and `0` simultaneously with one pointer. Make clicking either shift key
*latch* it (visibly highlighted) until the next non-shift key is clicked, after which it
releases. This is the standard solution in on-screen Spectrum keyboards.

### Host keyboard mapping

Take `KeyCode` events and map them per the table in
[spectrum-keyboard.md](spectrum-keyboard.md) §Suggested host-key mapping. Two rules:

- Map **physical** key positions (`KeyCode`), not text input, so the layout is
  predictable regardless of host locale.
- Synthesised chords (Backspace → CAPS SHIFT + `0`, arrows → CAPS SHIFT + `5`–`8`) must
  be held for **at least two emulated frames**. The ROM scans the matrix from the 50 Hz
  interrupt; a chord that appears and vanishes inside one frame is invisible to it.
  Model each synthesised press with a small frame countdown.

### Visual feedback

Highlight pressed keys by drawing the mask as an overlay: a small shader (or a
`Material2d`) that samples the mask, compares the red channel against a "currently
pressed" lookup passed in as a uniform array, and adds a translucent white tint where
it matches. One draw call, works with the perspective automatically, and needs no
per-key geometry.

A click should also play the ROM's keyboard click naturally — it comes out of the
beeper as a side effect of the ROM's own key handling, so no UI work is needed once
audio exists.

## 5. Audio

The beeper is one bit: bit 4 of `OUT (0xFE)`. Approach:

1. The ULA records `(t_state, level)` transitions into a per-frame buffer.
2. At end of frame, resample that square wave to the host rate (44.1/48 kHz),
   **low-pass filtered** — a raw 1-bit square resampled naively aliases horribly.
3. Push the samples to a ring buffer feeding a Bevy audio source (or, more practically,
   `cpal` directly — Bevy's audio API is aimed at playing assets, not at streaming
   generated PCM).

Defer this past the first working build; it is orthogonal to everything else. But
record the transitions in the ULA from the start, since retrofitting the timestamps is
the annoying part.

## 6. Debug UI

`bevy_egui` is the pragmatic choice for developer tooling — immediate-mode panels are
much less work than Bevy UI nodes for a register dump.

| Panel | Contents |
|---|---|
| **Registers** | AF BC DE HL IX IY SP PC, shadows, IFF1/2, IM, `R`, T-state within frame, frame count |
| **Disassembly** | Live disassembly around `PC` using `z80::disasm` with the ROM symbol table; click a line to toggle a breakpoint |
| **Memory** | Hex dump with a "go to address" box; highlight the display file and system variables regions |
| **Trace** | Tail of the `RingTracer`; "dump last 10k to file" button |
| **Controls** | Pause / step instruction / step frame / run to cursor / reset; speed selector |

These all sit on the `Tracer` and `Machine` interfaces already specified in
[boot-and-test.md](boot-and-test.md) §7 — the UI adds no new emulator capability, it just
exposes what is there.

## 7. File loading

Drag-and-drop onto the window (`FileDragAndDrop` events), plus a `File` menu:

| Extension | Action |
|---|---|
| `.sna`, `.z80` | Load snapshot — restores registers and all 48K. **Implement this early**: it is the fastest way to get a real game on screen and shake out CPU bugs |
| `.tap`, `.tzx` | Insert tape. Offer both real-time loading (with the loading stripes, which is half the point) and instant ROM-trap loading |
| `.scr` | Load a 6912-byte screen dump straight into `0x4000` — a one-line smoke test for the renderer |
| `.rom` | Replace the ROM |

## 8. Build order

| Step | Deliverable |
|---|---|
| 1 | Bevy window; screen texture filled with a test pattern; nearest sampling and integer scaling verified |
| 2 | `.scr` loading → the renderer is proven against a known-good image before the CPU is trusted |
| 3 | `Emulator` resource + `run_emulation`; the booted ROM's copyright screen appears |
| 4 | Host keyboard → matrix; type `PRINT 2+2` and get `4` |
| 5 | Photo + picking mask + click handling + highlight overlay |
| 6 | egui debug panels |
| 7 | Snapshot loading; play a game |
| 8 | Audio |
| 9 | Tape loading, CRT shader, save states |

Steps 1–4 give a genuinely usable emulator. Do not start step 5 until step 4 works,
because a broken clickable keyboard and a broken CPU are very hard to tell apart.
