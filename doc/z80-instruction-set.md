# The Z80 instruction set

Reference spec for the emulator's interpreter and disassembler. The Spectrum uses a
**Zilog Z80A at 3.5 MHz**.

Primary sources (mirrored under [doc/ref/](ref/) — see [ref/README.md](ref/README.md) for terms and for the two PDFs that are fetched rather than bundled):

| File | What it is |
|---|---|
| [ref/z80-decoding.htm](ref/z80-decoding.htm) | Cristian Dinu, *Decoding Z80 Opcodes* rev. 2 — the algorithmic decode used below |
| `ref/z80-documented.pdf` (run [ref/fetch.sh](ref/fetch.sh)) | Sean Young, *The Undocumented Z80 Documented* — flags, MEMPTR, undocumented opcodes |
| `ref/z80cpu_um.pdf` (run [ref/fetch.sh](ref/fetch.sh)) | Zilog *Z80 CPU User Manual* — official instruction descriptions |
| [ref/z80reference.htm](ref/z80reference.htm) | comp.sys.sinclair FAQ Z80A section — undocumented flags, DAA, R register, interrupts |
| [ref/z80ins.txt](ref/z80ins.txt) | Machine-cycle breakdown (OCF/MR/MW/IO) per instruction class |
| [ref/z80oplist.txt](ref/z80oplist.txt) | J.G.Harston's opcode list (note: its `ED` column is BBC-MOS specific, ignore it) |

The opcode tables at the end of this document were **generated** from the decoding
algorithm plus the standard timing rules, and spot-checked against the sources above.
The generator is `doc/tools/gen_z80.py` — it is also the intended source for the
emulator's decode tables, so the disassembler and the interpreter cannot drift apart.

---

## 1. Programming model

### Registers

```
 Main set                Alternate set        Special
 ┌────┬────┐             ┌────┬────┐          ┌─────────┐
 │ A  │ F  │  AF         │ A' │ F' │  AF'     │   PC    │ 16-bit program counter
 ├────┼────┤             ├────┼────┤          ├─────────┤
 │ B  │ C  │  BC         │ B' │ C' │  BC'     │   SP    │ 16-bit stack pointer
 ├────┼────┤             ├────┼────┤          ├─────────┤
 │ D  │ E  │  DE         │ D' │ E' │  DE'     │   IX    │ 16-bit index (IXH:IXL)
 ├────┼────┤             ├────┼────┤          ├─────────┤
 │ H  │ L  │  HL         │ H' │ L' │  HL'     │   IY    │ 16-bit index (IYH:IYL)
 └────┴────┘             └────┴────┘          ├────┬────┤
                                              │ I  │ R  │ interrupt page / refresh
                                              └────┴────┘
 Hidden: MEMPTR (a.k.a. WZ) — 16-bit, not directly readable, observable via BIT n,(HL)
 Flip-flops: IFF1, IFF2 (interrupt enable), IM (interrupt mode 0/1/2)
```

`EX AF,AF'` swaps `AF`↔`AF'`. `EXX` swaps `BC`/`DE`/`HL` ↔ `BC'`/`DE'`/`HL'`.
The two are independent.

### Flag register `F`

| Bit | 7 | 6 | 5 | 4 | 3 | 2 | 1 | 0 |
|---|---|---|---|---|---|---|---|---|
| Name | **S** | **Z** | **5** | **H** | **3** | **P/V** | **N** | **C** |

- **S** sign — copy of bit 7 of the result.
- **Z** zero — result is zero.
- **5**, **3** — *undocumented*. Normally copies of bits 5 and 3 of the result. Must be
  emulated: Sabre Wulf, Ghosts'n'Goblins and the Speedlock loaders depend on them.
- **H** half-carry — carry out of bit 3 (used only by `DAA`).
- **P/V** parity/overflow — parity of the result for logic ops, signed overflow for
  arithmetic, `BC≠0` for block ops, `IFF2` for `LD A,I` / `LD A,R`.
- **N** add/subtract — set by subtractions, used only by `DAA`.
- **C** carry.

### Undocumented flag rules

These are the cases where the Zilog manual says "undefined". Straight from
[ref/z80reference.htm](ref/z80reference.htm):

| Instruction | Behaviour of the non-standard flags |
|---|---|
| `CP x` | **5** and **3** are copied from *the operand*, not from the result |
| `ADD HL,rp`, `ADC HL,rp`, `SBC HL,rp` | Treat as two 8-bit steps (low then high). **3, H, 5, S** come from the high step; **Z** is set only if the whole 16-bit result is zero. `ADD HL,rp` leaves **S** and **Z** alone |
| `BIT n,r` | **P/V** = **Z**. **S** is reset unless it is `BIT 7,r` and bit 7 is set |
| `BIT n,(HL)`, `BIT n,(IX+d)` | **5** and **3** come from the high byte of MEMPTR, not the operand |
| `SCF` / `CCF` / `CPL` | **5** and **3** copied from `A`. `CCF` sets **H** to the previous **C** |
| `LDI`/`LDD`/`LDIR`/`LDDR` | Let `v = transferred byte + A`. **3** = bit 3 of `v`, **5** = bit 1 of `v` |
| `CPI`/`CPD`/`CPIR`/`CPDR` | Let `v = A - (HL) - H_after`. **3** = bit 3 of `v`, **5** = bit 1 of `v` |
| `INI`/`IND`/`OUTI`/`OUTD` and repeats | **S, 5, 3** as for `DEC B`. **N** = bit 7 of the byte transferred. **C** and **H**: take `C ± 1` (+1 for the `I` forms, −1 for the `D` forms), add the byte, take the carry. **P/V** per the table in [ref/z80reference.txt](ref/z80reference.txt) |

### MEMPTR / WZ

An internal 16-bit register whose high byte leaks into flags **5** and **3** on
`BIT n,(HL)`. Update rules (Sean Young, §MEMPTR):

| Instruction | New MEMPTR |
|---|---|
| `LD A,(nn)` / `LD (nn),A` | `nn + 1` (for `LD (nn),A`, high byte = `A`) |
| `LD A,(rp)` / `LD (rp),A` | `rp + 1` (for the store, high byte = `A`) |
| `LD (nn),rp` / `LD rp,(nn)` | `nn + 1` |
| `EX (SP),HL/IX/IY` | the new value of the register |
| `ADD/ADC/SBC HL,rp` | `HL + 1` (value *before* the operation) |
| `JR`/`JP`/`CALL` taken, `RST`, interrupt | the destination address |
| `IN A,(n)` | `(A << 8) + n + 1` |
| `IN r,(C)` / `OUT (C),r` | `BC + 1` |
| `OUT (n),A` | low byte = `n + 1`, high byte = `A` |
| block ops `LDI`… `CPI`… `INI`… `OUTI`… | see Young §4.3 |

Implement MEMPTR from day one; retro-fitting it is painful and the per-opcode vectors
of [boot-and-test.md](boot-and-test.md) §8.2 check it directly, as `wz`.

### The `R` register

Incremented on every M1 (opcode fetch) cycle. **Bit 7 is never changed by the
increment** — only the low 7 bits count. So:

- Unprefixed instruction: `R += 1`
- `CB`/`DD`/`ED`/`FD`-prefixed: `R += 2` (prefix is its own M1)
- `DDCB`/`FDCB`: `R += 2` (the `CB` here is *not* an M1)
- `LDIR` etc.: `R += 2` per iteration
- Interrupt / NMI acknowledge: `R += 1`

`LD A,R` reads the value *after* the increment.

### `DAA`

The Zilog manual's table is awkward; this formulation (from the c.s.s FAQ) is exact:

```
correction = 0
if (A & 0x0F) > 9 || H:   correction |= 0x06
if A > 0x99   || C:       correction |= 0x60 ; C_out = 1  else C_out = 0
A = N ? A - correction : A + correction
H = bit 4 of (A_before XOR A_after)
C = C_out
S, Z, P, 5, 3 = as for a logic op on the new A
N = unchanged
```

---

## 2. Interrupts

The Spectrum's ULA pulls `/INT` low for **32 T-states** once per frame. `/INT` is
level-triggered and sampled during the **last M-cycle of every instruction**, except
that a run of `DD`/`FD` prefixes cannot be interrupted.

