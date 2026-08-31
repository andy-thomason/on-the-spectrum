# Getting a game running

Stage **G** of [initial-plan.md](initial-plan.md): loading a real program and playing it.

The emulator half is done and tested — the CPU passes 1,604,000 per-opcode vectors down
to the bus state of each T-state, and the machine boots the ROM to a BASIC prompt that
answers `PRINT 2+2`. Nothing in this document needs any of that changed. What is missing
is a way to get a game *in*, and sound once it is there.

## What is already in place

| Needed to run a game | State |
|---|---|
| Z80 core, including the undocumented behaviour | Done, vector-verified |
| 64K with ROM write protection | Done |
| Port `0xFE`: border, keyboard, EAR readback | Done |
| 50 Hz interrupt, frame timing, `HALT` | Done |
| Display file → RGBA, FLASH | Done (whole-frame renderer) |
| Keyboard matrix, with ghosting | Done |
| Snapshot loading | `.sna` and `.z80` done — [snapshot.rs](../src/spectrum/snapshot.rs) |
| Tape loading | **Missing** |
| Beeper | **Missing** |
| Kempston joystick | **Missing** — and mis-reads today, see §6 |
| Contention | Not needed for most games; stage H |

New modules, in the layout [boot-and-test.md](boot-and-test.md) §2 already anticipates:
`spectrum/snapshot.rs` (**done**, `.sna` only), `spectrum/tape.rs`, and a beeper that
lives in `spectrum/ula.rs` beside the port it comes out of.

Both loaders have now been read a real file: an `#[ignore]`d test in the menu module
fetches the catalogue, downloads the first few items and loads them, which is how Jetpac,
Atic Atac and Manic Miner first came up on the screen.

    cargo test --features ui --bin zxbevy -- --ignored --nocapture

What that turned up: the snapshots in that collection are often taken at a title screen
that is sitting in the ROM's `PAUSE` loop, so what you see is the game's own artwork
rather than gameplay. Getting past it wants the keyboard, which works, and in some cases
the tape, which does not — §3.

One thing §1 turned up that was worth knowing before writing the `.z80` loader: a
round-trip through `.sna` is *not* byte-identical in RAM. The format has PC pushed onto
the stack, so the two bytes below `SP` come back holding it. They are free stack and no
program can tell, but a test that compares all 49152 bytes has to know.

## 1. `.sna` — the shortest path to a game on screen

A 48K `.sna` is 27 bytes of header and then the whole of RAM. Everything it restores
into already exists, which makes this the smallest possible step from here to a game.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | `I` |
| 1 | 8 | `HL'`, `DE'`, `BC'`, `AF'` |
| 9 | 10 | `HL`, `DE`, `BC`, `IY`, `IX` |
| 19 | 1 | Interrupt: bit 2 holds `IFF2` |
| 20 | 1 | `R` |
| 21 | 4 | `AF`, `SP` |
| 25 | 1 | Interrupt mode |
| 26 | 1 | Border colour |
| 27 | 49152 | RAM, `0x4000`–`0xFFFF` |

Word values are little-endian. Total length is exactly 49179 bytes; anything else is not
a 48K `.sna`.

**There is no `PC` field.** The snapshot was taken inside an interrupt, so the return
address is on the game's own stack: after restoring everything, pop `PC` from `SP` and
add 2. Set `IFF1 = IFF2`. Restore the border through the ULA rather than writing the
field somewhere, so a paused machine still shows the right frame.

Write it as `snapshot::load_sna(&mut Machine, &[u8]) -> Result<(), Error>` — it needs the
CPU registers and the memory, so it takes the machine, and it must poke straight through
the ROM protection.

**Test it without a window:** load a snapshot, run a few frames, and assert the screen
renders something other than an empty field — [tests/render.rs](../tests/render.rs)
already has the machinery. A snapshot of a known screen is the best fixture, but `.sna`
files are somebody else's copyright, so build the fixture instead: boot the ROM, type
something, save the machine state out as `.sna`, load it back, and assert the screen and
every register match. A round-trip needs no third-party file and tests both directions.

