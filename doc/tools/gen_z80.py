#!/usr/bin/env python3
"""Generate the complete Z80 opcode tables from the algorithmic decoding rules
(Cristian Dinu, "Decoding Z80 Opcodes") plus the standard T-state timings.

Emits markdown tables for: unprefixed, CB, ED, DD/FD, DDCB/FDCB.
"""

R    = ['B','C','D','E','H','L','(HL)','A']
RP   = ['BC','DE','HL','SP']
RP2  = ['BC','DE','HL','AF']
CC   = ['NZ','Z','NC','C','PO','PE','P','M']
ALU  = ['ADD A,','ADC A,','SUB ','SBC A,','AND ','XOR ','OR ','CP ']
ROT  = ['RLC','RRC','RL','RR','SLA','SRA','SLL','SRL']
IM   = ['0','0/1','1','2','0','0/1','1','2']
BLI  = {(4,0):'LDI',(4,1):'CPI',(4,2):'INI',(4,3):'OUTI',
        (5,0):'LDD',(5,1):'CPD',(5,2):'IND',(5,3):'OUTD',
        (6,0):'LDIR',(6,1):'CPIR',(6,2):'INIR',(6,3):'OTIR',
        (7,0):'LDDR',(7,1):'CPDR',(7,2):'INDR',(7,3):'OTDR'}

def xyzpq(op):
    return (op >> 6, (op >> 3) & 7, op & 7, (op >> 4) & 3, (op >> 3) & 1)

# ---------------------------------------------------------------- unprefixed
def unprefixed(op):
    """returns (mnemonic, bytes, tstates_str, undocumented)"""
    x, y, z, p, q = xyzpq(op)
    if x == 0:
        if z == 0:
            if y == 0: return 'NOP', 1, '4', False
            if y == 1: return "EX AF,AF'", 1, '4', False
            if y == 2: return 'DJNZ d', 2, '13/8', False
            if y == 3: return 'JR d', 2, '12', False
            return f'JR {CC[y-4]},d', 2, '12/7', False
        if z == 1:
            if q == 0: return f'LD {RP[p]},nn', 3, '10', False
            return f'ADD HL,{RP[p]}', 1, '11', False
        if z == 2:
            if q == 0:
                return [('LD (BC),A',1,'7'),('LD (DE),A',1,'7'),
                        ('LD (nn),HL',3,'16'),('LD (nn),A',3,'13')][p] + (False,)
            return [('LD A,(BC)',1,'7'),('LD A,(DE)',1,'7'),
                    ('LD HL,(nn)',3,'16'),('LD A,(nn)',3,'13')][p] + (False,)
        if z == 3:
            return (f'INC {RP[p]}' if q == 0 else f'DEC {RP[p]}'), 1, '6', False
        if z == 4: return f'INC {R[y]}', 1, ('11' if y == 6 else '4'), False
        if z == 5: return f'DEC {R[y]}', 1, ('11' if y == 6 else '4'), False
        if z == 6: return f'LD {R[y]},n', 2, ('10' if y == 6 else '7'), False
        return ['RLCA','RRCA','RLA','RRA','DAA','CPL','SCF','CCF'][y], 1, '4', False
    if x == 1:
        if z == 6 and y == 6: return 'HALT', 1, '4', False
        return f'LD {R[y]},{R[z]}', 1, ('7' if 6 in (y, z) else '4'), False
    if x == 2:
        return f'{ALU[y]}{R[z]}', 1, ('7' if z == 6 else '4'), False
    # x == 3
    if z == 0: return f'RET {CC[y]}', 1, '11/5', False
    if z == 1:
        if q == 0: return f'POP {RP2[p]}', 1, '10', False
        return [('RET',1,'10'),('EXX',1,'4'),('JP (HL)',1,'4'),('LD SP,HL',1,'6')][p] + (False,)
    if z == 2: return f'JP {CC[y]},nn', 3, '10', False
    if z == 3:
        return [('JP nn',3,'10'),('*CB prefix*',1,'-'),('OUT (n),A',2,'11'),
                ('IN A,(n)',2,'11'),('EX (SP),HL',1,'19'),('EX DE,HL',1,'4'),
                ('DI',1,'4'),('EI',1,'4')][y] + (False,)
    if z == 4: return f'CALL {CC[y]},nn', 3, '17/10', False
    if z == 5:
        if q == 0: return f'PUSH {RP2[p]}', 1, '11', False
        return [('CALL nn',3,'17'),('*DD prefix*',1,'-'),
                ('*ED prefix*',1,'-'),('*FD prefix*',1,'-')][p] + (False,)
    if z == 6: return f'{ALU[y]}n', 2, '7', False
    return f'RST {y*8:02X}h', 1, '11', False

