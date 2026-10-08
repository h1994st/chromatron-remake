# Chromatron 2 v1.4 — Game logic (diff against Chromatron 1)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2` (Mach-O, big-endian PowerPC).
All addresses are vmaddrs. File offset = vmaddr − 0x1000. This note cites addresses rather than
decompiled.c line numbers. Function and data names are the ones in `logic.names.tsv` (this note), plus
the data agent's `data.names.tsv`. Where a C2 function does the same job as a C1 one, it keeps the C1 name.

In prose, 'slot N' is the 1-based strip position, i.e. `current_slot = N − 1`. Code expressions use the 0-based `current_slot`.

Baseline: `re/notes/chromatron1/logic.md` (C1 L1–L8) and `re/notes/chromatron1/data.md`. The notation is the same:
`p` = the piece's `dir` byte, `d` = beam travel direction, `c` = colour mask (1 B, 2 G, 4 R), `t` = entanglement tag,
**`r = (d − p) & 7`**, `+k` = output direction `(p + k) & 7`, and "same" = keeps going in direction `d`.

**Summary of differences from C1**
1. The cell is **28 bytes (0x1c)**, not 20. A row is 0x1a4 bytes and the board is 19 rows = 0x1f2c bytes. Two cell fields are new:
   `+3` is the complementor **shut-down flag** (unused in C1), and `+20..+27` (`+0x14`) is a **snapshot of `in[0..7]`** used by the complementor pass.
2. There are three new piece types: **13 multi-filter** (movable), **14 twister** (fixed) and **15 complementor** (movable).
3. **The splitter changed**: a head-on hit (r = 2 or 6) now also reflects straight back, and every splitter output has its tag cleared.
4. `retrace_beams` has a new **complementor fixed-point pass** with a "shut down" rule.
5. The **unlock rule** now uses the *number* of completed slots and has new thresholds. At startup 5 levels are open.
6. The force-win cheat key is **`z`**, not `L`.
7. Win display: the level **name** is drawn at the bottom left. "freeware"/"more levels @" are replaced by
   "Thank you for purchasing this game." on every slot where `current_slot % 6 == 4`.

Everything else in the tracer is the same code as C1, byte for byte in behaviour: push order, the 32-beam cap, the 1023-generation limit,
mirror, bender, filter, prism, doppler, tangler, conduit, teleporter and the entanglement pass.

Globals reached through `__nl_symbol_ptr` (`r2 -qc 'pxw 4 @ <slot>'`), logic area only:

| slot | → target | name | meaning (as C1 unless noted) |
|---|---|---|---|
| 0x12334 | 0x161e4 | `cursor_row` | int |
| 0x12338 | 0x161e0 | `cursor_col` | int |
| 0x1234c | 0x13294 | `game_mode` | int, set to 2 by every `start_level` |
| 0x12350 | 0x13fb0 | `force_win` | int, set by the **`z`** key |
| 0x12354 | 0x140c0 | `level_won` | int |
| 0x12358 | 0x16110 | `saved_states` | `u8*[]` per level id, **0x1f2c**-byte board copies |
| 0x1235c | 0x13eb0 | `unlocked` | `int[64]` per slot |
| 0x12360 | 0x13fc0 | `completed` | `int[64]` per slot |
| 0x12364 | 0x13080 | `beam_cur_count` | int |
| 0x12368 | 0x13190 | `beam_buf_b` | 32 × 8-byte beams |
| 0x1236c | 0x13090 | `beam_buf_a` | 32 × 8-byte beams |
| 0x12370 | 0x1307c | `beam_cur_list` | pointer |
| 0x12378 | 0x16100 | `beam_next_list` | pointer |
| 0x1237c | 0x16104 | `beam_next_count` | int |

Directly addressed: `entangle_counter` @ 0x123dc, `drag_state` @ 0x123bc, `mouse_alt_button` @ 0x123b4,
`press_x`/`press_y` @ 0x12fec/0x12ff0, `drag_image` @ 0x80d0, `dir_dx`/`dir_dy` @ 0x8044/0x8064 (unchanged values),
`doppler_back_map` @ 0x8084 = `[0,2,4,0,1]`, `doppler_fwd_map` @ 0x8098 = `[0,4,1,0,2]` (unchanged).
(Board, level, name and hint tables belong to data.md: `board` 0x140d0, `level_table` 0xa934, level-name table 0xaa08,
`hint_table` 0xaadc, `level_order` 0x8110.)

## Findings

### L1. Piece kinds, movable and rotatable
**Answer:** The piece enum is the cell `type` byte. Types 0–12 are C1's types, unchanged. Types 13–15 are new. Anything above 15 hits the
`default:` assertion in `beam_enter_cell` (game.c line 0x1a6, the same line number as C1).

| type | piece | colour byte | movable = rotatable | `dir` bits that matter | image (cell_image_index @ 0x48c4) | beam behaviour (L3) |
|---|---|---|---|---|---|---|
| 0 | empty | – | – | – | 0 | passes |
| 1 | wall | – | no | – | 1 | absorbs |
| 2 | laser, 1/2/4/7 | yes | no | all 8 | as C1 | emits; absorbs incoming |
| 3 | mirror | – | yes | all 8 | 32+d | as C1 |
| 4 | bender | – | yes | all 8 | 48+d | as C1 |
| 5 | filter, 1/2/4 | yes | yes | `& 3` | as C1 | as C1 |
| 6 | prism | – | yes | all 8 | 24+d | as C1 |
| 7 | doppler | – | yes | all 8 | 64+d | as C1 |
| 8 | splitter | – | yes | `& 3` | 40+(d&3) | **changed**: pass + reflect at every non-parallel angle, tag cleared |
| 9 | quantum tangler | – | yes | all 8 | 80+d | as C1 |
| 10 | pinwheel, mask 0–7 | yes | no | – | as C1 (+1 lit) | passes, records colour |
| 11 | conduit | – | no | `& 3` | 44+(d&3) | as C1 |
| 12 | teleporter | – | no | – | 14 | as C1 |
| **13** | **MULTI-FILTER** | ignored (always 4 in data) | **yes** | `& 3` | **100+(d&3)** | passes one colour bit chosen by the angle |
| **14** | **TWISTER** | ignored | **no** | none (dir ignored; always 0 in data) | **15** | turns every beam to direction `d − 2` |
| **15** | **COMPLEMENTOR** | ignored | **yes** | `& 1` | **116+(d&1)** | absorbs; re-emits the complement colour (complementor pass, L3/L4) |

