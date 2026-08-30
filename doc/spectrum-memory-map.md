# ZX Spectrum 48K memory map and I/O

Sources: [ref/48kreference.htm](ref/48kreference.htm) (comp.sys.sinclair FAQ, the
canonical hardware reference) and the annotated ROM listing
[ref/Spectrum48-disassembly.asm](ref/Spectrum48-disassembly.asm).

## Machine summary

| | |
|---|---|
| CPU | Zilog Z80A @ **3.5 MHz** |
| ROM | 16 KiB at `0x0000`–`0x3FFF` |
| RAM | 48 KiB at `0x4000`–`0xFFFF` (16 KiB model: `0x4000`–`0x7FFF` only) |
| Contended RAM | `0x4000`–`0x7FFF` (the ULA shares this bank) |
| Frame | 69888 T-states = 312 lines × 224 T-states → interrupt at **50.08 Hz** |
| Interrupt | IM 1, `/INT` held low 32 T-states, handler at `0x0038` |
| I/O | One port: `0xFE` (any **even** port address selects the ULA) |

## Top-level map

```
0x0000 ┌───────────────────────────────────────┐
       │ ROM (16K)                             │
       │   0x0000 RST 00 — START / reset       │
       │   0x0008 RST 08 — ERROR-1             │
       │   0x0010 RST 10 — PRINT-A-1           │
       │   0x0018 RST 18 — GET-CHAR            │
       │   0x0020 RST 20 — NEXT-CHAR           │
       │   0x0028 RST 28 — FP-CALC             │
       │   0x0030 RST 30 — BC-SPACES           │
       │   0x0038 RST 38 — MASK-INT (IM1 ISR)  │
       │   0x0066 NMI handler                  │
       │   0x3D00 character set (96 × 8)       │
0x4000 ├───────────────────────────────────────┤ ── contended RAM begins
       │ Display file       6144 bytes         │  0x4000–0x57FF
0x5800 ├───────────────────────────────────────┤
       │ Attributes          768 bytes         │  0x5800–0x5AFF
0x5B00 ├───────────────────────────────────────┤
       │ Printer buffer      256 bytes         │  0x5B00–0x5BFF
0x5C00 ├───────────────────────────────────────┤
       │ System variables    182 bytes         │  0x5C00–0x5CB5
0x5CB6 ├───────────────────────────────────────┤
       │ Microdrive maps / channel information │  (CHANS)
       │ BASIC program                         │  (PROG)
       │ Variables                             │  (VARS)
       │ Edit line / work space                │  (E_LINE, WORKSP)
       │ Calculator stack                      │  (STKBOT → STKEND)
       │ ─── free memory ───                   │
       │ Machine stack (grows down from RAMTOP)│  ← SP
       │ GOSUB stack                           │
       │ User-defined graphics (21 chars)      │  (UDG, top of RAM)
0x8000 ├───────────────────────────────────────┤ ── contention ends
       │ Uncontended RAM (48K models)          │  0x8000–0xFFFF
0xFFFF └───────────────────────────────────────┘
```

At power-on the ROM sets `RAMTOP = 0x5CCB + …` and, on a 48K machine,
`P-RAMT = 0xFFFF`, `UDG = 0xFF58`, `RAMTOP = 0x5CCB`-relative — see
`NEW`/`START-NEW` at `L11CB` in the disassembly.

### ROM restart vectors

| Address | Name | Purpose |
|---|---|---|
| `0x0000` | START | `DI` / `XOR A` / `LD DE,0xFFFF` / `JP 0x11CB` — cold start. Also the target of `PRINT USR 0` |
| `0x0008` | ERROR-1 | Fetch the error code that follows the `RST 08` and jump to the error handler |
| `0x0010` | PRINT-A-1 | Print the character in `A` to the current channel |
| `0x0018` | GET-CHAR | Fetch the character addressed by `CH_ADD` |
| `0x0020` | NEXT-CHAR | Advance `CH_ADD` and fetch |
| `0x0028` | FP-CALC | Entry to the floating-point calculator (RPN byte-code interpreter) |
| `0x0030` | BC-SPACES | Create `BC` free bytes in the work space |
| `0x0038` | MASK-INT | The IM 1 interrupt handler: bumps `FRAMES` and scans the keyboard |
| `0x0066` | NMI | Reads `NMIADD`; returns unless it holds a non-zero address (buggy in the 48K ROM) |
| `0x0D6B` | CLS | |
| `0x1601` | CHAN-OPEN | |
| `0x203C` | SA-BYTES / tape save | |
| `0x0556` | LD-BYTES / tape load | |
| `0x3D00` | Character bitmaps for codes 32–127, 8 bytes each |

