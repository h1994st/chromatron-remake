# Chromatron 4 v1.0 — Game logic (diff against Chromatron 2)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 4.app/Contents/MacOS/chromatron4` (Mach-O, big-endian PowerPC).
All addresses are vmaddrs, and file offset = vmaddr − 0x1000. Function and data names are the ones in `logic.names.tsv` (this note), plus
the data agent's `data.names.tsv`. Where a C4 function does the same job as a C2 one, it keeps the C2 name.

In prose, 'slot N' is the 1-based strip position, i.e. `current_slot = N − 1`. Code expressions use the 0-based `current_slot`.

Baseline: `re/notes/chromatron2/logic.md` (itself a diff against C1), plus `re/notes/chromatron1/{logic,data}.md` for the cell layout.
The notation is the same: `p` = the piece's `dir` byte, `d` = beam travel direction, `c` = colour mask (1 B, 2 G, 4 R),
`t` = entanglement tag, **`r = (d − p) & 7`**, `+k` = output direction `(p + k) & 7`, "same" = keeps going in direction `d`,
`in[d]` = cell `+4+d`, `out[d]` = cell `+12+d`, `snap[d]` = cell `+0x14+d`, `shut` = cell `+3`.

**Summary of differences from C2**
1. The cell is unchanged: 28 bytes (0x1c), with rows of 0x1a4 bytes, 19 rows, and saved boards of 0x1f2c bytes. The C2 fields `+3` (complementor shut-down flag)
   and `+0x14` (input snapshot) keep their meanings.
2. There are **seven new piece types, 16–22**: 16 LOGIC GATE (movable), 17 STARBURST (fixed), 18 MOBILE TELEPORTER (movable),
   19 DUAL-REFLECTOR (movable), 20 QUAD-BENDER (movable), 21 OCCUPIED SQUARE (fixed) and 22 PRIMARY LOGIC GATE (movable).
   Types 0–15 behave exactly as in C2, except that the **fixed teleporter (12)** now also lands on a mobile teleporter (18).
3. **Both logic gates absorb in the tracer** and emit from a new **logic-gate pass** (`emit_logic_gates` @ 0x3f1c). This pass runs twice per
   iteration of the complementor fixed-point loop, once before and once after the complementors (L3, L4).
4. **The beam cap per generation is 128** (`push_beam` @ 0x3020: `cmpwi r10, 0x80`), not 32. The beam buffers are 0x400 bytes each.
5. **The unlock rule is simpler**: besides every completed slot, the 25 lowest uncompleted slots are always open.
6. **The force-win cheat key is char code 0x0F (Control-O)**, not `z`.
7. The splitter, complementor, twister, multi-filter, entanglement pass, win test and win display are **unchanged**.
   The toolbox is sorted by type at level start, as in C2 (data.md D3; `move_pieces_to_toolbox` @ 0x573c, qsort with the comparator @ 0x572c).

Chromatron 3 v1.1 has the same code for types 0–20 (its `beam_enter_cell` @ 0x3320 has the same cases 0x10–0x14) and the same
`retrace_beams` structure (C3 @ 0x4258, with its gate pass @ 0x4198). C3 has no type 21 or 22, keeps the 32-beam cap (C3 `push_beam` @ 0x32a4:
`cmpwi r10, 0x20`) and uses `^` as its cheat key. C3's unlock rule is the same "25 open" rule (C3 @ 0x4454: `iVar4 = 0x19`).
So types 16–20 date from C3, and types 21–22, the 128 cap and the Control-O key are new in C4.

Globals reached through `__nl_symbol_ptr` (`r2 -qc 'pxw 4 @ <slot>'`), logic area only:

| slot | → target | name | meaning (as C2 unless noted) |
|---|---|---|---|
| 0x15564 | 0x19b44 | `cursor_row` | int |
| 0x15568 | 0x19b40 | `cursor_col` | int |
| 0x1557c | 0x16ac4 | `game_mode` | int, set to 2 by every `start_level` (0x58ac) |
| 0x15580 | 0x177e0 | `force_win` | int, set by the **0x0F** key |
| 0x15584 | 0x178f0 | `level_won` | int |
| 0x15588 | 0x19940 | `saved_states` | `u8*[]` per level id, 0x1f2c-byte board copies |
| 0x1558c | 0x176e0 | `unlocked` | `int[64]` per slot |
| 0x15590 | 0x177f0 | `completed` | `int[64]` per slot |
| 0x15594 | 0x162b0 | `beam_cur_count` | int |
| 0x15598 | 0x166c0 | `beam_buf_b` | **128** × 8-byte beams |
| 0x1559c | 0x162c0 | `beam_buf_a` | **128** × 8-byte beams |
| 0x155a0 | 0x162ac | `beam_cur_list` | pointer |
| 0x155a8 | 0x19930 | `beam_next_list` | pointer |
| 0x155ac | 0x19934 | `beam_next_count` | int |

Directly addressed: `entangle_counter` @ 0x1560c, `drag_state` @ 0x155ec, `mouse_alt_button` @ 0x155e4,
`press_x`/`press_y` @ 0x1621c/0x16220, `drag_image` @ 0x80d0, and `dir_dx`/`dir_dy` @ 0x8044/0x8064, which hold C2's values.
`doppler_back_map` @ 0x8084 = `[0,2,4,0,1]` and `doppler_fwd_map` @ 0x8098 = `[0,4,1,0,2]` are also unchanged.
The board, level, name and hint tables belong to data.md: `board` 0x17900 (slot 0x155a4), `level_table` 0xbb6c (slot 0x155b8),
the level-name table 0xbc48 (0x155b0), `hint_table` 0xbd24 (0x155b4), `level_order` 0x8110 (0x15558) and `current_slot` 0x803c.

## Findings

### L1. Piece kinds, movable and rotatable
**Answer:** The cell `type` byte holds the piece enum. Types 0–15 are C2's. Types 16–22 are new. Anything above 22 hits the `default:` assertion
in `beam_enter_cell` (game.c line 0x1a6, the same as C1/C2): the check is `cmplwi cr7, r11, 0x16; bgt 0x376c`.