`is_movable(type)` @ 0x33a8 is true exactly for types **3–9, 13 and 15**. Every drag, drop, rotate, toolbox move and clipboard
copy/paste goes through it, as in C1. The twister is a fixed board piece, like a conduit. As in C1, `move_pieces_to_toolbox` @ 0x5810
moves **every** movable record to the toolbox at level start (and sets its `dir` to 0), so no level has a fixed movable piece.
C1 images 15, 100–103 and 116/117 were unused, and C2 uses them for the new pieces.
Record counts over all 52 level blobs: twister 25, multi-filter 27, complementor 12.
The most movable pieces in any level is 24 (id 6, slot 50), which exactly fills the 24 visible toolbox slots.
**Evidence:** `is_movable @ 0x33a8` (disassembly):
```
cmpwi cr7, r3, 0xd ; beq -> return 1
bgt -> (cmpwi r3, 0xf ; bne -> return 0) ; return 1
addi r0, r3, -3 ; cmplwi r0, 6 ; bgt -> return 0 ; return 1
```
`beam_enter_cell @ 0x3458`: `cmplwi cr7, r11, 0xf; bgt 0x39ac` (assert) at 0x34a0, then a 16-entry jump table at 0x34dc
(base 0x34dc): 0→0x3988 pass, 1/2→0x39cc absorb, 3→0x351c, 4→0x353c, 5→0x3738, 6→0x3584, 7→0x36bc, 8→0x37dc, 9→0x3770,
10→0x3988 pass, 11→0x3884, 12→0x3898, **13→0x3908, 14→0x3980, 15→0x39cc absorb**.
`cell_image_index @ 0x48c4`: `case 0xd: lVar5 = uVar2 + 100;` `case 0xe: lVar5 = 0xf;` `case 0xf: lVar5 = (uVar6 & 1) + 0x74;` (`uVar2` = dir & 3, `uVar6` = dir).
Names come from the hint strings at 0x7618 (COMPLEMENTOR), 0x7728 (MULTI-FILTER) and 0x74c4 (twister).
**Confidence:** confirmed.

### L2. Direction encoding and rotation
**Answer:** Same as C1. The 8 directions 0–7 are N, NE, …, NW, and the `dir_dx`/`dir_dy` tables at 0x8044/0x8064 hold the same values.
A plain left click rotates `dir − 1` (counter-clockwise); a right click, or a left click with Shift/Option/Control (`& 0x1a00`), rotates `dir + 1`.
Rotation works on the board and in the toolbox. A piece moved to the toolbox at level start gets `dir = 0`.
New pieces: the multi-filter's 8 rotations come in 4 identical pairs (`dir & 3`). The complementor's come in 2 classes (`dir & 1`):
even `dir` handles orthogonal beams and odd `dir` handles diagonal beams. The twister can't be rotated.
**Evidence:** `handle_mouse @ 0x53ec`: event 4 `puVar9[iVar11 + 2] + -1; & 7`, event 6 `+ '\x01'; & 7`, both guarded by `is_movable`.
`on_mouse_event @ 0x31bc`: `DAT_000123b4 = -(local_1c & 0x1a00) >> 0x1f` on mouse-down.
**Confidence:** confirmed.

### L3. Beam tracer and transfer table
**Answer:**

**Data structures**: same as C1. A beam is 8 bytes `{u8 x, u8 y, u8 dir, u8 colour, i32 tag}` and there are two 32-entry lists.
`push_beam` @ 0x33dc drops the beam if the next list already holds 32. `in[d]` is cell `+4+d` and `out[d]` is `+12+d`.
The cell stride is now 0x1c and the row stride 0x1a4.

**Per-laser trace** `trace_laser(x, y, d, c)` @ 0x39e0: identical to C1. It sets `entangle_counter = 1`, pushes `(x, y, d, c, tag 0)`
(`li r7, 0` @ 0x3a04) and allows up to 1023 generations. In each generation it marks `out`, steps, and calls `beam_enter_cell` if the new
position is on the board. Then it runs the entanglement pass over the next list (0x3bb4–0x3cb4, `doppler_fwd_map`/`doppler_back_map`, unchanged).

**Top level** (`retrace_beams` @ 0x415c, **new structure**):
```
retrace_beams():
    for rows 0..14, cols 0..14: shut(x,y) = 0             # clear_complementor_flags @ 0x3d5c, cell +3
    repeat:
        clear in[]/out[] of all 15×15 cells                # clear_beams @ 0x3ce8
        for y in 0..14: for x in 0..14:                    # trace_all_lasers @ 0x3da4 (row-major)
            if type(x,y) == 2: trace_laser(x, y, dir, colour)
        repeat:
            for every board cell: snap[0..7] = in[0..7]    # snapshot_cell_inputs @ 0x3e2c, cell +0x14
            emit_complementors()                           # 0x3fe8
        until not complementor_new_input()                 # 0x3f54
    until not shut_down_complementors()                    # 0x3e9c
```

