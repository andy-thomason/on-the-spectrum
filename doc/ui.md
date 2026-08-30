# The Bevy UI

The front end: the 256×192 display plus border, with the host keyboard driving the key
matrix. Bevy owns the window, the render loop and input; the emulator core stays a plain
library with no Bevy dependency (see [boot-and-test.md](boot-and-test.md) §2).

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
└──────────────────────────────────────────────────────────┘
```

One panel: the screen, keeping a fixed aspect and scaling by integer factors so pixels
stay square and crisp. Typing goes straight into the machine — there is no on-screen
keyboard to click.

### Plugin structure

| Plugin | Responsibility |
|---|---|
| `EmulatorPlugin` | Owns the `Machine` resource; runs frames; owns emulation-speed state |
| `ScreenPlugin` | The display texture, the border quad, and the blit |
| `KeyboardPlugin` | Host key events → the eight bytes of the matrix |
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
and right 48 above and 56 below (the standard "full frame with border" size; a
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

The emulator wants **eight bytes** — one per half-row, a set bit meaning a key is down —
and nothing else. Producing those eight bytes from host key events is the whole of this
plugin's job, and it is the same job `zxheadless` already does through
[`spectrum::keyboard`](../src/spectrum/keyboard.rs): `Key` is a position in the matrix,
`Chord` is a key with the shift held down beside it.

There is no on-screen keyboard. A photograph would need a picking mask, a highlight
shader, sticky shift keys — one pointer cannot hold CAPS SHIFT and another key at the same
time — and a share-alike licence obligation on the image, all of it in service of a slower
way to type than the keyboard already under the user's hands.

### Host key events → the matrix

```rust
#[derive(Resource, Default)]
struct KeyboardState {
    /// Keys the host is holding right now.
    physical: [u8; 8],
    /// Synthesised chords, each with the frames it still owes.
    held: Vec<(Chord, u8)>,
}
```

Merge the sources with OR into `machine.bus.keyboard` before each `run_frame`. Four rules:

- Map **physical key positions** (`KeyCode`), not text input, so the layout is predictable
  whatever the host locale is set to.
- Synthesised chords — Backspace → CAPS SHIFT + `0`, arrows → CAPS SHIFT + `5`–`8`,
  Escape → CAPS SHIFT + SPACE — must be held for **at least two emulated frames**. The ROM
  scans the matrix from its 50 Hz interrupt, so a chord that appears and vanishes inside
  one frame is invisible to it. `Machine::tap` uses three frames held and three released,
  and that is a good default here too.
- **Release everything when the window loses focus**, or the ROM will go on seeing a key
  held down that the user let go of while alt-tabbed.
- Do not try to send *characters*. Debounce, auto-repeat (`REPDEL`/`REPPER`), the
  K/L/C/E/G cursor modes and keyword decoding all happen inside the ROM's own interrupt
  handler. Supplying the matrix is enough, and it is why the single `P` key produces the
  whole `PRINT` keyword at a fresh prompt.

### The mapping

The table is in [spectrum-keyboard.md](spectrum-keyboard.md) §Suggested host-key mapping;
`Chord::for_char` already implements the character half of it, including SYM SHIFT for the
punctuation. The plugin adds only `KeyCode` → `Key`:

```rust
fn key_of(code: KeyCode) -> Option<Chord> {
    // KeyA..KeyZ and Digit0..Digit9 carry their own name; the rest are spelled out.
    if let Some(name) = letter_or_digit(code) {
        return Some(Chord::plain(Key::named(name)?));
    }
    Some(match code {
        KeyCode::Enter => Chord::plain(Key::ENTER),
        KeyCode::Space => Chord::plain(Key::SPACE),
        KeyCode::ShiftLeft => Chord::plain(Key::CAPS_SHIFT),
        KeyCode::ShiftRight | KeyCode::ControlLeft | KeyCode::AltLeft => {
            Chord::plain(Key::SYM_SHIFT)
        }
        KeyCode::Backspace => Chord::caps(Key::named("0")?),
        KeyCode::ArrowLeft => Chord::caps(Key::named("5")?),
        KeyCode::ArrowDown => Chord::caps(Key::named("6")?),
        KeyCode::ArrowUp => Chord::caps(Key::named("7")?),
        KeyCode::ArrowRight => Chord::caps(Key::named("8")?),
        KeyCode::Escape => Chord::caps(Key::SPACE),
        _ => return None,
    })
}
```

### Feedback

None is needed: the ROM clicks the beeper itself on every accepted key, so once audio
exists the machine gives its own confirmation, exactly as the real one does.

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
| 5 | egui debug panels |
| 6 | Snapshot loading; play a game |
| 7 | Audio |
| 8 | Tape loading, CRT shader, save states |

Steps 1–4 give a genuinely usable emulator, and each of them can be checked against
`zxheadless`, which already does the same work without a window in the way.