| type | piece | colour byte | movable = rotatable | `dir` bits that matter | image (`cell_image_index` @ 0x4784) | beam behaviour (L3) |
|---|---|---|---|---|---|---|
| 0 | empty | – | – | – | 0 | passes |
| 1 | wall | – | no | – | 1 | absorbs |
| 2 | laser, 1/2/4/7 | yes | no | all 8 | as C2 | emits; absorbs incoming |
| 3 | mirror | – | yes | all 8 | 32+d | as C2 |
| 4 | bender | – | yes | all 8 | 48+d | as C2 |
| 5 | filter, 1/2/4 | yes | yes | `& 3` | as C2 | as C2 |
| 6 | prism | – | yes | all 8 | 24+d | as C2 |
| 7 | doppler | – | yes | all 8 | 64+d | as C2 |
| 8 | splitter | – | yes | `& 3` | 40+(d&3) | as C2 (pass + reflect, tag cleared) |
| 9 | quantum tangler | – | yes | all 8 | 80+d | as C2 |
| 10 | pinwheel, mask 0–7 | yes | no | – | as C2 (+1 lit) | passes, records colour |
| 11 | conduit | – | no | `& 3` | 44+(d&3) | as C2 |
| 12 | teleporter | – | no | – | 14 | as C2, but lands on type 12 **or 18** |
| 13 | multi-filter | ignored | yes | `& 3` | 100+(d&3) | as C2 |
| 14 | twister | ignored | no | none | 15 | as C2 (`d − 2`) |
| 15 | complementor | ignored | yes | `& 1` | 116+(d&1) | as C2 (absorbs; complementor pass) |
| **16** | **LOGIC GATE** | ignored | **yes** | all 8 | **128+d** | absorbs; the logic-gate pass emits `in[p+1] & in[p+7]` in direction `p` |
| **17** | **STARBURST** | ignored | **no** | none (dir ignored; always 0 in data) | **144** | 4 beams: orthogonal in → 4 diagonals out, diagonal in → 4 orthogonals out; tag 0 |
| **18** | **MOBILE TELEPORTER** | ignored | **yes** | none | **145** | teleporter; lands on type 12 or 18 |
| **19** | **DUAL-REFLECTOR** | ignored | **yes** | `& 3` | **136+(d&3)** | double-sided mirror: out `2p − d`, absorbs r = 0/4 |
| **20** | **QUAD-BENDER** | ignored | **yes** | `& 3` | **140+(d&3)** | double-sided bender: out `2p + 7 − d` for every r |
| **21** | **OCCUPIED SQUARE** | ignored | **no** | none | **146** | passes (like empty); blocks drops because type ≠ 0 |
| **22** | **PRIMARY LOGIC GATE** | ignored | **yes** | all 8 | **150+d** | absorbs; the gate pass emits only if `in[p+1] & in[p+7]` is exactly 1, 2 or 4 |

`is_movable(type)` @ 0x2f88 is true exactly for types **3–9, 13, 15, 16, 18, 19, 20 and 22**. Every drag, drop, rotate, toolbox move and
clipboard copy/paste goes through it. The starburst, occupied square and twister are fixed board pieces. As in C2,
`move_pieces_to_toolbox` @ 0x573c moves **every** movable record to the toolbox at level start and sets its `dir` to 0, so no level has a
fixed movable piece. The mobile teleporter in particular always starts in the toolbox.
Record counts over the 54 level blobs (ids 1–54): logic gate 9, starburst 51, mobile teleporter 37, dual-reflector 4, quad-bender 1,
occupied square 356 and primary logic gate 6 (twister 7, multi-filter 1, complementor 5). The most movable pieces in a playable level is 21 (id 32, slot 31); 24 in unreachable id 35.
Names come from the hint strings: LOGIC GATE 0x7334 (id 1), STARBURST 0x7384 (id 4) and 0x73cc (id 6), MOBILE TELEPORTER 0x74c0 (id 16),
TWISTER 0x7520 (id 17), QUAD-BENDER 0x7590 (id 19), PRIMARY LOGIC GATE 0x75ec (id 27), MULTI-FILTER 0x7654 (id 32),
DUAL-REFLECTOR 0x76c4 (id 36), OCCUPIED SQUARE 0x772c (id 51) and COMPLEMENTOR 0x742c (id 7). Each hint's level contains that piece type.
**Evidence:** `is_movable @ 0x2f88`: `addi r3,r3,-3; cmplwi cr7,r3,0x13; bgt -> 0x3018 (return 0)`, then a 20-entry table at 0x2fc0
(0x50 → 0x3010 `li r3,1`, 0x58 → 0x3018 `li r3,0`): `P P P P P P P X X X P X P P X P P P X P` for types 3…22.
`beam_enter_cell @ 0x309c`: the jump table at 0x3120 (base 0x3120, 23 entries) gives 0/10/21 → 0x31a0 pass, 1/2/15/16/22 → 0x378c absorb,
3 → 0x317c, 4 → 0x31b4, 5 → 0x3454, 6 → 0x32a0, 7 → 0x33d8, 8 → 0x34f8, 9 → 0x348c, 11 → 0x359c, **12/18 → 0x35b0**, 13 → 0x3628,
14 → 0x36a0, **17 → 0x36a8, 19 → 0x3234, 20 → 0x31e0**.
`cell_image_index @ 0x4784`: `case 0x10: uVar6 + 0x80; case 0x11: 0x90; case 0x12: 0x91; case 0x13: (d&3)+0x88; case 0x14: (d&3)+0x8c; case 0x15: 0x92; case 0x16: uVar6 + 0x96`.
**Confidence:** confirmed.

### L2. Direction encoding and rotation
**Answer:** Same as C2/C1. The directions 0–7 are N, NE, …, NW, and `dir_dx`/`dir_dy` @ 0x8044/0x8064 hold the same values. A plain left click
rotates `dir − 1`. A right click, or a left click with Shift/Option/Control (`& 0x1a00`), rotates `dir + 1`. Both work on the board and in the
toolbox, and both are guarded by `is_movable`. New pieces: dual-reflector and quad-bender `dir` matter only `& 3`, because 180° rotations are identical
(`2(p+4) − d ≡ 2p − d`). Logic gates use all 8 directions. The starburst, mobile teleporter and occupied square ignore `dir`.
**Evidence:** `handle_mouse @ 0x5318`: event 4 `puVar9[iVar11 + 2] + -1; & 7`, event 6 `+ '\x01'; & 7`. `on_mouse_event @ 0x2d9c`:
`DAT_000155e4 = -(local_1c & 0x1a00) >> 0x1f` on mouse-down.
**Confidence:** confirmed.

### L3. Beam tracer and transfer table
**Answer:**