**Transfer table** (`beam_enter_cell` @ 0x3458). As in C1, `in[d] |= c` is recorded first for every cell type, and outputs are pushed in the listed
order. An output keeps colour `c` and tag `t` unless the table says otherwise. "–" means absorbed.

| type | r=0 | r=1 | r=2 | r=3 | r=4 | r=5 | r=6 | r=7 |
|---|---|---|---|---|---|---|---|---|
| 0 empty, 10 pinwheel | same | same | same | same | same | same | same | same |
| 1 wall, 2 laser, **15 complementor** | – | – | – | – | – | – | – | – |
| 3 mirror | – | +7 | +6 | +5 | – | – | – | – |
| 4 bender | +7 | +6 | +5 | +4 | – | – | – | – |
| 5 filter (colour f) | – | – | same, `c & f` | – | – | – | same, `c & f` | – |
| 6 prism | as C1 (per bit R, G, B; table in C1 L3) | | | | | | | |
| 7 doppler | – | – | same, `fwd(c)` | – | – | – | same, `back(c)` | – |
| **8 splitter** | – | same `t=0`; +7 `t=0` | **same `t=0`; +6 `t=0`** | same `t=0`; +5 `t=0` | – | same `t=0`; +3 `t=0` | **same `t=0`; +2 `t=0`** | same `t=0`; +1 `t=0` |
| 9 tangler | – | – | pair (as C1) | – | – | – | – | – |
| 11 conduit | same | – | – | – | same | – | – | – |
| 12 teleporter | jump | jump | jump | jump | jump | jump | jump | jump |
| **13 multi-filter** | same, `c & 1` | same, `c & 2` | same, `c & 4` | same | same, `c & 1` | same, `c & 2` | same, `c & 4` | same |
| **14 twister** | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` | `d − 2` |

Notes on the rows that differ from C1:
- **Splitter (changed)**. It still absorbs r = 0 and r = 4 (beam parallel to the surface). For **every other r (1, 2, 3, 5, 6, 7)** it pushes two beams in this order:
  (1) straight through, `(d, c, t = 0)`, and (2) reflected, `(2p − d) & 7` (the mirror formula), with `(c, t = 0)`.
  For r = 2 or 6 (head-on), `2p − d = d + 4`, so the second beam goes **straight back the way it came**. Both faces reflect.
  Compared with C1: (a) at r = 2/6, C1 passed straight only and **kept** the tag. C2 adds the back-reflection (this is the "one very important case"
  in the slot-12 hint at 0x7400: "one beam goes straight through, and another bounces off the front"). (b) C2 clears the tag on
  **every** splitter output, so any splitter hit disentangles. At r odd, C2 behaves exactly like C1.
- **Multi-filter (new)**. It depends only on `r mod 4`, i.e. on `p & 3`. One colour bit passes, chosen by the angle between beam and piece:
  r ≡ 0 (beam along the piece's axis): blue (1); r ≡ 1: green (2); r ≡ 2 (perpendicular): red (4). The output is that single bit, in direction `d`,
  and only if the bit is present in `c` (otherwise the beam is absorbed). At **r ≡ 3 the beam passes unchanged** (all colours). The tag is kept
  in every case. The colour byte is ignored. All paths are reversible (r and r + 4 behave the same).
- **Twister (new, fixed)**. It ignores its `dir`. Every beam, from any of the 8 directions, leaves in direction **`(d − 2) & 7`**, keeping
  `c` and `t`: N→W, NE→NW, E→N, SE→NE, S→E, SW→SE, W→S, NW→SW. The beam's heading turns 90° to the left, so its **exit side is
  90° clockwise from its entry side** (a beam entering from the west side leaves through the north side). That's the hint's "sends each beam back
  out 90 degrees clockwise" (0x74c4). It is **not reversible**: sending the output back (direction `d + 2`) gives `d`, not `d + 4`.
- **Complementor (new)**: the tracer **absorbs** every beam that enters it (`case 0xf` shares the wall's `return`). It records only
  `in[d] |= c`. Its output comes from the complementor pass below, as a fresh trace that starts in the complementor's cell.
- Prism, mirror, bender, filter, doppler (including "if `t ≠ 0` output `c` unchanged"), tangler, conduit, teleporter (x bound unsigned, y bound signed),
  empty and pinwheel: unchanged from C1 (same branch structure and constants, at the addresses in L1).

**Complementor pass** (`emit_complementors` @ 0x3fe8, row-major, y outer, then x, then d = 0..7):
```
for y in 0..14: for x in 0..14: for d in 0..7:
    if type(x,y) == 15 and shut(x,y) == 0 and in(x,y)[d] != 0 and (d & 1) == (dir(x,y) & 1):
        k = in(x,y)[d]                       # read live: includes beams added earlier in this same pass
        if k != 7: trace_laser(x, y, d, k ^ 7)   # 1→6, 2→5, 3→4, 4→3, 5→2, 6→1; white → nothing