# ------------------------------------------------------------------ CB table
def cb(op):
    x, y, z, p, q = xyzpq(op)
    m = z == 6
    if x == 0:
        return f'{ROT[y]} {R[z]}', 2, ('15' if m else '8'), ROT[y] == 'SLL'
    if x == 1: return f'BIT {y},{R[z]}', 2, ('12' if m else '8'), False
    if x == 2: return f'RES {y},{R[z]}', 2, ('15' if m else '8'), False
    return f'SET {y},{R[z]}', 2, ('15' if m else '8'), False

# ------------------------------------------------------------------ ED table
ED_DOCUMENTED = set()
for _y in range(8):
    ED_DOCUMENTED |= {0x40 + _y*8 + _z for _z in (0,1,2,3)}
def ed(op):
    x, y, z, p, q = xyzpq(op)
    if x in (0, 3):
        return 'NONI, NOP', 2, '8', True
    if x == 1:
        if z == 0:
            if y == 6: return 'IN (C)', 2, '12', True
            return f'IN {R[y]},(C)', 2, '12', False
        if z == 1:
            if y == 6: return 'OUT (C),0', 2, '12', True
            return f'OUT (C),{R[y]}', 2, '12', False
        if z == 2:
            return (f'SBC HL,{RP[p]}' if q == 0 else f'ADC HL,{RP[p]}'), 2, '15', False
        if z == 3:
            return (f'LD (nn),{RP[p]}' if q == 0 else f'LD {RP[p]},(nn)'), 4, '20', False
        if z == 4: return 'NEG', 2, '8', y != 0
        if z == 5: return ('RETI' if y == 1 else 'RETN'), 2, '14', y not in (0,1)
        if z == 6: return f'IM {IM[y]}', 2, '8', y not in (0,2,3)
        return [('LD I,A',2,'9',False),('LD R,A',2,'9',False),
                ('LD A,I',2,'9',False),('LD A,R',2,'9',False),
                ('RRD',2,'18',False),('RLD',2,'18',False),
                ('NOP',2,'8',True),('NOP',2,'8',True)][y]
    # x == 2
    if z <= 3 and y >= 4:
        m = BLI[(y, z)]
        return m, 2, ('21/16' if m.endswith('R') else '16'), False
    return 'NONI, NOP', 2, '8', True

# --------------------------------------------------------------- DD/FD table
def ddfd(op, ix='IX'):
    """Return (mnemonic, bytes, tstates, undocumented) or None if the prefix is inert."""
    h, l = ix + 'H', ix + 'L'
    x, y, z, p, q = xyzpq(op)
    if op in (0xDD, 0xED, 0xFD, 0xCB):
        return None                       # handled separately
    base, nb, tt, _ = unprefixed(op)
    # does it touch HL / H / L / (HL)?
    def sub_rp(name): return ix if name == 'HL' else name
    if x == 0:
        if z == 1 and q == 0 and p == 2: return f'LD {ix},nn', 4, '14', False
        if z == 1 and q == 1: return f'ADD {ix},{sub_rp(RP[p])}', 2, '15', False
        if z == 2 and p == 2:
            return (f'LD (nn),{ix}' if q == 0 else f'LD {ix},(nn)'), 4, '20', False
        if z == 3 and p == 2:
            return (f'INC {ix}' if q == 0 else f'DEC {ix}'), 2, '10', False
        if z in (4, 5) and y == 6:
            v = 'INC' if z == 4 else 'DEC'
            return f'{v} ({ix}+d)', 3, '23', False
        if z in (4, 5) and y in (4, 5):
            v = 'INC' if z == 4 else 'DEC'
            return f'{v} {h if y==4 else l}', 2, '8', True
        if z == 6 and y == 6: return f'LD ({ix}+d),n', 4, '19', False
        if z == 6 and y in (4, 5): return f'LD {h if y==4 else l},n', 3, '11', True
        return None
    if x == 1:
        if y == 6 and z == 6: return None            # HALT
        if y == 6: return f'LD ({ix}+d),{R[z]}', 3, '19', False
        if z == 6: return f'LD {R[y]},({ix}+d)', 3, '19', False
        ry = h if y == 4 else l if y == 5 else R[y]
        rz = h if z == 4 else l if z == 5 else R[z]
        if ry == R[y] and rz == R[z]: return None
        return f'LD {ry},{rz}', 2, '8', True
    if x == 2:
        if z == 6: return f'{ALU[y]}({ix}+d)', 3, '19', False
        if z in (4, 5): return f'{ALU[y]}{h if z==4 else l}', 2, '8', True
        return None
    # x == 3
    if z == 1 and q == 0 and p == 2: return f'POP {ix}', 2, '14', False
    if z == 1 and q == 1 and p == 2: return f'JP ({ix})', 2, '8', False
    if z == 1 and q == 1 and p == 3: return f'LD SP,{ix}', 2, '10', False
    if z == 3 and y == 4: return f'EX (SP),{ix}', 2, '23', False
    if z == 5 and q == 0 and p == 2: return f'PUSH {ix}', 2, '15', False
    return None