**Data structures**: same as C2. A beam is 8 bytes, `{u8 x, u8 y, u8 dir, u8 colour, i32 tag}`, and there are two lists, now **128** entries each
(`beam_buf_a` 0x162c0, `beam_buf_b` 0x166c0, 0x400 bytes apart). `push_beam` @ 0x3020 drops the beam if the next list already holds 128.

**Per-laser trace** `trace_laser(x, y, d, c)` @ 0x37a0: identical to C2. It sets `entangle_counter = 1`, pushes `(x, y, d, c, tag 0)`
(`li r7, 0` @ 0x37c4) and allows up to 1023 generations (`li r30, 0x3ff` @ 0x3810). Each generation marks `out`, steps, and calls
`beam_enter_cell` if the new `x < 15 && y < 15` (unsigned bytes). Then it runs the unchanged entanglement pass over the next list (doppler maps @ 0x8084/0x8098).

**Top level** (`retrace_beams` @ 0x405c, **new structure**):
```
retrace_beams():
    for rows 0..14, cols 0..14: shut(x,y) = 0             # clear_complementor_flags @ 0x3b1c
    repeat:
        clear in[]/out[] of all 15×15 cells                # clear_beams @ 0x3aa8
        snapshot_cell_inputs()                             # 0x3bec — new extra call; no effect (in[] is all 0, overwritten below)
        for y in 0..14: for x in 0..14:                    # trace_all_lasers @ 0x3b64 (row-major)
            if type(x,y) == 2: trace_laser(x, y, dir, colour)
        repeat:
            snapshot_cell_inputs()                         # snap[0..7] = in[0..7] for every cell
            changed  = emit_logic_gates()                  # 0x3f1c   (NEW, call 1)
            emit_complementors()                           # 0x3da8   (return value ignored)
            if complementor_new_input(): changed = 1       # 0x3d14
            if emit_logic_gates():       changed = 1       # 0x3f1c   (NEW, call 2)
        until not changed
    until not shut_down_complementors()                    # 0x3c5c
```

**Transfer table** (`beam_enter_cell` @ 0x309c). As in C1/C2, `in[d] |= c` is recorded first for every cell type, and outputs are pushed in
the listed order. An output keeps colour `c` and tag `t` unless the table says otherwise. "–" means absorbed.

| type | r=0 | r=1 | r=2 | r=3 | r=4 | r=5 | r=6 | r=7 |
|---|---|---|---|---|---|---|---|---|
| 0 empty, 10 pinwheel, **21 occupied** | same | same | same | same | same | same | same | same |
| 1 wall, 2 laser, 15 complementor, **16 logic gate, 22 primary logic gate** | – | – | – | – | – | – | – | – |
| 3 mirror | – | +7 | +6 | +5 | – | – | – | – |
| 4 bender | +7 | +6 | +5 | +4 | – | – | – | – |
| 5 filter (colour f) | – | – | same, `c & f` | – | – | – | same, `c & f` | – |
| 6 prism | as C1 (per bit R, G, B; table in C1 L3) | | | | | | | |
| 7 doppler | – | – | same, `fwd(c)` | – | – | – | same, `back(c)` | – |
| 8 splitter | – | same `t=0`; +7 `t=0` | same `t=0`; +6 `t=0` | same `t=0`; +5 `t=0` | – | same `t=0`; +3 `t=0` | same `t=0`; +2 `t=0` | same `t=0`; +1 `t=0` |
| 9 tangler | – | – | pair (as C1) | – | – | – | – | – |
| 11 conduit | same | – | – | – | same | – | – | – |
| 12 teleporter, **18 mobile teleporter** | jump | jump | jump | jump | jump | jump | jump | jump |
| 13 multi-filter | same, `c & 1` | same, `c & 2` | same, `c & 4` | same | same, `c & 1` | same, `c & 2` | same, `c & 4` | same |
| 14 twister | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` |
| **17 starburst** | see note: depends on `d & 1` only | | | | | | | |
| **19 dual-reflector** | – | +7 | +6 | +5 | – | +3 | +2 | +1 |
| **20 quad-bender** | +7 | +6 | +5 | +4 | +3 | +2 | +1 | +0 |

Notes on the new rows (all C3-identical except types 21 and 22):
- **Starburst (17, fixed)**. It ignores `p`, `r` and its colour byte. If `d` is **even** (orthogonal beam) it pushes four beams in directions
  **1, 7, 3, 5**, in that order. If `d` is **odd** (diagonal) it pushes **0, 2, 4, 6**. Every output has colour `c` and **tag 0**. So a beam always
  leaves as the four directions of the other parity: never straight through, never straight back. Every output's tag is
  cleared, so "a beam going through a STARBURST is no longer quantum entangled" (hint 0x73cc, slot 16). There is no entanglement counter bump
  and no special case for tagged input.
- **Mobile teleporter (18, movable)**. It runs the same code as the fixed teleporter (C1 L3: from the entry cell, step along `d` until the next cell whose type
  is **12 or 18**, giving up at the board edge with the x bound unsigned and the y bound signed, then push `(x', y', d, c, t)` there). The C4 change: both kinds count as
  a destination, so fixed and mobile teleporters pair with each other. Colour and tag are kept.
- **Dual-reflector (19, movable)**. Out `= (2p − d) & 7`, the mirror formula on both faces. It absorbs r = 0 and 4 (beam along the surface).
  At r = 2/6 (head-on) the output is `d + 4`, straight back ("90 degrees (or 180 degrees) on both sides", hint 0x76c4). It keeps `c` and `t`.
  For r = 1–3 it is exactly the mirror (type 3), and r = 5–7 is the mirror image.
- **Quad-bender (20, movable)**. Out `= (2p + 7 − d) & 7` for **every** r. For r = 0–3 it is exactly the bender (type 4), and r = 4–7 is the other face.
  It never absorbs. It keeps `c` and `t`.
- **Occupied square (21, fixed)**: shares the empty/pinwheel "push `d`, `c`, `t`" path. It differs from empty only in that its type ≠ 0, so drops onto it fail (L7).
- **Logic gates (16, 22, movable)**: the tracer **absorbs** every beam that enters (jump-table entries 16 and 22 → 0x378c) and records only
  `in[d] |= c`. Their output comes from the logic-gate pass below.
- Types 0–15: **unchanged from C2**. That includes the splitter (straight `t=0` plus reflection `2p − d` with `t=0` for all r ≠ 0, 4; table @ 0x3540) and the multi-filter
  (table @ 0x364c), the twister (`addi r5,r5,-2` @ 0x36a0), the doppler (with the "if `t ≠ 0` output `c` unchanged" rule), the prism, the tangler and the conduit.
  The complementor pass (`emit_complementors` @ 0x3da8, `complementor_new_input` @ 0x3d14, `shut_down_complementors` @ 0x3c5c) is also unchanged:
  the same parity test, the same complement table, white → nothing, and the same shut-down test, including the dead `snap == 7 && in == 0` exception.
  `emit_complementors` is C2's unchanged. Its return flag is ignored by `retrace_beams` in both games. The flag is set for any unshut
  complementor with non-zero matching-parity input, including white, which emits nothing.

**Logic-gate pass** (`emit_logic_gates` @ 0x3f1c, row-major: y outer, x inner):
```
emit_logic_gates():
    any = 0
    for y in 0..14: for x in 0..14:
        p = dir(x,y)
        if type(x,y) == 16:
            v = in[(p+1)&7] & in[(p+7)&7] & ~out[p]          # read live
            if v: trace_laser(x, y, p, v); any = 1
        if type(x,y) == 22:
            a = in[(p+1)&7] & in[(p+7)&7]
            if a in (4, 1, 2):
                v = a & ~out[p]
                if v: trace_laser(x, y, p, v); any = 1
    return any