```
- **Colour**: the complement, `7 − k`: red ↔ cyan ("teal"), blue ↔ yellow, green ↔ magenta. **White produces nothing**. The input is the OR of
  all beams entering in that one travel direction `d`. Inputs in different directions are complemented separately.
- **Direction**: the output continues in the input's travel direction `d`. Only directions whose parity matches `dir & 1` are processed
  (even `dir` = the 4 orthogonal directions, odd `dir` = the 4 diagonals). Beams of the other parity are absorbed with no output.
- **Entanglement**: each emission is a new `trace_laser`, so its tag is 0 (and `entangle_counter` is reset to 1). The complementor **disentangles**
  ("Like the splitter, the complementor will disentangle…", hint 0x75c4, slot 39).
- Each emission marks `out[d]` at the complementor cell in its first generation, so the complementor's cell is drawn with the input colour on
  the entry side and the complement on the exit side.

**Fixed point and shut-down** (`complementor_new_input` @ 0x3f54, `shut_down_complementors` @ 0x3e9c):
- `complementor_new_input` returns 1 if some non-shut complementor has a direction `d` (any of the 8, **parity not checked**) with
  `in[d] != 0` and `snap[d] == 0`, i.e. a complementor input appeared since the snapshot. The inner loop then snapshots again and re-emits
  **every** complementor output. Marks are ORed, so a repeated emission only adds what is new.
- When no new input appears, `shut_down_complementors` checks every non-shut complementor and every `d` (all 8, parity not checked):
  if `snap[d] != in[d]` and `snap[d] != 0` (and not (`snap[d] == 7` and `in[d] == 0`)), it sets `shut = 1` (cell `+3`). It does this
  for **all** such complementors in one call, then returns 1. That means an input that **changed after the complementor had already emitted
  for it**: typically its own complement looped back into it ("When does red plus green make blue?", "logical impossibility").
  `in[]` only grows inside the loop, so the `snap == 7 && in == 0` exception can never apply (dead code).
- If anything was shut, the **whole trace restarts** from clean `in[]`/`out[]`, with the shut complementors excluded. Shut flags stay set
  until the next `retrace_beams`.

What **"shuts down in all directions"** means mechanically: a shut complementor emits nothing in any of its directions (`emit_complementors`
skips it). It **still absorbs** every incoming beam, as every complementor does, and still records `in[]`. So for the rest of this retrace it
is an opaque block, like a wall. The flag is recomputed from scratch on every retrace (every `check_win`). The sprite doesn't change:
`cell_image_index` ignores `+3`. Quirks that follow from the code:
(a) the test compares only against the **last** snapshot, so if an input grew during an earlier inner iteration (in the same iteration as a
brand-new input), the complementor keeps both the stale and the new output and is not shut down;
(b) a wrong-parity slot that grows during the final emission also shuts the complementor down.
No shipped solution triggers (a). In slot 49 the shut-down complementor's wrong-parity slots are all already lit before it emits, so (b) is moot there.

**Evidence:**
- Splitter `0x37dc`: `cmpwi r29,0; beq absorb; cmpwi r29,4; beq absorb; mr r4,r28; li r7,0; bl push_beam` (straight, tag 0), then
  a jump table at 0x3824 on `r − 1` → 0x3840 (+7), 0x3874 (+6), 0x3848 (+5), 0x39cc (r = 4, unreachable), 0x3864 (+3), 0x387c (+2), 0x386c (+1),
  each going to `0x384c: clrlwi r5,r5,0x1d; …; mr r6,r26; li r7,0; b push_beam`. C1's splitter (C1 @ 0x38e4) branched on r = 2/6 to keep
  the tag and skip the reflection. C2 has no such branch.
- Multi-filter `0x3908`: `cmplwi r29,7; bgt absorb`, then a jump table at 0x392c = `[0x48,0x34,0x20,0x5c] ×2` (base 0x392c) →
  r ≡ 0: 0x3974 `andi. r0,r6,1; beq absorb; b 0x3698` (0x3698: `li r6,1; mr r7,r25` = colour 1, tag kept, `r5 = d`);
  r ≡ 1: 0x3960 `andi. r0,r6,2; …; li r6,2; b push`; r ≡ 2: 0x394c `andi. r0,r6,4; …; li r6,4; b push`;
  r ≡ 3: 0x3988 (the generic "push `d`, `c`, `t`" tail).
- Twister `0x3980`: `addi r5, r5, -2; clrlwi r5, r5, 0x1d`, then falls into the 0x3988 push tail with `r6 = c` and `r7 = t`.
- Complementor tracer case: jump-table entry 15 → 0x39cc (return, no push). `emit_complementors @ 0x3fe8`:
  ```
  0x402c cmpwi r9, 0xf ; 0x4034 lbz r0, 3(r11) ; cmpwi r0, 0 (not shut) ; 0x4044 lbz r9, 4(r2) ; cmpwi r9, 0 (in[d] != 0)
  0x4050 lbz r0, 2(r11) ; clrlwi r2, r30, 0x1f ; clrlwi r0, r0, 0x1f ; cmpw r2, r0 ; bne skip   # (d & 1) == (dir & 1)
  0x4064 addi r9, r9, -1 ; li r27, 1 ; cmplwi r9, 5 ; bgt skip                                   # white (7) -> nothing
  jump table @ 0x4090 = [0x2c,0x40,0x54,0x18,0x7c,0x68] -> li r6, 6/5/4/3/2/1 ; 0x411c bl trace_laser(x, y, d, ~k & 7)
  ```
- `retrace_beams @ 0x415c`: `bl 0x3d5c; bl 0x3ce8; bl 0x3da4; bl 0x3e2c; bl 0x3fe8; bl 0x3f54; cmpwi r3,0; bne 0x4174; bl 0x3e9c; cmpwi r3,0; bne 0x416c`.
- `shut_down_complementors @ 0x3e9c` (0x3ef4–0x3f24): `lbz r10, 4(r2)` (in), `lbz r0, 0x14(r2)` (snap); `cmpw r0,r10; beq next;
  cmpwi r0,0; beq next; cmpwi r0,7; bne set; cmpwi r10,0; beq next; set: stb r4, 3(r9); li r3, 1`.
**Confidence:** confirmed for every row (code and simulator agree: 50/50 levels win; see Verification). The angle → colour assignment
(blue/green/red) of the multi-filter and the `d − 2` twister direction are each pinned down by the data as well (sensitivity runs below).

### L4. Termination
**Answer:**
- **Per laser trace: unchanged.** There are no visited marks and no cycle detection. A trace stops when a generation produces no beams or after
  **1023 generations** (`li r30, 0x3ff` @ 0x3a50, `addic. r30, r30, -1` @ 0x3ccc). Each generation holds **at most 32 beams**
  (`cmpwi cr7, r10, 0x20; beq` @ 0x33fc in `push_beam`), and extra pushes are dropped silently. Loops are drawn as steady beams.
  The limits apply per `trace_laser` call, and **each complementor emission is its own `trace_laser` call** with fresh lists, its own 1023/32
  limits and `entangle_counter = 1`.
- **New: the complementor pass** (L3) adds two loops around the per-laser traces:
  - Inner loop (snapshot, emit everything, repeat while some complementor input slot went from 0 to non-zero). Slots only ever go
    0 → non-zero once per outer pass, so it repeats at most 8·N + 1 times (N = live complementors).
  - Outer loop (clear, trace all lasers, inner loop, shut down contradicted complementors, repeat while anything was shut). Each repeat shuts
    at least one more complementor, so it runs at most N + 1 times. With no complementors it runs exactly once and behaves exactly like C1's
    `clear_beams + trace_all_lasers`.
- "Shuts down in all directions": see L3. The flag (cell `+3`) disables every output of that complementor until the next retrace, and it
  keeps absorbing beams.
- Neither the cap nor the generation limit is reached in any stored solution (simulator: 0 dropped pushes and 0 traces cut at 1023).
  The only shut-down in any stored solution is in slot 49 ("Feedback Eliminator", id 37, hint 0x76c8). There the complementor at (7,7) with dir 0
  gets red heading E and emits cyan E. The cyan comes back into the same input, which turns it white, so the complementor shuts down. On the
  second outer pass it acts as a block, and that is the intended solution.
- Drawing is unchanged from C1 (`draw_cell_beams` @ 0x6508 uses the same half-segment and centre-dot rules, at the new strides).
**Evidence:** addresses above. `trace_laser` loop end: `0x3cc4 cmpwi r0,0; beq 0x3cd4; 0x3ccc addic. r30,r30,-1`.
**Confidence:** confirmed.

### L5. Colour accumulation and the win condition
**Answer:** Unchanged from C1. `pinwheel_satisfied` @ 0x41a4: `colour == OR(in[0..7])`. `check_win` @ 0x4784 runs `retrace_beams()`
(now with the complementor pass), sets `level_won = 1`, clears it if any pinwheel in rows 0–14 is unsatisfied (with the same useless
8× inner loop), then applies `force_win` and, if `game_mode != 0 && level_won && !completed[current_slot]`, sets `completed`, then
`save_level_state` (0x590c), `write_save_file` (0x462c) and `update_unlocked` (0x4328). Black pinwheels need no light at all.
White needs R, G and B from any mix. Pinwheels don't block. Nothing new is tested: complementors, twisters and multi-filters
play no part in the test except through the beams they produce. A beam absorbed by a complementor still lights nothing beyond it.
**Evidence:** `check_win @ 0x4784`: `bl 0x415c` @ 0x479c, `cmpwi r0, 0xa` @ 0x47d0, `bl 0x41a4` @ 0x47dc. Decompiled:
`if (*force_win != 0) { *level_won = 1; *force_win = 0; }` and
`if (game_mode != 0 && level_won != 0 && completed[DAT_0000803c] == 0) { completed[..] = 1; FUN_0000590c(); FUN_0000462c(); FUN_00004328(); }`.
**Confidence:** confirmed.

### L6. Progress and unlocking
**Answer:** The storage model is as in C1: `completed[64]` @ 0x13fc0 and `unlocked[64]` @ 0x13eb0, both per **slot**. There are 50 slots
(strip 25 × 2; `next_level` caps the slot at 49, `if (current_slot < 0x31)`). `completed[]` is loaded from the save file,
`unlocked[]` is recomputed at load and after each first completion, and it is only ever set. **The rule changed:**
```
update_unlocked():                                     # 0x4328
    request_redraw(0, 0, 640, 480)
    k = number of slots s in 0..63 with completed[s] != 0      # a COUNT, not the highest index (C1)
    n = 5
    if k == 1 or k == 2: n = 4                         # (k − 1) unsigned < 2
    if k > 5:  n += 1
    if k > 19: n += 2
    if k > 29: n += 2
    if k > 39: n += 2
    for s in 0..63:
        if completed[s]: unlocked[s] = 1
        elif n > 0:      unlocked[s] = 1; n -= 1