# ------------------------------------------------------------ DDCB/FDCB table
def ddcb(op, ix='IX'):
    x, y, z, p, q = xyzpq(op)
    if x == 0:
        if z == 6: return f'{ROT[y]} ({ix}+d)', 4, '23', ROT[y] == 'SLL'
        return f'LD {R[z]},{ROT[y]} ({ix}+d)', 4, '23', True
    if x == 1: return f'BIT {y},({ix}+d)', 4, '20', z != 6
    op_name = 'RES' if x == 2 else 'SET'
    if z == 6: return f'{op_name} {y},({ix}+d)', 4, '23', False
    return f'LD {R[z]},{op_name} {y},({ix}+d)', 4, '23', True

# ------------------------------------------------------------------ rendering
def grid(fn, title, note=''):
    """16x16 hex grid of mnemonics."""
    out = [f'### {title}', '']
    if note: out += [note, '']
    out.append('|    | ' + ' | '.join(f'_{c:X}' for c in range(16)) + ' |')
    out.append('|----|' + '|'.join(['---']*16) + '|')
    for hi in range(16):
        cells = []
        for lo in range(16):
            r = fn(hi*16 + lo)
            cells.append('—' if r is None else r[0].replace('|', '\\|'))
        out.append(f'| **{hi:X}_** | ' + ' | '.join(cells) + ' |')
    out.append('')
    return out

def listing(fn, title, prefix='', note=''):
    out = [f'### {title}', '']
    if note: out += [note, '']
    out += ['| Opcode | Mnemonic | Bytes | T-states | Doc |',
            '|---|---|---|---|---|']
    for op in range(256):
        r = fn(op)
        if r is None:
            out.append(f'| `{prefix}{op:02X}` | — (prefix has no effect; behaves as the unprefixed opcode + 4 T) |  |  |  |')
            continue
        m, nb, tt, undoc = r
        out.append(f'| `{prefix}{op:02X}` | `{m}` | {nb} | {tt} | {"undoc" if undoc else "yes"} |')
    out.append('')
    return out

if __name__ == '__main__':
    import sys
    what = sys.argv[1]
    if what == 'grids':
        L = []
        L += grid(unprefixed, 'Unprefixed opcode matrix')
        L += grid(cb, '`CB`-prefixed opcode matrix')
        L += grid(ed, '`ED`-prefixed opcode matrix')
        L += grid(lambda o: ddfd(o, 'IX'), '`DD`-prefixed opcode matrix (`FD` identical with IY)')
        L += grid(lambda o: ddcb(o, 'IX'), '`DD CB d`-prefixed opcode matrix (`FD CB d` identical with IY)')
        print('\n'.join(L))
    elif what == 'lists':
        L = []
        L += listing(unprefixed, 'Unprefixed opcodes')
        L += listing(cb, '`CB`-prefixed opcodes', 'CB ')
        L += listing(ed, '`ED`-prefixed opcodes', 'ED ')
        L += listing(lambda o: ddfd(o, 'IX'), '`DD`-prefixed opcodes', 'DD ')
        L += listing(lambda o: ddcb(o, 'IX'), '`DD CB d`-prefixed opcodes', 'DD CB d ')
        print('\n'.join(L))