## Screen memory

`0x4000`–`0x57FF` pixel data, `0x5800`–`0x5AFF` attributes. The layout is
**non-linear**; full detail in [spectrum-video.md](spectrum-video.md).

```
pixel byte address:   0 1 0 T T S S S  R R R C C C C C
                      │ └┬┘ └─┬─┘ └┬┘  └─┬┘ └───┬───┘
                      │  │    │    │     │      └─ C = column 0..31
                      │  │    │    │     └──────── R = char row within third 0..7
                      │  │    │    └────────────── S = scanline within char 0..7
                      │  │    └─────────────────── T = third of screen 0..2
                      │  └──────────────────────── constant 010 → base 0x4000
attribute address:    0 1 0 1 1 0 T T  R R R C C C C C   → base 0x5800
```

## System variables

`0x5C00`–`0x5CB5`. The ROM keeps `IY = 0x5C3A` (`ERR_NR`) throughout, so most
accesses appear as `(IY+n)`. The full list (from the Sinclair manual Appendix and
confirmed against the disassembly):

| Address | Name | Bytes | Purpose |
|---|---|---|---|
| `5C00` | KSTATE | 8 | Keyboard debounce state, two 4-byte sets |
| `5C08` | LAST_K | 1 | Code of the last key pressed |
| `5C09` | REPDEL | 1 | Delay before auto-repeat (frames, default 35) |
| `5C0A` | REPPER | 1 | Auto-repeat period (frames, default 5) |
| `5C0B` | DEFADD | 2 | Address of arguments of a user-defined function |
| `5C0D` | K_DATA | 1 | Second byte of a colour control sequence |
| `5C0E` | TVDATA | 2 | Colour/AT/TAB control data |
| `5C10` | STRMS | 38 | Stream→channel offset table (streams −3..15) |
| `5C36` | CHARS | 2 | Character set base − 256 (default `0x3C00`) |
| `5C38` | RASP | 1 | Length of warning buzz |
| `5C39` | PIP | 1 | Length of keyboard click |
| `5C3A` | ERR_NR | 1 | Error code − 1 (`0xFF` = OK). **`IY` points here** |
| `5C3B` | FLAGS | 1 | Assorted BASIC control flags |
| `5C3C` | TV_FLAG | 1 | Flags for the display handler |
| `5C3D` | ERR_SP | 2 | Stack address of the current error return |
| `5C3F` | LIST_SP | 2 | Return address from automatic listing |
| `5C41` | MODE | 1 | Cursor mode: K, L, C, E or G |
| `5C42` | NEWPPC | 2 | Line to jump to |
| `5C44` | NSPPC | 1 | Statement within that line |
| `5C45` | PPC | 2 | Line number of the statement being executed |
| `5C47` | SUBPPC | 1 | Number of that statement within the line |
| `5C48` | BORDCR | 1 | Border colour × 8, plus attributes for the lower screen |
| `5C49` | E_PPC | 2 | Line number of the line with the program cursor |
| `5C4B` | VARS | 2 | Address of the variables area |
| `5C4D` | DEST | 2 | Address of the variable being assigned |
| `5C4F` | CHANS | 2 | Address of the channel information area |
| `5C51` | CURCHL | 2 | Address of the channel currently in use |
| `5C53` | PROG | 2 | Address of the BASIC program |
| `5C55` | NXTLIN | 2 | Address of the next line in the program |
| `5C57` | DATADD | 2 | Address of the terminator of the last DATA item read |
| `5C59` | E_LINE | 2 | Address of the command being typed in |
| `5C5B` | K_CUR | 2 | Address of the cursor within the edit line |
| `5C5D` | CH_ADD | 2 | Address of the next character to interpret |
| `5C5F` | X_PTR | 2 | Address of the character after the `?` error marker |
| `5C61` | WORKSP | 2 | Address of temporary work space |
| `5C63` | STKBOT | 2 | Address of the bottom of the calculator stack |
| `5C65` | STKEND | 2 | Address of the start of spare space |
| `5C67` | BREG | 1 | Calculator's B register |
| `5C68` | MEM | 2 | Address of the calculator's memory area |
| `5C6A` | FLAGS2 | 1 | More flags |
| `5C6B` | DF_SZ | 1 | Number of lines in the lower part of the screen |
| `5C6C` | S_TOP | 2 | Top program line in an automatic listing |
| `5C6E` | OLDPPC | 2 | Line number to which CONTINUE jumps |
| `5C70` | OSPPC | 1 | Statement number for that line |
| `5C71` | FLAGX | 1 | Yet more flags |
| `5C72` | STRLEN | 2 | Length of a string type destination |
| `5C74` | T_ADDR | 2 | Address of the next item in the syntax table |
| `5C76` | SEED | 2 | Seed for `RND`, set by `RANDOMIZE` |
| `5C78` | FRAMES | 3 | Frame counter, incremented every 20 ms by the ISR |
| `5C7B` | UDG | 2 | Address of the first user-defined graphic |
| `5C7D` | COORDS | 2 | x, y coordinate of the last point plotted |
| `5C7F` | P_POSN | 1 | 33 − column number of the printer position |
| `5C80` | PR_CC | 2 | Address of the next position in the printer buffer |
| `5C82` | ECHO_E | 2 | 33 − column, 24 − line number of the end of the input buffer |
| `5C84` | DF_CC | 2 | Address in the display file of the print position |
| `5C86` | DF_CCL | 2 | Ditto for the lower part of the screen |
| `5C88` | S_POSN | 2 | 33 − column, 24 − line of the print position |
| `5C8A` | S_POSNL | 2 | Ditto for the lower part |
| `5C8C` | SCR_CT | 1 | Scroll counter: lines to scroll before "scroll?" |
| `5C8D` | ATTR_P | 1 | Permanent current colours |
| `5C8E` | MASK_P | 1 | Permanent transparency mask |
| `5C8F` | ATTR_T | 1 | Temporary current colours |
| `5C90` | MASK_T | 1 | Temporary transparency mask |
| `5C91` | P_FLAG | 1 | More flags (OVER, INVERSE, PAPER 9, INK 9) |
| `5C92` | MEMBOT | 30 | Calculator's memory area (six 5-byte numbers) |
| `5CB0` | NMIADD | 2 | NMI address (unused by the 48K ROM) |
| `5CB2` | RAMTOP | 2 | Address of the last byte of BASIC system area |
| `5CB4` | P_RAMT | 2 | Address of the last byte of physical RAM (`0xFFFF` on 48K) |

