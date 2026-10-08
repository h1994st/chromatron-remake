# Chromatron 1 v1.15 — Game logic

decompiled.c line numbers valid for Ghidra 12.1.4 with re/notes/chromatron1/*.names.tsv as of this commit.

Binary: `chromatron_mac/Chromatron.app/Contents/MacOS/chromatron1` (Mach-O, big-endian PowerPC).
All addresses are vmaddrs. File offset = vmaddr − 0x1000. Function and data names are the ones in
`data.names.tsv` and `logic.names.tsv` (applied by `just decompile 1`). Line numbers refer to
`re/out/chromatron1/decompiled.c` as produced by `just decompile 1` with both name files.

This builds on `data.md`: the cell layout (`type`, `colour`, `dir`, `in[8]` @ +4, `out[8]` @ +12),
the colour mask (1 = B, 2 = G, 4 = R), the direction encoding (0–7 = N, NE, E, SE, S, SW, W, NW,
clockwise, `dir_dx` @ 0x8044 / `dir_dy` @ 0x8064, y grows downwards) and the slot/level-id model.

Notation used below:
- `p` = the piece's `dir` byte, `d` = the beam's travel direction, `c` = the beam's colour mask,
  `t` = the beam's entanglement tag (0 = not entangled).
- **`r = (d − p) & 7`**, the beam direction relative to the piece. Every rule is written in `r`.
- `+k` in an output column means output direction `(p + k) & 7`.
- "same" means the beam keeps going in direction `d`.

Globals reached through `__nl_symbol_ptr` (`r2 -qc 'pxw 4 @ <slot>'`), in addition to those in data.md:

| slot | → target | name | meaning |
|---|---|---|---|
| 0x12d2c | 0x16414 | `cursor_row` | int, last pressed board row (15 = toolbox) |
| 0x12d30 | 0x16410 | `cursor_col` | int, last pressed column / toolbox slot |
| 0x12d44 | 0x13c74 | `game_mode` | int, 0 at launch, set to 2 by every `start_level` |
| 0x12d48 | 0x14990 | `force_win` | int, set by the `L` key |
| 0x12d4c | 0x14aa0 | `level_won` | int, result of the last win check |
| 0x12d50 | 0x16210 | `saved_states` | `u8*[128]`, per level id, 0x1644-byte board copies |
| 0x12d54 | 0x14890 | `unlocked` | `int[64]`, per slot |
| 0x12d58 | 0x149a0 | `completed` | `int[64]`, per slot |
| 0x12d5c | 0x13a60 | `beam_cur_count` | int |
| 0x12d60 | 0x13b70 | `beam_buf_b` | 32 × 8-byte beam entries |
| 0x12d64 | 0x13a70 | `beam_buf_a` | 32 × 8-byte beam entries |
| 0x12d68 | 0x13a5c | `beam_cur_list` | pointer to the list being processed |
| 0x12d70 | 0x16200 | `beam_next_list` | pointer to the list being filled |
| 0x12d74 | 0x16204 | `beam_next_count` | int |

Directly addressed: `entangle_counter` @ 0x12dc8, `drag_state` @ 0x12db0, `mouse_alt_button` @ 0x12da8,
`press_x`/`press_y` @ 0x139d8/0x139dc, `drag_image` @ 0x80d0, `doppler_back_map` @ 0x8084,
`doppler_fwd_map` @ 0x8098.

## Findings

### L1. Piece kinds, movable and rotatable
**Answer:** The piece enum is the cell `type` byte (data.md D2):

| type | piece | colour byte | movable | rotatable | beam behaviour (L3) |
|---|---|---|---|---|---|
| 0 | empty floor | – | – | – | passes |
| 1 | wall | – | no | no | absorbs |
| 2 | laser (emitter), colour 1/2/4/7 = blue/green/red/white | yes | no | no | emits in `dir`; absorbs incoming |
| 3 | mirror ("REFLECTOR") | – | yes | yes | one-sided reflector |
| 4 | bender | – | yes | yes | one-sided reflector at a half step |
| 5 | filter, colour 1/2/4 = blue/green/red | yes | yes | yes | colour mask, straight on its axis |
| 6 | prism | – | yes | yes | colour-dependent bend |
| 7 | doppler | – | yes | yes | colour shift (+ entanglement) |
| 8 | splitter | – | yes | yes | pass + reflect |
| 9 | quantum tangler | – | yes | yes | perpendicular entangled pair |
| 10 | pinwheel (target), required mask 0–7: 0 black, 1 blue, 2 green, 3 cyan, 4 red, 5 magenta, 6 yellow, 7 white | yes | no | no | passes (records colour) |
| 11 | conduit, axis `dir & 3` | – | no | no | passes only along its axis |
| 12 | teleporter | – | no | no | jumps to the next teleporter |

Movable = rotatable = `is_movable(type)` = `type − 3 < 7` (unsigned), i.e. exactly types 3–9.
Every drag, drop and rotate goes through this check (L7). Fixed kinds (walls, lasers, pinwheels,
conduits, teleporters) can't be moved or rotated. In C1 every movable piece starts in the toolbox
(data.md D3), so there are no fixed movable pieces. Type values above 12 hit the
`default:` assertion in `beam_enter_cell` (game.c line 0x1a6); no level contains one.
**Evidence:** `is_movable @ 0x34d8`: `return param_1 - 3U < 7;` (line 713). `beam_enter_cell @ 0x3568` switches on
the type (lines 764–977): cases 0/10, 1/2, 3, 4, 5, 6, 7, 8, 9, 0xb, 0xc, default assert.
The jump table at 0x35ec has 13 entries (types 0–12); `cmplwi r11,0xc; bgt` sends higher types to the assert.
**Confidence:** confirmed.

### L2. Direction encoding and rotation
**Answer:** 8 directions, `0..7` = N, NE, E, SE, S, SW, W, NW (45° steps clockwise; y grows downwards),
stored in the cell's `dir` byte. Lasers can face any of the 8. A rotation step is one unit (45°):
- **plain left click** (press and release without dragging) on a movable piece: `dir = (dir − 1) & 7`,
  i.e. **counter-clockwise** 45°;
- **right click**, or a left click with **Shift, Option or Control** held (`modifiers & 0x1a00`):
  `dir = (dir + 1) & 7`, i.e. **clockwise** 45°.

Rotation works on pieces on the board **and in the toolbox**. Pieces moved to the toolbox at level
start get `dir = 0`. A piece keeps its `dir` when it is dragged between board and toolbox.
Splitters, filters and conduits behave (and are drawn) by `dir & 3`, because their rules depend
only on `r mod 4` (L3), so their 8 rotations come in 4 identical pairs.
**Evidence:** `handle_mouse @ 0x50d4` lines 2144–2145 (`dir + -1`, `& 7`, on event 4) and 2167–2168 (`dir + '\x01'`, `& 7`, on event 6);
`on_mouse_event @ 0x32ec` lines 651–688 map the Carbon events to these codes (L7).
**Confidence:** confirmed (from code). That "−1" looks counter-clockwise on screen follows from 0 = N and clockwise numbering (data.md).

### L3. Beam tracer and transfer table
**Answer:**

**Data structures.** A beam is an 8-byte entry `{u8 x, u8 y, u8 dir, u8 colour, i32 tag}` and means
"a beam of `colour` sitting in cell (x, y) that will leave it in direction `dir`". There are two
32-entry lists (`beam_buf_a` @ 0x13a70, `beam_buf_b` @ 0x13b70, 0x100 bytes each). They swap
every generation through `beam_cur_list` / `beam_next_list`. `push_beam(x, y, dir, colour, tag)`
appends to the next list **only if it holds fewer than 32 entries**. Otherwise the beam is silently dropped.
Each cell records colour masks by travel direction: `in[d]` (+4+d) for beams that entered it
moving in direction d, and `out[d]` (+12+d) for beams that left it moving in direction d.

**Algorithm.**
```
retrace_beams():                                 # 0x3ea4 = clear_beams + trace_all_lasers
    for all 15×15 board cells: in[0..7] = out[0..7] = 0
    for y in 0..14: for x in 0..14:              # row-major, y outer
        if type(x,y) == 2: trace_laser(x, y, dir(x,y), colour(x,y))

trace_laser(x, y, d, c):                         # 0x3a58, each laser traced on its own
    cur = beam_buf_a; next = beam_buf_b; next_count = 0
    entangle_counter = 1
    push_beam(x, y, d, c, tag=0)                 # starts in the laser's own cell
    gens = 1023
    while true:
        swap(cur, next); cur_count = next_count; next_count = 0
        for b in cur[0 .. cur_count-1] (in order):
            out(b.x, b.y)[b.dir] |= b.colour
            b.x = (b.x + dx[b.dir]) & 0xff; b.y = (b.y + dy[b.dir]) & 0xff
            if b.x < 15 and b.y < 15:            # leaving the board ends the beam
                beam_enter_cell(b.x, b.y, b.dir, b.colour, b.tag)
        entanglement_pass(next)                  # see "Entanglement" below
        if next_count == 0: break
        gens -= 1; if gens == 0: break

beam_enter_cell(x, y, d, c, t):                  # 0x3568
    in(x, y)[d] |= c                             # always, for every cell type
    r = (d − dir(x,y)) & 7
    push the outputs from the transfer table, at (x, y)   # teleporter: at the target cell
```

**Transfer table** (outputs are pushed in the listed order; that order matters only when the
32-beam cap is reached). Unless stated otherwise an output keeps colour `c` and tag `t`; "–" = no output (the beam is absorbed).

| type | r=0 | r=1 | r=2 | r=3 | r=4 | r=5 | r=6 | r=7 |
|---|---|---|---|---|---|---|---|---|
| 0 empty, 10 pinwheel | same | same | same | same | same | same | same | same |
| 1 wall, 2 laser | – | – | – | – | – | – | – | – |
| 3 mirror | – | +7 | +6 | +5 | – | – | – | – |
| 4 bender | +7 | +6 | +5 | +4 | – | – | – | – |
| 5 filter (colour f) | – | – | same, `c & f` | – | – | – | same, `c & f` | – |
| 6 prism | see below | | | | | | | |
| 7 doppler | – | – | same, `fwd(c)` | – | – | – | same, `back(c)` | – |
| 8 splitter | – | same `t=0`; +7 `t=0` | same | same `t=0`; +5 `t=0` | – | same `t=0`; +3 `t=0` | same | same `t=0`; +1 `t=0` |
| 9 tangler | – | – | pair (below) | – | – | – | – | – |
| 11 conduit | same | – | – | – | same | – | – | – |
| 12 teleporter | jump | jump | jump | jump | jump | jump | jump | jump |

Notes on each row:
- **Mirror**: `out = (2p − d) & 7`, a reflection across a line along direction `p`, accepted only from one side
  (incoming `d ∈ {p+1, p+2, p+3}`). At r = 2 the beam hits the face square-on and goes straight back (`out = d + 4`).
  Every accepted path also works in reverse.
- **Bender**: `out = (2p + 7 − d) & 7`, a reflection across a line at `p + 3.5` steps (22.5° off the grid),
  accepted only for `d ∈ {p, p+1, p+2, p+3}`. It turns orthogonal beams diagonal and vice versa
  (r = 0: −45°, r = 1: −135°, r = 2: +135°, r = 3: +45°).
- **Filter**: passes straight only along its axis (r = 2 or 6), with colour `c & f`. If the result is 0
  there is no output. Other angles absorb.
- **Prism** (`p` uses all 8 values). Each colour bit present in `c` is handled separately, in the order
  **red, green, blue**. Each output carries just that one bit and tag `t`:

  | bit | r=0 | r=1 | r=2 | r=3 | r=4 | r=5 | r=6 | r=7 |
  |---|---|---|---|---|---|---|---|---|
  | R (4) | – | +1 (same) | +2 (same) | – | – | +5 (same) | +6 (same) | – |
  | G (2) | +1 | – | +3 | – | – | +4 | – | +6 |
  | B (1) | +6 | – | +4 | – | – | +3 | – | +1 |

  White in at r = 2 fans out into R straight, G turned +45° and B turned +90° (and at r = 5: R straight,
  G −45°, B −90°). Running the fan backwards merges: R at r = 6, G at r = 7 and B at r = 0 all leave at +6.
  A bit with "–" for that r is dropped (e.g. yellow in at r = 1 gives only a red beam).
- **Doppler**: r = 2 is "forward" and r = 6 "backwards"; other angles absorb. The shift is bitwise:
  `fwd(c)`: R→G, G→B, B→R (4→2, 2→1, 1→4), and `back(c)`: R→B, G→R, B→G (the inverse).
  For multi-bit masks each bit maps separately (e.g. fwd(yellow 6) = cyan 3, fwd(white) = white).
  **If `t ≠ 0` the handler outputs `c` unchanged**, and the shift is applied by the entanglement pass instead.
- **Splitter** (depends only on `p & 3`): r = 0 or 4 (beam parallel to the surface) is absorbed. r = 2 or 6 (head-on)
  passes straight and **keeps** its tag. At r odd (45°) it outputs two beams, in this order: straight
  through with `t = 0`, then reflected (`out = (2p − d) & 7`, same as the mirror) with `t = 0`.
  So entanglement collapses only on a 45° hit.
  The slot-29 tutorial hint (0x6ff8: "When a quantum-entangled beam goes through a splitter … the quantum wave function
  collapses") is broader than the code, where a head-on splitter pass keeps the tag. The port follows the code.
  A "merge" (hint text) isn't a separate rule: two beams that meet on the same path both continue, and every
  reader ORs the colour masks.
- **Tangler**: only r = 2 accepted (beam travelling `p + 2`). For each colour bit present, in the order
  **B (1), G (2), R (4)**, it pushes `(dir p, bit, tag k)` and then `(dir p + 4, bit, tag k)`, with
  `k = entangle_counter++`. That's two perpendicular beams in opposite directions per colour, and each pair
  gets a fresh tag. The incoming tag is discarded, so passing a tangler collapses the old entanglement.
- **Conduit** (axis `p & 3`): passes only r = 0 or 4, otherwise absorbs.
- **Pinwheel** and **empty**: always pass straight. **Pinwheels don't block beams.** They only record
  `in[d]`, so a beam can light several pinwheels in a row.
- **Teleporter**: from the teleporter cell, step `(dx[d], dy[d])` repeatedly. Skip every cell (walls included)
  until a cell of type 12 is found, then push `(target x, target y, d, c, t)` there. If the walk leaves the board
  first, the beam vanishes. The target teleporter's handler isn't run (no chaining, and no `in[]` mark at the target).
  On the next generation the beam leaves the target in direction `d` (marking `out[d]` there).
  The jump takes a single generation, whatever the distance.

**Entanglement pass** (runs after every generation, over the *next* list):
```
for i in 0 .. next_count-1:
    bi = next[i]
    if bi.x < 15 and bi.y < 15 and bi.tag != 0 and type(bi.x, bi.y) == 7:      # just left a doppler
        forward = ((bi.dir − dir(bi.x, bi.y)) & 7) == 2
        for j in 0 .. next_count-1:
            if next[j].tag == bi.tag:
                map = doppler_fwd_map if ((i == j) == forward) else doppler_back_map
                next[j].colour = map[next[j].colour]
```
`doppler_fwd_map` @ 0x8098 = int32 `[0, 4, 1, 0, 2]` (B→R, G→B, R→G = `fwd`) and
`doppler_back_map` @ 0x8084 = int32 `[0, 2, 4, 0, 1]` (B→G, G→R, R→B = `back`), indexed by mask 0–4
(the low byte of each int is read). The entangled beam itself gets the normal doppler shift, and
**every other beam with the same tag (its twin) gets the opposite shift in the same generation**. Tagged beams always
carry one colour bit, so the index is always 1, 2 or 4. Twins are pushed together and advance one cell per
generation, so the twin changes colour when it is the same number of steps from the tangler. It changes
"in place": its current cell has already recorded the old colour in `in[]`, so a pinwheel the twin is
standing on sees the old colour. The new colour applies from that cell's `out[]` onwards.
If several entangled beams change in one generation, the shifts compose. They commute, so order doesn't matter.

**Evidence:** `beam_enter_cell @ 0x3568` (lines 742–980). Mirror `case 3` at lines 771–791
(`uVar9 + 6` / `+ 7` / `+ 5` for r = 2/1/3). Bender `case 4` at lines 792–805 (r = 1 → +6, r = 0 → +7, r = 2 → +5, r = 3 → +4).
Prism `case 6` at lines 815–875 (red at lines 816–831, green at 834–849, blue at 852–874). Doppler `case 7` at lines 876–904.
Splitter `case 8` at lines 905–937. Tangler `case 9` at lines 938–951. Conduit `case 0xb` at lines 952–955.
Teleporter `case 0xc` at lines 958–973. Filter `case 5` at lines 806–814.
```c
  case 9:                                             /* lines 938-951 */
    if (uVar3 == 2) {
      uVar8 = 1;
      do {
        if ((uVar8 & param_4) != 0) {
          push_beam(param_1,param_2,uVar9 & 7,uVar8,entangle_counter);
          push_beam(param_1,param_2,uVar9 + 4 & 7,uVar8,entangle_counter);
          entangle_counter = entangle_counter + 1;
        }
        uVar8 = (uVar8 & 0x7fffffff) << 1;
      } while ((int)uVar8 < 5);
```
`trace_laser @ 0x3a58`: the initial push gets `li r7, 0` (tag 0) at 0x3a7c. Out-marking and the move are at lines 1033–1046, and the
entanglement pass is at lines 1062–1077 (`(iVar15 == iVar14) == ((cVar2 - cVar1 & 7U) == 2)` → `doppler_fwd_map`, else `doppler_back_map`).
`push_beam @ 0x34ec`: `if (iVar1 != 0x20)` at line 729. The disassembly confirms the prism, splitter-tag and teleporter branches
(`pd` at 0x368c, 0x38e4, 0x39b4). The teleporter's x bound uses `cmplwi` (unsigned) and its y bound `cmpwi` (signed, `< 0` and `> 14`).
**Confidence:** confirmed. The simulator in Verification solves 49 of the 50 playable levels and all 12 unreachable ones from
their stored placements. The one failure is a data exception (slot 16), not a rule gap.

### L4. Termination
**Answer:** There are **no visited marks** and no cycle detection. A laser's trace ends when either:
1. a generation produces no beams (every beam was absorbed, left the board, or found no teleporter target), or
2. **1023 generations** have run (`gens = 0x3ff`, decremented after each generation).

In addition, each generation holds **at most 32 beams**, and pushes beyond 32 are dropped silently (in push order).
So a loop (for example a ring of mirrors, or a splitter feeding itself) just keeps re-marking the same cells until
the 1023-generation limit. Because the marks are ORed colour masks, a loop is **drawn as a steady beam** with nothing
special: no flashing, no truncation marker. Beams that run out of the cap or the limit stop where they are.
The final generation's beams never get their `out[]` mark. The limits are per laser (each laser gets its own trace,
fresh lists and `entangle_counter = 1`). Neither limit is reached in any stored level solution (simulator).

Drawing (render detail, for reference): for each cell and side `s` the half-segment from the cell centre towards side `s`
is drawn in `beam_colors[out[s] | in[(s+4)&7]]` (data.md D6). Centre dot (`draw_cell_beams` @ 0x61ec, formerly
`FUN_000061ec`, lines 2778–2829): for every side `s` whose segment is non-empty (`out[s] | in[(s+4)&7] != 0`) **and**
`in[s] == out[s]` (this includes `in[s] == out[s] == 0`), axis `s & 3` is counted and `in[s]` is ORed into the dot colour.
If the dot colour is non-zero and two or more distinct axes were counted, one pixel at the cell centre is drawn in
`beam_colors[dot colour]`. A cell that has any beam gets its sprite redrawn on top with `min_index = 0x10`
(platform.md P2 has the pixel geometry).
**Evidence:** `trace_laser`: `lVar17 = 0x3ff;` (line 1013) and `} while ((*(int *)PTR_beam_next_count_00012d74 != 0) && (lVar17 = lVar17 + -1, lVar17 != 0));` (line 1086).
Disassembly: `li r30, 0x3ff` @ 0x3ac8 and `addic. r30, r30, -1` @ 0x3d44. `push_beam`: `cmpwi cr7, r10, 0x20; beq` @ 0x350c (line 729).
**Confidence:** confirmed.

### L5. Colour accumulation and the win condition
**Answer:** A pinwheel accumulates the OR of every beam that **enters** its cell, from any direction and any laser:
`got = in[0] | in[1] | … | in[7]`. It is **satisfied iff `got == colour`** (exact equality, `pinwheel_satisfied @ 0x3ecc`). So:
- red (4) needs some red and **no** green or blue (red + yellow = 6 ≠ 4 fails; red from two beams is fine);
- white (7) needs at least one each of R, G and B, from any mix of beams (e.g. one white beam, or yellow + blue);
- **black (0) is satisfied only if no light at all enters it**. That includes beams passing through, since pinwheels don't block.

A satisfied pinwheel is drawn with its "lit" sprite (`cell_image_index` +1, data.md D2), level won or not.

`check_win @ 0x4494` (run after every drop, rotate, paste, restart and level start):
```
retrace_beams()
level_won = 1
for each board cell (rows 0–14) with type 10: if !pinwheel_satisfied(cell): level_won = 0
if force_win: level_won = 1; force_win = 0              # 'L' key cheat
if game_mode != 0 and level_won and !completed[current_slot]:
    completed[current_slot] = 1
    save_level_state(); write the save file (FUN_0000433c); update_unlocked()
