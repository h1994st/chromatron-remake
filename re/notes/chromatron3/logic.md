# Chromatron 3 v1.1 — Game logic (diff against Chromatron 2)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 3.app/Contents/MacOS/chromatron3` (Mach-O, big-endian PowerPC).
All addresses are vmaddrs. File offset = vmaddr − 0x1000. Function and data names are the ones in `logic.names.tsv` (this note), plus
the data agent's `data.names.tsv`. Where a C3 function does the same job as a C2 one, it keeps the C2 name.

In prose, 'slot N' is the 1-based strip position, i.e. `current_slot = N − 1`. Code expressions use the 0-based `current_slot`.

Baseline: `re/notes/chromatron2/logic.md` (C2 L1–L8), which itself is a diff against `re/notes/chromatron1/logic.md` and `data.md`.
The notation is the same: `p` = the piece's `dir` byte, `d` = beam travel direction, `c` = colour mask (1 B, 2 G, 4 R), `t` = entanglement tag,
**`r = (d − p) & 7`**, `+k` = output direction `(p + k) & 7`, and "same" = keeps going in direction `d`.

**Summary of differences from C2**
1. The cell is still **28 bytes (0x1c)**, the row 0x1a4 bytes and the board 19 rows = 0x1f2c bytes, with the same fields as C2
   (`+3` complementor shut flag, `+0x14..+0x1b` input snapshot).
2. There are **five new piece types, 16–20**. Two are used by the shipped levels: **19 DUAL-REFLECTOR** (a mirror that reflects on both faces)
   and **20 QUAD-BENDER** (a bender that bends on both faces). The other three are fully implemented but appear only in unreachable level
   blobs: **16 gate** (an AND-combiner, emits through a new fixed-point pass), **17 starburst** (fixed, turns one beam into four),
   and **18 mobile teleporter** (a movable teleporter). The names of 16–18 are descriptive; no C3 string names them (L1).
3. `is_movable` is now a switch: types **3–9, 13, 15, 16, 18, 19, 20**.
4. `retrace_beams` has a new **gate pass** (`emit_gates` @ 0x4198) inside the complementor fixed-point loop.
5. The teleporter jump now stops at a cell of type 12 **or 18**.
6. **Unlock rule**: every completed slot plus the **25** lowest-numbered uncompleted slots. No thresholds, no count.
7. The force-win cheat key is **`^`** (0x5e), not `z`.

Unchanged from C2, instruction for instruction (only the PC-relative global offsets differ): `push_beam`, `trace_laser` (push order,
32-beam cap, 1023 generations, entanglement pass), `clear_beams`, `clear_complementor_flags`, `trace_all_lasers`, `snapshot_cell_inputs`,
`shut_down_complementors`, `complementor_new_input`, `emit_complementors`, `pinwheel_satisfied`, `check_win`, `handle_mouse`,
`save_level_state`, `start_level`, `goto_slot`, `next_level`, `prev_level`, `move_pieces_to_toolbox`, `on_wheel_event`, `on_mouse_event`.
In `beam_enter_cell` every C2 case (types 0–15) has the same branch structure and constants as C2, including the C2 splitter.
(Checked by diffing `pi` output of each C2/C3 pair with addresses masked: the only differences are the signs of `lwz rX, ±N(rY)` offsets.)

Globals reached through `__nl_symbol_ptr` (`r2 -qc 'pxw 4 @ <slot>'`), logic area only:

| slot | → target | name | meaning (as C2 unless noted) |
|---|---|---|---|
| 0x16528 | 0x1a504 | `cursor_row` | int |
| 0x1652c | 0x1a500 | `cursor_col` | int |
| 0x16548 | 0x17484 | `game_mode` | int, set to 2 by every `start_level` |
| 0x16540 | 0x181a0 | `force_win` | int, set by the **`^`** key |
| 0x16544 | 0x182b0 | `level_won` | int |
| 0x16554 | 0x1a300 | `saved_states` | `u8*[]` per level id, 0x1f2c-byte board copies |
| 0x16558 | 0x180a0 | `unlocked` | `int[64]` per slot |
| 0x1655c | 0x181b0 | `completed` | `int[64]` per slot |
| 0x16560 | 0x17270 | `beam_cur_count` | int |
| 0x16564 | 0x17380 | `beam_buf_b` | 32 × 8-byte beams |
| 0x16568 | 0x17280 | `beam_buf_a` | 32 × 8-byte beams |
| 0x1656c | 0x1726c | `beam_cur_list` | pointer |
| 0x16574 | 0x1a2f0 | `beam_next_list` | pointer |
| 0x16578 | 0x1a2f4 | `beam_next_count` | int |