## 2. `.z80` — the format you will actually find

Worth doing straight after `.sna`, because it restores through the same path.

- **Version 1**: a 30-byte header, uncompressed or RLE'd 48K.
- **Versions 2 and 3**: `PC` at offset 6 reads as zero, and an extra header length word
  at offset 30 says which. RAM arrives in per-page blocks with their own headers.
- **RLE**: `ED ED count byte`, for runs of five or more, and for runs of `ED` from two
  upwards. A version 1 compressed block ends with `00 ED ED 00` — a zero count, which no
  real run can have.
- **The rule that is easy to miss**: a literal `ED` must always be followed by a literal
  byte, never by an encoded run. Otherwise that `ED` sits against the `ED ED` of the run
  and no decoder can tell which of the three EDs opens the marker. Since a run of two or
  more EDs is always encoded, the byte after a literal `ED` is never another `ED`, so
  emitting it literally is always safe. Getting this wrong in an *encoder* produces files
  that every emulator mis-loads.

The 128K pages in a version 2/3 file are the one part with nowhere to go on a 48K
machine — reject those with a clear error rather than loading half of them.

## 3. `.tap` through a ROM trap

The ROM's `LD-BYTES` at `L0556` is the only route a standard-speed game takes into
memory. Trap it: when `PC` reaches `0x0556`, take the next block from the `.tap`, verify
the flag byte and checksum, write the data straight into memory, set the carry flag to
signal success, and `RET`. About the same size as the `.sna` loader and it will load most
standard-speed games.

A `.tap` is a flat sequence of `length` (two bytes, little-endian) then that many bytes of
block, the first of which is a flag (`0x00` header, `0xFF` data) and the last a checksum
that XORs everything to zero. The 17-byte header layout is in
[spectrum-memory-map.md](spectrum-memory-map.md) §Tape format.

**What this will not load:** anything with its own loader — Speedlock, Alkatraz, and most
commercial titles from 1985 onward. They never call `LD-BYTES`, so the trap never fires.
That is the honest limit of the approach, and the reason for §4.

## 4. Pulse-level tape — deferred, and why

Real loading means driving bit 6 of port `0xFE` from the T-state clock: 2168 T leader
pulses, 667 and 735 T sync, then 855 T per half-bit for a `0` and 1710 T for a `1`, all
of it timed against `frame_t` rather than against block boundaries. The numbers are in
[spectrum-memory-map.md](spectrum-memory-map.md) §Tape format.

This is the right way and it loads everything, but a custom loader measures pulse widths
with `IN`-and-count loops, so it wants **contention** implemented to be reliable. Leave it
until stage H is under way, and reach for a `.z80` snapshot of an already-loaded game in
the meantime.

## 5. Sound

The beeper is one bit — bit 4 of `OUT (0xFE)`, which `Ula::write_port` already sees. The
emulation is small:

1. Record `(frame_t, level)` transitions into a per-frame buffer as they are written.
2. At the end of a frame, integrate that into samples at the host rate — 44100 Hz means
   about 882 samples per frame, each one the average level across the 79.2 T-states it
   covers. Averaging rather than sampling is what stops a 1-bit beeper from aliasing into
   a screech.
3. Push the samples into a ring buffer the audio callback drains.

**What to check it against.** Manic Miner in-game, measured through a bus that tallied
the port writes:

| | |
|---|---|
| Writes to port `0xFE` | 10752 per second |
| Of those, ones that *change* bit 4 | 124 per second |
| Half-cycles | 3240 T (540 Hz), 3440 T (509 Hz), 3840 T (456 Hz) |
| Gaps between bursts | ~220600 T |

