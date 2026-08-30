# Asset credits and licences

## `zx-spectrum-48k.jpg`

Photograph of a Sinclair ZX Spectrum 48K. **Not used by the emulator** — the front end
takes its input from the host keyboard rather than from a clickable image, so this is
kept only as a reference picture of the machine. It is still redistributed in this
repository, so the attribution below still stands.

- **Author:** Bill Bertram (Wikimedia Commons user
  [Pixel8](https://commons.wikimedia.org/wiki/User:Pixel8))
- **Source:** [File:ZXSpectrum48k.jpg](https://commons.wikimedia.org/wiki/File:ZXSpectrum48k.jpg)
- **Licence:** [CC BY-SA 2.5](https://creativecommons.org/licenses/by-sa/2.5/)

**Obligations.** Attribution for redistributing the file, which is this entry. The
share-alike condition binds *modified versions of the image* and would have applied to a
key-picking mask derived from it — which is one reason the front end does not have one.
It never applied to the emulator's source code. Delete the file and this entry together
if the reference picture stops being wanted.

---

## `../roms/48.rom`

The Sinclair ZX Spectrum 48K ROM.

- **Size:** 16384 bytes
- **CRC32:** `ddee531f`
- **SHA-1:** `5ea7c2b824672e914525d1d5c419d71b84a426a2`
- **MD5:** `4c42a2f075212361c3117015b107ff68`

Copyright Amstrad plc, who acquired the Sinclair computer rights in 1986. Amstrad have
given permission for the Spectrum ROMs to be redistributed with emulators provided the
copyright notice within the ROM image remains intact. They have **not** placed the ROMs
in the public domain.

---

## `../doc/ref/Spectrum48-disassembly.asm`

Annotated assembly listing of the 48K ROM, from
[ZXSpectrumVault/rom-disassemblies](https://github.com/ZXSpectrumVault/rom-disassemblies),
derived from the work of Dr Ian Logan and Dr Frank O'Hara (*The Complete Spectrum ROM
Disassembly*) and Geoff Wearmouth. Used here as documentation and as the source of the
symbol table for the debugger.

---

## `../doc/ref/48kreference.htm`, `../doc/ref/z80reference.htm`

Sections of the comp.sys.sinclair FAQ, maintained by Philip Kendall. See the
[FAQ copyright notice](https://worldofspectrum.org/faq/copyright.htm) for its
distribution terms.

---

## `../tests/vectors/z80/` (not redistributed)

[SingleStepTests/z80](https://github.com/SingleStepTests/z80) — 1604 files of per-opcode
test vectors, 1000 cases each, giving initial and final CPU state, memory, and the bus
activity of every T-state. Generated from the Ares Z80 core by the JSMoo project.

- **Licence:** MIT
- **Fetched by:** [`tests/vectors/fetch.sh`](../tests/vectors/fetch.sh)

About 1.3 GB expanded, so they are downloaded on demand rather than vendored, and
`tests/z80_json.rs` skips if they are absent.