Directly addressed: `entangle_counter` @ 0x165d8, `drag_state` @ 0x165b8, `mouse_alt_button` @ 0x165b0,
`press_x`/`press_y` @ 0x171e8/0x171ec, `drag_image` @ 0x80d0, `current_slot` @ 0x803c (data agent's), `dir_dx`/`dir_dy` @ 0x8044/0x8064
(unchanged values), `doppler_back_map` @ 0x8084 = `[0,2,4,0,1]`, `doppler_fwd_map` @ 0x8098 = `[0,4,1,0,2]` (unchanged).
(Board and tables belong to data.md; as used by the simulator: `board` 0x182c0 via 0x16570, level table 0xd590 via 0x16584 (79 entries,
id 0 NULL), level-name table 0xd6cc via 0x1657c, hint table 0xd808 via 0x16580, `level_order` 0x8110 via 0x1654c, current level id 0x18090 via 0x16550.)

## Findings

### L1. Piece kinds, movable and rotatable
**Answer:** The piece enum is the cell `type` byte. Types 0–15 are C2's types, unchanged. Types 16–20 are new. Anything above 20 hits the
`default:` assertion in `beam_enter_cell` (game.c line 0x1a6, same as C1/C2).

| type | piece | colour byte | movable = rotatable | `dir` bits that matter | image (`cell_image_index` @ 0x4a54) | beam behaviour (L3) |
|---|---|---|---|---|---|---|
| 0 | empty | – | – | – | 0 | passes |
| 1 | wall | – | no | – | 1 | absorbs |
| 2 | laser, 1/2/4/7 | yes | no | all 8 | as C2 | emits; absorbs incoming |
| 3 | mirror | – | yes | all 8 | 32+d | as C1 |
| 4 | bender | – | yes | all 8 | 48+d | as C1 |
| 5 | filter, 1/2/4 | yes | yes | `& 3` | as C2 | as C1 |
| 6 | prism | – | yes | all 8 | 24+d | as C1 |
| 7 | doppler | – | yes | all 8 | 64+d | as C1 |
| 8 | splitter | – | yes | `& 3` | 40+(d&3) | as **C2** (pass + reflect at every non-parallel angle, tag cleared) |
| 9 | quantum tangler | – | yes | all 8 | 80+d | as C1 |
| 10 | pinwheel, mask 0–7 | yes | no | – | as C2 (+1 lit) | passes, records colour |
| 11 | conduit | – | no | `& 3` | 44+(d&3) | as C1 |
| 12 | teleporter | – | no | – | 14 | jumps to the next type 12 **or 18** |
| 13 | multi-filter | ignored | yes | `& 3` | 100+(d&3) | as C2 |
| 14 | twister | ignored | no | none | 15 | as C2 |
| 15 | complementor | ignored | yes | `& 1` | 116+(d&1) | as C2 (absorbs; complementor pass) |
| **16** | **gate** (AND-combiner) | ignored | **yes** | all 8 | **128+d** | absorbs; gate pass emits `in[p+1] & in[p−1]` in direction `p` |
| **17** | **starburst** | ignored | **no** | none (always 0 in data) | **144** | every beam → 4 beams at ±45°/±135°, tag 0 |
| **18** | **mobile teleporter** | ignored | **yes** | none | **14** (teleporter sprite) | as teleporter; is a jump target |
| **19** | **DUAL-REFLECTOR** | ignored | **yes** | `& 3` | **136+(d&3)** | double-sided mirror |
| **20** | **QUAD-BENDER** | ignored | **yes** | `& 3` | **140+(d&3)** | double-sided bender, never absorbs |

`is_movable(type)` @ 0x3214 is true exactly for **3–9, 13, 15, 16, 18, 19, 20**. As in C1/C2 every drag, drop, rotate, toolbox move and
clipboard copy/paste goes through it, and `move_pieces_to_toolbox` @ 0x59ec (instruction-identical to C2's 0x5810 apart from PC-relative global offsets) moves **every** movable
record to the toolbox at level start and sets its `dir` to 0, so there are no fixed movable pieces. A mobile teleporter can be rotated
(it passes `is_movable`), but its `dir` affects neither logic nor sprite.

**Which C2 pieces still exist:** all of them, in code: multi-filter (13), twister (14) and complementor (15) keep their C2 handlers,
images and movability. **No playable C3 level uses them.** The 50 playable levels use only types 1, 2, 3, 4, 6, 8, 10, 11, 19 and 20
(records: wall 1773, laser 104, mirror 323, bender 217, prism 3, splitter 21, pinwheel 630, conduit 51, dual-reflector 6, quad-bender 5;
no filter, doppler, tangler, teleporter, multi-filter, twister or complementor). The 28 unreachable blobs contain twister ×2 (id 17),
complementor ×1 (id 7), gate ×11 (ids 1, 2, 3, 10, 14, 26), starburst ×14 (ids 4–9, 26), mobile teleporter ×20 (ids 16, 17, 18, 20, 21),
quad-bender ×1 (id 53), doppler, tangler. The most movable pieces in a playable level is 24 (slot 48 = id 38 and slot 50 = id 57), which fills the toolbox.

Names: DUAL-REFLECTOR and QUAD-BENDER are the game's own (hints at 0x7f6c, slot 17, and 0x7f10, slot 22). No C3 string names types 16–18.
"gate", "starburst" and "mobile teleporter" describe the code. Check the C4 notes for official names.
**Evidence:** `is_movable @ 0x3214`: `addi r3,r3,-3; cmplwi cr7,r3,0x11; bgt → return 0`, then an 18-entry table at 0x324c (base 0x324c):
0x48 → 0x3294 `li r3,1`, 0x50 → 0x329c `li r3,0`; entries for types 3..20 = `1 1 1 1 1 1 1 0 0 0 1 0 1 1 0 1 1 1`.
`beam_enter_cell @ 0x3320`: `cmplwi cr7,r11,0x14; bgt 0x39e8` (assert) at 0x3368/0x3384, then a 21-entry jump table at 0x33a4 (base 0x33a4):
0/10 → 0x341c pass, 1/2/15/16 → 0x3a08 return, 3 → 0x33f8, 4 → 0x3430, 5 → 0x36d0, 6 → 0x351c, 7 → 0x3654, 8 → 0x3774, 9 → 0x3708,
11 → 0x3818, **12 and 18 → 0x382c**, 13 → 0x38a4, 14 → 0x391c, **17 → 0x3924, 19 → 0x34b0, 20 → 0x345c**.
`cell_image_index @ 0x4a54`: `case 0xc: case 0x12: 0xe;` `case 0x10: dir + 0x80;` `case 0x11: 0x90;` `case 0x13: (dir & 3) + 0x88;` `case 0x14: (dir & 3) + 0x8c;`.
**Confidence:** confirmed (code). Names of 16–18: unknown; a C3 hint or menu string would settle them, and none exists.

### L2. Direction encoding and rotation
**Answer:** Same as C2 (`handle_mouse` @ 0x55c8 is identical to C2's). The 8 directions 0–7 are N, NE, …, NW; a plain left click rotates
`dir − 1`, a right click or Shift/Option/Control-click rotates `dir + 1`, on the board and in the toolbox. New pieces: dual-reflector and
quad-bender depend only on `dir & 3` (their rules depend only on `2p mod 8`), so their 8 rotations come in 4 identical pairs. The gate
uses all 8. The mobile teleporter rotates but `dir` is ignored. The starburst can't be rotated.
**Evidence:** `handle_mouse @ 0x55c8`: event 4 `dir + -1; & 7`, event 6 `dir + 1; & 7`, both guarded by `is_movable`.
**Confidence:** confirmed.

### L3. Beam tracer and transfer table
**Answer:**

**Data structures**: same as C2 (8-byte beams, two 32-entry lists, `in[d]` at cell `+4+d`, `out[d]` at `+12+d`, stride 0x1c/0x1a4).
**Per-laser trace** `trace_laser(x, y, d, c)` @ 0x3a1c: identical to C2/C1 (tag 0 on the first push, `entangle_counter = 1`, 1023
generations, entanglement pass).

**Top level** (`retrace_beams` @ 0x4258, **changed**):
```
retrace_beams():
    clear_complementor_flags()                         # 0x3d98: cell +3 = 0 for rows 0..14
    repeat:
        clear_beams()                                  # 0x3d24
        snapshot_cell_inputs()                         # 0x3e68  (new call; in[] is all 0 here, so no effect)
        trace_all_lasers()                             # 0x3de0  (row-major)
        repeat:
            snapshot_cell_inputs()                     # 0x3e68  (snap = in, every board cell)
            changed = emit_gates()                     # 0x4198  NEW
            emit_complementors()                       # 0x4024  (return value ignored)
            if complementor_new_input(): changed = 1   # 0x3f90
            if emit_gates(): changed = 1               # 0x4198  (second call)
        while changed
    until not shut_down_complementors()                # 0x3ed8
```
With no gate on the board, `emit_gates` returns 0 and the inner loop is exactly C2's "snapshot, emit, until no new input".

**Transfer table** (`beam_enter_cell` @ 0x3320). As before, `in[d] |= c` is recorded first for every type, and outputs are pushed in the listed
order. An output keeps colour `c` and tag `t` unless the table says otherwise. "–" means absorbed.

| type | r=0 | r=1 | r=2 | r=3 | r=4 | r=5 | r=6 | r=7 |
|---|---|---|---|---|---|---|---|---|
| 0 empty, 10 pinwheel | same | same | same | same | same | same | same | same |
| 1 wall, 2 laser, 15 complementor, **16 gate** | – | – | – | – | – | – | – | – |
| 3 mirror | – | +7 | +6 | +5 | – | – | – | – |
| 4 bender | +7 | +6 | +5 | +4 | – | – | – | – |
| 5 filter (colour f) | – | – | same, `c & f` | – | – | – | same, `c & f` | – |
| 6 prism | as C1 (per bit R, G, B) | | | | | | | |
| 7 doppler | – | – | same, `fwd(c)` | – | – | – | same, `back(c)` | – |
| 8 splitter (as C2) | – | same `t=0`; +7 `t=0` | same `t=0`; +6 `t=0` | same `t=0`; +5 `t=0` | – | same `t=0`; +3 `t=0` | same `t=0`; +2 `t=0` | same `t=0`; +1 `t=0` |
| 9 tangler | – | – | pair (as C1) | – | – | – | – | – |
| 11 conduit | same | – | – | – | same | – | – | – |
| 12 teleporter, **18 mobile teleporter** | jump | jump | jump | jump | jump | jump | jump | jump |
| 13 multi-filter | same, `c & 1` | same, `c & 2` | same, `c & 4` | same | same, `c & 1` | same, `c & 2` | same, `c & 4` | same |
| 14 twister | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` |
| **17 starburst** | see below (depends on `d`, not `r`) | | | | | | | |
| **19 dual-reflector** | – | +7 | +6 | +5 | – | **+3** | **+2** | **+1** |
| **20 quad-bender** | +7 | +6 | +5 | +4 | **+3** | **+2** | **+1** | **+0** |

Notes on the new rows:
- **Dual-reflector (19)**. `out = (2p − d) & 7` (the mirror formula) for every `r` except 0 and 4, where the beam runs parallel to the
  surface and is absorbed. r = 1–3 is C1's mirror; r = 5–7 is the same reflection off the **back face** (the mirror rotated by 180°
  gives the same rule because `2(p+4) = 2p mod 8`). At r = 2 or 6 (head-on) the beam goes straight back (`d + 4`). One output, colour and
  tag kept, no straight-through beam. It is the reflected half of the C2 splitter without the pass-through and without the tag clear.
  Every path is reversible.
- **Quad-bender (20)**. `out = (2p + 7 − d) & 7` (the bender formula) for **all 8** `r`. It never absorbs. r = 0–3 is C1's bender; r = 4–7 is
  the back face (r = 4: +3, r = 5: +2, r = 6: +1, r = 7: +0). Colour and tag kept. Each path is reversible (input `d` → `o`, input `o + 4` → `d + 4`),
  so the 8 entry directions form **4 independent two-way paths**: that is the hint's "bounce up to four separate beams off it" (0x7f10).
  Like the bender, it turns orthogonal beams diagonal and vice versa.
- **Starburst (17, fixed)**. It ignores its `dir` and `r`. A beam travelling in an **even** (orthogonal) direction is replaced by four beams
  in directions **1, 7, 3, 5** (NE, NW, SE, SW, pushed in that order); a beam in an **odd** (diagonal) direction by four beams in
  **0, 2, 4, 6** (N, E, S, W). Each has colour `c` and **tag 0** (disentangles). The outputs include the two directions at ±135°, i.e.
  partly backwards. Nothing ever goes straight on.
- **Gate (16, movable)**. The tracer **absorbs** every beam (same `return` as the wall) and only records `in[d] |= c`. Its output comes from
  the gate pass below.
- **Mobile teleporter (18) and teleporter (12)**: one shared handler (0x382c). From the entry cell it steps `(dx[d], dy[d])` repeatedly
  (x bound unsigned, y bound signed, as C1) and stops at the first cell of **type 12 or 18**; it pushes `(target, d, c, t)` there. If the walk
  leaves the board the beam vanishes. So fixed and mobile teleporters pair with each other in any combination. The target's handler isn't run.
- **Splitter (8)**: unchanged from C2 (same code shape, jump table at 0x37bc = C2's at 0x3824 entry for entry): absorb r = 0/4; otherwise push
  straight `(d, c, t=0)` then reflected `((2p − d) & 7, c, t=0)`, including the head-on back-reflection at r = 2/6. There are no
  complementor passes or other new behaviour in the splitter.
- Mirror, bender, filter, prism, doppler (incl. "if `t ≠ 0` output `c` unchanged"), tangler, conduit, multi-filter, twister, complementor
  (absorb), empty and pinwheel: unchanged from C2, at the addresses in L1.

**Gate pass** (`emit_gates` @ 0x4198, row-major: y outer, x inner):
```
emit_gates():
    fired = 0
    for y in 0..14: for x in 0..14:
        if type(x,y) == 16:
            p = dir(x,y)
            k = in(x,y)[(p+1)&7] & in(x,y)[(p+7)&7] & ~out(x,y)[p]
            if k != 0: trace_laser(x, y, p, k); fired = 1
    return fired
```
- **Inputs**: the two beams travelling in directions `p + 1` and `p − 1`, i.e. converging at 45° on either side of the gate's axis
  (for `p = 0` (N): one beam heading NE, entering from the SW corner, and one heading NW, entering from the SE corner).
  Each input is the OR of everything entering in that direction. Beams entering in any other direction are absorbed and ignored.
- **Output**: one beam leaving straight ahead in direction `p`, with colour **`in[p+1] & in[p−1]`** (only the colours present in **both**
  inputs). If one input is missing or they share no colour, there's no output. The `& ~out[p]` term emits only bits not already emitted:
  each emission is a fresh `trace_laser` from the gate's cell (tag 0, `entangle_counter = 1`), which marks `out[p] |= k` there in its first
  generation, so the pass reaches a fixed point. No other beam can set `out[]` on a gate cell, since gates absorb.
- No white exception, no parity rule, no shut-down: `shut_down_complementors` and `complementor_new_input` test only type 15, and the gate
  ignores cell `+3`. The colour byte is ignored. Gates and complementors can feed each other; the inner loop runs until neither produces anything new.

**Complementor pass**: unchanged from C2 (`emit_complementors` @ 0x4024, `complementor_new_input` @ 0x3f90, `shut_down_complementors` @ 0x3ed8;
same parity test, complement table, white exception, snapshot comparison and shut-down rule).

**Evidence:**
- Dual-reflector `0x34b0`: `addi r9,r29,-1; cmplwi cr7,r9,6; bgt 0x3a08` (r = 0 → absorb), then table at 0x34d8 (base 0x34d8) indexed by `r − 1`:
  `[0x24,0x1c,-0xc4,0x530,0x3c,0x34,0x2c]` → 0x34fc `addi r5,r30,7`, 0x34f4 `+6`, 0x3414 `+5`, 0x3a08 (r = 4, absorb), 0x3514 `+3`, 0x350c `+2`,
  0x3504 `+1`; all then `0x3418: clrlwi r5,r5,0x1d` and `0x341c..0x3428: mr r6,r26 (c); mr r7,r25 (t)` → `push_beam`.
- Quad-bender `0x345c`: `cmplwi cr7,r29,7; bgt 0x3a08`, then table at 0x3480 (base 0x3480) indexed by `r`:
  `[0x7c,0x74,-0x6c,0x20,0x94,0x8c,0x84,0x28]` → +7 (0x34fc), +6 (0x34f4), +5 (0x3414), +4 (0x34a0 `addi r5,r30,4`), +3, +2, +1,
  +0 (0x34a8 `clrlwi r5,r30,0x1d`); same push tail with `c`, `t`.
- Starburst `0x3924`: `andi. r0,r5,1; bne 0x3980`; even: `li r5,1 / 7 / 3 / 5`, each with `li r7,0` and `r6 = c`; odd (0x3980): `li r5,0 / 2 / 4 / 6`, `li r7,0`.
- Teleporter `0x382c..0x3888`: step by `dir_dx`/`dir_dy` (0x8044/0x8064), `cmplwi r28,0xe; bgt`, `cmpwi r27,0; blt`, `cmpwi r27,0xe; bgt`,
  `cmpwi r0,0xc; beq 0x388c; cmpwi r0,0x12; bne 0x382c`, then `push_beam(x', y', d, c, t)`.
- Gate pass `0x4198`: `cmpwi r0,0x10` @ 0x41d4; `lbz r5,2(r11)` (p); `in[p+1]` via `addi r2,r5,1; clrlwi 0x1d; lbz r0,4(r2)`; `in[p+7]` via
  `addi r9,r5,7`; `lbz r2,0xc(r2)` (out[p]); `and r0,r0,r9; andc. r6,r0,r2; beq skip; mr r3,x; mr r4,y; bl 0x3a1c; li r3,1`.
- `retrace_beams @ 0x4258`: `bl 0x3d98; bl 0x3d24; bl 0x3e68; bl 0x3de0; bl 0x3e68; bl 0x4198; mr r30,r3; bl 0x4024; bl 0x3f90; …; bl 0x4198; …;
  cmpwi r30,0; bne 0x4278; bl 0x3ed8; cmpwi r3,0; bne 0x426c`.
- Splitter `0x3774`: `cmpwi r29,0; beq absorb; cmpwi r29,4; beq absorb; mr r4,r27; li r7,0; bl push_beam`, table at 0x37bc = `[0x1c,0x3c,0x24,0x24c,0x2c,0x44,0x34]`
  → +7, +6, +5, absorb, +3, +2, +1, then `0x39d0: li r7,0` → push.
**Confidence:** confirmed for every row (code; simulator wins 50/50 playable levels and all 26 unreachable blobs that solve (all except ids 32 and 53)). The back-face
behaviour of the dual-reflector and quad-bender, the C2 splitter, the gate's AND rule and input/output directions, the starburst's parity and
the mobile teleporter as a jump target are each pinned by the data (sensitivity runs in Verification).

### L4. Termination
**Answer:**
- **Per laser trace: unchanged** (no visited marks; ≤ 1023 generations, `li r30,0x3ff` @ 0x3a8c, `addic. r30,r30,-1` @ 0x3d08; ≤ 32 beams per
  generation, `cmpwi cr7,r10,0x20; beq` @ 0x32c4 in `push_beam`, extra pushes dropped). Every complementor **and gate** emission is its own
  `trace_laser` call with fresh lists and limits and `entangle_counter = 1`.
- **Passes**: outer loop (clear, trace lasers, inner loop, shut-down; repeat while a complementor was shut) as C2, at most N + 1 times for N
  complementors. Inner loop, exact call sequence per iteration (0x4278–0x42ac): `snapshot_cell_inputs` (0x3e68); `changed = emit_gates()`
  (0x4198); `emit_complementors()` (0x4024, result ignored); `if complementor_new_input() (0x3f90): changed = 1`; **`if emit_gates(): changed = 1`** (0x4198 again,
  second call @ 0x4298); loop while `changed != 0` (`cmpwi r30,0; bne 0x4278` @ 0x42a8). So it repeats while either gate call
  emitted, or a complementor input slot went from 0 to non-zero since the snapshot. Each repeat adds at least one
  new bit to some gate's `out[p]` (at most 3 per gate) or one new non-zero complementor slot (at most 8 per complementor), so it runs at most
  3·G + 8·N + 1 times. Gates are never shut down.
- The new pieces cannot loop forever: dual-reflector, quad-bender and starburst are ordinary tracer cases bounded by the 1023/32 limits.
  A quad-bender or starburst inside a ring just re-marks the same cells until the generation limit, drawn as a steady beam.
- Neither the cap nor the generation limit is reached in any stored solution, playable or unreachable (simulator: 0 dropped pushes,
  0 traces cut at 1023). No complementor is shut down in any stored solution.
**Evidence:** addresses above; `retrace_beams @ 0x4258` loop structure in L3.
**Confidence:** confirmed.

### L5. Colour accumulation and the win condition
**Answer:** Unchanged from C2/C1. `check_win` @ 0x4914 (identical code to C2's 0x4784) runs `retrace_beams()` (now with the gate pass), sets
`level_won = 1`, clears it if any pinwheel in rows 0–14 fails `pinwheel_satisfied` @ 0x42d0 (`colour == OR(in[0..7])`, same 8× inner loop),
applies `force_win`, and on `game_mode != 0 && level_won && !completed[current_slot]` sets `completed`, then calls `save_level_state` (0x5ae8),
`write_save_file` (0x47a0) and `update_unlocked` (0x4454). None of the new pieces is tested directly; gates and starbursts matter only through
the beams they produce. Black pinwheels need no light; pinwheels don't block.
**Evidence:** `check_win @ 0x4914`: `bl 0x4258` @ 0x492c, `cmpwi …, 0xa` (pinwheel), `bl 0x42d0`; tail as C2.
**Confidence:** confirmed.

### L6. Progress and unlocking
**Answer:** Storage as C2: `completed[64]` @ 0x181b0 and `unlocked[64]` @ 0x180a0, per **slot**. 50 slots (strip 25 × 2; `level_order` @ 0x8110
lists 50 level ids; `next_level` @ 0x5cf4 only advances while `current_slot ≤ 48` (`cmpwi r2,0x30`)). `unlocked[]` is only ever set. **The rule changed:**
```
update_unlocked():                                     # 0x4454
    request_redraw(0, 0, 640, 480)
    n = 25
    for s in 0..63:
        if completed[s]: unlocked[s] = 1
        elif n > 0:      unlocked[s] = 1; n -= 1
```

| completed slots k | any k |
|---|---|
| uncompleted slots open (n) | 25 (the 25 lowest-numbered uncompleted slots) |

A fresh game opens slots 1–25. Each first win opens one more slot, so the number of open slots is `min(50, k + 25)`; with k ≥ 25 every slot is open.
There is no count and no dip as in C2.
Called from the end of `load_save_file` (0x44e4) and from `check_win`.
Per-level board state as C2 (0x1f2c-byte copies; `save_level_state` @ 0x5ae8, `start_level(restore)` @ 0x5b5c, `goto_slot` @ 0x5c08;
restart `r` = `start_level(0)`). Strip colours as C2 (data/platform notes).
New at load (data/platform area, noted here because it fills `completed[]`): if the save file holds no `0xff` marker byte after its 32-byte
bitmap, `load_save_file` throws away the bitmap. It then replays every slot 0..63 whose level has a saved board: it sets `current_slot`, loads the
level text, copies the saved board, sets `game_mode = 2` and runs `check_win`, which re-marks `completed` and re-saves. After the loop
`current_slot = 0`. A file written by this version always has the marker (`write_save_file` writes `0xff` after the bitmap), and a missing
file leaves `completed[]` all 0.
**Evidence:** `update_unlocked @ 0x4454`: `bl 0x31f4` (redraw 0,0,0x280,0x1e0); `li r0,0x40; mtctr; li r9,0x19; …; lwzx r0,r2,r8 (completed);
cmpwi r0,0; beq → (cmpwi r9,0; ble skip; stwx r11 (1), unlocked; addi r9,r9,-1) else stwx r11; addi r2,r2,4; bdnz`. The C2 count loop and
the thresholds are gone.
**Confidence:** confirmed (code).

### L7. Interaction rules
**Answer:** Identical to C2 (`handle_mouse` @ 0x55c8, `on_mouse_event` @ 0x3028, `on_wheel_event` @ 0x2d80, `on_menu_command` @ 0x29d8 and
`on_key_event` @ 0x2b94 match C2 instruction for instruction), apart from the cheat key:
- Mouse: event codes 1 (down), 3 (alt down, ignored), 4 (up), 6 (alt up), 0 (dragged). Alt = button 2, or primary with Shift/Option/Control
  (`& 0x1a00`) latched at mouse-down. The registered kinds are the same as C2 (`mous` 1, 2, 6, wheel 10; EventTypeSpecs at 0x7fbc).
- Hit test, same constants: board `X/24 − 2`, `(Y+30)/24 − 2`; toolbox `(X−422)/26 − 1` in 0..5, `(Y+18)/26 − 1` in 0..3 → slot `col + 6·row`
  in row 15; strip `(X+10)/20 − 1` in 0..24, `(Y−390)/20 − 1` in 0..1 → slot `col + 25·row`.
- Drag (`begin_drag` @ 0x5430, needs `is_movable`; it now also accepts gates, mobile teleporters, dual-reflectors and quad-benders), drop only onto
  type-0 cells (28-byte copy), rotate on up without drag, strip click → `goto_slot` if `unlocked`. The stale-`drag_state` quirk is still there.
  Walls, lasers, pinwheels, conduits, teleporters (12), twisters and starbursts can't be moved or rotated.
- Keys (`handle_key` @ 0x6358, ignored when Cmd is held): Space, `=` or `+` → `next_level` if `unlocked[current_slot+1]`; `-` → `prev_level`;
  `r` → `start_level(0)`; **`^` (0x5e) → `force_win = 1; check_win()`** (C2: `z`, C1: `L`); Esc is swallowed. Menu: `Next` → `=`, `Prev` → `-`,
  `rvrt` → `r`, `copy`/`past` (clipboard, data/platform notes), `Whee` toggles the wheel preference. Wheel: delta > 0 → `-`, else `=`.
**Evidence:** `handle_key @ 0x6358`: `cmpwi r3,0x2d` @ 0x6358, `0x20` @ 0x637c, `0x1b` @ 0x6388, `0x2b` @ 0x6398, **`0x5e` @ 0x63a0**, `0x3d` @ 0x63ac,
`0x72` @ 0x63b8; decompiled `if (param_1 == 0x5e) { *force_win = 1; check_win(); return 0; }`.
**Confidence:** confirmed.

### L8. What happens on a win
**Answer:** Same as C2. Nothing automatic happens (no advance, timer, animation or sound). `paint_window` @ 0x6bd4 draws **"You win!"** at
(330, 385, 400, 420), or **"(won)"** at left 350 when the level is completed but not currently won. The hint box (450, 125, 620, 475) shows
**"Click on a level or press spacebar for next."** while `game_mode == 2 && level_won`. The level name is drawn at (50, 385, 300, 420)
while `game_mode == 2` (else at (350, 420, 450, 460)). **"Thank you for purchasing this game."** is drawn at (0, 450, 400, 480) whenever
`current_slot % 6 == 4`. The site-URL text is drawn at (470, 450, 640, 480) (de-obfuscated from 0x80f4; data/platform notes). The first win
sets `completed`, saves the board and the save file, and recomputes unlocks (L6).
**Evidence:** `paint_window @ 0x6bd4` (decompiled, condensed):
```c
  if (level_won == 0) { if (completed[current_slot] == 0) goto skip; l = 0x15e; s = "(won)"; }
  else { l = 0x14a; s = "You win!"; }
  draw_text_box(s, l, 0x181, 400, 0x1a4);
skip:
  if (game_mode == 2) draw_text_box(level_name, 0x32, 0x181, 300, 0x1a4); else draw_text_box(level_name, 0x15e, 0x1a4, 0x1c2, 0x1cc);
  s = hint_text; if (game_mode == 2 && level_won) s = "Click on a level or press spacebar for next.";
  draw_text_box(s, 0x1c2, 0x7d, 0x26c, 0x1db);
  if (current_slot % 6 == 4) draw_text_box("Thank you for purchasing this game.", 0, 0x1c2, 400, 0x1e0);
```
**Confidence:** confirmed.

## Verification
```bash
BIN="chromatron_mac/Chromatron 3.app/Contents/MacOS/chromatron3"
D=re/out/chromatron3/decompiled.c

# Names applied (after the controller re-runs the decompile with logic.names.tsv)
grep -c -E '^// (is_movable|push_beam|beam_enter_cell|trace_laser|clear_beams|clear_complementor_flags|trace_all_lasers|snapshot_cell_inputs|shut_down_complementors|complementor_new_input|emit_complementors|emit_gates|retrace_beams|pinwheel_satisfied|update_unlocked|check_win|check_win_and_redraw|begin_drag|handle_mouse|save_level_state|goto_slot|slot_next_id|slot_prev_id|next_level|prev_level|handle_key|on_menu_command|on_key_event|on_wheel_event|on_mouse_event) @ ' $D
# 30

# beam_enter_cell type jump table @ 0x33a4 (types 0..20; offsets from 0x33a4):
# 12 and 18 -> 0x488 (0x382c teleport), 15 and 16 -> 0x664 (0x3a08 absorb), 17 -> 0x580 (0x3924), 19 -> 0x10c (0x34b0), 20 -> 0xb8 (0x345c)
r2 -e scr.color=0 -qc 'pxw 84 @ 0x33a4' "$BIN" </dev/null 2>/dev/null
# 0x000033a4  0x00000078 0x00000664 0x00000664 0x00000054  ...x...d...d...T
# 0x000033b4  0x0000008c 0x0000032c 0x00000178 0x000002b0  .......,...x....
# 0x000033c4  0x000003d0 0x00000364 0x00000078 0x00000474  .......d...x...t
# 0x000033d4  0x00000488 0x00000500 0x00000578 0x00000664  ...........x...d
# 0x000033e4  0x00000664 0x00000580 0x00000488 0x0000010c  ...d............
# 0x000033f4  0x000000b8                                   ....

# is_movable: table for types 3..20 (0x48 -> movable, 0x50 -> not): 3-9, 13, 15, 16, 18, 19, 20
r2 -e scr.color=0 -qc 'pi 4 @ 0x3214; pxw 72 @ 0x324c' "$BIN" </dev/null 2>/dev/null
# addi r3, r3, -3
# mflr r0
# cmplwi cr7, r3, 0x11
# bcl 20, 31, 0x3224
# 0x0000324c  0x00000048 0x00000048 0x00000048 0x00000048  ...H...H...H...H
# 0x0000325c  0x00000048 0x00000048 0x00000048 0x00000050  ...H...H...H...P
# 0x0000326c  0x00000050 0x00000050 0x00000048 0x00000050  ...P...P...H...P
# 0x0000327c  0x00000048 0x00000048 0x00000050 0x00000048  ...H...H...P...H
# 0x0000328c  0x00000048 0x00000048                        ...H...H

# Piece 1, DUAL-REFLECTOR: r-1 table (from 0x34d8): +7, +6, +5, absorb, +3, +2, +1; r=0 absorbed by the bound check
r2 -e scr.color=0 -qc 'pi 3 @ 0x34b0; pxw 28 @ 0x34d8' "$BIN" </dev/null 2>/dev/null
# addi r9, r29, -1
# cmplwi cr7, r9, 6
# bgt cr7, 0x3a08
# 0x000034d8  0x00000024 0x0000001c 0xffffff3c 0x00000530  ...$.......<...0
# 0x000034e8  0x0000003c 0x00000034 0x0000002c             ...<...4...,

# Piece 2, QUAD-BENDER: r table (from 0x3480): +7 +6 +5 +4 +3 +2 +1 +0 (targets below)
r2 -e scr.color=0 -qc 'pi 2 @ 0x345c; pxw 32 @ 0x3480; pi 4 @ 0x34a0; pi 8 @ 0x34f4' "$BIN" </dev/null 2>/dev/null
# cmplwi cr7, r29, 7
# bgt cr7, 0x3a08
# 0x00003480  0x0000007c 0x00000074 0xffffff94 0x00000020  ...|...t....... 
# 0x00003490  0x00000094 0x0000008c 0x00000084 0x00000028  ...............(
# addi r5, r30, 4
# b 0x3418
# clrlwi r5, r30, 0x1d
# b 0x341c
# addi r5, r30, 6
# b 0x3418
# addi r5, r30, 7
# b 0x3418
# addi r5, r30, 1
# b 0x3418
# addi r5, r30, 2
# b 0x3418

# Piece 3, SPLITTER (unchanged from C2): absorb r=0/4, straight with tag 0, then reflect table (from 0x37bc)
r2 -e scr.color=0 -qc 'pi 8 @ 0x3774; pxw 28 @ 0x37bc' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r29, 0
# beq cr7, 0x3a08
# cmpwi cr7, r29, 4
# beq cr7, 0x3a08
# mr r4, r27
# li r7, 0
# bl 0x32a4
# clrlwi r2, r24, 0x1d
# 0x000037bc  0x0000001c 0x0000003c 0x00000024 0x0000024c  .......<...$...L
# 0x000037cc  0x0000002c 0x00000044 0x00000034             ...,...D...4

# Piece 4, STARBURST: parity of d selects diagonals (1,7,3,5) or orthogonals (0,2,4,6), tag 0
r2 -e scr.color=0 -qc 'pi 4 @ 0x3924; pi 3 @ 0x3980; pi 2 @ 0x39cc' "$BIN" </dev/null 2>/dev/null
# andi. r0, r5, 1
# bne 0x3980
# mr r4, r27
# li r5, 1
# mr r4, r27
# li r5, 0
# li r7, 0
# li r5, 6
# li r7, 0

# Piece 5, MOBILE TELEPORTER: the jump stops on type 12 or 18
r2 -e scr.color=0 -qc 'pi 4 @ 0x387c' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r0, 0xc
# beq cr7, 0x388c
# cmpwi cr7, r0, 0x12
# bne cr7, 0x382c

# Piece 6, GATE pass: k = in[p+1] & in[p+7] & ~out[p]
r2 -e scr.color=0 -qc 'pi 13 @ 0x41dc' "$BIN" </dev/null 2>/dev/null
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

# retrace_beams: flags; (clear, snapshot, lasers, (snapshot, gates, complementors, new-input, gates) while changed) until no shut-down
r2 -e scr.color=0 -qc 'pi 15 @ 0x4268' "$BIN" </dev/null 2>/dev/null
# bl 0x3d98
# bl 0x3d24
# bl 0x3e68
# bl 0x3de0
# bl 0x3e68
# bl 0x4198
# mr r30, r3
# bl 0x4024
# bl 0x3f90
# cmpwi cr7, r3, 0
# beq cr7, 0x4298
# li r30, 1
# bl 0x4198
# cmpwi cr7, r3, 0
# beq cr7, 0x42a8

# Complementor (unchanged): parity test, white skipped, complement table
r2 -e scr.color=0 -qc 'pi 9 @ 0x408c; pxw 24 @ 0x40cc' "$BIN" </dev/null 2>/dev/null
# lbz r0, 2(r11)
# clrlwi r2, r30, 0x1f
# clrlwi r0, r0, 0x1f
# cmpw cr7, r2, r0
# bne cr7, 0x415c
# addi r9, r9, -1
# li r27, 1
# cmplwi cr7, r9, 5
# bgt cr7, 0x415c
# 0x000040cc  0x0000002c 0x00000040 0x00000054 0x00000018  ...,...@...T....
# 0x000040dc  0x0000007c 0x00000068                        ...|...h

# Multi-filter (unchanged) table, twister (unchanged)
r2 -e scr.color=0 -qc 'pxw 32 @ 0x38c8; pi 2 @ 0x391c' "$BIN" </dev/null 2>/dev/null
# 0x000038c8  0x00000048 0x00000034 0x00000020 0xfffffb54  ...H...4... ...T
# 0x000038d8  0x00000048 0x00000034 0x00000020 0xfffffb54  ...H...4... ...T
# addi r5, r5, -2
# b 0x3418

# Termination unchanged: tag 0 on the laser push, 1023 generations, 32-beam cap
r2 -e scr.color=0 -qc 'pi 1 @ 0x3a40; pi 1 @ 0x3a8c; pi 1 @ 0x3d08; pi 2 @ 0x32c4' "$BIN" </dev/null 2>/dev/null
# li r7, 0
# li r30, 0x3ff
# addic. r30, r30, -1
# cmpwi cr7, r10, 0x20
# beq cr7, 0x3318

# check_win calls retrace_beams; unlock n = 25; cheat key '^'; next_level cap
r2 -e scr.color=0 -qc 'pi 1 @ 0x492c; pi 1 @ 0x4488; pi 1 @ 0x63a0; pi 1 @ 0x5d18' "$BIN" </dev/null 2>/dev/null
# bl 0x4258
# li r9, 0x19
# cmpwi cr7, r3, 0x5e
# cmpwi cr7, r2, 0x30

# dir tables and doppler maps unchanged (dx @0x8044, dy @0x8064, back @0x8084, fwd @0x8098)
xxd -s 0x7044 -l 0x68 "$BIN"
# 00007044: 0000 0000 0000 0001 0000 0001 0000 0001  ................
# 00007054: 0000 0000 ffff ffff ffff ffff ffff ffff  ................
# 00007064: ffff ffff ffff ffff 0000 0000 0000 0001  ................
# 00007074: 0000 0001 0000 0001 0000 0000 ffff ffff  ................
# 00007084: 0000 0000 0000 0002 0000 0004 0000 0000  ................
# 00007094: 0000 0001 0000 0000 0000 0004 0000 0001  ................
# 000070a4: 0000 0000 0000 0002                      ........

# Level table, name table, hint table, level_order, board (nl_symbol_ptr slots) used by the simulator
r2 -e scr.color=0 -qc 'pxw 4 @ 0x16584; pxw 4 @ 0x1657c; pxw 4 @ 0x16580; pxw 4 @ 0x1654c; pxw 4 @ 0x16570' "$BIN" </dev/null 2>/dev/null
# 0x00016584  0x0000d590                                   ....
# 0x0001657c  0x0000d6cc                                   ....
# 0x00016580  0x0000d808                                   ....
# 0x0001654c  0x00008110                                   ....
# 0x00016570  0x000182c0                                   ....
```

**Simulator check** (throwaway Python, `/private/tmp/claude-503/c3sim/`, not committed). It finds the tables through the pointer slots above
(level table 0xd590 = 79 pointers, id 0 NULL, 5-byte records `type, dir, colour, x, y` terminated by type 0, as decoded by `load_level_data` @ 0x6efc).
It uses `level_order` @ 0x8110 (50 slots) and leaves every movable piece at its stored x/y/dir. It runs `retrace_beams` exactly as in L3 (the C2 tracer
plus dual-reflector, quad-bender, starburst, mobile teleporter, gate pass, complementor pass, 32 cap, 1023 generations, entanglement pass) and then the L5 test:
- **50 / 50 playable levels are won** from their stored placement; **0 / 50** with all movable pieces removed.
- Unreachable ids: **26 / 28 won** from their stored placement, 0 with movables removed. The two failures, id 32 ("level032", one laser,
  8 mirrors) and id 53 ("level053", the only quad-bender outside the playable set), both carry the placeholder hint "blah". They are unfinished
  drafts: each leaves one pinwheel completely unlit (id 32: red at (9,5); id 53: blue at (12,0)), and no rule variant below makes them win.
- The 32-beam cap and the 1023-generation limit are never hit. No complementor shut-down occurs. Gate emissions: 10 (unreachable ids 1, 2, 3, 10, 14);
  starburst hits: 20 (ids 4–9, 26); mobile-teleporter jumps through ids 16, 17, 18, 20, 21.
- Beam entries per piece type in the playable solutions: mirror 334, bender 271, prism 5, splitter 23 (1 head-on, slot 35), conduit 11, pinwheel 735,
  **dual-reflector 15 (8 on the back face)**, **quad-bender 18 (9 on the back face)**. Dual-reflectors are hit in slots 17, 18, 33, 37 and 39,
  quad-benders in slots 22, 23, 24, 38 and 49.
- Sensitivity (playable, 50 levels):
  - dual-reflector one-sided (C1 mirror rule) → 45 won (fails 17, 18, 33, 37, 39); dual-reflector absorbing → same 45;
    dual-reflector as a splitter (pass + reflect) → 47 (fails 17, 37, 39);
  - quad-bender one-sided (C1 bender rule) → 45 won (fails 22, 23, 24, 38, 49); quad-bender with the mirror formula on both faces → 45;
    quad-bender `2p + 9 − d` (rotation off by one) → 46;
  - C1 splitter (no head-on back-reflection) → 49 won (slot 35 fails).
- Sensitivity (the 26 unreachable blobs that solve (all except ids 32 and 53)):
  - gate with OR instead of AND → 25 (id 2 fails); gate never emitting → 21 (ids 1, 2, 3, 10, 14); gate inputs `p ± 3` or output `p + 4` → 21;
    gate with a single input `p + 1` → 25 (id 2);
  - starburst with parity swapped, or absorbing → 19 (ids 4–9, 26); starburst keeping the tag → 25 (id 6: the entangled pair from the
    tangler at (9,1) hits the starburst at (7,7));
  - mobile teleporter not a jump target, or absorbing → 21 (ids 16, 17, 18, 20, 21);
  - twister `d + 2` → 25 (id 17); no complementor emission → 25 (id 7).

  Not pinned: tag handling of the dual-reflector and quad-bender (no tagged beam reaches them), and the order of the starburst's four pushes.