That is "In the Hall of the Mountain King": bursts of about thirty transitions making a
28 ms note, eleven or so notes a second, with the game loop running in the silences. Two
things follow. Ninety-eight per cent of the writes to the port do not move the speaker at
all, so a beeper that resamples on every `OUT` will do a great deal of nothing. And the
shortest half-cycle is 0.93 ms against 22.7 µs for a 44100 Hz sample, so the sample rate
is nowhere near the difficulty.

**The plumbing is the hard part, not the emulation.** `bevy_audio` is built for playing
assets, not for streaming a buffer generated every 20 ms, and it is not in the `2d`
feature set the UI uses. Expect to add `cpal` (or `rodio`) directly and to spend the time
on the seam rather than the signal: the emulator produces 19.968 ms of audio per frame
while the device asks for buffers on its own schedule, so the ring buffer needs enough
slack to absorb the difference without either underrunning into clicks or drifting into
latency. Budget more for this than for all three loaders together, and treat the sample
rate as the thing to tune last.

## 6. Two small things worth bundling in

**The Kempston joystick, port `0x1F`.** Bits 0–4 are right, left, down, up, fire, and
they are **active high**. Today `SpectrumBus::in_port` returns `0xFF` for every odd port,
so a game polling Kempston reads every direction and fire held down at once — which will
send it haywire before you have plugged anything in. Fix that with the joystick itself:
return `0x00` when no joystick is attached, and the five bits when one is. The Sinclair
joysticks need no work at all, being keyboard rows 3 and 4.

**A file to load.** Done: a path on the command line for `zxbevy`, `--load` for
`zxheadless` so a snapshot's screen can be dumped as text or PPM without a window.
Drag-and-drop is a Bevy `FileDragAndDrop` event and can come later.

**A startup menu.** Also done, and it is where the files come from now: `zxbevy` opens
with a list drawn on the Spectrum's own screen in the ROM's font
([`spectrum::text`](../src/spectrum/text.rs)), fetched live from the Internet Archive's
ZX Spectrum collection. Arrows or `1`–`9` to choose, `R` for another page, `ESC` to boot
the ROM instead. Snapshots already in `games/` are listed first, so it works with no
network at all. Nothing is checked into this repository: no URLs to rot, nothing of
anyone else's in the source, and the choice of what to download stays with whoever is
sitting in front of it. `games/` is not tracked.

## 7. What you do not need

**Contention.** Most games run correctly without it. It matters for multicolour effects,
for pulse-level tape loading, and for anything that counts T-states, which is exactly why
it is stage H and why the hooks — `contention_enabled`, `Bus::contention` and the single
`Bus::tick` — are already in place and returning zero.

**A beam renderer.** The whole-frame renderer draws the display file as it stands at the
end of a frame. Games that change the border or the attributes part-way down the screen
will look wrong; they are the minority, and the fix is the same phase H work.

## 8. Order

| Step | Deliverable | Proof |
|---|---|---|
| ✅ 1 | `.sna` load and save | Round-trip: save the booted machine, reload it, every register and the screen match. [tests/snapshot.rs](../tests/snapshot.rs) |
| ✅ 2 | A file argument on `zxbevy` and `zxheadless` | `zxbevy game.z80`, and `zxheadless --load`/`--save-sna` |
| ✅ 3 | `.z80` versions 1–3, 48K only | Every header field at its offset, RLE against an independent encoder, pages placed by number, 128K refused |
| 4 | Kempston, and `0x00` for unattached ports | A game that auto-detects a joystick behaves |
| 5 | `.tap` through the `L0556` trap | A standard-speed game loads from tape |
| 6 | Beeper through `cpal` | The ROM's key click, then a game's music |

Steps 1 and 2 are one sitting and they are the demo — a real game in the window derisks
everything after them.

**One warning.** The `Tracer` of [boot-and-test.md](boot-and-test.md) §7 still does not
exist, and a game that loads to a black screen or a hang is precisely what the ring
buffer and the watchpoints were designed for. Today that would be debugged with
[examples/trace.rs](../examples/trace.rs) and printf. Consider building §7 first if step 1
does not work immediately.
