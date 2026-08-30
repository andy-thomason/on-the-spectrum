# Reference material

Primary sources for the emulator, mirrored so the design documents cite something
stable. Each is the work of its respective author, retained here for reference; see
[../../assets/CREDITS.md](../../assets/CREDITS.md) for full attribution.

## Bundled in this repository

| File | What it is | Terms |
|---|---|---|
| [`Spectrum48-disassembly.asm`](Spectrum48-disassembly.asm) | Fully annotated 20340-line disassembly of the 48K ROM, from [ZXSpectrumVault/rom-disassemblies](https://github.com/ZXSpectrumVault/rom-disassemblies). Derived from Logan & O'Hara's *The Complete Spectrum ROM Disassembly* and Geoff Wearmouth's listings. Also the source of the debugger's symbol table | Freely circulated for study |
| [`48kreference.htm`](48kreference.htm) / `.txt` | comp.sys.sinclair FAQ, 48K hardware section — the canonical document for ULA timing, contention and port `0xFE` | See the [FAQ copyright notice](https://worldofspectrum.org/faq/copyright.htm) |
| [`z80reference.htm`](z80reference.htm) / `.txt` | comp.sys.sinclair FAQ, Z80A section — undocumented flags, `DAA`, the `R` register, interrupt behaviour | As above |
| [`z80-decoding.htm`](z80-decoding.htm) / `.txt` | Cristian Dinu, *Decoding Z80 Opcodes* rev. 2 — the algorithmic decode the interpreter mirrors | Freely circulated |
| [`z80ins.txt`](z80ins.txt) | Per-instruction machine-cycle breakdown (OCF/MR/MW/IO), scanned from *Microprocessor Technology* | Freely circulated |
| [`z80oplist.txt`](z80oplist.txt) | J.G. Harston's opcode list. **Note:** its `ED` column is BBC-MOS specific — ignore it | Freely circulated |

## Not bundled — fetch locally

These two are copyright their authors and are not formally licensed for redistribution,
so they are gitignored. Run [`fetch.sh`](fetch.sh) to download them:

```sh
doc/ref/fetch.sh
```

| File | What it is | Source |
|---|---|---|
| `z80cpu_um.pdf` | Zilog *Z80 CPU User Manual* — the official instruction descriptions | <http://www.z80.info/zip/z80cpu_um.pdf> |
| `z80-documented.pdf` | Sean Young, *The Undocumented Z80 Documented* — flags, MEMPTR, undocumented opcodes. The single most useful document for emulator authors | <https://github.com/floooh/emu-info/blob/master/z80/z80-documented.pdf> |

Everything the design documents actually depend on is either bundled above or
reproduced inline in [`../z80-instruction-set.md`](../z80-instruction-set.md), so the
repository is self-contained without them. Fetch them when you need the prose.