## I/O port `0xFE`

Address decoding is **partial**: the ULA responds to every port with **A0 = 0**, i.e.
every even address. Software should nonetheless use `0xFE`. In the emulator, decode
as `if (port & 1) == 0 { ula }`.

### Write — `OUT (0xFE),A`

```
 bit  7   6   5   4   3   2   1   0
    ┌───┬───┬───┬───┬───┬───┬───┬───┐
    │ - │ - │ - │EAR│MIC│  BORDER   │
    └───┴───┴───┴───┴───┴───┴───┴───┘
```

- bits 0–2: border colour (0–7, never bright).
- bit 3: MIC output (0 = active). Tape save.
- bit 4: EAR output and the internal speaker (1 = active). Beeper.
- bits 5–7: unused.

EAR and MIC share ULA pin 28, so writing either affects the level read back on bit 6
of an `IN`.

### Read — `IN A,(0xFE)`

```
 bit  7   6   5   4   3   2   1   0
    ┌───┬───┬───┬───┬───┬───┬───┬───┐
    │ 1 │EAR│ 1 │      keyboard     │
    └───┴───┴───┴───┴───┴───┴───┴───┘
```

- bits 0–4: the five keys of the selected half-row(s), **0 = pressed**.
- bit 5: always 1.
- bit 6: EAR input (tape load), and it reflects the last EAR/MIC write.
- bit 7: always 1.