```
So, besides every completed slot, the **n lowest-numbered uncompleted slots** are open:

| completed count k | 0 | 1–2 | 3–5 | 6–19 | 20–29 | 30–39 | ≥ 40 |
|---|---|---|---|---|---|---|---|
| uncompleted slots open (n) | 5 | 4 | 5 | 6 | 8 | 10 | 12 |

A fresh game opens slots 1–5 (0–4). Note the dip at k = 1–2, where n drops from 5 to 4: after the first win the number of open uncompleted
levels goes from 5 to 4, so the total open (completed + uncompleted) stays 5. With k ≥ 40 every remaining level is open (40 + 12 > 50).
Per-level board state works as in C1 but with 0x1f2c-byte copies: `save_level_state` @ 0x590c, `start_level(restore)` @ 0x5980
(restore → memcpy then `check_win`; otherwise `move_pieces_to_toolbox` @ 0x5810), `goto_slot` @ 0x5a2c. Restart (`r`) = `start_level(0)`.
Strip colours are the same offsets as C1 (`strip_pal_offset` @ 0x67bc: current 0, completed 0x48, unlocked 0x30, locked 0x10).
**Evidence:** `update_unlocked @ 0x4328` disassembly from 0x4384: `addi r0,r11,-1; li r9,5; cmplwi r0,1; bgt +8; li r9,4; cmpwi r11,5; ble; addi r9,r9,1;
cmpwi r11,0x13; ble; addi r9,r9,2; cmpwi r11,0x1d; ble; addi r9,r9,2; cmpwi r11,0x27; ble; addi r9,r9,2`. The count loop is 0x4354–0x4380
(`li r0,0x40; mtctr; … lwz r0,0(r2); addi r2,r2,-4; cmpwi r0,0; beq; addi r11,r11,1; bdnz`). Called from `load_save_file` (0x4428, at the end) and `check_win`.
**Confidence:** confirmed (code). The reason for the k = 1–2 dip is unknown (a design choice or a slip); playing the original would show it.

### L7. Interaction rules
**Answer:** Identical to C1 (C1 L7), apart from the cheat key and the 28-byte cell copy:
- `on_mouse_event` @ 0x31bc: event codes 1 (down), 3 (alt down, ignored), 4 (up), 6 (alt up), 0 (dragged). Alt = button 2, or primary with
  Shift/Option/Control (`& 0x1a00`) latched at mouse-down. The registered kinds are the same as C1: mouse down 1, up 2, dragged 6 (EventTypeSpecs at 0x7fbc),
  so moves arrive only with a button held.
- Hit test in `handle_mouse` @ 0x53ec, same constants: board `X/24 − 2`, `(Y+30)/24 − 2`; toolbox `(X−422)/26 − 1` in 0..5, `(Y+18)/26 − 1`
  in 0..3 → slot `col + 6·row` in row 15; strip `(X+10)/20 − 1` in 0..24, `(Y−390)/20 − 1` in 0..1 → slot `col + 25·row`.
- State machine (`drag_state` @ 0x123bc: 0 idle, 1 pressed, 2 dragging): same as C1. The drag starts after more than 1 px of movement over the board or
  toolbox (`begin_drag` @ 0x5264, which needs `is_movable`). A drop succeeds only onto a cell whose type is 0: it copies all **28** bytes and
  sets the source type to 0. Every other drop leaves the piece where it was. After either, `check_win_and_redraw` (0x522c) runs, `drag_state = 0`
  and `drag_image = −1`. Up without a drag on a movable board or toolbox piece rotates `dir − 1`. Up-6 with `drag_state == 0` rotates `dir + 1`.
  A strip click opens the slot via `goto_slot` if `unlocked[slot]`. C1's stale-`drag_state` quirk is still there (the reset sits inside `if (is_movable)`).
  There is no delete; a piece is removed by dragging it back to an empty toolbox slot. Twisters can't be dragged or rotated, because `is_movable` is false.
- Keys (`handle_key` @ 0x617c, ignored when Cmd is held, `on_key_event` @ 0x2d28 tests `& 0x100`): Space, `=` or `+` → `next_level` if
  `unlocked[current_slot+1]`; `-` → `prev_level`; `r` → `start_level(0)`; **`z` (0x7a) → `force_win = 1; check_win()`** (in C1 this was `L`, 0x4c);
  Esc is swallowed. The menu (`on_menu_command` @ 0x2b6c) does the same as C1: `Next` → `=`, `Prev` → `-`, `rvrt` → `r`, `copy`/`past` (clipboard,
  now with a `2-NN-` prefix, see platform/data notes), and `Whee` toggles the wheel preference. The wheel (`on_wheel_event` @ 0x2f14) maps delta > 0 to `-` and anything else to `=`.
**Evidence:** `handle_key @ 0x617c` disassembly: `cmpwi r3,0x2d` @ 0x617c, `0x20` @ 0x61a0, `0x1b` @ 0x61ac, `0x2b` @ 0x61bc, `0x72` @ 0x61c4,
`0x3d` @ 0x61d0, **`0x7a` @ 0x61dc** → `bl check_win (0x4784)` @ 0x6204. Drop copy in `handle_mouse`: 7 words, `puVar9[iVar11] = 0`.
**Confidence:** confirmed. The `game_mode == 0` branch of `handle_mouse` is still unreachable after startup, and its purpose is unknown (as in C1).

### L8. What happens on a win
**Answer:** As in C1, nothing automatic happens: no advance, timer, animation or sound. On the redraw after the winning move (`paint_window` @ 0x69fc):
- **"You win!"** in rect left 330, top 385, right 400, bottom 420. If the level is un-solved later, **"(won)"** at left 350, because `completed` stays set.
- The hint box (450, 125, 620, 475) shows **"Click on a level or press spacebar for next."** in place of the hint.
- Pinwheels show their lit sprites. The first win sets `completed`, saves the board and the save file, and recomputes unlocks (L6).

New in C2 (not tied to winning): the **level name** (copied by `load_level_data` from the name table at 0xaa08 into 0x16000) is always drawn
at left 50, top 385, right 300, bottom 420 while `game_mode == 2`. When `game_mode != 2` it's drawn in C1's `mode_text` box
(350, 420, 450, 460), which never happens after startup. C1's "freeware" and "more levels @" lines are gone. Instead
**"Thank you for purchasing this game."** is drawn at (0, 450, 400, 480) whenever `current_slot % 6 == 4` (slots 5, 11, 17, 23, 29, 35, 41, 47),
won or not. The site-URL text is unchanged (rect 470, 450, 640, 480).
**Evidence:** `paint_window @ 0x69fc` (decompiled, condensed):
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
```
(`draw_text_box(str, left, top, right, bottom)` @ 0x30f8.)
**Confidence:** confirmed.