```
- **Geometry**: the two inputs are the beams **travelling** in directions `p+1` and `p+7`, i.e. 45° either side of the output direction `p`.
  They come from the two "rear diagonal" neighbours and converge on the gate. Example: `p = 0` (N) takes a beam travelling NE (from the SW neighbour) and one
  travelling NW (from the SE neighbour), and emits N. For odd `p`, the inputs are orthogonal: `p = 1` takes beams travelling N and E and emits NE.
  Beams entering from any other direction are absorbed and do nothing.
- **Colour**: "sees the same color at both of its inputs" is the **bitwise AND** of the two input masks (each input is the OR of every beam that entered in that
  direction). Example, slot 6 (id 2 "Merge and Merge Again"): magenta (5) AND yellow (6) gives red (4). Only bits not yet in `out[p]` are emitted, so each
  gate emits each colour bit at most once per outer pass. `out[p]` at a gate cell is set only by the gate's own emissions, because the gate absorbs everything else.
- **Primary logic gate**: it emits only if the AND is **exactly** one primary (1, 2 or 4). With an AND of 3, 5, 6 or 7 it emits nothing, not even its
  primary components. If its AND later grows from a primary to a mixture during the same retrace, the output it already emitted stays: gates have no
  shut-down rule. No stored solution exercises this. In slots 41/42 every primary gate sees a single primary.
- **Entanglement**: each emission is a fresh `trace_laser`, so the output has tag 0 and `entangle_counter` is reset to 1. Like the complementor, a gate
  **disentangles**. Its inputs are plain colour masks, so tags never matter to it (tagged beams do reach primary gates in slots 41/42).
- Each emission marks `out[p]` at the gate cell in its first generation. The gate is drawn with the input colours on the two input sides and the output on its exit.
- Within one call, gates see the emissions of gates processed earlier in that call (and their own looped-back output), because `in[]` is read live.

**Evidence:**
- Gate pass, type 16 @ 0x3f60: `lbz r5,2(r11); addi r2,r5,1; addi r9,r5,7; clrlwi …0x1d; lbz r0,4(r2); lbz r9,4(r9); lbz r2,0xc(r2) [out[p]];
  and r0,r0,r9; andc. r6,r0,r2; beq skip; mr r3,r29; mr r4,r28; bl 0x37a0; li r3,1`.
  Type 22 @ 0x3fbc: the same loads, then `and r2,r2,r0; cmpwi r2,4; beq; cmpwi r2,1; beq; cmpwi r2,2; bne skip; andc. r6,r2,r9; beq skip; … lbz r5,2(r2); bl 0x37a0; li r3,1`.
- Starburst @ 0x36a8: `andi. r0,r5,1; bne 0x3704`; even: `li r5,1; li r7,0; bl push_beam`, then `li r5,7 …`, `li r5,3 …`, `li r5,5; b 0x3754 (li r7,0; b push_beam)`;
  odd @ 0x3704: `li r5,0`, `2`, `4`, then `li r5,6; li r7,0`.
- Dual-reflector @ 0x3234: `addi r9,r29,-1; cmplwi r9,6; bgt absorb`, then the table @ 0x325c (offsets from 0x30a4): r1 → 0x3280 `+7`, r2 → 0x3278 `+6`, r3 → 0x3198 `+5`,
  r4 → 0x378c absorb, r5 → 0x3298 `+3`, r6 → 0x3290 `+2`, r7 → 0x3288 `+1`. All of them reach 0x319c → 0x31a0 `mr r6,r26; mr r7,r25` (c, t kept).
- Quad-bender @ 0x31e0: `cmplwi r29,7`, then the table @ 0x3204: r0 → 0x3280 `+7`, r1 → 0x3278 `+6`, r2 → 0x3198 `+5`, r3 → 0x3224 `+4`, r4 → 0x3298 `+3`,
  r5 → 0x3290 `+2`, r6 → 0x3288 `+1`, r7 → 0x322c `clrlwi r5,r30,0x1d` (`+0`).
- Teleporter @ 0x35b0: the step loop then `cmpwi r0,0xc; beq 0x3610; cmpwi r0,0x12; bne 0x35b0`. 0x3610 then pushes `(x', y', d, c, t)`.
- `retrace_beams @ 0x405c`: `bl 0x3b1c; [0x4070] bl 0x3aa8; bl 0x3bec; bl 0x3b64; [0x407c] bl 0x3bec; bl 0x3f1c; mr r30,r3; bl 0x3da8; bl 0x3d14; …
  li r30,1; bl 0x3f1c; … li r30,1; cmpwi r30,0; bne 0x407c; bl 0x3c5c; cmpwi r3,0; bne 0x4070`.
- `beam_enter_cell` absorb set: jump-table entries 1, 2, 15, 16, 22 = 0x66c → 0x378c (`blr` path). Entry 21 = 0x80 → 0x31a0 (pass).

**Confidence:** confirmed for every row. Code and simulator agree: 50/50 levels win (Verification). The data pins down the gate geometry and output direction, the starburst output set and its
tag clearing, both double-sided pieces, mobile-teleporter pairing, occupied pass-through and the second gate call (sensitivity runs below).
The data does **not** pin down the "exactly one primary" condition of type 22 (treating it as a plain gate still wins 50/50). That condition is code-only.

### L4. Termination
**Answer:**
- **Per laser trace**: as C2, except for the cap. There are no visited marks and no cycle detection. A trace stops when a generation produces no beams or after
  **1023 generations** (`li r30, 0x3ff` @ 0x3810, `addic. r30,r30,-1` @ 0x3a8c). Each generation holds **at most 128 beams**
  (`cmpwi cr7, r10, 0x80; beq` @ 0x3040 in `push_beam`), and extra pushes are dropped silently. **Each gate emission and each complementor
  emission is its own `trace_laser`**, with fresh lists, its own 1023/128 limits and `entangle_counter = 1`.
- **The passes, in exact order** (L3 pseudocode): clear shut flags; then the outer loop: clear in/out, snapshot (no effect), all lasers row-major;
  then the inner loop: snapshot, **gate pass**, **complementor pass**, complementor new-input test, **gate pass again**, repeat while
  either gate pass emitted or a complementor input slot went 0 → non-zero. Then the shut-down test, and restart the outer loop if anything was shut.
- **The inner loop terminates.** `in[]` only grows within an outer iteration. Gates emit only bits not already in their `out[p]`, so each gate emits at most
  3 times per outer pass. The complementor test fires at most once per input slot. So the loop runs at most `8·Ncomp + 3·Ngates + 1` iterations.
  The outer loop runs at most `Ncomp + 1` times, as in C2. With no gates and no complementors, retrace is C1's `clear_beams + trace_all_lasers`.
- Gates have **no shut-down**. Only type 15 is tested by `shut_down_complementors`. A gate input that grows only ever adds output bits. A complementor whose input
  is changed by a gate output (or by anything else) after it emitted is shut down exactly as in C2. The snapshot that test compares against is taken at the start of the
  last inner iteration, before that iteration's first gate call.
- **Why the second gate call exists**: a complementor output can reach a gate's input in the same iteration. If no complementor input is new, the
  loop would end before the gate saw it. Slot 15 (id 29 "Color Challenge I") depends on this: complementor (8,6) feeds gate (8,10) in iteration 2.
- Stored solutions: the 128 cap is never reached (the largest generation is 19 beams, so C2's 32 would also do). One trace runs into the 1023-generation limit:
  slot 30 (id 37 "Choice: It's What's For Dinner") has a steady starburst loop. Winning doesn't depend on it (a 200-generation limit still wins).
  There are 0 complementor shut-downs. Drawing is unchanged from C2.
**Evidence:** addresses above. `trace_laser` loop end @ 0x3a78–0x3a90: `lwz r0,0(next_count); cmpwi r0,0; beq exit; addic. r30,r30,-1; bne 0x3814`.
**Confidence:** confirmed.

### L5. Colour accumulation and the win condition
**Answer:** Unchanged from C2. `pinwheel_satisfied` @ 0x40d4 tests `colour == OR(in[0..7])`. `check_win` @ 0x4644 runs `retrace_beams()` (now with the
gate pass), sets `level_won = 1` and clears it if any pinwheel in rows 0–14 is unsatisfied (with the same useless 8× inner loop). It then applies `force_win`, and if
`game_mode != 0 && level_won && !completed[current_slot]` it sets `completed`, then calls `save_level_state` (0x5838), `write_save_file` (0x44ec) and `update_unlocked` (0x4258).
Black pinwheels need no light. Logic gates, starbursts, occupied squares and the double-sided pieces are not tested themselves. Only the beams they produce count.
A beam absorbed by a gate lights nothing beyond it.
**Evidence:** `check_win @ 0x4644`: `bl 0x405c` @ 0x465c, `cmpwi r0, 0xa` @ 0x4690, `bl 0x40d4` @ 0x469c, `bl 0x5838` @ 0x4754, `bl 0x44ec` @ 0x4758. Decompiled:
`if (*force_win != 0) { *level_won = 1; *force_win = 0; }` and
`if (game_mode != 0 && level_won != 0 && completed[DAT_0000803c] == 0) { completed[..] = 1; FUN_00005838(); FUN_000044ec(); FUN_00004258(); }`.
**Confidence:** confirmed.

### L6. Progress and unlocking
**Answer:** The storage model is as in C2: `completed[64]` @ 0x177f0 and `unlocked[64]` @ 0x176e0, both per **slot**. There are 50 slots
(strip 25 × 2; `next_level` @ 0x5a44 caps the slot with `if (current_slot < 0x31)`). `update_unlocked` runs at the end of `load_save_file` (0x42e8) and on
each first completion (`check_win`). It only ever sets entries. **The rule changed**: the count and the thresholds are gone.
```
update_unlocked():                                     # 0x4258
    request_redraw(0, 0, 640, 480)
    n = 25
    for s in 0..63:
        if completed[s]: unlocked[s] = 1
        elif n > 0:      unlocked[s] = 1; n -= 1
