# Original prompt

Verbatim record of the prompt that started this project (2026-08-30).

---

I wish to build a sinclair spectrum emulator.

Create a detailed plan in doc/initial-plan.md
* Fetch details of the z80 instruction set and save as an md file.
* Fetch the memory map of the Sinclair Spectrum 48k and save as an md
* Fetch the binary of the Spectrum ROM and any annotated disassembly or source code.
* Get the spectrum keyboard layout.
* Get details of how the video buffer works

in: doc/boot-and-test.md

We will create a traceable Z80 emulator from the instruction spec consisting of a loop and match interpreter. We need a Z80 disassembler for tracing and debugging.
We will test by booting the Spectrum ROM

in: doc/ui.md

We will use bevy to construct a UI consisting of the screen display and a clickable keyboard usin an image of the original device.

Save this prompt in doc/original-prompt.md
