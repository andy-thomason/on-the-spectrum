# on-the-spectrum

A Sinclair ZX Spectrum 48K emulator in Rust.

A traceable Z80 interpreter built directly from the instruction spec, a disassembler
sharing the same decoder for tracing and debugging, and a [Bevy](https://bevyengine.org)
front end with the display and a clickable keyboard built from a photograph of the real
machine.

**Status:** planning complete, implementation not yet started.

## Documentation

| Document | Contents |
|---|---|
| [doc/initial-plan.md](doc/initial-plan.md) | Architecture, phases, decisions, risks — **start here** |
| [doc/boot-and-test.md](doc/boot-and-test.md) | The core: decoder, T-state model, interpreter, disassembler, tracing, test strategy |
| [doc/ui.md](doc/ui.md) | The Bevy app: screen, emulation pacing, clickable keyboard, debug panels |
| [doc/z80-instruction-set.md](doc/z80-instruction-set.md) | Z80 reference: flags, MEMPTR, interrupts, decode algorithm, complete opcode tables |
| [doc/spectrum-memory-map.md](doc/spectrum-memory-map.md) | Memory map, ROM entry points, system variables, port `0xFE`, contention, tape format |
| [doc/spectrum-video.md](doc/spectrum-video.md) | Display file layout, attributes, palette, frame and line timing |
| [doc/spectrum-keyboard.md](doc/spectrum-keyboard.md) | The 8×5 key matrix and host key mapping |

Primary reference material is mirrored under [doc/ref/](doc/ref/).

## The machine

| | |
|---|---|
| CPU | Zilog Z80A @ 3.5 MHz |
| Memory | 16K ROM at `0x0000`, 48K RAM at `0x4000`; `0x4000`–`0x7FFF` contended by the ULA |
| Display | 256×192, 15 colours, 8×8 attribute cells, at `0x4000`–`0x5AFF` |
| I/O | One port, `0xFE` — border, beeper, tape and the keyboard matrix |
| Timing | 69888 T-states per frame = 312 lines × 224 T → 50.08 Hz interrupt, IM 1 |

## Design in three lines

1. The `z80` core knows nothing about the Spectrum — it talks to a `Bus` trait, so it can
   be tested in isolation against the standard exercisers.
2. One decoder feeds both the interpreter and the disassembler, so traces cannot lie.
3. T-states are *spent* through machine-cycle primitives, not tallied per instruction —
   which is what makes ULA contention and a beam renderer possible without a rewrite.

## Building

```sh
cargo build
```

## Licences and credits

The emulator source is the author's own work. Bundled reference material and assets
belong to others — see [assets/CREDITS.md](assets/CREDITS.md) for full attribution:

- **`roms/48.rom`** — the Sinclair 48K ROM, copyright Amstrad plc, who permit
  redistribution with emulators provided the copyright notice within the image remains
  intact. It is *not* public domain.
- **`assets/zx-spectrum-48k.jpg`** — photograph by Bill Bertram, CC BY-SA 2.5.
- **`doc/ref/`** — third-party technical documents, retained for reference and
  attributed to their authors in `assets/CREDITS.md`.