```

| completed count k | any (0–50) |
|---|---|
| uncompleted slots open (n) | 25 (the lowest-numbered ones) |

A fresh game opens slots 1–25 (0–24). Each first completion opens the next uncompleted slot in strip order, if any. With k ≥ 25 every
slot is open. The leftover `n` then spills into the non-existent slots 50–63 (also true in C2 for large k). Its only visible effect is that Space/`=`/`+` on
slot 50 passes the `unlocked[current_slot+1]` test, and `next_level` then reloads slot 50 from its saved state, because the slot cap stops the advance.
Per-level board state is as in C2: `save_level_state` @ 0x5838 (0x1f2c bytes), `start_level(restore)` @ 0x58ac and `goto_slot` @ 0x5958.
Restart (`r`) = `start_level(0)`.
**Evidence:** `update_unlocked @ 0x4258`: `bl 0x2f68 (0,0,0x280,0x1e0); li r0,0x40; mtctr; li r9,0x19; li r11,1; … lwzx r0,r2,r8 (completed); cmpwi r0,0; beq →
cmpwi r9,0; ble skip; stwx r11 (unlocked); addi r9,r9,-1`. No counting loop and no thresholds.
**Confidence:** confirmed.

### L7. Interaction rules
**Answer:** Identical to C2 (C2 L7) apart from the cheat key:
- `on_mouse_event` @ 0x2d9c: event codes 1 (down), 3 (alt down), 4 (up), 6 (alt up), 0 (dragged or moved). The alt latch is `& 0x1a00` at mouse-down. The registered kinds
  are mouse down 1, up 2 and dragged 6 (EventTypeSpecs @ 0x7fbc, count 3), so moves arrive only with a button held.
- The hit test in `handle_mouse` @ 0x5318 uses the same constants: board `X/24 − 2`, `(Y+30)/24 − 2`. Toolbox `(X−422)/26 − 1` in 0..5 and `(Y+18)/26 − 1` in 0..3
  give slot `col + 6·row` in row 15. Strip `(X+10)/20 − 1` in 0..24 and `(Y−390)/20 − 1` in 0..1 give slot `col + 25·row`.
- The state machine (`drag_state` @ 0x155ec: 0 idle, 1 pressed, 2 dragging) is the same. The drag starts after more than 1 px of movement (`begin_drag` @ 0x5180 requires
  `is_movable`). **A drop succeeds only onto a cell whose type is 0.** It copies all 28 bytes and sets the source type to 0. Any other drop leaves the piece.
  So **occupied squares (21) block placement** simply because their type ≠ 0, with no special code. Starbursts, twisters, walls, lasers and so on block in the same way.
  The **mobile teleporter** is movable (`is_movable(18)`), so it can be dragged, dropped on any empty cell or toolbox slot, and "rotated" (its dir is ignored).
  After a drop or rotate, `check_win_and_redraw` (0x5148) runs. Up without a drag on a movable piece rotates `dir − 1`. Up-6 with `drag_state == 0` rotates `dir + 1`.
  A strip click opens the slot via `goto_slot` if `unlocked[slot]`. There is no delete.
- Initial toolbox order (data-side, as in C2, see data.md D3): `move_pieces_to_toolbox` @ 0x573c `qsort`s the filled toolbox cells by type byte (comparator @ 0x572c,
  `return a->type − b->type`). The order of pieces of equal type (e.g. filters of different colours) is whatever the system qsort leaves.
- Keys (`handle_key` @ 0x60a8, reached from `on_key_event` @ 0x2908 with the `kEventParamKeyMacCharCodes` byte, ignored when Cmd (`& 0x100`) is held):
  Space, `=` or `+` → `next_level` if `unlocked[current_slot+1]`; `-` → `prev_level`; `r` → `start_level(0)`; Esc → swallowed (returns 1; every other key returns 0);
  **char code 0x0F → `force_win = 1; check_win()`**. 0x0F is the Mac char code of **Control-O** (C2 used `z` = 0x7a, and C3 uses `^` = 0x5e).
  Port note: on modern macOS, Ctrl+O arrives as `NSEvent.characters == "\x0F"`, i.e. winit `text_with_all_modifiers()`. `KeyEvent.text` gives "o".
  The menu (`on_menu_command` @ 0x274c) is the same as C2: `Next` → `=`, `Prev` → `-`, `rvrt` → `r`, `copy`/`past` (clipboard, data/platform notes),
  and `Whee` toggles the wheel preference. The wheel (`on_wheel_event` @ 0x2af4, only when the preference is on) maps delta > 0 to `-` and anything else to `=`.
**Evidence:** `handle_key @ 0x60a8` disassembly: `cmpwi r3,0x2b` @ 0x60a8, `0x1b` @ 0x60cc (`li r0,1`), **`0xf` @ 0x60dc** → 0x6120 `li r0,1; stw force_win; bl 0x4644`,
`0x20` @ 0x60e8, `0x3d` @ 0x60f4, `0x2d` @ 0x6100, `0x72` @ 0x610c. Drop copy in `handle_mouse`: 7 words copied only when `PTR_DAT_000155a4[iVar11] == '\0'`, then `puVar9[iVar11] = 0`.
**Confidence:** confirmed (code). That 0x0F is typed as Control-O is likely (standard Carbon char-code mapping, `'O' & 0x1f`). Pressing it in the original would settle it.

### L8. What happens on a win
**Answer:** Identical to C2. Nothing automatic happens: no advance, timer, animation or sound. `paint_window` (0x692c) draws:
- **"You win!"** in rect left 330, top 385, right 400, bottom 420. If the level is un-solved later, **"(won)"** is drawn at left 350 instead, because `completed` stays set.
- The level name at (50, 385, 300, 420) while `game_mode == 2`; otherwise at (350, 420, 450, 460).
- The hint box (450, 125, 620, 475) shows **"Click on a level or press spacebar for next."** when `game_mode == 2 && level_won`.
- **"Thank you for purchasing this game."** at (0, 450, 400, 480) when `current_slot % 6 == 4`. Then the site URL at (470, 450, 640, 480).
- Pinwheels show their lit sprites. The first win sets `completed`, saves the board and the save file, and recomputes unlocks (L6).
**Evidence:** `paint_window @ 0x692c` (decompiled, condensed):
```c
  if (level_won == 0) { if (completed[current_slot] == 0) goto skip; l = 0x15e; s = "(won)"; }
  else { l = 0x14a; s = "You win!"; }
  draw_text_box(s, l, 0x181, 400, 0x1a4);