The **high byte of the port address** selects the half-rows: a 0 in bit *n* of the
high byte selects half-row *n*. Multiple zeros AND the results together. See
[spectrum-keyboard.md](spectrum-keyboard.md).

### Floating bus

Reading an unattached port (e.g. `0xFF`) while the ULA is fetching display data
returns the byte the ULA just read, otherwise `0xFF`. Used by Arkanoid and others
for frame synchronisation. Low priority for a first emulator; note it as a TODO.

## Contention

The ULA has priority on `0x4000`–`0x7FFF` while it is drawing the 256×192 display
area. During border and retrace there is no contention.

Delay by T-state within the frame (T=0 is the moment `/INT` goes low):

| T-state | Delay |
|---|---|
| 14335 | 6 |
| 14336 | 5 |
| 14337 | 4 |
| 14338 | 3 |
| 14339 | 2 |
| 14340 | 1 |
| 14341 | 0 |
| 14342 | 0 |
| 14343 | 6 (pattern repeats every 8 T) |

The 8-T-state pattern `6,5,4,3,2,1,0,0` runs for 128 T-states per line (the 32
character columns), then 96 T-states of border/retrace with no delay, then repeats.
This holds for all 192 display lines. A compact implementation:

```rust
fn contention_delay(t: u32) -> u32 {
    if !(14335..14335 + 192 * 224).contains(&t) { return 0; }
    let line_t = (t - 14335) % 224;
    if line_t >= 128 { return 0; }          // border / retrace
    [6, 5, 4, 3, 2, 1, 0, 0][(line_t % 8) as usize]
}
```

> On 128K/+2 machines the first contended cycle is 14361, not 14335. Some 48K
> machines are one T-state later than the figures above; this is a known and
> unexplained variation.

### Contended I/O

`IN`/`OUT` normally take 4 T-states for the port access. Two effects lengthen it:

1. If the port address has **bit 0 reset**, the ULA must supply the result → delay.
2. If the **high byte** of the port address is in `0x40..0x7F`, the ULA sees what looks
   like a contended memory access → delay.

| High byte in 0x40–0x7F? | A0 | Pattern |
|---|---|---|
| No | 0 | `N:1, C:3` |
| No | 1 | `N:4` |
| Yes | 0 | `C:1, C:3` |
| Yes | 1 | `C:1, C:1, C:1, C:1` |

`N:n` = run *n* T-states with no delay. `C:n` = apply the contention delay for the
current T-state, then run *n* T-states.

### Instruction contention table

Where within each instruction the delays apply. `pc:4` means "if `PC` is in
`0x4000..0x7FFF`, insert the current contention delay, then advance 4 T-states".
Entries in `[]` apply only when the condition is met (or always, for unconditional
instructions). Verbatim from the c.s.s FAQ.