## Verification
```bash
BIN="chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2"
D=re/out/chromatron2/decompiled.c

# Names applied (after the controller re-runs the decompile with logic.names.tsv)
grep -c -E '^// (is_movable|push_beam|beam_enter_cell|trace_laser|clear_beams|clear_complementor_flags|trace_all_lasers|snapshot_cell_inputs|shut_down_complementors|complementor_new_input|emit_complementors|retrace_beams|pinwheel_satisfied|update_unlocked|check_win|check_win_and_redraw|begin_drag|handle_mouse|save_level_state|goto_slot|slot_next_id|slot_prev_id|next_level|prev_level|handle_key|on_menu_command|on_key_event|on_wheel_event|on_mouse_event) @ ' $D
# 29

# beam_enter_cell type jump table @ 0x34dc (types 0..15; offsets from 0x34dc): 13 -> 0x42c (0x3908 multi-filter),
# 14 -> 0x4a4 (0x3980 twister), 15 -> 0x4f0 (0x39cc absorb, same as wall/laser)
r2 -e scr.color=0 -qc 'pxw 64 @ 0x34dc' "$BIN" </dev/null 2>/dev/null
# 0x000034dc  0x000004ac 0x000004f0 0x000004f0 0x00000040  ...............@
# 0x000034ec  0x00000060 0x0000025c 0x000000a8 0x000001e0  ...`...\........
# 0x000034fc  0x00000300 0x00000294 0x000004ac 0x000003a8  ................
# 0x0000350c  0x000003bc 0x0000042c 0x000004a4 0x000004f0  .......,........