- Accepting an interrupt resets **both** `IFF1` and `IFF2`.
- An interrupt cannot be taken immediately after `EI` (IFF1 is still clear when
  sampled during `EI`'s single M-cycle), nor after `DD`/`FD`.
- `HALT` behaves as an infinite run of `NOP`s; on accepting the interrupt, `PC` is the
  address *after* the `HALT`.
- Block-repeat instructions (`LDIR` etc.) can be interrupted between iterations; `PC`
  is rewound by 2 so the instruction re-executes.

| Mode | Behaviour | T-states to reach the handler |
|---|---|---|
| IM 0 | Executes the instruction on the data bus. On a bare Spectrum the bus floats to `FF` = `RST 38h` | 12 (for a `RST`) / 19 (for a `CALL nn`) |
| IM 1 | Always `RST 38h`. **This is the mode the Spectrum ROM uses** | 13 |
| IM 2 | Vector = `(I << 8) | bus`. Bus floats to `FF`, so the vector is `256*I + 255` | 19 |

`NMI` (not wired on a stock Spectrum, but reachable via the edge connector) resets
`IFF1`, leaves `IFF2`, and jumps to `0x0066` in 11 T-states. `RETN` copies `IFF2`
back into `IFF1`. On the Z80, **all** `ED xx` return instructions do this, including
`RETI`; `RETI` differs only in the bus signalling that daisy-chained peripherals
watch for.

> Beware `I` in the range `0x40..0x7F`: the refresh address then looks to the ULA
> like a very frequent read of contended RAM, and the display shows "snow". The
> machine does not crash. Optional to emulate; see [spectrum-video.md](spectrum-video.md).

---

## 3. Decoding algorithm

This is the structure the interpreter's match arms should mirror. Split each opcode
byte into octal digits:

```
 bit   7 6   5 4 3   2 1 0
      ┌─────┬───────┬───────┐
      │  x  │   y   │   z   │
      └─────┴───────┴───────┘
              p   q            p = y >> 1  (bits 5-4)
                               q = y & 1   (bit 3)
```

```rust
let x = op >> 6;
let y = (op >> 3) & 7;
let z = op & 7;
let p = y >> 1;
let q = y & 1;
```

### Decode tables

| Table | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
|---|---|---|---|---|---|---|---|---|
| `r` | B | C | D | E | H | L | (HL) | A |
| `rp` | BC | DE | HL | SP | | | | |
| `rp2` | BC | DE | HL | AF | | | | |
| `cc` | NZ | Z | NC | C | PO | PE | P | M |
| `alu` | ADD A, | ADC A, | SUB | SBC A, | AND | XOR | OR | CP |
| `rot` | RLC | RRC | RL | RR | SLA | SRA | **SLL** | SRL |
| `im` | 0 | 0/1 | 1 | 2 | 0 | 0/1 | 1 | 2 |

`bli[y][z]` (block instructions, `ED` `x=2`, `y≥4`, `z≤3`):

| | z=0 | z=1 | z=2 | z=3 |
|---|---|---|---|---|
| **y=4** | LDI | CPI | INI | OUTI |
| **y=5** | LDD | CPD | IND | OUTD |
| **y=6** | LDIR | CPIR | INIR | OTIR |
| **y=7** | LDDR | CPDR | INDR | OTDR |

`rot[6]` is `SLL` (Shift Left Logical: shift left, bit 0 becomes 1) — undocumented
but used by real software (Bounder, Enduro Racer).

### Unprefixed

| x | z | Rule |
|---|---|---|
| 0 | 0 | `y=0` NOP · `y=1` EX AF,AF' · `y=2` DJNZ d · `y=3` JR d · `y=4..7` JR cc[y−4],d |
| 0 | 1 | `q=0` LD rp[p],nn · `q=1` ADD HL,rp[p] |
| 0 | 2 | `q=0`: LD (BC),A / LD (DE),A / LD (nn),HL / LD (nn),A · `q=1`: the loads reversed |
| 0 | 3 | `q=0` INC rp[p] · `q=1` DEC rp[p] |
| 0 | 4 | INC r[y] |
| 0 | 5 | DEC r[y] |
| 0 | 6 | LD r[y],n |
| 0 | 7 | RLCA · RRCA · RLA · RRA · DAA · CPL · SCF · CCF |
| 1 | — | LD r[y],r[z] — **except** `y=z=6`, which is HALT |
| 2 | — | alu[y] r[z] |
| 3 | 0 | RET cc[y] |
| 3 | 1 | `q=0` POP rp2[p] · `q=1`: RET / EXX / JP (HL) / LD SP,HL |
| 3 | 2 | JP cc[y],nn |
| 3 | 3 | JP nn · **CB prefix** · OUT (n),A · IN A,(n) · EX (SP),HL · EX DE,HL · DI · EI |
| 3 | 4 | CALL cc[y],nn |
| 3 | 5 | `q=0` PUSH rp2[p] · `q=1`: CALL nn / **DD prefix** / **ED prefix** / **FD prefix** |
| 3 | 6 | alu[y] n |
| 3 | 7 | RST y×8 |

### `CB`-prefixed

| x | Rule |
|---|---|
| 0 | `rot[y] r[z]` |
| 1 | `BIT y,r[z]` |
| 2 | `RES y,r[z]` |
| 3 | `SET y,r[z]` |

All 256 are valid; the `CB 30..37` block is the undocumented `SLL`.

### `ED`-prefixed

`x=0` and `x=3` are invalid: they behave as **NONI followed by NOP** (8 T-states,
`R += 2`, and interrupts are inhibited for the following instruction).

| x | z | Rule |
|---|---|---|
| 1 | 0 | `IN r[y],(C)`; `y=6` → `IN (C)` (reads and sets flags, discards the byte) |
| 1 | 1 | `OUT (C),r[y]`; `y=6` → `OUT (C),0` |
| 1 | 2 | `q=0` SBC HL,rp[p] · `q=1` ADC HL,rp[p] |
| 1 | 3 | `q=0` LD (nn),rp[p] · `q=1` LD rp[p],(nn) |
| 1 | 4 | NEG (all 8 `y` values) |
| 1 | 5 | RETN; `y=1` → RETI |
| 1 | 6 | IM im[y] |
| 1 | 7 | LD I,A · LD R,A · LD A,I · LD A,R · RRD · RLD · NOP · NOP |
| 2 | ≤3 | `y≥4` → bli[y][z]; otherwise NONI, NOP |

### `DD` / `FD`-prefixed

`DD` means "read `HL` as `IX`" for the *next* opcode; `FD` means `IY`.

1. If the next byte is `DD`, `ED` or `FD`, **the current prefix is discarded** (it acts
   as a 4 T-state NONI) and decoding restarts. A run of prefixes is therefore just a
   chain of 4-cycle NOPs that cannot be interrupted.
2. If the next byte is `CB`, decode as `DDCB`/`FDCB` (below).
3. Otherwise, in the decoded instruction:
   - `HL` → `IX`, `H` → `IXH`, `L` → `IXL`. **Exception:** `EX DE,HL` is unaffected.
   - `(HL)` → `(IX+d)`, where `d` is a **signed byte following the opcode**, before any
     immediate `n`. When this substitution happens, `H` and `L` in the *same*
     instruction are **not** substituted — hence `LD H,(IX+d)` exists but
     `LD IXH,(IX+d)` does not.
   - Anything else is unaffected, and the prefix just costs 4 T-states.

Byte order matters: for singly-shifted opcodes the displacement follows the opcode
(`DD 36 d n` = `LD (IX+d),n`); for doubly-shifted ones it precedes it.

### `DDCB` / `FDCB`-prefixed

Format is **`DD`/`FD`, `CB`, `d`, opcode** — the displacement comes *before* the
opcode byte.

| x | Rule |
|---|---|
| 0 | `z=6` → `rot[y] (IX+d)`; else `LD r[z], rot[y] (IX+d)` |
| 1 | `BIT y,(IX+d)` — the `z` field is ignored (all 8 aliases behave the same) |
| 2 | `z=6` → `RES y,(IX+d)`; else `LD r[z], RES y,(IX+d)` |
| 3 | `z=6` → `SET y,(IX+d)`; else `LD r[z], SET y,(IX+d)` |

The `LD r,op (IX+d)` forms compute the result, write it back to `(IX+d)` **and** copy
it to `r`. If `(IX+d)` is in ROM the write is lost but `r` still gets the value.

---

## 4. Timing

The T-state counts in the tables below are the *uncontended* Z80 figures. On the
Spectrum, accesses to `0x4000..0x7FFF` and to even I/O ports are additionally delayed
by the ULA — see [spectrum-video.md](spectrum-video.md) §Contention. The per-instruction
machine-cycle breakdown needed for cycle-exact contention is in
[spectrum-memory-map.md](spectrum-memory-map.md) §Instruction contention table.

Conditional instructions show `taken/not-taken`.

Prefix arithmetic:

| Form | T-states |
|---|---|
| unprefixed base | *T* |
| `DD`/`FD` + register-only substitution | *T* + 4 |
| `DD`/`FD` + `(HL)`→`(IX+d)` substitution | *T* + 12 (4 prefix + 3 displacement fetch + 5 internal add) |
| `LD (IX+d),n` | 19 — a special case, **not** 10 + 12 |
| `DDCB`/`FDCB` | 23, or 20 for `BIT` |

---

## 5. Complete opcode tables

### Opcode matrices

Quick-reference grids; the full listings with byte counts and timings follow.

### Unprefixed opcode matrix

|    | _0 | _1 | _2 | _3 | _4 | _5 | _6 | _7 | _8 | _9 | _A | _B | _C | _D | _E | _F |
|----|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **0_** | NOP | LD BC,nn | LD (BC),A | INC BC | INC B | DEC B | LD B,n | RLCA | EX AF,AF' | ADD HL,BC | LD A,(BC) | DEC BC | INC C | DEC C | LD C,n | RRCA |
| **1_** | DJNZ d | LD DE,nn | LD (DE),A | INC DE | INC D | DEC D | LD D,n | RLA | JR d | ADD HL,DE | LD A,(DE) | DEC DE | INC E | DEC E | LD E,n | RRA |
| **2_** | JR NZ,d | LD HL,nn | LD (nn),HL | INC HL | INC H | DEC H | LD H,n | DAA | JR Z,d | ADD HL,HL | LD HL,(nn) | DEC HL | INC L | DEC L | LD L,n | CPL |
| **3_** | JR NC,d | LD SP,nn | LD (nn),A | INC SP | INC (HL) | DEC (HL) | LD (HL),n | SCF | JR C,d | ADD HL,SP | LD A,(nn) | DEC SP | INC A | DEC A | LD A,n | CCF |
| **4_** | LD B,B | LD B,C | LD B,D | LD B,E | LD B,H | LD B,L | LD B,(HL) | LD B,A | LD C,B | LD C,C | LD C,D | LD C,E | LD C,H | LD C,L | LD C,(HL) | LD C,A |
| **5_** | LD D,B | LD D,C | LD D,D | LD D,E | LD D,H | LD D,L | LD D,(HL) | LD D,A | LD E,B | LD E,C | LD E,D | LD E,E | LD E,H | LD E,L | LD E,(HL) | LD E,A |
| **6_** | LD H,B | LD H,C | LD H,D | LD H,E | LD H,H | LD H,L | LD H,(HL) | LD H,A | LD L,B | LD L,C | LD L,D | LD L,E | LD L,H | LD L,L | LD L,(HL) | LD L,A |
| **7_** | LD (HL),B | LD (HL),C | LD (HL),D | LD (HL),E | LD (HL),H | LD (HL),L | HALT | LD (HL),A | LD A,B | LD A,C | LD A,D | LD A,E | LD A,H | LD A,L | LD A,(HL) | LD A,A |
| **8_** | ADD A,B | ADD A,C | ADD A,D | ADD A,E | ADD A,H | ADD A,L | ADD A,(HL) | ADD A,A | ADC A,B | ADC A,C | ADC A,D | ADC A,E | ADC A,H | ADC A,L | ADC A,(HL) | ADC A,A |
| **9_** | SUB B | SUB C | SUB D | SUB E | SUB H | SUB L | SUB (HL) | SUB A | SBC A,B | SBC A,C | SBC A,D | SBC A,E | SBC A,H | SBC A,L | SBC A,(HL) | SBC A,A |
| **A_** | AND B | AND C | AND D | AND E | AND H | AND L | AND (HL) | AND A | XOR B | XOR C | XOR D | XOR E | XOR H | XOR L | XOR (HL) | XOR A |
| **B_** | OR B | OR C | OR D | OR E | OR H | OR L | OR (HL) | OR A | CP B | CP C | CP D | CP E | CP H | CP L | CP (HL) | CP A |
| **C_** | RET NZ | POP BC | JP NZ,nn | JP nn | CALL NZ,nn | PUSH BC | ADD A,n | RST 00h | RET Z | RET | JP Z,nn | *CB prefix* | CALL Z,nn | CALL nn | ADC A,n | RST 08h |
| **D_** | RET NC | POP DE | JP NC,nn | OUT (n),A | CALL NC,nn | PUSH DE | SUB n | RST 10h | RET C | EXX | JP C,nn | IN A,(n) | CALL C,nn | *DD prefix* | SBC A,n | RST 18h |
| **E_** | RET PO | POP HL | JP PO,nn | EX (SP),HL | CALL PO,nn | PUSH HL | AND n | RST 20h | RET PE | JP (HL) | JP PE,nn | EX DE,HL | CALL PE,nn | *ED prefix* | XOR n | RST 28h |
| **F_** | RET P | POP AF | JP P,nn | DI | CALL P,nn | PUSH AF | OR n | RST 30h | RET M | LD SP,HL | JP M,nn | EI | CALL M,nn | *FD prefix* | CP n | RST 38h |

### `CB`-prefixed opcode matrix

|    | _0 | _1 | _2 | _3 | _4 | _5 | _6 | _7 | _8 | _9 | _A | _B | _C | _D | _E | _F |
|----|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **0_** | RLC B | RLC C | RLC D | RLC E | RLC H | RLC L | RLC (HL) | RLC A | RRC B | RRC C | RRC D | RRC E | RRC H | RRC L | RRC (HL) | RRC A |
| **1_** | RL B | RL C | RL D | RL E | RL H | RL L | RL (HL) | RL A | RR B | RR C | RR D | RR E | RR H | RR L | RR (HL) | RR A |
| **2_** | SLA B | SLA C | SLA D | SLA E | SLA H | SLA L | SLA (HL) | SLA A | SRA B | SRA C | SRA D | SRA E | SRA H | SRA L | SRA (HL) | SRA A |
| **3_** | SLL B | SLL C | SLL D | SLL E | SLL H | SLL L | SLL (HL) | SLL A | SRL B | SRL C | SRL D | SRL E | SRL H | SRL L | SRL (HL) | SRL A |
| **4_** | BIT 0,B | BIT 0,C | BIT 0,D | BIT 0,E | BIT 0,H | BIT 0,L | BIT 0,(HL) | BIT 0,A | BIT 1,B | BIT 1,C | BIT 1,D | BIT 1,E | BIT 1,H | BIT 1,L | BIT 1,(HL) | BIT 1,A |
| **5_** | BIT 2,B | BIT 2,C | BIT 2,D | BIT 2,E | BIT 2,H | BIT 2,L | BIT 2,(HL) | BIT 2,A | BIT 3,B | BIT 3,C | BIT 3,D | BIT 3,E | BIT 3,H | BIT 3,L | BIT 3,(HL) | BIT 3,A |
| **6_** | BIT 4,B | BIT 4,C | BIT 4,D | BIT 4,E | BIT 4,H | BIT 4,L | BIT 4,(HL) | BIT 4,A | BIT 5,B | BIT 5,C | BIT 5,D | BIT 5,E | BIT 5,H | BIT 5,L | BIT 5,(HL) | BIT 5,A |
| **7_** | BIT 6,B | BIT 6,C | BIT 6,D | BIT 6,E | BIT 6,H | BIT 6,L | BIT 6,(HL) | BIT 6,A | BIT 7,B | BIT 7,C | BIT 7,D | BIT 7,E | BIT 7,H | BIT 7,L | BIT 7,(HL) | BIT 7,A |
| **8_** | RES 0,B | RES 0,C | RES 0,D | RES 0,E | RES 0,H | RES 0,L | RES 0,(HL) | RES 0,A | RES 1,B | RES 1,C | RES 1,D | RES 1,E | RES 1,H | RES 1,L | RES 1,(HL) | RES 1,A |
| **9_** | RES 2,B | RES 2,C | RES 2,D | RES 2,E | RES 2,H | RES 2,L | RES 2,(HL) | RES 2,A | RES 3,B | RES 3,C | RES 3,D | RES 3,E | RES 3,H | RES 3,L | RES 3,(HL) | RES 3,A |
| **A_** | RES 4,B | RES 4,C | RES 4,D | RES 4,E | RES 4,H | RES 4,L | RES 4,(HL) | RES 4,A | RES 5,B | RES 5,C | RES 5,D | RES 5,E | RES 5,H | RES 5,L | RES 5,(HL) | RES 5,A |
| **B_** | RES 6,B | RES 6,C | RES 6,D | RES 6,E | RES 6,H | RES 6,L | RES 6,(HL) | RES 6,A | RES 7,B | RES 7,C | RES 7,D | RES 7,E | RES 7,H | RES 7,L | RES 7,(HL) | RES 7,A |
| **C_** | SET 0,B | SET 0,C | SET 0,D | SET 0,E | SET 0,H | SET 0,L | SET 0,(HL) | SET 0,A | SET 1,B | SET 1,C | SET 1,D | SET 1,E | SET 1,H | SET 1,L | SET 1,(HL) | SET 1,A |
| **D_** | SET 2,B | SET 2,C | SET 2,D | SET 2,E | SET 2,H | SET 2,L | SET 2,(HL) | SET 2,A | SET 3,B | SET 3,C | SET 3,D | SET 3,E | SET 3,H | SET 3,L | SET 3,(HL) | SET 3,A |
| **E_** | SET 4,B | SET 4,C | SET 4,D | SET 4,E | SET 4,H | SET 4,L | SET 4,(HL) | SET 4,A | SET 5,B | SET 5,C | SET 5,D | SET 5,E | SET 5,H | SET 5,L | SET 5,(HL) | SET 5,A |
| **F_** | SET 6,B | SET 6,C | SET 6,D | SET 6,E | SET 6,H | SET 6,L | SET 6,(HL) | SET 6,A | SET 7,B | SET 7,C | SET 7,D | SET 7,E | SET 7,H | SET 7,L | SET 7,(HL) | SET 7,A |

### `ED`-prefixed opcode matrix

|    | _0 | _1 | _2 | _3 | _4 | _5 | _6 | _7 | _8 | _9 | _A | _B | _C | _D | _E | _F |
|----|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **0_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **1_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **2_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **3_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **4_** | IN B,(C) | OUT (C),B | SBC HL,BC | LD (nn),BC | NEG | RETN | IM 0 | LD I,A | IN C,(C) | OUT (C),C | ADC HL,BC | LD BC,(nn) | NEG | RETI | IM 0/1 | LD R,A |
| **5_** | IN D,(C) | OUT (C),D | SBC HL,DE | LD (nn),DE | NEG | RETN | IM 1 | LD A,I | IN E,(C) | OUT (C),E | ADC HL,DE | LD DE,(nn) | NEG | RETN | IM 2 | LD A,R |
| **6_** | IN H,(C) | OUT (C),H | SBC HL,HL | LD (nn),HL | NEG | RETN | IM 0 | RRD | IN L,(C) | OUT (C),L | ADC HL,HL | LD HL,(nn) | NEG | RETN | IM 0/1 | RLD |
| **7_** | IN (C) | OUT (C),0 | SBC HL,SP | LD (nn),SP | NEG | RETN | IM 1 | NOP | IN A,(C) | OUT (C),A | ADC HL,SP | LD SP,(nn) | NEG | RETN | IM 2 | NOP |
| **8_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **9_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **A_** | LDI | CPI | INI | OUTI | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | LDD | CPD | IND | OUTD | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **B_** | LDIR | CPIR | INIR | OTIR | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | LDDR | CPDR | INDR | OTDR | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **C_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **D_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **E_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |
| **F_** | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP | NONI, NOP |

### `DD`-prefixed opcode matrix (`FD` identical with IY)

|    | _0 | _1 | _2 | _3 | _4 | _5 | _6 | _7 | _8 | _9 | _A | _B | _C | _D | _E | _F |
|----|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **0_** | — | — | — | — | — | — | — | — | — | ADD IX,BC | — | — | — | — | — | — |
| **1_** | — | — | — | — | — | — | — | — | — | ADD IX,DE | — | — | — | — | — | — |
| **2_** | — | LD IX,nn | LD (nn),IX | INC IX | INC IXH | DEC IXH | LD IXH,n | — | — | ADD IX,IX | LD IX,(nn) | DEC IX | INC IXL | DEC IXL | LD IXL,n | — |
| **3_** | — | — | — | — | INC (IX+d) | DEC (IX+d) | LD (IX+d),n | — | — | ADD IX,SP | — | — | — | — | — | — |
| **4_** | — | — | — | — | LD B,IXH | LD B,IXL | LD B,(IX+d) | — | — | — | — | — | LD C,IXH | LD C,IXL | LD C,(IX+d) | — |
| **5_** | — | — | — | — | LD D,IXH | LD D,IXL | LD D,(IX+d) | — | — | — | — | — | LD E,IXH | LD E,IXL | LD E,(IX+d) | — |
| **6_** | LD IXH,B | LD IXH,C | LD IXH,D | LD IXH,E | LD IXH,IXH | LD IXH,IXL | LD H,(IX+d) | LD IXH,A | LD IXL,B | LD IXL,C | LD IXL,D | LD IXL,E | LD IXL,IXH | LD IXL,IXL | LD L,(IX+d) | LD IXL,A |
| **7_** | LD (IX+d),B | LD (IX+d),C | LD (IX+d),D | LD (IX+d),E | LD (IX+d),H | LD (IX+d),L | — | LD (IX+d),A | — | — | — | — | LD A,IXH | LD A,IXL | LD A,(IX+d) | — |
| **8_** | — | — | — | — | ADD A,IXH | ADD A,IXL | ADD A,(IX+d) | — | — | — | — | — | ADC A,IXH | ADC A,IXL | ADC A,(IX+d) | — |
| **9_** | — | — | — | — | SUB IXH | SUB IXL | SUB (IX+d) | — | — | — | — | — | SBC A,IXH | SBC A,IXL | SBC A,(IX+d) | — |
| **A_** | — | — | — | — | AND IXH | AND IXL | AND (IX+d) | — | — | — | — | — | XOR IXH | XOR IXL | XOR (IX+d) | — |
| **B_** | — | — | — | — | OR IXH | OR IXL | OR (IX+d) | — | — | — | — | — | CP IXH | CP IXL | CP (IX+d) | — |
| **C_** | — | — | — | — | — | — | — | — | — | — | — | — | — | — | — | — |
| **D_** | — | — | — | — | — | — | — | — | — | — | — | — | — | — | — | — |
| **E_** | — | POP IX | — | EX (SP),IX | — | PUSH IX | — | — | — | JP (IX) | — | — | — | — | — | — |
| **F_** | — | — | — | — | — | — | — | — | — | LD SP,IX | — | — | — | — | — | — |

### `DD CB d`-prefixed opcode matrix (`FD CB d` identical with IY)

|    | _0 | _1 | _2 | _3 | _4 | _5 | _6 | _7 | _8 | _9 | _A | _B | _C | _D | _E | _F |
|----|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **0_** | LD B,RLC (IX+d) | LD C,RLC (IX+d) | LD D,RLC (IX+d) | LD E,RLC (IX+d) | LD H,RLC (IX+d) | LD L,RLC (IX+d) | RLC (IX+d) | LD A,RLC (IX+d) | LD B,RRC (IX+d) | LD C,RRC (IX+d) | LD D,RRC (IX+d) | LD E,RRC (IX+d) | LD H,RRC (IX+d) | LD L,RRC (IX+d) | RRC (IX+d) | LD A,RRC (IX+d) |
| **1_** | LD B,RL (IX+d) | LD C,RL (IX+d) | LD D,RL (IX+d) | LD E,RL (IX+d) | LD H,RL (IX+d) | LD L,RL (IX+d) | RL (IX+d) | LD A,RL (IX+d) | LD B,RR (IX+d) | LD C,RR (IX+d) | LD D,RR (IX+d) | LD E,RR (IX+d) | LD H,RR (IX+d) | LD L,RR (IX+d) | RR (IX+d) | LD A,RR (IX+d) |
| **2_** | LD B,SLA (IX+d) | LD C,SLA (IX+d) | LD D,SLA (IX+d) | LD E,SLA (IX+d) | LD H,SLA (IX+d) | LD L,SLA (IX+d) | SLA (IX+d) | LD A,SLA (IX+d) | LD B,SRA (IX+d) | LD C,SRA (IX+d) | LD D,SRA (IX+d) | LD E,SRA (IX+d) | LD H,SRA (IX+d) | LD L,SRA (IX+d) | SRA (IX+d) | LD A,SRA (IX+d) |
| **3_** | LD B,SLL (IX+d) | LD C,SLL (IX+d) | LD D,SLL (IX+d) | LD E,SLL (IX+d) | LD H,SLL (IX+d) | LD L,SLL (IX+d) | SLL (IX+d) | LD A,SLL (IX+d) | LD B,SRL (IX+d) | LD C,SRL (IX+d) | LD D,SRL (IX+d) | LD E,SRL (IX+d) | LD H,SRL (IX+d) | LD L,SRL (IX+d) | SRL (IX+d) | LD A,SRL (IX+d) |
| **4_** | BIT 0,(IX+d) | BIT 0,(IX+d) | BIT 0,(IX+d) | BIT 0,(IX+d) | BIT 0,(IX+d) | BIT 0,(IX+d) | BIT 0,(IX+d) | BIT 0,(IX+d) | BIT 1,(IX+d) | BIT 1,(IX+d) | BIT 1,(IX+d) | BIT 1,(IX+d) | BIT 1,(IX+d) | BIT 1,(IX+d) | BIT 1,(IX+d) | BIT 1,(IX+d) |
| **5_** | BIT 2,(IX+d) | BIT 2,(IX+d) | BIT 2,(IX+d) | BIT 2,(IX+d) | BIT 2,(IX+d) | BIT 2,(IX+d) | BIT 2,(IX+d) | BIT 2,(IX+d) | BIT 3,(IX+d) | BIT 3,(IX+d) | BIT 3,(IX+d) | BIT 3,(IX+d) | BIT 3,(IX+d) | BIT 3,(IX+d) | BIT 3,(IX+d) | BIT 3,(IX+d) |
| **6_** | BIT 4,(IX+d) | BIT 4,(IX+d) | BIT 4,(IX+d) | BIT 4,(IX+d) | BIT 4,(IX+d) | BIT 4,(IX+d) | BIT 4,(IX+d) | BIT 4,(IX+d) | BIT 5,(IX+d) | BIT 5,(IX+d) | BIT 5,(IX+d) | BIT 5,(IX+d) | BIT 5,(IX+d) | BIT 5,(IX+d) | BIT 5,(IX+d) | BIT 5,(IX+d) |
| **7_** | BIT 6,(IX+d) | BIT 6,(IX+d) | BIT 6,(IX+d) | BIT 6,(IX+d) | BIT 6,(IX+d) | BIT 6,(IX+d) | BIT 6,(IX+d) | BIT 6,(IX+d) | BIT 7,(IX+d) | BIT 7,(IX+d) | BIT 7,(IX+d) | BIT 7,(IX+d) | BIT 7,(IX+d) | BIT 7,(IX+d) | BIT 7,(IX+d) | BIT 7,(IX+d) |
| **8_** | LD B,RES 0,(IX+d) | LD C,RES 0,(IX+d) | LD D,RES 0,(IX+d) | LD E,RES 0,(IX+d) | LD H,RES 0,(IX+d) | LD L,RES 0,(IX+d) | RES 0,(IX+d) | LD A,RES 0,(IX+d) | LD B,RES 1,(IX+d) | LD C,RES 1,(IX+d) | LD D,RES 1,(IX+d) | LD E,RES 1,(IX+d) | LD H,RES 1,(IX+d) | LD L,RES 1,(IX+d) | RES 1,(IX+d) | LD A,RES 1,(IX+d) |
| **9_** | LD B,RES 2,(IX+d) | LD C,RES 2,(IX+d) | LD D,RES 2,(IX+d) | LD E,RES 2,(IX+d) | LD H,RES 2,(IX+d) | LD L,RES 2,(IX+d) | RES 2,(IX+d) | LD A,RES 2,(IX+d) | LD B,RES 3,(IX+d) | LD C,RES 3,(IX+d) | LD D,RES 3,(IX+d) | LD E,RES 3,(IX+d) | LD H,RES 3,(IX+d) | LD L,RES 3,(IX+d) | RES 3,(IX+d) | LD A,RES 3,(IX+d) |
| **A_** | LD B,RES 4,(IX+d) | LD C,RES 4,(IX+d) | LD D,RES 4,(IX+d) | LD E,RES 4,(IX+d) | LD H,RES 4,(IX+d) | LD L,RES 4,(IX+d) | RES 4,(IX+d) | LD A,RES 4,(IX+d) | LD B,RES 5,(IX+d) | LD C,RES 5,(IX+d) | LD D,RES 5,(IX+d) | LD E,RES 5,(IX+d) | LD H,RES 5,(IX+d) | LD L,RES 5,(IX+d) | RES 5,(IX+d) | LD A,RES 5,(IX+d) |
| **B_** | LD B,RES 6,(IX+d) | LD C,RES 6,(IX+d) | LD D,RES 6,(IX+d) | LD E,RES 6,(IX+d) | LD H,RES 6,(IX+d) | LD L,RES 6,(IX+d) | RES 6,(IX+d) | LD A,RES 6,(IX+d) | LD B,RES 7,(IX+d) | LD C,RES 7,(IX+d) | LD D,RES 7,(IX+d) | LD E,RES 7,(IX+d) | LD H,RES 7,(IX+d) | LD L,RES 7,(IX+d) | RES 7,(IX+d) | LD A,RES 7,(IX+d) |
| **C_** | LD B,SET 0,(IX+d) | LD C,SET 0,(IX+d) | LD D,SET 0,(IX+d) | LD E,SET 0,(IX+d) | LD H,SET 0,(IX+d) | LD L,SET 0,(IX+d) | SET 0,(IX+d) | LD A,SET 0,(IX+d) | LD B,SET 1,(IX+d) | LD C,SET 1,(IX+d) | LD D,SET 1,(IX+d) | LD E,SET 1,(IX+d) | LD H,SET 1,(IX+d) | LD L,SET 1,(IX+d) | SET 1,(IX+d) | LD A,SET 1,(IX+d) |
| **D_** | LD B,SET 2,(IX+d) | LD C,SET 2,(IX+d) | LD D,SET 2,(IX+d) | LD E,SET 2,(IX+d) | LD H,SET 2,(IX+d) | LD L,SET 2,(IX+d) | SET 2,(IX+d) | LD A,SET 2,(IX+d) | LD B,SET 3,(IX+d) | LD C,SET 3,(IX+d) | LD D,SET 3,(IX+d) | LD E,SET 3,(IX+d) | LD H,SET 3,(IX+d) | LD L,SET 3,(IX+d) | SET 3,(IX+d) | LD A,SET 3,(IX+d) |
| **E_** | LD B,SET 4,(IX+d) | LD C,SET 4,(IX+d) | LD D,SET 4,(IX+d) | LD E,SET 4,(IX+d) | LD H,SET 4,(IX+d) | LD L,SET 4,(IX+d) | SET 4,(IX+d) | LD A,SET 4,(IX+d) | LD B,SET 5,(IX+d) | LD C,SET 5,(IX+d) | LD D,SET 5,(IX+d) | LD E,SET 5,(IX+d) | LD H,SET 5,(IX+d) | LD L,SET 5,(IX+d) | SET 5,(IX+d) | LD A,SET 5,(IX+d) |
| **F_** | LD B,SET 6,(IX+d) | LD C,SET 6,(IX+d) | LD D,SET 6,(IX+d) | LD E,SET 6,(IX+d) | LD H,SET 6,(IX+d) | LD L,SET 6,(IX+d) | SET 6,(IX+d) | LD A,SET 6,(IX+d) | LD B,SET 7,(IX+d) | LD C,SET 7,(IX+d) | LD D,SET 7,(IX+d) | LD E,SET 7,(IX+d) | LD H,SET 7,(IX+d) | LD L,SET 7,(IX+d) | SET 7,(IX+d) | LD A,SET 7,(IX+d) |

---

### Full listings

### Unprefixed opcodes

| Opcode | Mnemonic | Bytes | T-states | Doc |
|---|---|---|---|---|
| `00` | `NOP` | 1 | 4 | yes |
| `01` | `LD BC,nn` | 3 | 10 | yes |
| `02` | `LD (BC),A` | 1 | 7 | yes |
| `03` | `INC BC` | 1 | 6 | yes |
| `04` | `INC B` | 1 | 4 | yes |
| `05` | `DEC B` | 1 | 4 | yes |
| `06` | `LD B,n` | 2 | 7 | yes |
| `07` | `RLCA` | 1 | 4 | yes |
| `08` | `EX AF,AF'` | 1 | 4 | yes |
| `09` | `ADD HL,BC` | 1 | 11 | yes |
| `0A` | `LD A,(BC)` | 1 | 7 | yes |
| `0B` | `DEC BC` | 1 | 6 | yes |
| `0C` | `INC C` | 1 | 4 | yes |
| `0D` | `DEC C` | 1 | 4 | yes |
| `0E` | `LD C,n` | 2 | 7 | yes |
| `0F` | `RRCA` | 1 | 4 | yes |
| `10` | `DJNZ d` | 2 | 13/8 | yes |
| `11` | `LD DE,nn` | 3 | 10 | yes |
| `12` | `LD (DE),A` | 1 | 7 | yes |
| `13` | `INC DE` | 1 | 6 | yes |
| `14` | `INC D` | 1 | 4 | yes |
| `15` | `DEC D` | 1 | 4 | yes |
| `16` | `LD D,n` | 2 | 7 | yes |
| `17` | `RLA` | 1 | 4 | yes |
| `18` | `JR d` | 2 | 12 | yes |
| `19` | `ADD HL,DE` | 1 | 11 | yes |
| `1A` | `LD A,(DE)` | 1 | 7 | yes |
| `1B` | `DEC DE` | 1 | 6 | yes |
| `1C` | `INC E` | 1 | 4 | yes |
| `1D` | `DEC E` | 1 | 4 | yes |
| `1E` | `LD E,n` | 2 | 7 | yes |
| `1F` | `RRA` | 1 | 4 | yes |
| `20` | `JR NZ,d` | 2 | 12/7 | yes |
| `21` | `LD HL,nn` | 3 | 10 | yes |
| `22` | `LD (nn),HL` | 3 | 16 | yes |
| `23` | `INC HL` | 1 | 6 | yes |
| `24` | `INC H` | 1 | 4 | yes |
| `25` | `DEC H` | 1 | 4 | yes |
| `26` | `LD H,n` | 2 | 7 | yes |
| `27` | `DAA` | 1 | 4 | yes |
| `28` | `JR Z,d` | 2 | 12/7 | yes |
| `29` | `ADD HL,HL` | 1 | 11 | yes |
| `2A` | `LD HL,(nn)` | 3 | 16 | yes |
| `2B` | `DEC HL` | 1 | 6 | yes |
| `2C` | `INC L` | 1 | 4 | yes |
| `2D` | `DEC L` | 1 | 4 | yes |
| `2E` | `LD L,n` | 2 | 7 | yes |
| `2F` | `CPL` | 1 | 4 | yes |
| `30` | `JR NC,d` | 2 | 12/7 | yes |
| `31` | `LD SP,nn` | 3 | 10 | yes |
| `32` | `LD (nn),A` | 3 | 13 | yes |
| `33` | `INC SP` | 1 | 6 | yes |
| `34` | `INC (HL)` | 1 | 11 | yes |
| `35` | `DEC (HL)` | 1 | 11 | yes |
| `36` | `LD (HL),n` | 2 | 10 | yes |
| `37` | `SCF` | 1 | 4 | yes |
| `38` | `JR C,d` | 2 | 12/7 | yes |
| `39` | `ADD HL,SP` | 1 | 11 | yes |
| `3A` | `LD A,(nn)` | 3 | 13 | yes |
| `3B` | `DEC SP` | 1 | 6 | yes |
| `3C` | `INC A` | 1 | 4 | yes |
| `3D` | `DEC A` | 1 | 4 | yes |
| `3E` | `LD A,n` | 2 | 7 | yes |
| `3F` | `CCF` | 1 | 4 | yes |
| `40` | `LD B,B` | 1 | 4 | yes |
| `41` | `LD B,C` | 1 | 4 | yes |
| `42` | `LD B,D` | 1 | 4 | yes |
| `43` | `LD B,E` | 1 | 4 | yes |
| `44` | `LD B,H` | 1 | 4 | yes |
| `45` | `LD B,L` | 1 | 4 | yes |
| `46` | `LD B,(HL)` | 1 | 7 | yes |
| `47` | `LD B,A` | 1 | 4 | yes |
| `48` | `LD C,B` | 1 | 4 | yes |
| `49` | `LD C,C` | 1 | 4 | yes |
| `4A` | `LD C,D` | 1 | 4 | yes |
| `4B` | `LD C,E` | 1 | 4 | yes |
| `4C` | `LD C,H` | 1 | 4 | yes |
| `4D` | `LD C,L` | 1 | 4 | yes |
| `4E` | `LD C,(HL)` | 1 | 7 | yes |
| `4F` | `LD C,A` | 1 | 4 | yes |
| `50` | `LD D,B` | 1 | 4 | yes |
| `51` | `LD D,C` | 1 | 4 | yes |
| `52` | `LD D,D` | 1 | 4 | yes |
| `53` | `LD D,E` | 1 | 4 | yes |
| `54` | `LD D,H` | 1 | 4 | yes |
| `55` | `LD D,L` | 1 | 4 | yes |
| `56` | `LD D,(HL)` | 1 | 7 | yes |
| `57` | `LD D,A` | 1 | 4 | yes |
| `58` | `LD E,B` | 1 | 4 | yes |
| `59` | `LD E,C` | 1 | 4 | yes |
| `5A` | `LD E,D` | 1 | 4 | yes |
| `5B` | `LD E,E` | 1 | 4 | yes |
| `5C` | `LD E,H` | 1 | 4 | yes |
| `5D` | `LD E,L` | 1 | 4 | yes |
| `5E` | `LD E,(HL)` | 1 | 7 | yes |
| `5F` | `LD E,A` | 1 | 4 | yes |
| `60` | `LD H,B` | 1 | 4 | yes |
| `61` | `LD H,C` | 1 | 4 | yes |
| `62` | `LD H,D` | 1 | 4 | yes |
| `63` | `LD H,E` | 1 | 4 | yes |
| `64` | `LD H,H` | 1 | 4 | yes |
| `65` | `LD H,L` | 1 | 4 | yes |
| `66` | `LD H,(HL)` | 1 | 7 | yes |
| `67` | `LD H,A` | 1 | 4 | yes |
| `68` | `LD L,B` | 1 | 4 | yes |
| `69` | `LD L,C` | 1 | 4 | yes |
| `6A` | `LD L,D` | 1 | 4 | yes |
| `6B` | `LD L,E` | 1 | 4 | yes |
| `6C` | `LD L,H` | 1 | 4 | yes |
| `6D` | `LD L,L` | 1 | 4 | yes |
| `6E` | `LD L,(HL)` | 1 | 7 | yes |
| `6F` | `LD L,A` | 1 | 4 | yes |
| `70` | `LD (HL),B` | 1 | 7 | yes |
| `71` | `LD (HL),C` | 1 | 7 | yes |
| `72` | `LD (HL),D` | 1 | 7 | yes |
| `73` | `LD (HL),E` | 1 | 7 | yes |
| `74` | `LD (HL),H` | 1 | 7 | yes |
| `75` | `LD (HL),L` | 1 | 7 | yes |
| `76` | `HALT` | 1 | 4 | yes |
| `77` | `LD (HL),A` | 1 | 7 | yes |
| `78` | `LD A,B` | 1 | 4 | yes |
| `79` | `LD A,C` | 1 | 4 | yes |
| `7A` | `LD A,D` | 1 | 4 | yes |
| `7B` | `LD A,E` | 1 | 4 | yes |
| `7C` | `LD A,H` | 1 | 4 | yes |
| `7D` | `LD A,L` | 1 | 4 | yes |
| `7E` | `LD A,(HL)` | 1 | 7 | yes |
| `7F` | `LD A,A` | 1 | 4 | yes |
| `80` | `ADD A,B` | 1 | 4 | yes |
| `81` | `ADD A,C` | 1 | 4 | yes |
| `82` | `ADD A,D` | 1 | 4 | yes |
| `83` | `ADD A,E` | 1 | 4 | yes |
| `84` | `ADD A,H` | 1 | 4 | yes |
| `85` | `ADD A,L` | 1 | 4 | yes |
| `86` | `ADD A,(HL)` | 1 | 7 | yes |
| `87` | `ADD A,A` | 1 | 4 | yes |
| `88` | `ADC A,B` | 1 | 4 | yes |
| `89` | `ADC A,C` | 1 | 4 | yes |
| `8A` | `ADC A,D` | 1 | 4 | yes |
| `8B` | `ADC A,E` | 1 | 4 | yes |
| `8C` | `ADC A,H` | 1 | 4 | yes |
| `8D` | `ADC A,L` | 1 | 4 | yes |
| `8E` | `ADC A,(HL)` | 1 | 7 | yes |
| `8F` | `ADC A,A` | 1 | 4 | yes |
| `90` | `SUB B` | 1 | 4 | yes |
| `91` | `SUB C` | 1 | 4 | yes |
| `92` | `SUB D` | 1 | 4 | yes |
| `93` | `SUB E` | 1 | 4 | yes |
| `94` | `SUB H` | 1 | 4 | yes |
| `95` | `SUB L` | 1 | 4 | yes |
| `96` | `SUB (HL)` | 1 | 7 | yes |
| `97` | `SUB A` | 1 | 4 | yes |
| `98` | `SBC A,B` | 1 | 4 | yes |
| `99` | `SBC A,C` | 1 | 4 | yes |
| `9A` | `SBC A,D` | 1 | 4 | yes |
| `9B` | `SBC A,E` | 1 | 4 | yes |
| `9C` | `SBC A,H` | 1 | 4 | yes |
| `9D` | `SBC A,L` | 1 | 4 | yes |
| `9E` | `SBC A,(HL)` | 1 | 7 | yes |
| `9F` | `SBC A,A` | 1 | 4 | yes |
| `A0` | `AND B` | 1 | 4 | yes |
| `A1` | `AND C` | 1 | 4 | yes |
| `A2` | `AND D` | 1 | 4 | yes |
| `A3` | `AND E` | 1 | 4 | yes |
| `A4` | `AND H` | 1 | 4 | yes |
| `A5` | `AND L` | 1 | 4 | yes |
| `A6` | `AND (HL)` | 1 | 7 | yes |
| `A7` | `AND A` | 1 | 4 | yes |
| `A8` | `XOR B` | 1 | 4 | yes |
| `A9` | `XOR C` | 1 | 4 | yes |
| `AA` | `XOR D` | 1 | 4 | yes |
| `AB` | `XOR E` | 1 | 4 | yes |
| `AC` | `XOR H` | 1 | 4 | yes |
| `AD` | `XOR L` | 1 | 4 | yes |
| `AE` | `XOR (HL)` | 1 | 7 | yes |
| `AF` | `XOR A` | 1 | 4 | yes |
| `B0` | `OR B` | 1 | 4 | yes |
| `B1` | `OR C` | 1 | 4 | yes |
| `B2` | `OR D` | 1 | 4 | yes |
| `B3` | `OR E` | 1 | 4 | yes |
| `B4` | `OR H` | 1 | 4 | yes |
| `B5` | `OR L` | 1 | 4 | yes |
| `B6` | `OR (HL)` | 1 | 7 | yes |
| `B7` | `OR A` | 1 | 4 | yes |
| `B8` | `CP B` | 1 | 4 | yes |
| `B9` | `CP C` | 1 | 4 | yes |
| `BA` | `CP D` | 1 | 4 | yes |
| `BB` | `CP E` | 1 | 4 | yes |
| `BC` | `CP H` | 1 | 4 | yes |
| `BD` | `CP L` | 1 | 4 | yes |
| `BE` | `CP (HL)` | 1 | 7 | yes |
| `BF` | `CP A` | 1 | 4 | yes |
| `C0` | `RET NZ` | 1 | 11/5 | yes |
| `C1` | `POP BC` | 1 | 10 | yes |
| `C2` | `JP NZ,nn` | 3 | 10 | yes |
| `C3` | `JP nn` | 3 | 10 | yes |
| `C4` | `CALL NZ,nn` | 3 | 17/10 | yes |
| `C5` | `PUSH BC` | 1 | 11 | yes |
| `C6` | `ADD A,n` | 2 | 7 | yes |
| `C7` | `RST 00h` | 1 | 11 | yes |
| `C8` | `RET Z` | 1 | 11/5 | yes |
| `C9` | `RET` | 1 | 10 | yes |
| `CA` | `JP Z,nn` | 3 | 10 | yes |
| `CB` | `*CB prefix*` | 1 | - | yes |
| `CC` | `CALL Z,nn` | 3 | 17/10 | yes |
| `CD` | `CALL nn` | 3 | 17 | yes |
| `CE` | `ADC A,n` | 2 | 7 | yes |
| `CF` | `RST 08h` | 1 | 11 | yes |
| `D0` | `RET NC` | 1 | 11/5 | yes |
| `D1` | `POP DE` | 1 | 10 | yes |
| `D2` | `JP NC,nn` | 3 | 10 | yes |
| `D3` | `OUT (n),A` | 2 | 11 | yes |
| `D4` | `CALL NC,nn` | 3 | 17/10 | yes |
| `D5` | `PUSH DE` | 1 | 11 | yes |
| `D6` | `SUB n` | 2 | 7 | yes |
| `D7` | `RST 10h` | 1 | 11 | yes |
| `D8` | `RET C` | 1 | 11/5 | yes |
| `D9` | `EXX` | 1 | 4 | yes |
| `DA` | `JP C,nn` | 3 | 10 | yes |
| `DB` | `IN A,(n)` | 2 | 11 | yes |
| `DC` | `CALL C,nn` | 3 | 17/10 | yes |
| `DD` | `*DD prefix*` | 1 | - | yes |
| `DE` | `SBC A,n` | 2 | 7 | yes |
| `DF` | `RST 18h` | 1 | 11 | yes |
| `E0` | `RET PO` | 1 | 11/5 | yes |
| `E1` | `POP HL` | 1 | 10 | yes |
| `E2` | `JP PO,nn` | 3 | 10 | yes |
| `E3` | `EX (SP),HL` | 1 | 19 | yes |
| `E4` | `CALL PO,nn` | 3 | 17/10 | yes |
| `E5` | `PUSH HL` | 1 | 11 | yes |
| `E6` | `AND n` | 2 | 7 | yes |
| `E7` | `RST 20h` | 1 | 11 | yes |
| `E8` | `RET PE` | 1 | 11/5 | yes |
| `E9` | `JP (HL)` | 1 | 4 | yes |
| `EA` | `JP PE,nn` | 3 | 10 | yes |
| `EB` | `EX DE,HL` | 1 | 4 | yes |
| `EC` | `CALL PE,nn` | 3 | 17/10 | yes |
| `ED` | `*ED prefix*` | 1 | - | yes |
| `EE` | `XOR n` | 2 | 7 | yes |
| `EF` | `RST 28h` | 1 | 11 | yes |
| `F0` | `RET P` | 1 | 11/5 | yes |
| `F1` | `POP AF` | 1 | 10 | yes |
| `F2` | `JP P,nn` | 3 | 10 | yes |
| `F3` | `DI` | 1 | 4 | yes |
| `F4` | `CALL P,nn` | 3 | 17/10 | yes |
| `F5` | `PUSH AF` | 1 | 11 | yes |
| `F6` | `OR n` | 2 | 7 | yes |
| `F7` | `RST 30h` | 1 | 11 | yes |
| `F8` | `RET M` | 1 | 11/5 | yes |
| `F9` | `LD SP,HL` | 1 | 6 | yes |
| `FA` | `JP M,nn` | 3 | 10 | yes |
| `FB` | `EI` | 1 | 4 | yes |
| `FC` | `CALL M,nn` | 3 | 17/10 | yes |
| `FD` | `*FD prefix*` | 1 | - | yes |
| `FE` | `CP n` | 2 | 7 | yes |
| `FF` | `RST 38h` | 1 | 11 | yes |

### `CB`-prefixed opcodes

| Opcode | Mnemonic | Bytes | T-states | Doc |
|---|---|---|---|---|
| `CB 00` | `RLC B` | 2 | 8 | yes |
| `CB 01` | `RLC C` | 2 | 8 | yes |
| `CB 02` | `RLC D` | 2 | 8 | yes |
| `CB 03` | `RLC E` | 2 | 8 | yes |
| `CB 04` | `RLC H` | 2 | 8 | yes |
| `CB 05` | `RLC L` | 2 | 8 | yes |
| `CB 06` | `RLC (HL)` | 2 | 15 | yes |
| `CB 07` | `RLC A` | 2 | 8 | yes |
| `CB 08` | `RRC B` | 2 | 8 | yes |
| `CB 09` | `RRC C` | 2 | 8 | yes |
| `CB 0A` | `RRC D` | 2 | 8 | yes |
| `CB 0B` | `RRC E` | 2 | 8 | yes |
| `CB 0C` | `RRC H` | 2 | 8 | yes |
| `CB 0D` | `RRC L` | 2 | 8 | yes |
| `CB 0E` | `RRC (HL)` | 2 | 15 | yes |
| `CB 0F` | `RRC A` | 2 | 8 | yes |
| `CB 10` | `RL B` | 2 | 8 | yes |
| `CB 11` | `RL C` | 2 | 8 | yes |
| `CB 12` | `RL D` | 2 | 8 | yes |
| `CB 13` | `RL E` | 2 | 8 | yes |
| `CB 14` | `RL H` | 2 | 8 | yes |
| `CB 15` | `RL L` | 2 | 8 | yes |
| `CB 16` | `RL (HL)` | 2 | 15 | yes |
| `CB 17` | `RL A` | 2 | 8 | yes |
| `CB 18` | `RR B` | 2 | 8 | yes |
| `CB 19` | `RR C` | 2 | 8 | yes |
| `CB 1A` | `RR D` | 2 | 8 | yes |
| `CB 1B` | `RR E` | 2 | 8 | yes |
| `CB 1C` | `RR H` | 2 | 8 | yes |
| `CB 1D` | `RR L` | 2 | 8 | yes |
| `CB 1E` | `RR (HL)` | 2 | 15 | yes |
| `CB 1F` | `RR A` | 2 | 8 | yes |
| `CB 20` | `SLA B` | 2 | 8 | yes |
| `CB 21` | `SLA C` | 2 | 8 | yes |
| `CB 22` | `SLA D` | 2 | 8 | yes |
| `CB 23` | `SLA E` | 2 | 8 | yes |
| `CB 24` | `SLA H` | 2 | 8 | yes |
| `CB 25` | `SLA L` | 2 | 8 | yes |
| `CB 26` | `SLA (HL)` | 2 | 15 | yes |
| `CB 27` | `SLA A` | 2 | 8 | yes |
| `CB 28` | `SRA B` | 2 | 8 | yes |
| `CB 29` | `SRA C` | 2 | 8 | yes |
| `CB 2A` | `SRA D` | 2 | 8 | yes |
| `CB 2B` | `SRA E` | 2 | 8 | yes |
| `CB 2C` | `SRA H` | 2 | 8 | yes |
| `CB 2D` | `SRA L` | 2 | 8 | yes |
| `CB 2E` | `SRA (HL)` | 2 | 15 | yes |
| `CB 2F` | `SRA A` | 2 | 8 | yes |
| `CB 30` | `SLL B` | 2 | 8 | undoc |
| `CB 31` | `SLL C` | 2 | 8 | undoc |
| `CB 32` | `SLL D` | 2 | 8 | undoc |
| `CB 33` | `SLL E` | 2 | 8 | undoc |
| `CB 34` | `SLL H` | 2 | 8 | undoc |
| `CB 35` | `SLL L` | 2 | 8 | undoc |
| `CB 36` | `SLL (HL)` | 2 | 15 | undoc |
| `CB 37` | `SLL A` | 2 | 8 | undoc |
| `CB 38` | `SRL B` | 2 | 8 | yes |
| `CB 39` | `SRL C` | 2 | 8 | yes |
| `CB 3A` | `SRL D` | 2 | 8 | yes |
| `CB 3B` | `SRL E` | 2 | 8 | yes |
| `CB 3C` | `SRL H` | 2 | 8 | yes |
| `CB 3D` | `SRL L` | 2 | 8 | yes |
| `CB 3E` | `SRL (HL)` | 2 | 15 | yes |
| `CB 3F` | `SRL A` | 2 | 8 | yes |
| `CB 40` | `BIT 0,B` | 2 | 8 | yes |
| `CB 41` | `BIT 0,C` | 2 | 8 | yes |
| `CB 42` | `BIT 0,D` | 2 | 8 | yes |
| `CB 43` | `BIT 0,E` | 2 | 8 | yes |
| `CB 44` | `BIT 0,H` | 2 | 8 | yes |
| `CB 45` | `BIT 0,L` | 2 | 8 | yes |
| `CB 46` | `BIT 0,(HL)` | 2 | 12 | yes |
| `CB 47` | `BIT 0,A` | 2 | 8 | yes |
| `CB 48` | `BIT 1,B` | 2 | 8 | yes |
| `CB 49` | `BIT 1,C` | 2 | 8 | yes |
| `CB 4A` | `BIT 1,D` | 2 | 8 | yes |
| `CB 4B` | `BIT 1,E` | 2 | 8 | yes |
| `CB 4C` | `BIT 1,H` | 2 | 8 | yes |
| `CB 4D` | `BIT 1,L` | 2 | 8 | yes |
| `CB 4E` | `BIT 1,(HL)` | 2 | 12 | yes |
| `CB 4F` | `BIT 1,A` | 2 | 8 | yes |
| `CB 50` | `BIT 2,B` | 2 | 8 | yes |
| `CB 51` | `BIT 2,C` | 2 | 8 | yes |
| `CB 52` | `BIT 2,D` | 2 | 8 | yes |
| `CB 53` | `BIT 2,E` | 2 | 8 | yes |
| `CB 54` | `BIT 2,H` | 2 | 8 | yes |
| `CB 55` | `BIT 2,L` | 2 | 8 | yes |
| `CB 56` | `BIT 2,(HL)` | 2 | 12 | yes |
| `CB 57` | `BIT 2,A` | 2 | 8 | yes |
| `CB 58` | `BIT 3,B` | 2 | 8 | yes |
| `CB 59` | `BIT 3,C` | 2 | 8 | yes |
| `CB 5A` | `BIT 3,D` | 2 | 8 | yes |
| `CB 5B` | `BIT 3,E` | 2 | 8 | yes |
| `CB 5C` | `BIT 3,H` | 2 | 8 | yes |
| `CB 5D` | `BIT 3,L` | 2 | 8 | yes |
| `CB 5E` | `BIT 3,(HL)` | 2 | 12 | yes |
| `CB 5F` | `BIT 3,A` | 2 | 8 | yes |
| `CB 60` | `BIT 4,B` | 2 | 8 | yes |
| `CB 61` | `BIT 4,C` | 2 | 8 | yes |
| `CB 62` | `BIT 4,D` | 2 | 8 | yes |
| `CB 63` | `BIT 4,E` | 2 | 8 | yes |
| `CB 64` | `BIT 4,H` | 2 | 8 | yes |
| `CB 65` | `BIT 4,L` | 2 | 8 | yes |
| `CB 66` | `BIT 4,(HL)` | 2 | 12 | yes |
| `CB 67` | `BIT 4,A` | 2 | 8 | yes |
| `CB 68` | `BIT 5,B` | 2 | 8 | yes |
| `CB 69` | `BIT 5,C` | 2 | 8 | yes |
| `CB 6A` | `BIT 5,D` | 2 | 8 | yes |
| `CB 6B` | `BIT 5,E` | 2 | 8 | yes |
| `CB 6C` | `BIT 5,H` | 2 | 8 | yes |
| `CB 6D` | `BIT 5,L` | 2 | 8 | yes |
| `CB 6E` | `BIT 5,(HL)` | 2 | 12 | yes |
| `CB 6F` | `BIT 5,A` | 2 | 8 | yes |
| `CB 70` | `BIT 6,B` | 2 | 8 | yes |
| `CB 71` | `BIT 6,C` | 2 | 8 | yes |
| `CB 72` | `BIT 6,D` | 2 | 8 | yes |
| `CB 73` | `BIT 6,E` | 2 | 8 | yes |
| `CB 74` | `BIT 6,H` | 2 | 8 | yes |
| `CB 75` | `BIT 6,L` | 2 | 8 | yes |
| `CB 76` | `BIT 6,(HL)` | 2 | 12 | yes |
| `CB 77` | `BIT 6,A` | 2 | 8 | yes |
| `CB 78` | `BIT 7,B` | 2 | 8 | yes |
| `CB 79` | `BIT 7,C` | 2 | 8 | yes |
| `CB 7A` | `BIT 7,D` | 2 | 8 | yes |
| `CB 7B` | `BIT 7,E` | 2 | 8 | yes |
| `CB 7C` | `BIT 7,H` | 2 | 8 | yes |
| `CB 7D` | `BIT 7,L` | 2 | 8 | yes |
| `CB 7E` | `BIT 7,(HL)` | 2 | 12 | yes |
| `CB 7F` | `BIT 7,A` | 2 | 8 | yes |
| `CB 80` | `RES 0,B` | 2 | 8 | yes |
| `CB 81` | `RES 0,C` | 2 | 8 | yes |
| `CB 82` | `RES 0,D` | 2 | 8 | yes |
| `CB 83` | `RES 0,E` | 2 | 8 | yes |
| `CB 84` | `RES 0,H` | 2 | 8 | yes |
| `CB 85` | `RES 0,L` | 2 | 8 | yes |
| `CB 86` | `RES 0,(HL)` | 2 | 15 | yes |
| `CB 87` | `RES 0,A` | 2 | 8 | yes |
| `CB 88` | `RES 1,B` | 2 | 8 | yes |
| `CB 89` | `RES 1,C` | 2 | 8 | yes |
| `CB 8A` | `RES 1,D` | 2 | 8 | yes |
| `CB 8B` | `RES 1,E` | 2 | 8 | yes |
| `CB 8C` | `RES 1,H` | 2 | 8 | yes |
| `CB 8D` | `RES 1,L` | 2 | 8 | yes |
| `CB 8E` | `RES 1,(HL)` | 2 | 15 | yes |
| `CB 8F` | `RES 1,A` | 2 | 8 | yes |
| `CB 90` | `RES 2,B` | 2 | 8 | yes |
| `CB 91` | `RES 2,C` | 2 | 8 | yes |
| `CB 92` | `RES 2,D` | 2 | 8 | yes |
| `CB 93` | `RES 2,E` | 2 | 8 | yes |
| `CB 94` | `RES 2,H` | 2 | 8 | yes |
| `CB 95` | `RES 2,L` | 2 | 8 | yes |
| `CB 96` | `RES 2,(HL)` | 2 | 15 | yes |
| `CB 97` | `RES 2,A` | 2 | 8 | yes |
| `CB 98` | `RES 3,B` | 2 | 8 | yes |
| `CB 99` | `RES 3,C` | 2 | 8 | yes |
| `CB 9A` | `RES 3,D` | 2 | 8 | yes |
| `CB 9B` | `RES 3,E` | 2 | 8 | yes |
| `CB 9C` | `RES 3,H` | 2 | 8 | yes |
| `CB 9D` | `RES 3,L` | 2 | 8 | yes |
| `CB 9E` | `RES 3,(HL)` | 2 | 15 | yes |
| `CB 9F` | `RES 3,A` | 2 | 8 | yes |
| `CB A0` | `RES 4,B` | 2 | 8 | yes |
| `CB A1` | `RES 4,C` | 2 | 8 | yes |
| `CB A2` | `RES 4,D` | 2 | 8 | yes |
| `CB A3` | `RES 4,E` | 2 | 8 | yes |
| `CB A4` | `RES 4,H` | 2 | 8 | yes |
| `CB A5` | `RES 4,L` | 2 | 8 | yes |
| `CB A6` | `RES 4,(HL)` | 2 | 15 | yes |
| `CB A7` | `RES 4,A` | 2 | 8 | yes |
| `CB A8` | `RES 5,B` | 2 | 8 | yes |
| `CB A9` | `RES 5,C` | 2 | 8 | yes |
| `CB AA` | `RES 5,D` | 2 | 8 | yes |
| `CB AB` | `RES 5,E` | 2 | 8 | yes |
| `CB AC` | `RES 5,H` | 2 | 8 | yes |
| `CB AD` | `RES 5,L` | 2 | 8 | yes |
| `CB AE` | `RES 5,(HL)` | 2 | 15 | yes |
| `CB AF` | `RES 5,A` | 2 | 8 | yes |
| `CB B0` | `RES 6,B` | 2 | 8 | yes |
| `CB B1` | `RES 6,C` | 2 | 8 | yes |
| `CB B2` | `RES 6,D` | 2 | 8 | yes |
| `CB B3` | `RES 6,E` | 2 | 8 | yes |
| `CB B4` | `RES 6,H` | 2 | 8 | yes |
| `CB B5` | `RES 6,L` | 2 | 8 | yes |
| `CB B6` | `RES 6,(HL)` | 2 | 15 | yes |
| `CB B7` | `RES 6,A` | 2 | 8 | yes |
| `CB B8` | `RES 7,B` | 2 | 8 | yes |
| `CB B9` | `RES 7,C` | 2 | 8 | yes |
| `CB BA` | `RES 7,D` | 2 | 8 | yes |
| `CB BB` | `RES 7,E` | 2 | 8 | yes |
| `CB BC` | `RES 7,H` | 2 | 8 | yes |
| `CB BD` | `RES 7,L` | 2 | 8 | yes |
| `CB BE` | `RES 7,(HL)` | 2 | 15 | yes |
| `CB BF` | `RES 7,A` | 2 | 8 | yes |
| `CB C0` | `SET 0,B` | 2 | 8 | yes |
| `CB C1` | `SET 0,C` | 2 | 8 | yes |
| `CB C2` | `SET 0,D` | 2 | 8 | yes |
| `CB C3` | `SET 0,E` | 2 | 8 | yes |
| `CB C4` | `SET 0,H` | 2 | 8 | yes |
| `CB C5` | `SET 0,L` | 2 | 8 | yes |
| `CB C6` | `SET 0,(HL)` | 2 | 15 | yes |
| `CB C7` | `SET 0,A` | 2 | 8 | yes |
| `CB C8` | `SET 1,B` | 2 | 8 | yes |
| `CB C9` | `SET 1,C` | 2 | 8 | yes |
| `CB CA` | `SET 1,D` | 2 | 8 | yes |
| `CB CB` | `SET 1,E` | 2 | 8 | yes |
| `CB CC` | `SET 1,H` | 2 | 8 | yes |
| `CB CD` | `SET 1,L` | 2 | 8 | yes |
| `CB CE` | `SET 1,(HL)` | 2 | 15 | yes |
| `CB CF` | `SET 1,A` | 2 | 8 | yes |
| `CB D0` | `SET 2,B` | 2 | 8 | yes |
| `CB D1` | `SET 2,C` | 2 | 8 | yes |
| `CB D2` | `SET 2,D` | 2 | 8 | yes |
| `CB D3` | `SET 2,E` | 2 | 8 | yes |
| `CB D4` | `SET 2,H` | 2 | 8 | yes |
| `CB D5` | `SET 2,L` | 2 | 8 | yes |
| `CB D6` | `SET 2,(HL)` | 2 | 15 | yes |
| `CB D7` | `SET 2,A` | 2 | 8 | yes |
| `CB D8` | `SET 3,B` | 2 | 8 | yes |
| `CB D9` | `SET 3,C` | 2 | 8 | yes |
| `CB DA` | `SET 3,D` | 2 | 8 | yes |
| `CB DB` | `SET 3,E` | 2 | 8 | yes |
| `CB DC` | `SET 3,H` | 2 | 8 | yes |
| `CB DD` | `SET 3,L` | 2 | 8 | yes |
| `CB DE` | `SET 3,(HL)` | 2 | 15 | yes |
| `CB DF` | `SET 3,A` | 2 | 8 | yes |
| `CB E0` | `SET 4,B` | 2 | 8 | yes |
| `CB E1` | `SET 4,C` | 2 | 8 | yes |
| `CB E2` | `SET 4,D` | 2 | 8 | yes |
| `CB E3` | `SET 4,E` | 2 | 8 | yes |
| `CB E4` | `SET 4,H` | 2 | 8 | yes |
| `CB E5` | `SET 4,L` | 2 | 8 | yes |
| `CB E6` | `SET 4,(HL)` | 2 | 15 | yes |
| `CB E7` | `SET 4,A` | 2 | 8 | yes |
| `CB E8` | `SET 5,B` | 2 | 8 | yes |
| `CB E9` | `SET 5,C` | 2 | 8 | yes |
| `CB EA` | `SET 5,D` | 2 | 8 | yes |
| `CB EB` | `SET 5,E` | 2 | 8 | yes |
| `CB EC` | `SET 5,H` | 2 | 8 | yes |
| `CB ED` | `SET 5,L` | 2 | 8 | yes |
| `CB EE` | `SET 5,(HL)` | 2 | 15 | yes |
| `CB EF` | `SET 5,A` | 2 | 8 | yes |
| `CB F0` | `SET 6,B` | 2 | 8 | yes |
| `CB F1` | `SET 6,C` | 2 | 8 | yes |
| `CB F2` | `SET 6,D` | 2 | 8 | yes |
| `CB F3` | `SET 6,E` | 2 | 8 | yes |
| `CB F4` | `SET 6,H` | 2 | 8 | yes |
| `CB F5` | `SET 6,L` | 2 | 8 | yes |
| `CB F6` | `SET 6,(HL)` | 2 | 15 | yes |
| `CB F7` | `SET 6,A` | 2 | 8 | yes |
| `CB F8` | `SET 7,B` | 2 | 8 | yes |
| `CB F9` | `SET 7,C` | 2 | 8 | yes |
| `CB FA` | `SET 7,D` | 2 | 8 | yes |
| `CB FB` | `SET 7,E` | 2 | 8 | yes |
| `CB FC` | `SET 7,H` | 2 | 8 | yes |
| `CB FD` | `SET 7,L` | 2 | 8 | yes |
| `CB FE` | `SET 7,(HL)` | 2 | 15 | yes |
| `CB FF` | `SET 7,A` | 2 | 8 | yes |

### `ED`-prefixed opcodes

| Opcode | Mnemonic | Bytes | T-states | Doc |
|---|---|---|---|---|
| `ED 00` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 01` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 02` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 03` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 04` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 05` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 06` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 07` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 08` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 09` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 0A` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 0B` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 0C` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 0D` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 0E` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 0F` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 10` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 11` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 12` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 13` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 14` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 15` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 16` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 17` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 18` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 19` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 1A` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 1B` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 1C` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 1D` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 1E` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 1F` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 20` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 21` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 22` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 23` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 24` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 25` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 26` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 27` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 28` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 29` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 2A` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 2B` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 2C` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 2D` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 2E` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 2F` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 30` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 31` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 32` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 33` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 34` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 35` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 36` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 37` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 38` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 39` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 3A` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 3B` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 3C` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 3D` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 3E` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 3F` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 40` | `IN B,(C)` | 2 | 12 | yes |
| `ED 41` | `OUT (C),B` | 2 | 12 | yes |
| `ED 42` | `SBC HL,BC` | 2 | 15 | yes |
| `ED 43` | `LD (nn),BC` | 4 | 20 | yes |
| `ED 44` | `NEG` | 2 | 8 | yes |
| `ED 45` | `RETN` | 2 | 14 | yes |
| `ED 46` | `IM 0` | 2 | 8 | yes |
| `ED 47` | `LD I,A` | 2 | 9 | yes |
| `ED 48` | `IN C,(C)` | 2 | 12 | yes |
| `ED 49` | `OUT (C),C` | 2 | 12 | yes |
| `ED 4A` | `ADC HL,BC` | 2 | 15 | yes |
| `ED 4B` | `LD BC,(nn)` | 4 | 20 | yes |
| `ED 4C` | `NEG` | 2 | 8 | undoc |
| `ED 4D` | `RETI` | 2 | 14 | yes |
| `ED 4E` | `IM 0/1` | 2 | 8 | undoc |
| `ED 4F` | `LD R,A` | 2 | 9 | yes |
| `ED 50` | `IN D,(C)` | 2 | 12 | yes |
| `ED 51` | `OUT (C),D` | 2 | 12 | yes |
| `ED 52` | `SBC HL,DE` | 2 | 15 | yes |
| `ED 53` | `LD (nn),DE` | 4 | 20 | yes |
| `ED 54` | `NEG` | 2 | 8 | undoc |
| `ED 55` | `RETN` | 2 | 14 | undoc |
| `ED 56` | `IM 1` | 2 | 8 | yes |
| `ED 57` | `LD A,I` | 2 | 9 | yes |
| `ED 58` | `IN E,(C)` | 2 | 12 | yes |
| `ED 59` | `OUT (C),E` | 2 | 12 | yes |
| `ED 5A` | `ADC HL,DE` | 2 | 15 | yes |
| `ED 5B` | `LD DE,(nn)` | 4 | 20 | yes |
| `ED 5C` | `NEG` | 2 | 8 | undoc |
| `ED 5D` | `RETN` | 2 | 14 | undoc |
| `ED 5E` | `IM 2` | 2 | 8 | yes |
| `ED 5F` | `LD A,R` | 2 | 9 | yes |
| `ED 60` | `IN H,(C)` | 2 | 12 | yes |
| `ED 61` | `OUT (C),H` | 2 | 12 | yes |
| `ED 62` | `SBC HL,HL` | 2 | 15 | yes |
| `ED 63` | `LD (nn),HL` | 4 | 20 | yes |
| `ED 64` | `NEG` | 2 | 8 | undoc |
| `ED 65` | `RETN` | 2 | 14 | undoc |
| `ED 66` | `IM 0` | 2 | 8 | undoc |
| `ED 67` | `RRD` | 2 | 18 | yes |
| `ED 68` | `IN L,(C)` | 2 | 12 | yes |
| `ED 69` | `OUT (C),L` | 2 | 12 | yes |
| `ED 6A` | `ADC HL,HL` | 2 | 15 | yes |
| `ED 6B` | `LD HL,(nn)` | 4 | 20 | yes |
| `ED 6C` | `NEG` | 2 | 8 | undoc |
| `ED 6D` | `RETN` | 2 | 14 | undoc |
| `ED 6E` | `IM 0/1` | 2 | 8 | undoc |
| `ED 6F` | `RLD` | 2 | 18 | yes |
| `ED 70` | `IN (C)` | 2 | 12 | undoc |
| `ED 71` | `OUT (C),0` | 2 | 12 | undoc |
| `ED 72` | `SBC HL,SP` | 2 | 15 | yes |
| `ED 73` | `LD (nn),SP` | 4 | 20 | yes |
| `ED 74` | `NEG` | 2 | 8 | undoc |
| `ED 75` | `RETN` | 2 | 14 | undoc |
| `ED 76` | `IM 1` | 2 | 8 | undoc |
| `ED 77` | `NOP` | 2 | 8 | undoc |
| `ED 78` | `IN A,(C)` | 2 | 12 | yes |
| `ED 79` | `OUT (C),A` | 2 | 12 | yes |
| `ED 7A` | `ADC HL,SP` | 2 | 15 | yes |
| `ED 7B` | `LD SP,(nn)` | 4 | 20 | yes |
| `ED 7C` | `NEG` | 2 | 8 | undoc |
| `ED 7D` | `RETN` | 2 | 14 | undoc |
| `ED 7E` | `IM 2` | 2 | 8 | undoc |
| `ED 7F` | `NOP` | 2 | 8 | undoc |
| `ED 80` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 81` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 82` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 83` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 84` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 85` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 86` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 87` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 88` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 89` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 8A` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 8B` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 8C` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 8D` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 8E` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 8F` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 90` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 91` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 92` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 93` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 94` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 95` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 96` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 97` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 98` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 99` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 9A` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 9B` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 9C` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 9D` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 9E` | `NONI, NOP` | 2 | 8 | undoc |
| `ED 9F` | `NONI, NOP` | 2 | 8 | undoc |
| `ED A0` | `LDI` | 2 | 16 | yes |
| `ED A1` | `CPI` | 2 | 16 | yes |
| `ED A2` | `INI` | 2 | 16 | yes |
| `ED A3` | `OUTI` | 2 | 16 | yes |
| `ED A4` | `NONI, NOP` | 2 | 8 | undoc |
| `ED A5` | `NONI, NOP` | 2 | 8 | undoc |
| `ED A6` | `NONI, NOP` | 2 | 8 | undoc |
| `ED A7` | `NONI, NOP` | 2 | 8 | undoc |
| `ED A8` | `LDD` | 2 | 16 | yes |
| `ED A9` | `CPD` | 2 | 16 | yes |
| `ED AA` | `IND` | 2 | 16 | yes |
| `ED AB` | `OUTD` | 2 | 16 | yes |
| `ED AC` | `NONI, NOP` | 2 | 8 | undoc |
| `ED AD` | `NONI, NOP` | 2 | 8 | undoc |
| `ED AE` | `NONI, NOP` | 2 | 8 | undoc |
| `ED AF` | `NONI, NOP` | 2 | 8 | undoc |
| `ED B0` | `LDIR` | 2 | 21/16 | yes |
| `ED B1` | `CPIR` | 2 | 21/16 | yes |
| `ED B2` | `INIR` | 2 | 21/16 | yes |
| `ED B3` | `OTIR` | 2 | 21/16 | yes |
| `ED B4` | `NONI, NOP` | 2 | 8 | undoc |
| `ED B5` | `NONI, NOP` | 2 | 8 | undoc |
| `ED B6` | `NONI, NOP` | 2 | 8 | undoc |
| `ED B7` | `NONI, NOP` | 2 | 8 | undoc |
| `ED B8` | `LDDR` | 2 | 21/16 | yes |
| `ED B9` | `CPDR` | 2 | 21/16 | yes |
| `ED BA` | `INDR` | 2 | 21/16 | yes |
| `ED BB` | `OTDR` | 2 | 21/16 | yes |
| `ED BC` | `NONI, NOP` | 2 | 8 | undoc |
| `ED BD` | `NONI, NOP` | 2 | 8 | undoc |
| `ED BE` | `NONI, NOP` | 2 | 8 | undoc |
| `ED BF` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C0` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C1` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C2` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C3` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C4` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C5` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C6` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C7` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C8` | `NONI, NOP` | 2 | 8 | undoc |
| `ED C9` | `NONI, NOP` | 2 | 8 | undoc |
| `ED CA` | `NONI, NOP` | 2 | 8 | undoc |
| `ED CB` | `NONI, NOP` | 2 | 8 | undoc |
| `ED CC` | `NONI, NOP` | 2 | 8 | undoc |
| `ED CD` | `NONI, NOP` | 2 | 8 | undoc |
| `ED CE` | `NONI, NOP` | 2 | 8 | undoc |
| `ED CF` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D0` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D1` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D2` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D3` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D4` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D5` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D6` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D7` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D8` | `NONI, NOP` | 2 | 8 | undoc |
| `ED D9` | `NONI, NOP` | 2 | 8 | undoc |
| `ED DA` | `NONI, NOP` | 2 | 8 | undoc |
| `ED DB` | `NONI, NOP` | 2 | 8 | undoc |
| `ED DC` | `NONI, NOP` | 2 | 8 | undoc |
| `ED DD` | `NONI, NOP` | 2 | 8 | undoc |
| `ED DE` | `NONI, NOP` | 2 | 8 | undoc |
| `ED DF` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E0` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E1` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E2` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E3` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E4` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E5` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E6` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E7` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E8` | `NONI, NOP` | 2 | 8 | undoc |
| `ED E9` | `NONI, NOP` | 2 | 8 | undoc |
| `ED EA` | `NONI, NOP` | 2 | 8 | undoc |
| `ED EB` | `NONI, NOP` | 2 | 8 | undoc |
| `ED EC` | `NONI, NOP` | 2 | 8 | undoc |
| `ED ED` | `NONI, NOP` | 2 | 8 | undoc |
| `ED EE` | `NONI, NOP` | 2 | 8 | undoc |
| `ED EF` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F0` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F1` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F2` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F3` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F4` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F5` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F6` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F7` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F8` | `NONI, NOP` | 2 | 8 | undoc |
| `ED F9` | `NONI, NOP` | 2 | 8 | undoc |
| `ED FA` | `NONI, NOP` | 2 | 8 | undoc |
| `ED FB` | `NONI, NOP` | 2 | 8 | undoc |
| `ED FC` | `NONI, NOP` | 2 | 8 | undoc |
| `ED FD` | `NONI, NOP` | 2 | 8 | undoc |
| `ED FE` | `NONI, NOP` | 2 | 8 | undoc |
| `ED FF` | `NONI, NOP` | 2 | 8 | undoc |

### `DD`-prefixed opcodes

| Opcode | Mnemonic | Bytes | T-states | Doc |
|---|---|---|---|---|
| `DD 00` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 01` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 02` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 03` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 04` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 05` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 06` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 07` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 08` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 09` | `ADD IX,BC` | 2 | 15 | yes |
| `DD 0A` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 0B` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 0C` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 0D` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 0E` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 0F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 10` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 11` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 12` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 13` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 14` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 15` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 16` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 17` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 18` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 19` | `ADD IX,DE` | 2 | 15 | yes |
| `DD 1A` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 1B` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 1C` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 1D` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 1E` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 1F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 20` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 21` | `LD IX,nn` | 4 | 14 | yes |
| `DD 22` | `LD (nn),IX` | 4 | 20 | yes |
| `DD 23` | `INC IX` | 2 | 10 | yes |
| `DD 24` | `INC IXH` | 2 | 8 | undoc |
| `DD 25` | `DEC IXH` | 2 | 8 | undoc |
| `DD 26` | `LD IXH,n` | 3 | 11 | undoc |
| `DD 27` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 28` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 29` | `ADD IX,IX` | 2 | 15 | yes |
| `DD 2A` | `LD IX,(nn)` | 4 | 20 | yes |
| `DD 2B` | `DEC IX` | 2 | 10 | yes |
| `DD 2C` | `INC IXL` | 2 | 8 | undoc |
| `DD 2D` | `DEC IXL` | 2 | 8 | undoc |
| `DD 2E` | `LD IXL,n` | 3 | 11 | undoc |
| `DD 2F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 30` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 31` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 32` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 33` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 34` | `INC (IX+d)` | 3 | 23 | yes |
| `DD 35` | `DEC (IX+d)` | 3 | 23 | yes |
| `DD 36` | `LD (IX+d),n` | 4 | 19 | yes |
| `DD 37` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 38` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 39` | `ADD IX,SP` | 2 | 15 | yes |
| `DD 3A` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 3B` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 3C` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 3D` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 3E` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 3F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 40` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 41` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 42` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 43` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 44` | `LD B,IXH` | 2 | 8 | undoc |
| `DD 45` | `LD B,IXL` | 2 | 8 | undoc |
| `DD 46` | `LD B,(IX+d)` | 3 | 19 | yes |
| `DD 47` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 48` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 49` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 4A` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 4B` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 4C` | `LD C,IXH` | 2 | 8 | undoc |
| `DD 4D` | `LD C,IXL` | 2 | 8 | undoc |
| `DD 4E` | `LD C,(IX+d)` | 3 | 19 | yes |
| `DD 4F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 50` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 51` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 52` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 53` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 54` | `LD D,IXH` | 2 | 8 | undoc |
| `DD 55` | `LD D,IXL` | 2 | 8 | undoc |
| `DD 56` | `LD D,(IX+d)` | 3 | 19 | yes |
| `DD 57` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 58` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 59` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 5A` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 5B` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 5C` | `LD E,IXH` | 2 | 8 | undoc |
| `DD 5D` | `LD E,IXL` | 2 | 8 | undoc |
| `DD 5E` | `LD E,(IX+d)` | 3 | 19 | yes |
| `DD 5F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 60` | `LD IXH,B` | 2 | 8 | undoc |
| `DD 61` | `LD IXH,C` | 2 | 8 | undoc |
| `DD 62` | `LD IXH,D` | 2 | 8 | undoc |
| `DD 63` | `LD IXH,E` | 2 | 8 | undoc |
| `DD 64` | `LD IXH,IXH` | 2 | 8 | undoc |
| `DD 65` | `LD IXH,IXL` | 2 | 8 | undoc |
| `DD 66` | `LD H,(IX+d)` | 3 | 19 | yes |
| `DD 67` | `LD IXH,A` | 2 | 8 | undoc |
| `DD 68` | `LD IXL,B` | 2 | 8 | undoc |
| `DD 69` | `LD IXL,C` | 2 | 8 | undoc |
| `DD 6A` | `LD IXL,D` | 2 | 8 | undoc |
| `DD 6B` | `LD IXL,E` | 2 | 8 | undoc |
| `DD 6C` | `LD IXL,IXH` | 2 | 8 | undoc |
| `DD 6D` | `LD IXL,IXL` | 2 | 8 | undoc |
| `DD 6E` | `LD L,(IX+d)` | 3 | 19 | yes |
| `DD 6F` | `LD IXL,A` | 2 | 8 | undoc |
| `DD 70` | `LD (IX+d),B` | 3 | 19 | yes |
| `DD 71` | `LD (IX+d),C` | 3 | 19 | yes |
| `DD 72` | `LD (IX+d),D` | 3 | 19 | yes |
| `DD 73` | `LD (IX+d),E` | 3 | 19 | yes |
| `DD 74` | `LD (IX+d),H` | 3 | 19 | yes |
| `DD 75` | `LD (IX+d),L` | 3 | 19 | yes |
| `DD 76` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 77` | `LD (IX+d),A` | 3 | 19 | yes |
| `DD 78` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 79` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 7A` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 7B` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 7C` | `LD A,IXH` | 2 | 8 | undoc |
| `DD 7D` | `LD A,IXL` | 2 | 8 | undoc |
| `DD 7E` | `LD A,(IX+d)` | 3 | 19 | yes |
| `DD 7F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 80` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 81` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 82` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 83` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 84` | `ADD A,IXH` | 2 | 8 | undoc |
| `DD 85` | `ADD A,IXL` | 2 | 8 | undoc |
| `DD 86` | `ADD A,(IX+d)` | 3 | 19 | yes |
| `DD 87` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 88` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 89` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 8A` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 8B` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 8C` | `ADC A,IXH` | 2 | 8 | undoc |
| `DD 8D` | `ADC A,IXL` | 2 | 8 | undoc |
| `DD 8E` | `ADC A,(IX+d)` | 3 | 19 | yes |
| `DD 8F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 90` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 91` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 92` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 93` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 94` | `SUB IXH` | 2 | 8 | undoc |
| `DD 95` | `SUB IXL` | 2 | 8 | undoc |
| `DD 96` | `SUB (IX+d)` | 3 | 19 | yes |
| `DD 97` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 98` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 99` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 9A` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 9B` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD 9C` | `SBC A,IXH` | 2 | 8 | undoc |
| `DD 9D` | `SBC A,IXL` | 2 | 8 | undoc |
| `DD 9E` | `SBC A,(IX+d)` | 3 | 19 | yes |
| `DD 9F` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD A0` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD A1` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD A2` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD A3` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD A4` | `AND IXH` | 2 | 8 | undoc |
| `DD A5` | `AND IXL` | 2 | 8 | undoc |
| `DD A6` | `AND (IX+d)` | 3 | 19 | yes |
| `DD A7` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD A8` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD A9` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD AA` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD AB` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD AC` | `XOR IXH` | 2 | 8 | undoc |
| `DD AD` | `XOR IXL` | 2 | 8 | undoc |
| `DD AE` | `XOR (IX+d)` | 3 | 19 | yes |
| `DD AF` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD B0` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD B1` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD B2` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD B3` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD B4` | `OR IXH` | 2 | 8 | undoc |
| `DD B5` | `OR IXL` | 2 | 8 | undoc |
| `DD B6` | `OR (IX+d)` | 3 | 19 | yes |
| `DD B7` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD B8` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD B9` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD BA` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD BB` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD BC` | `CP IXH` | 2 | 8 | undoc |
| `DD BD` | `CP IXL` | 2 | 8 | undoc |
| `DD BE` | `CP (IX+d)` | 3 | 19 | yes |
| `DD BF` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C0` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C1` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C2` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C3` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C4` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C5` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C6` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C7` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C8` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD C9` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD CA` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD CB` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD CC` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD CD` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD CE` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD CF` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D0` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D1` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D2` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D3` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D4` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D5` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D6` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D7` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D8` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD D9` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD DA` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD DB` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD DC` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD DD` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD DE` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD DF` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD E0` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD E1` | `POP IX` | 2 | 14 | yes |
| `DD E2` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD E3` | `EX (SP),IX` | 2 | 23 | yes |
| `DD E4` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD E5` | `PUSH IX` | 2 | 15 | yes |
| `DD E6` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD E7` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD E8` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD E9` | `JP (IX)` | 2 | 8 | yes |
| `DD EA` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD EB` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD EC` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD ED` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD EE` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD EF` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F0` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F1` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F2` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F3` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F4` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F5` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F6` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F7` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F8` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD F9` | `LD SP,IX` | 2 | 10 | yes |
| `DD FA` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD FB` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD FC` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD FD` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD FE` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |
| `DD FF` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |

### `DD CB d`-prefixed opcodes

| Opcode | Mnemonic | Bytes | T-states | Doc |
|---|---|---|---|---|
| `DD CB d 00` | `LD B,RLC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 01` | `LD C,RLC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 02` | `LD D,RLC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 03` | `LD E,RLC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 04` | `LD H,RLC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 05` | `LD L,RLC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 06` | `RLC (IX+d)` | 4 | 23 | yes |
| `DD CB d 07` | `LD A,RLC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 08` | `LD B,RRC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 09` | `LD C,RRC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 0A` | `LD D,RRC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 0B` | `LD E,RRC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 0C` | `LD H,RRC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 0D` | `LD L,RRC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 0E` | `RRC (IX+d)` | 4 | 23 | yes |
| `DD CB d 0F` | `LD A,RRC (IX+d)` | 4 | 23 | undoc |
| `DD CB d 10` | `LD B,RL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 11` | `LD C,RL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 12` | `LD D,RL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 13` | `LD E,RL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 14` | `LD H,RL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 15` | `LD L,RL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 16` | `RL (IX+d)` | 4 | 23 | yes |
| `DD CB d 17` | `LD A,RL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 18` | `LD B,RR (IX+d)` | 4 | 23 | undoc |
| `DD CB d 19` | `LD C,RR (IX+d)` | 4 | 23 | undoc |
| `DD CB d 1A` | `LD D,RR (IX+d)` | 4 | 23 | undoc |
| `DD CB d 1B` | `LD E,RR (IX+d)` | 4 | 23 | undoc |
| `DD CB d 1C` | `LD H,RR (IX+d)` | 4 | 23 | undoc |
| `DD CB d 1D` | `LD L,RR (IX+d)` | 4 | 23 | undoc |
| `DD CB d 1E` | `RR (IX+d)` | 4 | 23 | yes |
| `DD CB d 1F` | `LD A,RR (IX+d)` | 4 | 23 | undoc |
| `DD CB d 20` | `LD B,SLA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 21` | `LD C,SLA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 22` | `LD D,SLA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 23` | `LD E,SLA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 24` | `LD H,SLA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 25` | `LD L,SLA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 26` | `SLA (IX+d)` | 4 | 23 | yes |
| `DD CB d 27` | `LD A,SLA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 28` | `LD B,SRA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 29` | `LD C,SRA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 2A` | `LD D,SRA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 2B` | `LD E,SRA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 2C` | `LD H,SRA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 2D` | `LD L,SRA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 2E` | `SRA (IX+d)` | 4 | 23 | yes |
| `DD CB d 2F` | `LD A,SRA (IX+d)` | 4 | 23 | undoc |
| `DD CB d 30` | `LD B,SLL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 31` | `LD C,SLL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 32` | `LD D,SLL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 33` | `LD E,SLL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 34` | `LD H,SLL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 35` | `LD L,SLL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 36` | `SLL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 37` | `LD A,SLL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 38` | `LD B,SRL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 39` | `LD C,SRL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 3A` | `LD D,SRL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 3B` | `LD E,SRL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 3C` | `LD H,SRL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 3D` | `LD L,SRL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 3E` | `SRL (IX+d)` | 4 | 23 | yes |
| `DD CB d 3F` | `LD A,SRL (IX+d)` | 4 | 23 | undoc |
| `DD CB d 40` | `BIT 0,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 41` | `BIT 0,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 42` | `BIT 0,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 43` | `BIT 0,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 44` | `BIT 0,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 45` | `BIT 0,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 46` | `BIT 0,(IX+d)` | 4 | 20 | yes |
| `DD CB d 47` | `BIT 0,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 48` | `BIT 1,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 49` | `BIT 1,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 4A` | `BIT 1,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 4B` | `BIT 1,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 4C` | `BIT 1,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 4D` | `BIT 1,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 4E` | `BIT 1,(IX+d)` | 4 | 20 | yes |
| `DD CB d 4F` | `BIT 1,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 50` | `BIT 2,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 51` | `BIT 2,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 52` | `BIT 2,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 53` | `BIT 2,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 54` | `BIT 2,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 55` | `BIT 2,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 56` | `BIT 2,(IX+d)` | 4 | 20 | yes |
| `DD CB d 57` | `BIT 2,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 58` | `BIT 3,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 59` | `BIT 3,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 5A` | `BIT 3,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 5B` | `BIT 3,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 5C` | `BIT 3,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 5D` | `BIT 3,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 5E` | `BIT 3,(IX+d)` | 4 | 20 | yes |
| `DD CB d 5F` | `BIT 3,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 60` | `BIT 4,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 61` | `BIT 4,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 62` | `BIT 4,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 63` | `BIT 4,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 64` | `BIT 4,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 65` | `BIT 4,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 66` | `BIT 4,(IX+d)` | 4 | 20 | yes |
| `DD CB d 67` | `BIT 4,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 68` | `BIT 5,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 69` | `BIT 5,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 6A` | `BIT 5,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 6B` | `BIT 5,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 6C` | `BIT 5,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 6D` | `BIT 5,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 6E` | `BIT 5,(IX+d)` | 4 | 20 | yes |
| `DD CB d 6F` | `BIT 5,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 70` | `BIT 6,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 71` | `BIT 6,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 72` | `BIT 6,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 73` | `BIT 6,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 74` | `BIT 6,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 75` | `BIT 6,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 76` | `BIT 6,(IX+d)` | 4 | 20 | yes |
| `DD CB d 77` | `BIT 6,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 78` | `BIT 7,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 79` | `BIT 7,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 7A` | `BIT 7,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 7B` | `BIT 7,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 7C` | `BIT 7,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 7D` | `BIT 7,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 7E` | `BIT 7,(IX+d)` | 4 | 20 | yes |
| `DD CB d 7F` | `BIT 7,(IX+d)` | 4 | 20 | undoc |
| `DD CB d 80` | `LD B,RES 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 81` | `LD C,RES 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 82` | `LD D,RES 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 83` | `LD E,RES 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 84` | `LD H,RES 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 85` | `LD L,RES 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 86` | `RES 0,(IX+d)` | 4 | 23 | yes |
| `DD CB d 87` | `LD A,RES 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 88` | `LD B,RES 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 89` | `LD C,RES 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 8A` | `LD D,RES 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 8B` | `LD E,RES 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 8C` | `LD H,RES 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 8D` | `LD L,RES 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 8E` | `RES 1,(IX+d)` | 4 | 23 | yes |
| `DD CB d 8F` | `LD A,RES 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 90` | `LD B,RES 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 91` | `LD C,RES 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 92` | `LD D,RES 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 93` | `LD E,RES 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 94` | `LD H,RES 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 95` | `LD L,RES 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 96` | `RES 2,(IX+d)` | 4 | 23 | yes |
| `DD CB d 97` | `LD A,RES 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 98` | `LD B,RES 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 99` | `LD C,RES 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 9A` | `LD D,RES 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 9B` | `LD E,RES 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 9C` | `LD H,RES 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 9D` | `LD L,RES 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d 9E` | `RES 3,(IX+d)` | 4 | 23 | yes |
| `DD CB d 9F` | `LD A,RES 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A0` | `LD B,RES 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A1` | `LD C,RES 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A2` | `LD D,RES 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A3` | `LD E,RES 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A4` | `LD H,RES 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A5` | `LD L,RES 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A6` | `RES 4,(IX+d)` | 4 | 23 | yes |
| `DD CB d A7` | `LD A,RES 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A8` | `LD B,RES 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d A9` | `LD C,RES 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d AA` | `LD D,RES 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d AB` | `LD E,RES 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d AC` | `LD H,RES 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d AD` | `LD L,RES 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d AE` | `RES 5,(IX+d)` | 4 | 23 | yes |
| `DD CB d AF` | `LD A,RES 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B0` | `LD B,RES 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B1` | `LD C,RES 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B2` | `LD D,RES 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B3` | `LD E,RES 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B4` | `LD H,RES 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B5` | `LD L,RES 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B6` | `RES 6,(IX+d)` | 4 | 23 | yes |
| `DD CB d B7` | `LD A,RES 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B8` | `LD B,RES 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d B9` | `LD C,RES 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d BA` | `LD D,RES 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d BB` | `LD E,RES 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d BC` | `LD H,RES 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d BD` | `LD L,RES 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d BE` | `RES 7,(IX+d)` | 4 | 23 | yes |
| `DD CB d BF` | `LD A,RES 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C0` | `LD B,SET 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C1` | `LD C,SET 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C2` | `LD D,SET 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C3` | `LD E,SET 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C4` | `LD H,SET 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C5` | `LD L,SET 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C6` | `SET 0,(IX+d)` | 4 | 23 | yes |
| `DD CB d C7` | `LD A,SET 0,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C8` | `LD B,SET 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d C9` | `LD C,SET 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d CA` | `LD D,SET 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d CB` | `LD E,SET 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d CC` | `LD H,SET 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d CD` | `LD L,SET 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d CE` | `SET 1,(IX+d)` | 4 | 23 | yes |
| `DD CB d CF` | `LD A,SET 1,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D0` | `LD B,SET 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D1` | `LD C,SET 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D2` | `LD D,SET 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D3` | `LD E,SET 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D4` | `LD H,SET 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D5` | `LD L,SET 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D6` | `SET 2,(IX+d)` | 4 | 23 | yes |
| `DD CB d D7` | `LD A,SET 2,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D8` | `LD B,SET 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d D9` | `LD C,SET 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d DA` | `LD D,SET 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d DB` | `LD E,SET 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d DC` | `LD H,SET 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d DD` | `LD L,SET 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d DE` | `SET 3,(IX+d)` | 4 | 23 | yes |
| `DD CB d DF` | `LD A,SET 3,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E0` | `LD B,SET 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E1` | `LD C,SET 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E2` | `LD D,SET 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E3` | `LD E,SET 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E4` | `LD H,SET 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E5` | `LD L,SET 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E6` | `SET 4,(IX+d)` | 4 | 23 | yes |
| `DD CB d E7` | `LD A,SET 4,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E8` | `LD B,SET 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d E9` | `LD C,SET 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d EA` | `LD D,SET 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d EB` | `LD E,SET 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d EC` | `LD H,SET 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d ED` | `LD L,SET 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d EE` | `SET 5,(IX+d)` | 4 | 23 | yes |
| `DD CB d EF` | `LD A,SET 5,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F0` | `LD B,SET 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F1` | `LD C,SET 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F2` | `LD D,SET 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F3` | `LD E,SET 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F4` | `LD H,SET 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F5` | `LD L,SET 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F6` | `SET 6,(IX+d)` | 4 | 23 | yes |
| `DD CB d F7` | `LD A,SET 6,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F8` | `LD B,SET 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d F9` | `LD C,SET 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d FA` | `LD D,SET 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d FB` | `LD E,SET 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d FC` | `LD H,SET 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d FD` | `LD L,SET 7,(IX+d)` | 4 | 23 | undoc |
| `DD CB d FE` | `SET 7,(IX+d)` | 4 | 23 | yes |
| `DD CB d FF` | `LD A,SET 7,(IX+d)` | 4 | 23 | undoc |