skip:
  if (game_mode == 2) draw_text_box(level_name_text, 0x32, 0x181, 300, 0x1a4);
  else                draw_text_box(level_name_text, 0x15e, 0x1a4, 0x1c2, 0x1cc);
  s = hint_text; if (game_mode == 2 && level_won) s = "Click on a level or press spacebar for next.";
  draw_text_box(s, 0x1c2, 0x7d, 0x26c, 0x1db);
  if (current_slot % 6 == 4) draw_text_box("Thank you for purchasing this game.", 0, 0x1c2, 400, 0x1e0);
  draw_text_box(site_url_text, 0x1d6, 0x1c2, 0x280, 0x1e0);
```
(`draw_text_box(str, left, top, right, bottom)` @ 0x2cd8.)
**Confidence:** confirmed.

## Verification
```bash
BIN="chromatron_mac/Chromatron 4.app/Contents/MacOS/chromatron4"
D=re/out/chromatron4/decompiled.c

# Names applied (after the controller re-runs the decompile with logic.names.tsv)
grep -c -E '^// (is_movable|push_beam|beam_enter_cell|trace_laser|clear_beams|clear_complementor_flags|trace_all_lasers|snapshot_cell_inputs|shut_down_complementors|complementor_new_input|emit_complementors|emit_logic_gates|retrace_beams|pinwheel_satisfied|update_unlocked|check_win|check_win_and_redraw|begin_drag|handle_mouse|save_level_state|goto_slot|slot_next_id|slot_prev_id|next_level|prev_level|handle_key|on_menu_command|on_key_event|on_wheel_event|on_mouse_event) @ ' $D
# 30

