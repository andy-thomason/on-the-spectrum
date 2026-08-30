# A Sinclair ZX Spectrum 48K emulator — initial plan

Build a cycle-aware ZX Spectrum 48K emulator in Rust: a traceable Z80 interpreter, a
disassembler for debugging, and a Bevy front end with the display, driven from the host
keyboard.

## Documents

| Document | Contents |
|---|---|
| [original-prompt.md](original-prompt.md) | The brief, verbatim |
| **initial-plan.md** | This file — architecture, phases, risks |
| [z80-instruction-set.md](z80-instruction-set.md) | Registers, flags (including the undocumented ones), MEMPTR, interrupts, the octal decode algorithm, and complete opcode tables for all seven prefix banks |
| [spectrum-memory-map.md](spectrum-memory-map.md) | Memory layout, ROM entry points, all 182 bytes of system variables, port `0xFE`, contention, tape format |
| [spectrum-keyboard.md](spectrum-keyboard.md) | The 8×5 matrix, half-row port addresses, ROM key codes, host mapping |
| [spectrum-video.md](spectrum-video.md) | Display file address arithmetic, attributes, palette, frame and line timing, rendering strategies |
| [boot-and-test.md](boot-and-test.md) | Core design: decoder, interpreter, disassembler, tracing, and the five-layer test strategy ending in ROM boot milestones |
| [ui.md](ui.md) | The Bevy app: screen texture, emulation pacing, host keyboard input, debug panels |

## Materials gathered

| Artefact | Location | Notes |
|---|---|---|
| **48K ROM binary** | [`roms/48.rom`](../roms/48.rom) | 16384 bytes, CRC32 `ddee531f` — verified against the known-good checksum |
| **Annotated disassembly** | [`ref/Spectrum48-disassembly.asm`](ref/Spectrum48-disassembly.asm) | 20340 lines, fully commented, Logan & O'Hara lineage. Also the source for the debugger's symbol table |
| **Spectrum hardware reference** | [`ref/48kreference.htm`](ref/48kreference.htm) | comp.sys.sinclair FAQ — the canonical timing and contention document |
| **Z80 undocumented behaviour** | `ref/z80-documented.pdf` — [fetch](ref/fetch.sh) | Sean Young, *The Undocumented Z80 Documented*. Not redistributed; see [ref/README.md](ref/README.md) |
| **Z80 official manual** | `ref/z80cpu_um.pdf` — [fetch](ref/fetch.sh) | Zilog Z80 CPU User Manual. Not redistributed; see [ref/README.md](ref/README.md) |
| **Z80 decode algorithm** | [`ref/z80-decoding.htm`](ref/z80-decoding.htm) | Cristian Dinu — the structure the interpreter mirrors |
| **Z80 flags / interrupts** | [`ref/z80reference.htm`](ref/z80reference.htm) | c.s.s FAQ Z80A section |
| **Machine-cycle breakdown** | [`ref/z80ins.txt`](ref/z80ins.txt) | Per-instruction M-cycle sequences |
| **Opcode table generator** | [`tools/gen_z80.py`](tools/gen_z80.py) | Emits the tables in the spec; will also emit the Rust decode fixtures |

## The machine, in one table

| | |
|---|---|
| CPU | Zilog Z80A @ 3.500 MHz |
| Memory | 16K ROM at `0x0000`, 48K RAM at `0x4000`; `0x4000`–`0x7FFF` contended |
| Display | 256×192, 15 colours, 8×8 attribute cells, at `0x4000`–`0x5AFF` |
| I/O | One port, `0xFE` — border, beeper, tape, and the keyboard matrix |
| Timing | 69888 T-states per frame = 312 lines × 224 T → 50.08 Hz interrupt, IM 1, handler at `0x0038` |
| Input | 40 keys in an 8×5 matrix, scanned by the ROM's interrupt handler |

## Architecture