```
NOP / LD r,r' / alo A,r / INC,DEC r / EXX / EX AF,AF' / EX DE,HL
DAA / CPL / CCF / SCF / DI / EI / RLA / RRA / RLCA / RRCA / JP (HL)
                        pc:4

sro r / BIT b,r / SET b,r / RES b,r / NEG / IM 0,1,2
                        pc:4,pc+1:4

LD A,I / LD A,R / LD I,A / LD R,A
                        pc:4,pc+1:5

INC,DEC dd / LD SP,HL   pc:6
ADD HL,dd               pc:11
ADC,SBC HL,dd           pc:4,pc+1:11
LD r,n / alo A,n        pc:4,pc+1:3
LD r,(ss) / LD (ss),r   pc:4,ss:3
alo A,(HL)              pc:4,hl:3
LD r,(ii+n) / LD (ii+n),r / alo A,(ii+n)
                        pc:4,pc+1:4,pc+2:3,pc+2:1 x 5,ii+n:3
BIT b,(HL)              pc:4,pc+1:4,hl:3,hl:1
BIT b,(ii+n)            pc+1:4,pc+2:3,pc+3:3,pc+3:1 x 2,ii+n:3,ii+n:1
LD dd,nn / JP nn / JP cc,nn
                        pc:4,pc+1:3,pc+2:3
LD (HL),n               pc:4,pc+1:3,hl:3
LD (ii+n),n             pc:4,pc+1:4,pc+2:3,pc+3:3,pc+3:1 x 2,ii+n:3
LD A,(nn) / LD (nn),A   pc:4,pc+1:3,pc+2:3,nn:3
LD HL,(nn) / LD (nn),HL  (unprefixed 22/2A)
                        pc:4,pc+1:3,pc+2:3,nn:3,nn+1:3
LD dd,(nn) / LD (nn),dd  (ED-prefixed)
                        pc:4,pc+1:4,pc+2:3,pc+3:3,nn:3,nn+1:3
INC,DEC (HL)            pc:4,hl:3,hl:1,hl(write):3
SET,RES b,(HL) / sro (HL)
                        pc:4,pc+1:4,hl:3,hl:1,hl(write):3
INC,DEC (ii+n)          pc:4,pc+1:4,pc+2:3,pc+2:1 x 5,ii+n:3,ii+n:1,ii+n(write):3
SET,RES b,(ii+n) / sro (ii+n)
                        pc:4,pc+1:4,pc+2:3,pc+3:3,pc+3:1 x 2,ii+n:3,ii+n:1,ii+n(write):3
POP dd / RET            pc:4,sp:3,sp+1:3
RETI / RETN             pc:4,pc+1:4,sp:3,sp+1:3
RET cc                  pc:5,[sp:3,sp+1:3]
PUSH dd / RST n         pc:5,sp-1:3,sp-2:3
CALL nn / CALL cc,nn    pc:4,pc+1:3,pc+2:3,[pc+2:1,sp-1:3,sp-2:3]
JR n / JR cc,n          pc:4,pc+1:3,[pc+1:1 x 5]
DJNZ n                  pc:5,pc+1:3,[pc+1:1 x 5]
RLD / RRD               pc:4,pc+1:4,hl:3,hl:1 x 4,hl(write):3
IN A,(n) / OUT (n),A    pc:4,pc+1:3,IO
IN r,(C) / OUT (C),r    pc:4,pc+1:4,IO
EX (SP),HL              pc:4,sp:3,sp+1:4,sp(write):3,sp+1(write):3,sp+1(write):1 x 2
LDI,LDIR,LDD,LDDR       pc:4,pc+1:4,hl:3,de:3,de:1 x 2,[de:1 x 5]
CPI,CPIR,CPD,CPDR       pc:4,pc+1:4,hl:3,hl:1 x 5,[hl:1 x 5]
INI,INIR,IND,INDR       pc:4,pc+1:5,IO,hl:3,[hl:1 x 5]
OUTI,OTIR,OUTD,OTDR     pc:4,pc+1:5,hl:3,IO,[hl:1 x 5]
```

Notes:
- A `DD`/`FD` prefix on an instruction that does not involve `HL` just adds `pc:4`.
- The undocumented `DDCB`/`FDCB` variants have the same timings as the documented ones.
- In read-modify-write instructions the write is always last; the point is marked
  `(write)` because it determines when the display sees the change.

## Tape format

`SAVE` emits a 19-byte header block and a variable-length data block.

Each block: **8063** leader pulses (header) or **3223** (data) of 2168 T-states each;
sync pulses of 667 then 735 T; then the data — a `0` bit is two pulses of 855 T, a `1`
bit two pulses of 1710 T, LSB-first in memory, MSB-first within each byte.

Block contents: flag byte (`0x00` header, `0xFF` data), the data, then a checksum byte
such that XORing everything including the flag yields `0x00`.

17-byte header:

| Offset | Len | Field |
|---|---|---|
| 0 | 1 | Type: 0 PROGRAM, 1 number array, 2 character array, 3 CODE |
| 1 | 10 | Filename, space-padded |
| 11 | 2 | Length of the data block |
| 13 | 2 | Parameter 1 |
| 15 | 2 | Parameter 2 |

For `PROGRAM`, parameter 1 is the autostart line (≥ 32768 if none) and parameter 2 the
offset of the variables area. For `CODE`, parameter 1 is the load address and
parameter 2 is 32768. `SCREEN$` is a `CODE` file at 16384, length 6912.