```
The level is won when **every** pinwheel is satisfied. Lasers, toolbox contents and unused pieces don't matter.
(The loop at lines 1489–1499 tests each cell 8 times, an inner `lVar6 = 7..0` loop that changes nothing.)
**Evidence:** `pinwheel_satisfied` lines 1196–1216 (`return *(byte *)(param_1 + 1) == bVar3;` at line 1215, `bVar3` = OR of +4..+11).
`check_win` lines 1468–1520. Hint 0x75d8 (id 43, slot 40, `hint_table` entry 0xb7ac): "If there are any black pinwheel targets, you don't win the level until none of the black pinwheels are being hit by lasers of any color."
Hint 0x752c: "The white pinwheel requires at least one each of red, blue, and green."
**Confidence:** confirmed.

### L6. Progress and unlocking
**Answer:** Progress is kept per **slot** (0-based strip position, data.md) in `completed[64]` @ 0x149a0 and `unlocked[64]` @ 0x14890.
- **Initial state**: with no save file, `completed[]` is all 0. `update_unlocked()` then opens slot 0 only. The game always starts
  on slot 0 (`current_slot` isn't saved). With a save file, `completed[]` is read from it (platform.md) and `unlocked[]` is
  recomputed. `unlocked[]` itself is never saved.
- **`update_unlocked()` @ 0x4050** (at load and after each first-time completion):
  ```
  h = highest slot index with completed[h] != 0, or −1
  n = 1 + (h > 8) + (h > 18) + (h > 25) + 2·(h > 34)
  for s in 0..63:
      if completed[s]: unlocked[s] = 1
      elif n > 0:      unlocked[s] = 1; n -= 1
  ```
  So every completed slot is open, plus the **n lowest-numbered uncompleted slots**. With the highest completed slot shown
  as 1–9 that's n = 1 open level; 10–19 gives 2; 20–26 gives 3; 27–35 gives 4; 36 or more gives 6. `unlocked[]` is only ever set, never cleared.
- **Strip colours** (data.md D6): current slot grey, completed blue, unlocked green, locked red. Hint 0x7460 (slot 11):
  "You can now try two different levels…" matches n = 2 once slot 10 (index 9) is done.
- **Per-level board state**: leaving a level (strip click, next, previous, quit) or winning it for the first time copies the
  whole 0x1644-byte board (toolbox included) into `saved_states[level_id]`. `start_level(1)` restores that copy if it exists.
  Restart (`r`) calls `start_level(0)`, which reloads the level data and puts every piece back in the toolbox.
  It doesn't delete the saved copy, which is overwritten on the next save.
**Evidence:** `update_unlocked` lines 1276–1329 (`if (8 < iVar5)` at line 1301, `if (0x22 < iVar5) iVar6 = iVar6 + 2;` at lines 1310–1311).
`check_win` lines 1510–1517. `save_level_state @ 0x55f4`. `start_level @ 0x5668`. `FUN_00004138` (load) calls `_memset(completed,0,0x100)` when there is no file, then `update_unlocked`.
**Confidence:** confirmed.

### L7. Interaction rules
**Answer:** `on_mouse_event @ 0x32ec` turns Carbon mouse events (only those inside the window while it is frontmost) into
`handle_mouse(event, X, Y)` with window-local pixel coordinates:

| Carbon event | primary button | button 2, or primary with Shift/Option/Control at mouse-down |
|---|---|---|
| mouse down | 1 | 3 (ignored) |
| mouse up | 4 | 6 |
| mouse dragged (button held; plain mouse-moved isn't registered) | 0 | 0 |

The "alternate" choice is latched in `mouse_alt_button` at mouse-down and also applies to the matching mouse-up. Any button number other than 2 counts as primary.

**Hit test** (C integer division, truncating):
- board: `col = X/24 − 2`, `row = (Y+30)/24 − 2`, both 0..14;
- else toolbox: `col = (X−422)/26 − 1` in 0..5, `row = (Y+18)/26 − 1` in 0..3 → toolbox slot `col + 6·row`, addressed as row 15
  (`board + 0x1194 + 20·slot`; only slots 0–23 can be hit);
- else level strip: `col = (X+10)/20 − 1` in 0..24, `row = (Y−390)/20 − 1` in 0..1 → slot `col + 25·row` ("row −2");
- else nothing ("row −1").

**State machine** (`drag_state`: 0 idle, 1 pressed, 2 dragging):
- **Down (1) on a board or toolbox cell**: `cursor = cell`. If the cell holds a movable piece: `drag_state = 1`, `press = (X, Y)`.
- **Move (0) while `drag_state == 1`**: nothing while `|X − press_x| ≤ 1` and `|Y − press_y| ≤ 1`, or while the pointer is off the
  board/toolbox. Otherwise `begin_drag`: if the cursor cell is movable, `drag_state = 2` and the sprite follows the pointer
  (`drag_image`, drawn with min index 0x0e), and the source cell is drawn as an empty cell. Beams aren't retraced during a drag.
- **Up (4) while dragging**: if the pointer is over a board cell or toolbox slot **whose type is 0 (empty)**, the 20-byte cell is copied
  there and the source's type is set to 0. In every other case the piece stays where it was: dropped on a wall, laser, pinwheel,
  conduit, teleporter or another piece, onto itself, outside the board and toolbox, or onto the level strip.
  Then `check_win` runs and the screen is redrawn, `drag_state = 0`.
- **Up (4) without a drag**, over a board/toolbox cell holding a movable piece: rotate `dir − 1` (CCW), `check_win`, `drag_state = 0`.
  (The press doesn't have to have been on that piece.) Over the level strip: if `unlocked[slot]`, `goto_slot(slot)`, which saves the
  current board and starts that slot with its saved state. Locked slots do nothing.
- **Up (6)** (right/modified click) with `drag_state == 0`, over a movable piece: rotate `dir + 1` (CW), `check_win`.
- **Removing a piece** = dragging it back to any empty toolbox slot. There is no delete action.
- Quirk: `drag_state` goes back to 0 only on a drop, on a left-up rotate of a movable piece, or when `begin_drag` finds no
  movable piece. If a press on a piece is released off the board and toolbox without a drag starting, `drag_state` stays 1.
  A left-up over a **non-movable** board or toolbox cell (empty, wall, laser, …) also leaves `drag_state = 1`, because the
  reset at lines 2141–2147 is inside `if (is_movable)`, so a stale 1 survives ordinary clicks on empty cells and fixed pieces.
  Only `kEventMouseDragged` is registered, not `kEventMouseMoved` (platform.md P4), so the stale state does nothing until the
  next button-held drag. Once the pointer is over the board or toolbox and more than 1 px from the stale press point,
  `begin_drag` runs on the current `cursor` cell (the last board/toolbox cell pressed with the primary button). Until then a
  right/modified-click rotate is ignored (up 6 needs `drag_state == 0`). A port may treat this as a bug.

**Keys** (`handle_key @ 0x5e64`, ignored when Cmd is held): Space, `=` or `+` → `next_level` if `unlocked[current_slot + 1]`
(saves, `current_slot += 1` unless it is already 49, then `start_level(1)`). `-` → `prev_level` (saves, `current_slot −= 1` if > 0, `start_level(1)`,
no unlock check). `r` → `start_level(0)` (restart). `L` (capital) → `force_win = 1; check_win()`, a cheat that wins and completes the level.
Esc → swallowed, no effect. The menu commands map onto these: `Next` → `=`, `Prev` → `-`, `rvrt` → `r`. `copy`/`past` → clipboard solution export/import
(`FUN_00005914`/`FUN_00005ad4`, see platform.md; paste places pieces then calls `check_win_and_redraw`), `Whee` toggles the
`UseMouseWheelToChangeLevel` preference (default on). With it on, the scroll wheel maps delta > 0 to `-` and anything else to `=`.
**Evidence:** `on_mouse_event` lines 630–695. `handle_mouse @ 0x50d4` lines 2024–2174: hit test at lines 2047–2072, drop at lines 2116–2139
(`PTR_board_00012d6c[iVar10] == '\0'` at line 2121), rotate-left at lines 2141–2147, rotate-right at lines 2163–2171, strip at lines 2103–2113.
`handle_key` lines 2622–2666. `on_menu_command @ 0x2c9c`, `on_wheel_event @ 0x3044`, `on_key_event @ 0x2e58` (`(local_1c[0] & 0x100) == 0`).
**Confidence:** confirmed. The `game_mode == 0` branch of `handle_mouse` (lines 2075–2081, which only moves the cursor) is unreachable,
because `start_level` sets `game_mode = 2` before the event loop starts. Its purpose is **unknown**, probably an editor remnant;
reading the original source would settle it.

### L8. What happens on a win
**Answer:** Nothing is automatic. There's no advance, timer, animation or sound (the binary imports no timer or sound API;
its only event handlers are menu command, key, wheel, mouse, draw and quit).
On the redraw after the winning move:
- **"You win!"** is drawn with `DrawThemeTextBox` in rect left 330, top 385, right 400, bottom 420.
- The hint box (450, 125, 620, 475) shows **"Click on a level or press spacebar for next."** in place of the level hint.
- Every pinwheel already shows its lit sprite (L5).
- The first time, the slot is marked completed, the board and save file are written, and unlocks are recomputed (L6),
  so the strip changes colour.

The board stays fully interactive. `level_won` is recomputed after every change. If a completed level is later un-solved, the
text becomes **"(won)"** (rect 350, 385, 400, 420), because `completed[slot]` stays set, and the hint comes back.
The player moves on with Space/`=`/`+`, the strip or the menu.
**Evidence:** `FUN_000066e0` lines 2981–3000:
```c
  if (*(int *)PTR_level_won_00012d4c == 0) {
    if (*(int *)(PTR_completed_00012d58 + current_slot * 4) == 0) goto LAB_000067c8;
    uVar5 = 0x15e;  pcVar4 = "(won)";
  } else { uVar5 = 0x14a;  pcVar4 = "You win!"; }
  FUN_00003228(pcVar4,uVar5,0x181,400,0x1a4);
  ...
  if ((*(int *)puVar3 == 2) && (*(int *)PTR_level_won_00012d4c != 0)) {
    pcVar4 = "Click on a level or press spacebar for next.";
```
(Condensed. `FUN_00003228(str, left, top, right, bottom)`, see data.md D4.)
**Confidence:** confirmed.

## Verification
```bash
BIN=chromatron_mac/Chromatron.app/Contents/MacOS/chromatron1
D=re/out/chromatron1/decompiled.c

# Names applied (after `just decompile 1`)
grep -c -E '^// (is_movable|push_beam|beam_enter_cell|trace_laser|clear_beams|trace_all_lasers|retrace_beams|pinwheel_satisfied|update_unlocked|check_win|check_win_and_redraw|begin_drag|handle_mouse|save_level_state|goto_slot|slot_next_id|slot_prev_id|next_level|prev_level|handle_key|on_menu_command|on_key_event|on_wheel_event|on_mouse_event) @ ' $D
# 24

# Piece 1, MIRROR (type 3): r=2 -> +6, r=1 -> +7, r=3 -> +5, else absorbed (beam_enter_cell, lines 771-791)
sed -n '771p;772p;774p;777p;782p;785p;789p' $D
#   case 3:
#     if (uVar3 == 2) {
#       uVar9 = uVar9 + 6;
#       bVar1 = uVar3 == 1;
#       uVar9 = uVar9 + 7;
#       if (uVar3 != 3) {
#       uVar9 = uVar9 + 5;

# Piece 2, BENDER (type 4): r=1 -> +6 (shared label), r=0 -> +7, r=2 -> +5, r=3 -> +4 (lines 792-805)
sed -n '792,800p' $D
#   case 4:
#     if (uVar3 == 1) goto LAB_00003678;
#     if (uVar3 < 2) {
#       bVar1 = (uVar7 & 7) == 0;
#       goto LAB_00003650;
#     }
#     if (uVar3 == 2) goto LAB_00003670;
#     uVar9 = uVar9 + 4;
#     if (uVar3 != 3) {

# Mirror/bender tail in the disassembly (+4/+5/+6/+7 then & 7)
r2 -e scr.color=0 -qc 'pi 12 @ 0x3658' $BIN 2>/dev/null
# cmpwi cr7, r29, 2
# beq cr7, 0x3670
# cmpwi cr7, r29, 3
# addi r5, r27, 4
# beq cr7, 0x3684
# b 0x3a44
# addi r5, r27, 5
# b 0x3684
# addi r5, r27, 6
# b 0x3684
# addi r5, r27, 7
# clrlwi r5, r5, 0x1d

# Piece 3, PRISM (type 6): red r=2/1/5/6 -> +2/+1/+5/+6, green +3/+1/+4/+6, blue +4/+6/+3/+1 (lines 818-871)
sed -n '818p;822p;825p;829p;831p;836p;840p;843p;847p;849p;856p;862p;865p;871p' $D
#         uVar5 = uVar9 + 2;
#         uVar5 = uVar9 + 1;
#         uVar5 = uVar9 + 5;
#         uVar5 = uVar9 + 6;
#       push_beam(param_1,param_2,uVar5 & 7,4,param_5);
#         uVar5 = uVar9 + 3;
#         uVar5 = uVar9 + 1;
#         uVar5 = uVar9 + 4;
#         uVar5 = uVar9 + 6;
#       push_beam(param_1,param_2,uVar5 & 7,2,param_5);
#       param_3 = uVar9 + 4;
#       param_3 = uVar9 + 6;
#       param_3 = uVar9 + 3;
#       param_3 = uVar9 + 1;

# Piece 4, TANGLER (type 9): pair p / p+4 per colour bit with a fresh tag (lines 938-948)
sed -n '939p;943,945p;948p' $D
#     if (uVar3 == 2) {
#           push_beam(param_1,param_2,uVar9 & 7,uVar8,entangle_counter);
#           push_beam(param_1,param_2,uVar9 + 4 & 7,uVar8,entangle_counter);
#           entangle_counter = entangle_counter + 1;
#       } while ((int)uVar8 < 5);

# Piece 5, SPLITTER (type 8): r=0/4 absorbed; tag kept only at r=2/6; straight pushed first (disassembly @ 0x38e4)
r2 -e scr.color=0 -qc 'pi 10 @ 0x38e4' $BIN 2>/dev/null
# cmpwi cr7, r29, 0
# beq cr7, 0x3a44
# cmpwi cr7, r29, 4
# beq cr7, 0x3a44
# cmpwi cr7, r29, 2
# beq cr7, 0x3908
# cmpwi cr7, r29, 6
# li r7, 0
# bne cr7, 0x390c
# mr r7, r25

# Entanglement pass: the beam itself gets fwd at r=2, its twin gets the opposite (lines 1071-1075)
sed -n '1071,1075p' $D
#                   if ((iVar15 == iVar14) == ((cVar2 - cVar1 & 7U) == 2)) {
#                     puVar11 = &doppler_fwd_map + (uint)*(byte *)(iVar16 + 3) * 4;
#                   }
#                   else {
#                     puVar11 = &doppler_back_map + (uint)*(byte *)(iVar16 + 3) * 4;

# doppler_back_map @ 0x8084 = [0,2,4,0,1], doppler_fwd_map @ 0x8098 = [0,4,1,0,2] (int32 BE)
xxd -s 0x7084 -l 40 $BIN
# 00007084: 0000 0000 0000 0002 0000 0004 0000 0000  ................
# 00007094: 0000 0001 0000 0000 0000 0004 0000 0001  ................
# 000070a4: 0000 0000 0000 0002                      ........

# Termination: 32-beam cap per generation, 1023 generations per laser
sed -n '729p;1013p' $D
#   if (iVar1 != 0x20) {
#     lVar17 = 0x3ff;
r2 -e scr.color=0 -qc 'pi 1 @ 0x3ac8; pi 1 @ 0x3d44' $BIN 2>/dev/null
# li r30, 0x3ff
# addic. r30, r30, -1

# Win condition: pinwheel colour == OR of in[0..7]
sed -n '1215p' $D
#   return *(byte *)(param_1 + 1) == bVar3;

# Unlock thresholds
sed -n '1301p;1304p;1307p;1310,1311p' $D
#   if (8 < iVar5) {
#   if (0x12 < iVar5) {
#   if (0x19 < iVar5) {
#   if (0x22 < iVar5) {
#     iVar6 = iVar6 + 2;

# Rotation: left click (event 4) dir-1, right/modified click (event 6) dir+1
sed -n '2144p;2167p' $D
#       puVar7[iVar10 + 2] = puVar7[iVar10 + 2] + -1;
#       puVar7[iVar10 + 2] = puVar7[iVar10 + 2] + '\x01';
```

**Simulator check** (throwaway Python, not committed). It loads every level blob as data.md D1 describes, leaves every
movable piece at its stored x/y/dir, runs the L3 tracer exactly as written above (32-beam cap, 1023-generation limit,
entanglement pass), and applies the L5 test:
- **49 / 50 playable levels are won** from their stored placement. The only failure is **slot 16 (level id 32)**, where the red
  pinwheel at (9,7) stays dark. The mirror stored at (12,2) with dir 6 is hit by no beam. Moving that one mirror to (12,4) with
  dir 0 wins the level (red: laser (7,0)↓ → mirror (7,3) → E → bender (11,3) → SE → mirror (12,4) → SW → (9,7)). That is the only
  single-piece relocation that does, and no rotation of one or two stored pieces does. So for this level the stored placement is
  not quite the solution (a data exception, not a rule gap).
- **12 / 12 unreachable ids** (15, 18, 31, 33, 40, 47, 50, 51, 53, 59, 60, 61) are won from their stored placement.
- **0 / 50** playable levels are won with all movable pieces removed (no trivially-won levels).
- The 32-beam cap and the 1023-generation limit are never reached by any stored solution.
- Pieces hit by a beam in the stored solutions, by type: mirror 132, bender 139, filter 6, prism 15, doppler 42, splitter 36,
  tangler 14, conduit 55, teleporter 18, pinwheel 291. So every kind is exercised.
- Sensitivity: dropping the entanglement pass, or giving the twin the same shift as the entangled beam, makes 10 more levels fail
  (slots 26, 27, 28, 29, 30, 31, 32, 35, 44, 49).