```
                     ┌──────────────────────────────────────┐
                     │              Bevy app                │
                     │  ScreenPlugin  KeyboardPlugin        │
                     │  DebugPlugin   EmulatorPlugin        │
                     └───────────────────┬──────────────────┘
                                         │  run_frame() / step() / rows[8] / render_into()
                     ┌───────────────────▼──────────────────┐
                     │          crate `spectrum`            │
                     │  Machine = Memory + ULA + Keyboard   │
                     │  screen.rs  tape.rs  snapshot.rs     │
                     └───────────────────┬──────────────────┘
                                         │  impl Bus
                     ┌───────────────────▼──────────────────┐
                     │             crate `z80`              │
                     │  decode.rs ──┬── exec.rs             │
                     │              └── disasm.rs           │
                     │  alu.rs   trace.rs                   │
                     └──────────────────────────────────────┘
```

Three rules hold this together:

1. **`z80` knows nothing about the Spectrum.** It talks to a `Bus` trait. This is what
   lets the ZEXALL exercisers run against a trivial 64K-RAM bus, and it keeps the CPU
   testable in isolation.
2. **One decoder feeds both the interpreter and the disassembler.** If they diverge,
   traces lie exactly when you need them. `decode()` returns data; `exec` executes it,
   `disasm` formats it.
3. **The Bevy layer adds no emulator capability.** Every debug panel sits on interfaces
   the headless runner already uses, so anything the UI can do, a test can do.

## Phases

Each phase ends in something demonstrable. Detail for A–H is in
[boot-and-test.md](boot-and-test.md) §9 and [ui.md](ui.md) §8.