# is_movable: types 3..9, 13, 15
r2 -e scr.color=0 -qc 'pi 13 @ 0x33a8' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r3, 0xd
# beq cr7, 0x33cc
# bgt cr7, 0x33c4
# addi r0, r3, -3
# cmplwi cr7, r0, 6
# bgt cr7, 0x33d4
# b 0x33cc
# cmpwi cr7, r3, 0xf
# bne cr7, 0x33d4
# li r3, 1
# blr
# li r3, 0
# blr

# Piece 1, SPLITTER (changed): absorb r=0/4, push straight with tag 0, then reflect (table) with tag 0
r2 -e scr.color=0 -qc 'pi 8 @ 0x37dc' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r29, 0
# beq cr7, 0x39cc
# cmpwi cr7, r29, 4
# beq cr7, 0x39cc
# mr r4, r28
# li r7, 0
# bl 0x33dc
# clrlwi r2, r24, 0x1d
# reflection table indexed by r-1 (offsets from 0x3824): 0x3840 +7, 0x3874 +6, 0x3848 +5, 0x39cc none, 0x3864 +3, 0x387c +2, 0x386c +1
r2 -e scr.color=0 -qc 'pxw 28 @ 0x3824' "$BIN" </dev/null 2>/dev/null
# 0x00003824  0x0000001c 0x00000050 0x00000024 0x000001a8  .......P...$....
# 0x00003834  0x00000040 0x00000058 0x00000048             ...@...X...H

# Piece 2, MULTI-FILTER: table on r (offsets from 0x392c): r%4==0 -> 0x3974 (blue), 1 -> 0x3960 (green), 2 -> 0x394c (red), 3 -> 0x3988 (pass)
r2 -e scr.color=0 -qc 'pxw 32 @ 0x392c' "$BIN" </dev/null 2>/dev/null
# 0x0000392c  0x00000048 0x00000034 0x00000020 0x0000005c  ...H...4... ...\
# 0x0000393c  0x00000048 0x00000034 0x00000020 0x0000005c  ...H...4... ...\

# Piece 3, TWISTER: out = (d - 2) & 7, dir ignored
r2 -e scr.color=0 -qc 'pi 2 @ 0x3980' "$BIN" </dev/null 2>/dev/null
# addi r5, r5, -2
# clrlwi r5, r5, 0x1d

# Piece 4, COMPLEMENTOR emission: parity test, white skipped, complement table -> trace_laser
r2 -e scr.color=0 -qc 'pi 9 @ 0x4050' "$BIN" </dev/null 2>/dev/null
# lbz r0, 2(r11)
# clrlwi r2, r30, 0x1f
# clrlwi r0, r0, 0x1f
# cmpw cr7, r2, r0
# bne cr7, 0x4120
# addi r9, r9, -1
# li r27, 1
# cmplwi cr7, r9, 5
# bgt cr7, 0x4120
# colour-1 -> target (offsets from 0x4090): 1->0x40bc(li r6,6) 2->0x40d0(5) 3->0x40e4(4) 4->0x40a8(3) 5->0x410c(2) 6->0x40f8(1)
r2 -e scr.color=0 -qc 'pxw 24 @ 0x4090' "$BIN" </dev/null 2>/dev/null
# 0x00004090  0x0000002c 0x00000040 0x00000054 0x00000018  ...,...@...T....
# 0x000040a0  0x0000007c 0x00000068                        ...|...h

# retrace_beams: flags, (clear, lasers, (snapshot, emit) until no new input) until no shut-down
r2 -e scr.color=0 -qc 'pi 11 @ 0x4168' "$BIN" </dev/null 2>/dev/null
# bl 0x3d5c
# bl 0x3ce8
# bl 0x3da4
# bl 0x3e2c
# bl 0x3fe8
# bl 0x3f54
# cmpwi cr7, r3, 0
# bne cr7, 0x4174
# bl 0x3e9c
# cmpwi cr7, r3, 0
# bne cr7, 0x416c