# beam_enter_cell: type bound 0x16, then 23-entry jump table @ 0x3120 (target = 0x3120 + entry):
# 0/10/21 -> 0x31a0 pass; 1/2/15/16/22 -> 0x378c absorb; 12/18 -> 0x35b0 teleporter; 17 -> 0x36a8; 19 -> 0x3234; 20 -> 0x31e0
r2 -e scr.color=0 -qc 'pi 1 @ 0x30e4' "$BIN" </dev/null 2>/dev/null
# cmplwi cr7, r11, 0x16
r2 -e scr.color=0 -qc 'pxw 92 @ 0x3120' "$BIN" </dev/null 2>/dev/null
# 0x00003120  0x00000080 0x0000066c 0x0000066c 0x0000005c  .......l...l...\
# 0x00003130  0x00000094 0x00000334 0x00000180 0x000002b8  .......4........
# 0x00003140  0x000003d8 0x0000036c 0x00000080 0x0000047c  .......l.......|
# 0x00003150  0x00000490 0x00000508 0x00000580 0x0000066c  ...............l
# 0x00003160  0x0000066c 0x00000588 0x00000490 0x00000114  ...l............
# 0x00003170  0x000000c0 0x00000080 0x0000066c             ...........l

# is_movable: table for types 3..22 (0x50 = movable, 0x58 = not): 3-9, 13, 15, 16, 18, 19, 20, 22
r2 -e scr.color=0 -qc 'pxw 80 @ 0x2fc0' "$BIN" </dev/null 2>/dev/null
# 0x00002fc0  0x00000050 0x00000050 0x00000050 0x00000050  ...P...P...P...P
# 0x00002fd0  0x00000050 0x00000050 0x00000050 0x00000058  ...P...P...P...X
# 0x00002fe0  0x00000058 0x00000058 0x00000050 0x00000058  ...X...X...P...X
# 0x00002ff0  0x00000050 0x00000050 0x00000058 0x00000050  ...P...P...X...P
# 0x00003000  0x00000050 0x00000050 0x00000058 0x00000050  ...P...P...X...P

# Beam cap 128; trace_laser tag 0, 1023 generations
r2 -e scr.color=0 -qc 'pi 2 @ 0x3040; pi 1 @ 0x37c4; pi 1 @ 0x3810; pi 1 @ 0x3a8c' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r10, 0x80
# beq cr7, 0x3094
# li r7, 0
# li r30, 0x3ff
# addic. r30, r30, -1

# Piece 1, STARBURST: parity of d selects output set; tag 0 on every output
r2 -e scr.color=0 -qc 'pi 6 @ 0x36a8' "$BIN" </dev/null 2>/dev/null
# andi. r0, r5, 1
# bne 0x3704
# mr r4, r27
# li r5, 1
# li r7, 0
# bl 0x3020
r2 -e scr.color=0 -qc 'pi 1 @ 0x36c8; pi 1 @ 0x36e0; pi 1 @ 0x36fc; pi 1 @ 0x3708; pi 1 @ 0x371c; pi 1 @ 0x3734; pi 1 @ 0x3750; pi 1 @ 0x3754' "$BIN" </dev/null 2>/dev/null
# li r5, 7
# li r5, 3
# li r5, 5
# li r5, 0
# li r5, 2
# li r5, 4
# li r5, 6
# li r7, 0

# Piece 2, QUAD-BENDER table @ 0x3204 (r=0..7) and DUAL-REFLECTOR table @ 0x325c (r=1..7); targets = 0x30a4 + entry
# quad: 0x3280 +7, 0x3278 +6, 0x3198 +5, 0x3224 +4, 0x3298 +3, 0x3290 +2, 0x3288 +1, 0x322c +0
# dual: 0x3280 +7, 0x3278 +6, 0x3198 +5, 0x378c absorb, 0x3298 +3, 0x3290 +2, 0x3288 +1
r2 -e scr.color=0 -qc 'pxw 32 @ 0x3204; pxw 28 @ 0x325c' "$BIN" </dev/null 2>/dev/null
# 0x00003204  0x0000007c 0x00000074 0xffffff94 0x00000020  ...|...t....... 
# 0x00003214  0x00000094 0x0000008c 0x00000084 0x00000028  ...............(
# 0x0000325c  0x00000024 0x0000001c 0xffffff3c 0x00000530  ...$.......<...0
# 0x0000326c  0x0000003c 0x00000034 0x0000002c             ...<...4...,

# Piece 3, MOBILE TELEPORTER: the teleporter search lands on type 12 or 18
r2 -e scr.color=0 -qc 'pi 4 @ 0x3600' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r0, 0xc
# beq cr7, 0x3610
# cmpwi cr7, r0, 0x12
# bne cr7, 0x35b0

# Piece 4, LOGIC GATE (16): in[p+1] & in[p+7] & ~out[p] -> trace_laser(x, y, p, v)
r2 -e scr.color=0 -qc 'pi 12 @ 0x3f60; pi 1 @ 0x3f90' "$BIN" </dev/null 2>/dev/null
# lbz r5, 2(r11)
# addi r2, r5, 1
# addi r9, r5, 7
# clrlwi r2, r2, 0x1d
# clrlwi r9, r9, 0x1d
# add r2, r11, r2
# add r9, r11, r9
# lbz r0, 4(r2)
# add r2, r11, r5
# lbz r9, 4(r9)
# lbz r2, 0xc(r2)
# and r0, r0, r9
# andc. r6, r0, r2

# Piece 5, PRIMARY LOGIC GATE (22): AND must be exactly 4, 1 or 2
r2 -e scr.color=0 -qc 'pi 9 @ 0x3fe8' "$BIN" </dev/null 2>/dev/null
# and r2, r2, r0
# cmpwi cr7, r2, 4
# beq cr7, 0x4004
# cmpwi cr7, r2, 1
# beq cr7, 0x4004
# cmpwi cr7, r2, 2
# bne cr7, 0x4028
# andc. r6, r2, r9
# beq 0x4028