| Phase | Work | Proof it is done |
|---|---|---|
| **A** | Decoder + disassembler | `zxdis roms/48.rom` output matches the annotated listing; all 1792 (prefix, opcode) pairs round-trip against the generated spec tables |
| **B** | Interpreter + the M-cycle timing primitives (contention returning 0) | [SingleStepTests/z80](https://github.com/SingleStepTests/z80) per-opcode vectors pass, including flags 5/3, MEMPTR and the **per-cycle bus activity** |
| **C** | Memory, ROM protection, `Machine` | Headless boot reaches `CLS`: display file zeroed, attributes all `0x38` |
| **D** | ULA — port `0xFE`, 50 Hz interrupt, frame timing | The copyright message renders; `FRAMES` at `0x5C78` advances correctly |
| **E** | Keyboard matrix | Type `PRINT 2+2` headlessly, get `4`. **This is "the emulator works."** |
| **F** | Bevy shell — screen, pacing, host keyboard | Usable interactively |
| **G** | Snapshots, tape, beeper | Load a `.z80` of a real game and play it with sound |
| **H** | Contention, floating bus, beam renderer | Patrik Rak's Z80 test suite passes on-emulator |

A–E are the emulator; F onward is presentation and fidelity. **Do not build F before E
passes headlessly** — a broken UI and a broken CPU are hard to tell apart, and the
headless runner is much faster to iterate against.

## Decisions taken up front

**Rust, single crate with modules initially.** Split into workspace crates only when the
`z80`/`spectrum` boundary is proven; module boundaries enforce the same discipline at
lower cost.

**Loop-and-match interpreter, not a jump table or JIT.** A naive `match` runs the
Spectrum at hundreds of times real speed on modern hardware. Speed is not a constraint
here; readability and traceability are.

**Emulate the undocumented behaviour from day one.** Flags 5 and 3, MEMPTR, `SLL`, the
`DDCB` register-copy quirk, `IN (C)` / `OUT (C),0`. Real software depends on all of it
(Sabre Wulf, Ghosts'n'Goblins, Speedlock, Bounder), and these are far cheaper to build
in than to retrofit — MEMPTR especially.

**Model T-states as machine cycles, from phase B.** This is the one structural decision
that is genuinely expensive to reverse, so it is made up front. The CPU does *not* look
up a per-instruction cycle count and add it at the end; it spends time through five
primitives — opcode fetch (4T), memory read (3T), memory write (3T), internal operation
(n × 1T), and I/O (4T) — each of which charges `Bus::tick` and asks the bus for a
contention delay first. See [boot-and-test.md](boot-and-test.md) §5 for the primitives
and the per-instruction M-cycle recipes.

Three things follow that a per-instruction total cannot give you:

- **Contention lands at the right T-state.** The delay depends on *when within the
  instruction* each access happens. `LD (HL),A` is contended twice, at `PC` and then at
  `HL`, six T-states apart.
- **The ULA sees writes in the right order.** `INC (HL)` reads, computes, then writes,
  and the write is the last M-cycle — which is what determines when a change to the
  display file becomes visible.
- **A scanline or beam renderer becomes a change of `Bus::tick`, not a rewrite.**

The generated T-state column in [z80-instruction-set.md](z80-instruction-set.md) then
serves as a **test oracle** rather than as the implementation: if the M-cycle sequences
are right, the totals match it, and any mismatch is a real bug. The correspondence with
the c.s.s FAQ's `pc:4,hl:3` contention notation is one-to-one, which makes both the
implementation and the review of it straightforward.

**Symbol table harvested from the annotated disassembly.** The listing has ~2000
`;; NAME` / `Lxxxx:` pairs. A build script turns them into `&[(u16, &str)]`, so traces
read `CALL $0D6B ; CLS`. Highest debugging value per line of code in the whole project.

**Contention switched on at phase H, but the model is there from phase B.** The
accounting above is identical whether `contention()` returns a delay or zero. Deferring
contention is therefore deferring one function and one flag, not deferring the design.

**Host keyboard, not a clickable photograph.** A departure from
[the original brief](original-prompt.md), which asked for a keyboard built from an image
of the real machine. A photograph needs a picking mask, a highlight shader, sticky shift
keys — one pointer cannot hold CAPS SHIFT and `0` at once — and a share-alike obligation
on the image, all in service of a slower way to type than the keyboard already under the
user's hands. The emulator wants eight bytes; the host keyboard produces them directly,
and `zxheadless` already types `PRINT 2+2` down that same path. See [ui.md](ui.md) §4.

## Risks

| Risk | Mitigation |
|---|---|
| **Silent CPU bugs** — a wrong flag surfaces as a game misbehaving 20 minutes in | Phase B is non-negotiable. The SingleStepTests vectors find these in seconds; hand-debugging finds them in days |
| **Per-instruction cycle totals instead of machine cycles** — the tempting shortcut in phase B. Contention then cannot be placed, and phase H becomes a rewrite of every instruction | Build the five M-cycle primitives *first*, in `z80/cycles.rs`, and make every instruction spend time only through them. The SingleStepTests vectors include cycle-by-cycle bus activity, so a shortcut fails phase B rather than surviving to phase H |
| **Frame-boundary drift** — resetting the T-state counter to zero at each frame, when instructions almost never land exactly on 69888 | Carry the overshoot: `frame_t -= 69888`. Otherwise timing slips a few T every frame |
| **Timing drift** — running one emulated frame per rendered frame | Time accumulator in [ui.md](ui.md) §2. 69888 T at 3.5 MHz is 19.968 ms, not 20 ms, and not the host refresh rate |
| **Blurry screen** — linear filtering and fractional scaling destroy 1-pixel strokes | Nearest sampling plus integer scale factors, verified in UI step 1 before the CPU is involved |
| **Bevy 0.19 API drift** — 0.19 postdates the API details assumed in the sketches | Treat [ui.md](ui.md) code as structural. Check names against the 0.19 migration guide; the architecture does not depend on them |
| **Docs and code drifting apart** | The decode fixture test diffs the implementation against the generated spec tables, so drift is a test failure rather than a stale document |
| **ROM redistribution** — Amstrad permit bundling, they did not release to public domain | Keep the ROM's internal copyright notice intact; state the permission in `CREDITS.md`; do not claim public domain |

## Immediate next steps

1. `cargo add bevy@0.19` and confirm a window opens on this machine — get the graphics
   stack out of the way before it can be confused with an emulator bug.
2. Write `z80/decode.rs` and `z80/disasm.rs` (phase A).
3. Extend `doc/tools/gen_z80.py` with a `--rust-fixture` mode and wire up the
   round-trip test.
4. Write the build script that harvests the ROM symbol table from
   `doc/ref/Spectrum48-disassembly.asm`.
5. Write `z80/cycles.rs` — the five M-cycle primitives — **before** any of `exec.rs`, so
   there is no path by which an instruction can spend a T-state any other way.
6. Vendor the SingleStepTests vectors for phase B, and assert on their per-cycle bus
   activity from the outset, not just the register and memory end-state.