# Shut-down test (snap = +0x14, in = +4): changed && snap != 0 && !(snap == 7 && in == 0) -> +3 = 1
r2 -e scr.color=0 -qc 'pi 8 @ 0x3f00' "$BIN" </dev/null 2>/dev/null
# cmpw cr7, r0, r10
# beq cr7, 0x3f28
# cmpwi cr7, r0, 0
# beq cr7, 0x3f28
# cmpwi cr7, r0, 7
# bne cr7, 0x3f20
# cmpwi cr7, r10, 0
# beq cr7, 0x3f28

# Termination unchanged: tag 0 on the laser push, 1023 generations, 32-beam cap
r2 -e scr.color=0 -qc 'pi 1 @ 0x3a04; pi 1 @ 0x3a50; pi 1 @ 0x3ccc; pi 2 @ 0x33fc' "$BIN" </dev/null 2>/dev/null
# li r7, 0
# li r30, 0x3ff
# addic. r30, r30, -1
# cmpwi cr7, r10, 0x20
# beq cr7, 0x3450

# dir tables and doppler maps unchanged (dx @0x8044, dy @0x8064, back @0x8084, fwd @0x8098)
xxd -s 0x7044 -l 0x68 "$BIN"
# 00007044: 0000 0000 0000 0001 0000 0001 0000 0001  ................
# 00007054: 0000 0000 ffff ffff ffff ffff ffff ffff  ................
# 00007064: ffff ffff ffff ffff 0000 0000 0000 0001  ................
# 00007074: 0000 0001 0000 0001 0000 0000 ffff ffff  ................
# 00007084: 0000 0000 0000 0002 0000 0004 0000 0000  ................
# 00007094: 0000 0001 0000 0000 0000 0004 0000 0001  ................
# 000070a4: 0000 0000 0000 0002                      ........

# Unlock thresholds (count-based)
r2 -e scr.color=0 -qc 'pi 17 @ 0x4384' "$BIN" </dev/null 2>/dev/null
# addi r0, r11, -1
# li r9, 5
# cmplwi cr7, r0, 1
# bgt cr7, 0x4398
# li r9, 4
# cmpwi cr7, r11, 5
# ble cr7, 0x43a4
# addi r9, r9, 1
# cmpwi cr7, r11, 0x13
# ble cr7, 0x43b0
# addi r9, r9, 2
# cmpwi cr7, r11, 0x1d
# ble cr7, 0x43bc
# addi r9, r9, 2
# cmpwi cr7, r11, 0x27
# ble cr7, 0x43c8
# addi r9, r9, 2

# Cheat key is 'z' (0x7a)
r2 -e scr.color=0 -qc 'pi 1 @ 0x61dc' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r3, 0x7a

# Level/name/hint tables and level_order used by the simulator (nl_symbol_ptr slots)
r2 -e scr.color=0 -qc 'pxw 4 @ 0x12388; pxw 4 @ 0x12380; pxw 4 @ 0x12384; pxw 4 @ 0x12328' "$BIN" </dev/null 2>/dev/null
# 0x00012388  0x0000a934                                   ...4
# 0x00012380  0x0000aa08                                   ....
# 0x00012384  0x0000aadc                                   ....
# 0x00012328  0x00008110                                   ....
```

**Simulator check** (throwaway Python, `/private/tmp/claude-503/c2sim/`, not committed). It decodes the level table @ 0xa934 itself:
53 pointers, id 0 NULL, 5-byte records `type, dir, colour, x, y`, terminated by type 0, as in C1. It uses `level_order` @ 0x8110 (50 slots)
and leaves every movable piece at its stored x/y/dir. It runs `retrace_beams` exactly as written in L3 (C1 tracer, the new splitter,
multi-filter, twister, the complementor pass with snapshot/new-input/shut-down loops, the 32 cap, 1023 generations, the entanglement pass) and then the L5 test:
- **50 / 50 playable levels are won** from their stored placement. Unlike C1 there is no data exception.
- **0 / 50** are won with all movable pieces removed.
- The 2 unreachable ids are also won from their stored placement: 24 ("Splitting Outside the Box", 66 records, 18 movable) and 52 ("level052", 8 records, placeholder hint "blah").
- The 32-beam cap and the 1023-generation limit are never hit. There are 32 complementor emissions in total and **1 shut-down** (slot 49, L4).
- Beam entries per piece type in the stored solutions: mirror 206, bender 228, filter 11, prism 16, doppler 30, splitter 48, tangler 10,
  conduit 45, teleporter 37, pinwheel 515, **multi-filter 52, twister 87, complementor 36**. Head-on splitter hits (the new back-reflection)
  occur in slots 12, 23, 24, 33 and 47. Multi-filter "pass-all" (r ≡ 3) hits occur in slots 17 and 45. Tagged beams reach a complementor in slot 39,
  a twister in slots 22 and 42, and a multi-filter in slots 17 and 46.
- Sensitivity (each variant against the same 50 levels):
  - C1's splitter rule → 47 won (fails slots 12, 23, 47);
  - twister `d + 2` instead of `d − 2` → 37 won;
  - multi-filter absorbing at r ≡ 3 → 48 won (slots 17, 45 fail);
  - multi-filter with red and blue swapped → 40 won;
  - no complementor emission → 39 won;
  - no shut-down pass → 49 won (slot 49 fails);
  - twister clearing the tag → 49 won (slot 42 fails);
  - multi-filter clearing the tag → 49 won (slot 17 fails).

  Not pinned by the data (code only): the splitter clearing the tag at r = 2/6 (keeping it still wins 50/50). For the
  complementor parity rule: ignoring it still wins 50/50, but inverting it (even dir = diagonals) wins only 38/50, so the
  orientation is pinned.