# retrace_beams: flags; (clear, snapshot, lasers; (snapshot, gates, complementors, new-input, gates) until no change) until no shut-down
r2 -e scr.color=0 -qc 'pi 22 @ 0x406c' "$BIN" </dev/null 2>/dev/null
# bl 0x3b1c
# bl 0x3aa8
# bl 0x3bec
# bl 0x3b64
# bl 0x3bec
# bl 0x3f1c
# mr r30, r3
# bl 0x3da8
# bl 0x3d14
# cmpwi cr7, r3, 0
# beq cr7, 0x409c
# li r30, 1
# bl 0x3f1c
# cmpwi cr7, r3, 0
# beq cr7, 0x40ac
# li r30, 1
# cmpwi cr7, r30, 0
# bne cr7, 0x407c
# bl 0x3c5c
# cmpwi cr7, r3, 0
# bne cr7, 0x4070
# lwz r0, 0x58(r1)

# Splitter and multi-filter tables unchanged from C2 (same offsets relative to their own bases)
r2 -e scr.color=0 -qc 'pxw 28 @ 0x3540; pxw 32 @ 0x364c' "$BIN" </dev/null 2>/dev/null
# 0x00003540  0x0000001c 0x0000003c 0x00000024 0x0000024c  .......<...$...L
# 0x00003550  0x0000002c 0x00000044 0x00000034             ...,...D...4
# 0x0000364c  0x00000048 0x00000034 0x00000020 0xfffffb54  ...H...4... ...T
# 0x0000365c  0x00000048 0x00000034 0x00000020 0xfffffb54  ...H...4... ...T

# Complementor complement table unchanged (colour-1 -> li r6, 6/5/4/3/2/1)
r2 -e scr.color=0 -qc 'pxw 24 @ 0x3e50' "$BIN" </dev/null 2>/dev/null
# 0x00003e50  0x0000002c 0x00000040 0x00000054 0x00000018  ...,...@...T....
# 0x00003e60  0x0000007c 0x00000068                        ...|...h

# Unlock: constant 25, no count
r2 -e scr.color=0 -qc 'pi 1 @ 0x428c' "$BIN" </dev/null 2>/dev/null
# li r9, 0x19

# Cheat key is char code 0x0F (Control-O)
r2 -e scr.color=0 -qc 'pi 1 @ 0x60dc; pi 1 @ 0x6130' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r3, 0xf
# bl 0x4644

# dir tables and doppler maps unchanged
xxd -s 0x7044 -l 0x68 "$BIN"
# 00007044: 0000 0000 0000 0001 0000 0001 0000 0001  ................
# 00007054: 0000 0000 ffff ffff ffff ffff ffff ffff  ................
# 00007064: ffff ffff ffff ffff 0000 0000 0000 0001  ................
# 00007074: 0000 0001 0000 0001 0000 0000 ffff ffff  ................
# 00007084: 0000 0000 0000 0002 0000 0004 0000 0000  ................
# 00007094: 0000 0001 0000 0000 0000 0004 0000 0001  ................
# 000070a4: 0000 0000 0000 0002                      ........

# Tables used by the simulator (nl_symbol_ptr slots): board, level table, name table, level_order
r2 -e scr.color=0 -qc 'pxw 4 @ 0x155a4; pxw 4 @ 0x155b8; pxw 4 @ 0x155b0; pxw 4 @ 0x15558' "$BIN" </dev/null 2>/dev/null
# 0x000155a4  0x00017900                                   ..y.
# 0x000155b8  0x0000bb6c                                   ...l
# 0x000155b0  0x0000bc48                                   ...H
# 0x00015558  0x00008110                                   ....
```

**Simulator check** (throwaway Python, `/private/tmp/claude-503/c4sim/`, not committed). It decodes the level table @ 0xbb6c itself:
55 pointers, id 0 NULL, 5-byte records `type, dir, colour, x, y`, terminated by type 0, as in C1/C2. It uses `level_order` @ 0x8110 (50 slots) and leaves every
movable piece at its stored x/y/dir. It runs `retrace_beams` exactly as written in L3 (the C2 tracer plus types 16–22, the 128 cap, 1023 generations,
the entanglement pass, the gate pass called twice per inner iteration, and the complementor snapshot/new-input/shut-down loops), then the L5 test:
- **50 / 50 playable levels are won** from their stored placement.
- **0 / 50** are won with all movable pieces removed.
- The 4 unreachable ids are also won from their stored placement: 5 ("level005", 8 records), 11 ("level011", 51), 35 ("level035", 104), 46 ("level046", 40).
- The 128 cap is never hit (the largest generation is 19 beams). One trace reaches the 1023-generation limit (slot 30, a starburst loop). There are 0 shut-downs,
  21 complementor emissions, 8 logic-gate emissions (slots 4, 5, 6, 15, 50) and 6 primary-gate emissions (slots 41, 42).
- Beam entries per new type in the stored solutions: logic gate 28, starburst 345, mobile teleporter 39, dual-reflector 12, quad-bender 2,
  occupied square 83, primary gate 12. Tagged beams reach starbursts (slot 16), dual-reflectors and primary gates.
- Sensitivity (each variant against the same 50 levels):
  - no gate pass → 43 won (fails 4, 5, 6, 15, 41, 42, 50);
  - gate inputs at `p±2` or `p±3` instead of `p±1` → 43;
  - gate output `p + 4` instead of `p` → 43;
  - gate = OR instead of AND → 48 (fails 6, 15);
  - no second gate call → 49 (fails 15);
  - primary gate never emits → 48 (fails 41, 42);
  - starburst keeps the tag → 49 (fails 16);
  - starburst emits the same parity (`d, d+2, d+4, d+6`) → 38;
  - quad-bender as single-sided bender → 49 (fails 2), and with out `2p + 1 − d` → 49 (fails 2);
  - dual-reflector as single-sided mirror → 47 (fails 35, 36, 45);
  - mobile teleporter inert → 43 (fails 19, 20, 21, 23, 24, 33, 49);
  - occupied square absorbing → 47 (fails 26, 32, 37);
  - beam cap 16 → 49 (fails 30);
  - no complementor emission → 45.

  Not pinned by the data (code only): the primary gate's "exactly one primary" test (a plain gate still wins 50/50); the 128 cap (32 still wins);
  the first gate call (gates only after the complementors still wins); same-type-only teleporter pairing (still wins); and the shut-down pass (no shut-down occurs).
