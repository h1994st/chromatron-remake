# Chromatron 2 v1.4 — Data formats (diff against Chromatron 1)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2` (Mach-O, big-endian PowerPC, SHA-256
`ca85620cecc41fee5b5a88a697b9f7ce879e456d2f64191aca7f9b355f90e77e`). All addresses are vmaddrs. File offset = vmaddr − 0x1000.
All multi-byte values are big-endian. Function names are the ones in `re/notes/chromatron2/*.names.tsv`; each C2 function
keeps the name of its C1 equivalent. This note cites functions by **address** (stable), not by decompiled.c line.
Baseline: `re/notes/chromatron1/data.md` (C1). Only differences are spelled out; "same as C1" means byte-identical
data or the same algorithm with only addresses changed.

Sections: `__TEXT,__cstring` 0x6ec0–0x778b, `__TEXT,__const` 0x7fac–0x7ff3, `__DATA,__data` 0x8000–0x121e7,
`__nl_symbol_ptr` 0x122f8–0x12393, `__bss` 0x123b0–0x13031, `__common` 0x13040–0x1622f.

`game.c` reaches its globals through these local `__nl_symbol_ptr` slots (`r2 -qc 'p8 4 @ <slot>'`):

| slot | → target | meaning | C1 equivalent |
|---|---|---|---|
| 0x12318 | 0x136a0 | `hint_text` (char[2048], common) | 0x14080 |
| 0x1231c | 0x16000 | `level_name_text` (char[256], common) — **new** | (C1 `mode_text` 0x16100, unused) |
| 0x12320 | 0x161f0 | `site_url_text` | 0x16420 |
| 0x12324 | 0x121e4 | `num_images` | 0x12bdc |
| 0x12328 | 0x8110 | `level_order` | 0x8110 |
| 0x12330 | 0x13ea0 | `current_level_id` | 0x14880 |
| 0x1233c | 0x132a0 | `decoded_images` | 0x13c80 |
| 0x12348 | 0x11ee4 | `palette` | 0x128dc |
| 0x12374 | 0x140d0 | `board` | 0x14ab0 |
| 0x12380 | 0xaa08 | `level_name_table` — **new** | – |
| 0x12384 | 0xaadc | `hint_table` | 0xb700 |
| 0x12388 | 0xa934 | `level_table` | 0xb604 |
| 0x1238c | 0x11ce4 | `image_table` | 0x126dc |

Terms (level id, slot, colour mask, direction, `dx`/`dy` at 0x8044/0x8064) are exactly as in C1.

**What breaks a C1 decoder/renderer** (summary; details below):
1. **Cell stride is 28 bytes, not 20** (row stride 420, board 0x1f2c bytes). The board is still 15 × 15, the toolbox is still
   rows 15–18 (visible 6 × 4 = 24 slots), and the 5-byte level record is unchanged.
2. **52 encoded levels (table of 53), not 62 (table of 63)**; still **50 playable**; different `level_order`.
3. **Three new piece types**: 13 multi-filter (movable), 14 twister (fixed), 15 complementor (movable).
   `is_movable` = types 3–9, **13, 15**.
4. **New sprite indices**: 100–103 multi-filter, 15 twister, 116–117 complementor (all were unused placeholders in C1;
   C2 ships new pixels for images 15, 100–103, 116, 117; all other 121 images, the palette and the beam colours are
   byte-identical to C1).
5. **New level-name table** (`level_name_table` @ 0xaa08) and a level-name text box.
6. Save file: different name, **different `save_bit_perm`**, saved states for ids 0–51 only (platform.md P7).
   Clipboard prefix `2-` (platform.md P8).

## Findings

### D1. Level table: address, count, record layout, loader
**Answer:**
- **`level_table` @ 0xa934**: **53** big-endian `u32` pointers, indexed by level id 0–52. Entry 0 is NULL. Ids 1–52 point
  to blobs in `level_data` 0x81d0–0xa933 (id 1 @ 0x81d0, id 52 @ 0xa908). The table ends where `level_name_table`
  starts (0xaa08). (C1: 63 entries @ 0xb604.)
- **Count**: 52 levels are encoded, **50 are playable**. `level_order` @ 0x8110 (same address as C1) is a `u8[50]` mapping
  slot → id:
  `0d 20 15 0f 09 01 29 28 05 0a 0c 08 04 12 11 1e 10 23 24 22 21 2f 0e 03 17 26 27 2e 2c 1c 13 2d 14 32 0b 16 2a 19 1d 2b 1f 1a 31 1b 07 30 33 02 25 06`.
  The slot cap is the same as C1 (`next_level` @ 0x5b18: `if (current_slot < 0x31)`; the strip draws 50 numbers).
  Two ids are in no slot and can't be reached: **24** ("Splitting Outside the Box", 66 records, 18 movable) and
  **52** ("level052", 8 records, 4 movable, hint "blah"). The byte after `level_order` (0x8142) is `06`, followed by
  141 zero bytes up to 0x81d0 (unreferenced; see D8).
- **Level blob and record**: **unchanged from C1**: a list of 5-byte records `type, dir, colour, x, y`, ended by a single
  `0x00` type byte, then 0–3 zero pad bytes to 4-byte alignment. Checked for all 52 blobs (each terminator is followed only
  by zeros up to the next pointer, and id 52's padding ends exactly at 0xa934). Every x and y is 0–14 and no two records
  share a cell. There are **no new record fields**; the new pieces are new `type` values (D2).
- **Loader**: `load_level_data` @ 0x6d24 is C1's algorithm with two changes:
  1. it also does `strcpy(level_name_text, level_name_table[current_level_id])` (after the hint copy);
  2. cell addressing uses the C2 stride: `board[y*0x1a4 + x*0x1c + {0,1,2}] = {type, colour, dir}`.
  `memset(board, 0, 0x1be4)` clears cells 0–254 (board rows 0–16, i.e. toolbox slots 0–29) — the same 255 cells as C1's
  0x13ec. Callers are as in C1: `start_level` @ 0x5980 (copies a saved state of **0x1f2c** bytes instead of 0x1644).
- Per-slot table (slot 1-based; `movable` = types 3–9, 13, 15; new types MF = multi-filter 13, TW = twister 14,
  CO = complementor 15; hint "–" = the empty string 0x6ec0):

| slot | id | data @ | records | movable | new types | name (string @) | hint @ |
|---|---|---|---|---|---|---|---|
| 1 | 13 | 0x8d4c | 16 | 5 | – | Line of Sight (0x7140) | 0x7530 |
| 2 | 32 | 0x9d70 | 47 | 10 | – | Boxed Set (0x7288) | – |
| 3 | 21 | 0x922c | 13 | 4 | – | Winter Wonderland (0x71c4) | – |
| 4 | 15 | 0x8de4 | 41 | 10 | – | Evasive Maneuvers (0x7164) | 0x757c |
| 5 | 9 | 0x8af4 | 11 | 6 | – | Quantum Warm-Up (0x7104) | – |
| 6 | 1 | 0x81d0 | 107 | 6 | – | Which One? (0x708c) | – |
| 7 | 41 | 0xa12c | 7 | 3 | MF | We Caught the Perp... (0x7314) | 0x7728 |
| 8 | 40 | 0xa108 | 7 | 3 | MF | ...Standing in the Intersection (0x72f4) | – |
| 9 | 5 | 0x8840 | 20 | 7 | MF | Dual-Purpose (0x70c8) | – |
| 10 | 10 | 0x8b2c | 28 | 7 | TW | Every Which Way (0x7114) | 0x74c4 |
| 11 | 12 | 0x8c78 | 42 | 8 | TW | Spider Web (0x7134) | – |
| 12 | 8 | 0x8ab0 | 13 | 3 | – | Back-Back (0x70f8) | 0x7400 |
| 13 | 4 | 0x87d8 | 20 | 10 | MF | Omnichromatic (0x70b8) | – |
| 14 | 18 | 0x9148 | 9 | 3 | TW | Reflections (0x7194) | 0x75a4 |
| 15 | 17 | 0x9028 | 57 | 4 | TW | No Right Turn (0x7184) | – |
| 16 | 30 | 0x9b90 | 62 | 9 | – | Squeeze Play (0x7264) | – |
| 17 | 16 | 0x8eb4 | 74 | 7 | MF | One of Each (0x7178) | – |
| 18 | 35 | 0x9ee0 | 9 | 4 | CO | Crossing Guard (0x72a8) | 0x7618 |
| 19 | 36 | 0x9f10 | 12 | 7 | CO | Color Wheel (0x72b8) | 0x76a4 |
| 20 | 34 | 0x9ea8 | 11 | 6 | CO | Second in Command (0x7294) | – |
| 21 | 33 | 0x9e5c | 15 | 6 | – | Limited Access (0x71d8) | – |
| 22 | 47 | 0xa4ac | 40 | 15 | TW | Tangled Web (0x737c) | – |
| 23 | 14 | 0x8da0 | 13 | 7 | MF | On Second Thought (0x7150) | – |
| 24 | 3 | 0x8790 | 14 | 5 | – | Right Through (0x70a8) | – |
| 25 | 23 | 0x9378 | 65 | 17 | TW | Try Again (0x71e8) | – |
| 26 | 38 | 0xa050 | 17 | 8 | CO | Flowery (0x72d8) | – |
| 27 | 39 | 0xa0a8 | 19 | 6 | CO, TW | In the Line Of Fire (0x72e0) | – |
| 28 | 46 | 0xa264 | 116 | 13 | – | Good Show (0x7370) | – |
| 29 | 44 | 0xa1dc | 6 | 4 | CO | Grass Is Always Greener (0x7350) | – |
| 30 | 28 | 0x99a8 | 86 | 14 | MF | The Twist (0x7244) | – |
| 31 | 19 | 0x9178 | 21 | 8 | TW | Get Bent (0x71a0) | – |
| 32 | 45 | 0xa1fc | 20 | 10 | CO | Head-On (0x7368) | – |
| 33 | 20 | 0x91e4 | 14 | 9 | MF, TW | The Column of Despair (0x71ac) | – |
| 34 | 50 | 0xa7c8 | 25 | 12 | – | Matchmaker (0x73a0) | – |
| 35 | 11 | 0x8bbc | 37 | 8 | TW | Drunk Spider (0x7124) | – |
| 36 | 22 | 0x9270 | 52 | 12 | – | Limited Access (0x71d8) | – |
| 37 | 42 | 0xa150 | 15 | 8 | CO | Two out of Three (0x732c) | – |
| 38 | 25 | 0x960c | 100 | 21 | TW | Blue vs Green II (0x7210) | – |
| 39 | 29 | 0x9b58 | 11 | 7 | CO | Onwards and Upwards (0x7250) | 0x75c4 |
| 40 | 43 | 0xa19c | 12 | 6 | CO | Just One Second (0x7340) | – |
| 41 | 31 | 0x9cc8 | 33 | 15 | – | Why Would I Lie? (0x7274) | – |
| 42 | 26 | 0x9804 | 29 | 9 | TW | Quantum Uncertainty (0x7224) | – |
| 43 | 49 | 0xa5cc | 101 | 14 | – | Zig Zag (0x7398) | – |
| 44 | 27 | 0x9898 | 54 | 16 | – | Prismagic (0x7238) | – |
| 45 | 7 | 0x8a3c | 23 | 13 | MF | Doppler Mania (0x70e8) | – |
| 46 | 48 | 0xa578 | 16 | 9 | CO, MF | One Of Each II (0x7388) | – |
| 47 | 51 | 0xa848 | 38 | 7 | TW | The Impossible Dream (0x73ac) | – |
| 48 | 2 | 0x83e8 | 187 | 15 | – | Back And Forth (0x7098) | – |
| 49 | 37 | 0x9f50 | 51 | 12 | CO | Feedback Eliminator (0x72c4) | 0x76c8 |
| 50 | 6 | 0x88a8 | 80 | 24 | – | Teleporter Maze (0x70d8) | 0x73d0 |
| – | 24 | 0x94c0 | 66 | 18 | MF | Splitting Outside the Box (0x71f4) | – |
| – | 52 | 0xa908 | 8 | 4 | – | level052 (0x73c4) | 0x7784 ("blah") |

  The most movable pieces in any level is **24** (id 6, slot 50), which exactly fills the 24 visible toolbox slots. Unlike
  C1 (ids 50/51), **no C2 level overflows the toolbox**, but a decoder still shouldn't clamp.

**Evidence:** `load_level_data @ 0x6d24`:
```c
  pcVar4 = *(char **)(PTR_level_table_00012388 + *(int *)PTR_current_level_id_00012330 * 4);
  _strcpy(PTR_hint_text_00012318,*(char **)(PTR_hint_table_00012384 + *(int *)PTR_current_level_id_00012330 * 4));
  _strcpy(PTR_level_name_text_0001231c,*(char **)(PTR_level_name_table_00012380 + *(int *)puVar7 * 4));
  puVar7 = _memset(PTR_board_00012374,0,0x1be4);
  while (cVar1 != '\0') { iVar6 = (uint)(byte)pcVar4[4] * 0x1a4 + (uint)(byte)pcVar4[3] * 0x1c;
    puVar5[iVar6] = cVar1;  puVar5[iVar6 + 1] = cVar3 /*rec[2] colour*/;  puVar5[iVar6 + 2] = cVar2 /*rec[1] dir*/;
    pcVar4 = pcVar4 + 5; ... }
```
(Condensed; the disassembly at 0x6dbc–0x6de4 is `lbzu` ×4, `mulli r2,r2,0x1c`, `mulli r0,r0,0x1a4`, `stb r9,1(r2)`,
`stb r11,2(r2)`.) Slot → id: `goto_slot` @ 0x5a2c, `slot_next_id` @ 0x5a84, `slot_prev_id` @ 0x5ac4 read `level_order`;
`game_init` @ 0x6254 starts with `level_order[0]` (= 13).
Worked check (slot 1 = id 13 @ 0x8d4c, 16 records): `02 00 01 07 0e` = blue laser at (7,14) facing N; pinwheels
`0a 00 01 ..` are blue; four splitters (`08`) and one mirror (`03 07 04 07 05`, dir 7 at (7,5)) are the movable pieces.
**Confidence:** confirmed.

### D2. Board: grid size and cell encoding
**Answer:**
- **Grid**: still a fixed **15 × 15** board on every level, plus rows 15–18 for the toolbox (60 addressable slots, 24 drawn).
- **Memory layout changed**: `board` @ 0x140d0 (common) is **19 rows × 15 columns × 28-byte cells = 0x1f2c (7980) bytes**.
  Cell (x, y) is at `board + y*0x1a4 + x*0x1c` (row stride 420). Toolbox slot s is at `board + 0x189c + 28*s`
  (0x189c = 225 × 28). The loader's `memset` covers cells 0–254 (0x1be4 bytes), as in C1.
- **Cell layout (28 bytes)**:

  | off | size | field | vs C1 |
  |---|---|---|---|
  | +0 | u8 | `type` | same |
  | +1 | u8 | `colour` mask | same |
  | +2 | u8 | `dir` 0–7 | same |
  | +3 | u8 | runtime flag: complementor "shut down" (set by `shut_down_complementors` @ 0x3e9c, cleared for all 15 × 15 cells by `clear_complementor_flags` @ 0x3d5c at the start of every `retrace_beams`) | was unused (C1's clearing function was dead code) |
  | +4..+11 | u8[8] | runtime: colour of beams **entering**, by travel direction | same |
  | +12..+19 | u8[8] | runtime: colour of beams **leaving**, by travel direction | same |
  | +20..+27 | u8[8] | runtime: copy of +4..+11 from the previous complementor iteration (`snapshot_cell_inputs` @ 0x3e2c) | **new** |

  Bytes +3..+27 are zero after loading. Their use belongs to `logic.md`. The save file still stores only bytes +0..+2
  (platform.md P7), so the new runtime bytes never reach disk.
- **`type` values** (`cell_image_index` @ 0x48c4; `is_movable` @ 0x33a8 = `type == 13 || type == 15 || type − 3 ≤ 6`):

  | type | kind | movable | uses `colour` | uses `dir` | image index |
  |---|---|---|---|---|---|
  | 0–12 | as C1 (empty, wall, laser, mirror, bender, filter, prism, doppler, splitter, tangler, pinwheel, conduit, teleporter) | 3–9 yes | as C1 | as C1 | as C1 |
  | **13** | **multi-filter** | **yes** | no (data always 4) | yes, 0–7; drawn `dir & 3` | **100 + (dir & 3)** |
  | **14** | **twister** | **no (fixed)** | no (data always 4) | no (data always 0) | **15** |
  | **15** | **complementor** | **yes** | no (data always 4) | yes, 0–7; drawn `dir & 1` | **116 + (dir & 1)** |

  Any other type makes `cell_image_index` return −1 (nothing drawn), as in C1. The beam router `beam_enter_cell` @ 0x3458
  has explicit cases for 13 (passes blue/green/red/all depending on `(beam_dir − dir) & 7` = 0,4 / 1,5 / 2,6 / 3,7),
  14 (out dir = beam dir − 2) and 15 (absorbs; a separate pass at 0x3fe8 emits the complement), and asserts
  (`game.c:0x1a6`) on anything above 15. Full semantics: `logic.md`.
- **Names**: from the hint strings (0x7728 "The MULTI-FILTER filters light differently depending on which direction the
  light hits it.", 0x74c4 "The twister sends each beam back out 90 degrees clockwise…", 0x7618 "The COMPLEMENTOR turns
  colors to their complements…") and the sprite art (D5). The twister is the first **fixed** non-trivial piece: it is
  not in `is_movable`, so it stays on the board and can't be dragged or rotated.
- **Data noise** as in C1: walls, mirrors, benders etc. carry `colour` 4 (sometimes 1, 2 or 7) and pinwheel/wall `dir` is
  usually 0. Keep it byte-exact (save file, clipboard sort, D3).
- Record counts over all 52 blobs: wall 800, laser 98, mirror 171, bender 183, filter 5, prism 7, doppler 25,
  splitter 35, tangler 9, pinwheel 466 (66 black), conduit 95, teleporter 32, **multi-filter 27, twister 25,
  complementor 12**.
- **Screen positions**: unchanged from C1 (`draw_slot_cell` @ 0x6444: board centre (60 + 24x, 30 + 24y), toolbox
  centre (460 + 26(s mod 6), 20 + 26(s div 6))).

**Evidence:** `cell_image_index @ 0x48c4` new cases:
```c
  case 0xd: lVar5 = uVar2 + 100;        /* uVar2 = dir & 3 */
  case 0xe: lVar5 = 0xf;
  case 0xf: lVar5 = (uVar6 & 1) + 0x74; /* uVar6 = dir */
```
(disassembly 0x4ad8 `addi r10,r8,0x64`; 0x4ae0 `li r10,0xf`; 0x4ae8–0x4aec `clrlwi r2,r11,0x1f; addi r10,r2,0x74`;
the switch bound at 0x48d4 is `cmplwi cr7,r9,0xf`). `is_movable @ 0x33a8`: `cmpwi r3,0xd; beq →1; …; addi r0,r3,-3;
cmplwi r0,6; …; cmpwi r3,0xf; bne →0`. Stride: every board access uses `* 0x1a4 + * 0x1c`; `save_level_state` @ 0x590c
and `start_level` @ 0x5980 copy 0x1f2c bytes; `move_pieces_to_toolbox` @ 0x5810 uses `board + 0x189c`.
**Confidence:** confirmed for layout, type ids, movability and image mapping. The runtime meaning of +3 and +20..+27 is
summarised only (logic.md owns it).

### D3. Toolbox: how available pieces and counts are stored
**Answer:** **Same model as C1**: no table and no counts; the toolbox is exactly the level's movable records (now types 3–9,
13, 15), and their stored `x, y, dir` are the intended placement. `move_pieces_to_toolbox` @ 0x5810 is C1's algorithm
with the 28-byte stride: scan cells 0–224 row-major; for each movable cell find the next empty toolbox slot, copy all
**28** bytes there, set the slot's `dir = 0` and clear the board cell's `type`; then
`qsort(board + 0x189c, n, 0x1c, cmp)` with `cmp` @ 0x5800 = `a.type − b.type` (pointer at 0x810c). Then re-trace and
redraw. Visible toolbox: 6 × 4 = 24 slots (`render_scene` @ 0x68ec draws slots 0..0x17; `handle_mouse` @ 0x53ec
hit-tests 6 × 4).
- New types sort after the C1 types: … tangler (9), **multi-filter (13)**, **complementor (15)**. The twister is fixed and
  never enters the toolbox.
- **Ordering caveat**: only pieces of the same type with different `colour` bytes can be reordered by an unstable qsort.
  In C2 no level has two filters (the only movable type whose colour is visible) of different colours, so **no visible
  toolbox order depends on the qsort implementation**. Colour-noise ties exist only for mirrors/benders in ids 3, 4, 10,
  20, 42, 43, 46. They are invisible: the clipboard entry order doesn't change (Copy sorts by (type, colour)); what
  changes is which toolbox cell index each differently-coloured mirror or bender carries if Copy is used while it is
  still in the toolbox. Paste still matches by sorted (type, colour), and nothing visible changes. Follow BSD qsort as
  for C1.
- Capacity: the largest playable toolbox is 24 (id 6, slot 50), the largest unreachable one 18 (id 24). Nothing overflows.

**Evidence:** `move_pieces_to_toolbox @ 0x5810`:
```c
  pcVar17 = PTR_board_00012374 + 0x189c;  lVar14 = 0xe0;
  do { iVar11 = is_movable(PTR_board_00012374[iVar16]);
    if (iVar11 != 0) { ...find next empty slot (step 0x1c)...;  /* copy 7 words */
      puVar8[iVar13 + 0x189e] = 0;  puVar8[iVar16] = 0; }
    lVar14 = lVar14 + -1;  iVar16 = iVar16 + 0x1c; } while (-1 < lVar14);
  _qsort(PTR_board_00012374 + 0x189c,iVar12 + 1,0x1c,DAT_0000810c);
```
`r2 -qc 'pi 4 @ 0x5800'` → `lbz r0,0(r3); lbz r3,0(r4); subf r3,r3,r0; blr`.
**Confidence:** confirmed (storage model and algorithm); the qsort caveat has no visible effect in C2.

### D4. Level names and tutorial/hint text
**Answer:** **New in C2: every level has a name.**
- **`level_name_table` @ 0xaa08**: 53 big-endian `u32` pointers into `__cstring`, indexed by level id (entry 0 NULL),
  parallel to `level_table`. `load_level_data` copies the name into `level_name_text` @ 0x16000 (256 bytes, common).
  All 50 names are listed in D1. Ids 22 and 33 share one string ("Limited Access" @ 0x71d8). The longest name is
  "...Standing in the Intersection" (31 chars). Id 52's name is the placeholder "level052".
- The name is drawn by `paint_window` @ 0x69fc with `DrawThemeTextBox` in rect **left 50, top 385, right 300, bottom 420**
  whenever `game_mode == 2` (always after startup). That call reuses C1's `mode_text` slot (C1 drew an always-empty
  buffer at 350,420–450,460 there; C2 still has that rect for `game_mode != 2`, which never happens). See platform.md P3.
- **`hint_table` @ 0xaadc**: 53 `u32` pointers indexed by level id, entry 0 NULL, empty hints → **0x6ec0** (the first byte
  of `__cstring`, a NUL). Copied into `hint_text` @ 0x136a0 (2048 bytes) and drawn at 450,125–620,475 as in C1, replaced by
  "Click on a level or press spacebar for next." (0x7038) when the level is won.
- Slots that show a hint (11 strings, each referenced once, plus "blah" for unreachable id 52):

  | slot | id | string @ | starts with |
  |---|---|---|---|
  | 1 | 13 | 0x7530 | An easy warm up puzzle. Surely you won't need just one more… |
  | 4 | 15 | 0x757c | Things never go the way you'd expect. |
  | 7 | 41 | 0x7728 | The MULTI-FILTER filters light differently depending on whic… |
  | 10 | 10 | 0x74c4 | The twister sends each beam back out 90 degrees clockwise. U… |
  | 12 | 8 | 0x7400 | When you split a beam with the splitter, one beam goes strai… |
  | 14 | 18 | 0x75a4 | Send it back the way it came. |
  | 18 | 35 | 0x7618 | The COMPLEMENTOR turns colors to their complements: red to t… |
  | 19 | 36 | 0x76a4 | When does red plus green make blue? |
  | 39 | 29 | 0x75c4 | Like the splitter, the complementor will disentangle any qua… |
  | 49 | 37 | 0x76c8 | If the complementor is presented with a logical impossibilit… |
  | 50 | 6 | 0x73d0 | Hint: you do not need to hit any bender twice |

  The longest hint is 195 characters (0x7400). All names and hints are 7-bit ASCII without line breaks
  (`draw_text_box` @ 0x30f8 uses encoding 0x0600 as in C1).

**Evidence:** `load_level_data @ 0x6d24` (the two `_strcpy` calls in D1). Data: `xxd -s 0x9a08 -l 16` →
`0000 0000 0000 708c 0000 7098 0000 70a8` (NULL, "Which One?", "Back And Forth", "Right Through");
`xxd -s 0x9adc -l 16` → `0000 0000 0000 6ec0 …` (NULL, then empty hints). `paint_window @ 0x69fc`:
```c
  if (*(int *)PTR_game_mode_0001234c == 2) { uVar4 = 0x32; uVar5 = 0x181; uVar6 = 300; uVar7 = 0x1a4; }
  else { uVar4 = 0x15e; uVar5 = 0x1a4; uVar6 = 0x1c2; uVar7 = 0x1cc; }
  draw_text_box(PTR_level_name_text_0001231c,uVar4,uVar5,uVar6,uVar7);
```
**Confidence:** confirmed.

### D5. Images/sprites
**Answer:**
- **`num_images` @ 0x121e4** = **128** (same count as C1).
- **`image_table` @ 0x11ce4**: 128 `u32` pointers into **`image_data` 0xabb0–0x11ce3** (0x7134 = 28980 bytes incl. padding).
  Image 0 @ 0xabb0, image 127 @ 0x11c54; image 127's stream plus padding ends at the table.
- **Encoding: identical to C1**: no header, 24 × 24, 8-bit palette indices, PCX-style RLE (byte ≤ 0xC0 literal, else run of
  `b − 0xC0` copies of the next byte), `decode_images` @ 0x6c10 (decodes 127 → 0, `malloc(0x240)` each, into
  `decoded_images` @ 0x132a0). All 128 streams decode to exactly 576 bytes and end at their zero padding.
- **Blit**: `blit_indexed` @ 0x4af8 and its callers (`draw_cell_image` @ 0x6364 with the `b != -1` / `b < num_images`
  asserts at game.c 0xa5c/0xa5d, `draw_drag_sprite` @ 0x5118 with min 0x0e, `draw_digit_string` @ 0x6718) are unchanged.
- **Diff against C1's sheet** (decoded pixels compared image by image): **121 images are pixel-identical**. These differ:

  | index | C1 content | C2 content |
  |---|---|---|
  | 15 | unused white spoked disc | **twister** (type 14); same disc art, retouched (adds palette index 27) |
  | 100–103 | unused floor tile + one pixel 252 (101: pinwheel copy) | **multi-filter** dir & 3 = 0..3: a ring of red/green/blue/grey segments; the four images differ in the segments' arrangement (one per `dir & 3`) |
  | 116–117 | unused floor tile + one pixel 252 | **complementor** dir & 1 = 0 (upright "+" shape) / 1 (diagonal "×" shape), multicoloured |

- **Full C2 index map**: 0 floor; 1 wall; pinwheels 2/3 red, 4/5 green, 6/7 blue, 12/13 black, 16/17 yellow, 18/19 cyan,
  20/21 magenta, 22/23 white; filters 8–11 blue, 96–99 red, 112–115 green; 14 teleporter; **15 twister**; 24–31 prism;
  32–39 mirror; 40–43 splitter; 44–47 conduit; 48–55 bender; 56–63 red laser; 64–71 doppler; 72–79 green laser;
  80–87 tangler; 88–95 blue laser; **100–103 multi-filter**; 104–111 white laser; **116–117 complementor**;
  118–127 digits '0'–'9'. Every index 0–127 is now referenced (C1's unused 15, 100–103, 116, 117 are taken by the
  new pieces); a renderer written for C1 must add the three new `cell_image_index` cases.

**Evidence:** `decode_images @ 0x6c10` is C1's loop with `PTR_image_table_0001238c`, `PTR_decoded_images_0001233c` and
`PTR_num_images_00012324`. `xxd -s 0x10ce4 -l 8` → `0000 abb0 0000 abf8`; image 0 starts `d9 06 d7 08 06 …`, byte-identical
to C1's image 0. The decode/compare script in Verification prints the differing indices.
**Confidence:** confirmed (format, table, which images changed); the piece-to-image assignment is confirmed by
`cell_image_index`, and the descriptions of the new art come from a decoded sheet.

### D6. Colours
**Answer:** **All identical to C1.**
- **`palette` @ 0x11ee4**: 256 × BGR, byte-for-byte equal to C1's 0x128dc palette; `decode_images` swaps it to RGB in place
  exactly as C1.
- **`beam_colors` @ 0x80d4** (same address): `u32[8]` = 000000, 0000ff, 00c800, 00c8ff, ff0000, ff00ff, ffc800, ffffff.
  `draw_cell_beams` @ 0x6508 is C1's routine with the 28-byte stride.
- **Background** `memset(fb, 0xa4, 640·480·3)` in `render_scene` @ 0x68ec; strip digit palette offsets
  `strip_pal_offset` @ 0x67bc return 0 / 0x48 / 0x30 / 0x10 exactly as C1 (current / completed / unlocked / locked).
- Text colour: none set (theme default), as C1.

**Evidence:** byte comparison of the two 768-byte palettes (Verification, `palette same True`); `xxd -s 0x70d4 -l 32`
equals C1's. `strip_pal_offset @ 0x67bc`: `if (param_1 == current_slot) return 0; if (completed[...]) return 0x48;
if (unlocked[...]) return 0x30; return 0x10;`.
**Confidence:** confirmed.

### D7. Fonts
**Answer:** **Same as C1.** Digit glyphs are images 118–127 (pixel-identical to C1), drawn 12 × 12 from the top-left of each
24 × 24 image by `draw_digit_string` @ 0x6718 (x += 8 per digit, += 4 for a space, min index 0x0e). The strip is drawn
by `render_scene` @ 0x68ec as numbers 1..50 at x = 10 + 20((n−1) mod 25), y = 410 + 20((n−1) div 25), leading '0' → ' '.
All other text, including the new level name and the new "Thank you for purchasing this game." line, goes through
`draw_text_box` @ 0x30f8 (`DrawThemeTextBox`, `kThemeSystemFont`); rects are in platform.md P3.
**Evidence:** `draw_digit_string @ 0x6718`: `blit_indexed(param_1,param_2,*(… + (c - 0x30) * 4 + 0x1d8),0xc,0xc,0x18,0xe,param_4)`.
**Confidence:** confirmed.

### D8. Map of `__DATA,__data` (0x8000–0x121e7)
**Answer:**

| range | size | purpose | vs C1 |
|---|---|---|---|
| 0x8000–0x800f | 16 | `_NXArgc`, `_NXArgv`, `_environ`, `___progname` (→ 0x6ec0) | same layout |
| 0x8010–0x801f | 16 | crt/dyld glue pointers 0x13044, 0x2590, 0x2474, 0x23f0 | addresses differ |
| 0x8020 | 4 | `use_wheel_pref` = 1 | same |
| 0x8024–0x803b | 24 | event handlers 0x2b6c (menu), 0x2d28 (key), 0x2f14 (wheel), 0x31bc (mouse), 0x2cbc (draw), 0x2c98 (close) | addresses differ |
| 0x803c | 4 | `current_slot` = 0 | same |
| 0x8040 | 4 | `redraw_all_flag` = 1 | same |
| 0x8044–0x8063 | 32 | `dir_dx[8]` | same bytes |
| 0x8064–0x8083 | 32 | `dir_dy[8]` | same bytes |
| 0x8084–0x8097 | 20 | int32[5] 0,2,4,0,1 (tangler/doppler map, logic.md) | same bytes |
| 0x8098–0x80ab | 20 | int32[5] 0,4,1,0,2 | same bytes |
| 0x80ac–0x80cb | 32 | `save_bit_perm`: `14 1f 07 08 1e 0e 03 1c 02 16 05 1b 10 0b 19 0c 00 09 0d 13 17 04 11 1d 15 0a 06 18 12 1a 01 0f` | **DIFFERENT** (C1's eight 4-byte groups in the order 6,2,7,3,0,4,1,5; platform.md P7) |
| 0x80cc | 4 | −1, unreferenced | same |
| 0x80d0 | 4 | `drag_image` = −1 | same |
| 0x80d4–0x80f3 | 32 | `beam_colors` (D6) | same bytes |
| 0x80f4–0x8107 | 20 | `site_url_xor` → "silverspaceship.com" | same bytes |
| 0x8108 | 4 | clipboard-entry comparator → 0x5bb8 (type, then colour, unsigned) | address differs |
| 0x810c | 4 | toolbox comparator → 0x5800 (type) | address differs |
| 0x8110–0x8141 | 50 | `level_order` u8[50] (D1) | **different contents** |
| 0x8142–0x81cf | 142 | `06` + 141 zero bytes, unreferenced (probably `level_order` declared as 192 bytes with 51 initialisers) | C1: `01 01` + 140 zeros |
| 0x81d0–0xa933 | 0x2764 | `level_data`: 52 blobs (D1) | C1: 62 blobs, 0x3434 |
| 0xa934–0xaa07 | 212 | `level_table` u32[53] | C1: u32[63] @ 0xb604 |
| 0xaa08–0xaadb | 212 | **`level_name_table` u32[53]** (D4) | **new** |
| 0xaadc–0xabaf | 212 | `hint_table` u32[53] | C1: u32[63] @ 0xb700 |
| 0xabb0–0x11ce3 | 0x7134 | `image_data`: 128 RLE images (D5) | moved; 7 images changed |
| 0x11ce4–0x11ee3 | 512 | `image_table` u32[128] | moved |
| 0x11ee4–0x121e3 | 768 | `palette` 256 × BGR | moved, same bytes |
| 0x121e4–0x121e7 | 4 | `num_images` = 128 | moved |

Runtime (not in `__data`): `__bss` — `main_window` 0x123b0, `mouse_alt_button` 0x123b4, `wheel_menu_mark` 0x123b8,
`drag_state` 0x123bc, `empty_cell` 0x123c0 (28 bytes), `entangle_counter` 0x123dc, `framebuffer` 0x123e0,
`fb_width` 0x123e4, `fb_height` 0x123e8, `drag_save_under` 0x123ec (0x900), `press_x/y` 0x12fec/0x12ff0,
`drag_inval_x/y` 0x12ff4/0x12ff8, `clip_text` 0x12ffc (54 bytes, to the end of `__bss`). `__common` — beam lists
0x1307c–0x1328f, drag sprite x/y 0x13084/0x13088, `level_started` 0x13290, `game_mode` 0x13294, `decoded_images` 0x132a0
(128 × 4; 0x134a0–0x1369f has no references — C1 has the same 0x200 gap, so probably a 256-entry declaration),
`hint_text` 0x136a0 (2048), `current_level_id` 0x13ea0, `unlocked[64]` 0x13eb0, `force_win` 0x13fb0, `completed[64]`
0x13fc0, `level_won` 0x140c0, `board` 0x140d0 (0x1f2c), `level_name_text` 0x16000 (256), beam next list 0x16100/0x16104,
**`saved_states[52]` 0x16110** (0xd0 bytes; C1 had 128 entries), `cursor_col/row` 0x161e0/0x161e4, `site_url_text`
0x161f0 (to 0x1622f).

**Evidence:** every `DAT_`/`PTR_` reference in `re/out/chromatron2/decompiled.c` falls in one of these ranges; a byte diff of
0x8044–0x810f against C1 differs only at 0x80ac–0x80cb and 0x8108–0x810f. `0x80cc` and 0x8142–0x81cf have no references.
**Confidence:** confirmed, except the purpose of 0x80cc, 0x8142–0x81cf and 0x134a0–0x1369f (**unknown**, unreferenced; only
the original source would settle them; irrelevant to a reimplementation).

## Verification
```bash
BIN="chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2"
B1=chromatron_mac/Chromatron.app/Contents/MacOS/chromatron1

# Level-table header (level_table @ 0xa934 = file 0x9934): id0 NULL, id1 0x81d0, id2 0x83e8, id3 0x8790
xxd -s 0x9934 -l 16 "$BIN"
# 00009934: 0000 0000 0000 81d0 0000 83e8 0000 8790  ................
# ... and its last entry (id 52 @ 0xa908), directly followed by level_name_table @ 0xaa08
xxd -s 0x9a04 -l 4 "$BIN"
# 00009a04: 0000 a908                                ....

# First level record: id 1 @ 0x81d0 (file 0x71d0): 02 04 01 07 00 = blue laser facing S at (7,0); 01 06 04 04 02 = wall ...
xxd -s 0x71d0 -l 16 "$BIN"
# 000071d0: 0204 0107 0001 0604 0402 0100 0407 0201  ................
# Slot 1 = id 13 @ 0x8d4c (file 0x7d4c): 16 records + 00 terminator
xxd -s 0x7d4c -l 0x51 "$BIN"
# 00007d4c: 0a00 010b 010a 0001 0102 0a00 0107 0208  ................
# 00007d5c: 0304 0b02 0a00 0101 0508 0504 0305 0a00  ................
# 00007d6c: 0105 0503 0704 0705 0a00 010b 050a 0001  ................
# 00007d7c: 0309 0801 0407 0908 0104 0b09 0a00 010d  ................
# 00007d8c: 090a 0001 030c 0a00 0107 0c02 0001 070e  ................
# 00007d9c: 00                                       .
r2 -qc 'p8 16 @ 0x8d4c' "$BIN" </dev/null 2>/dev/null
# 0a00010b010a000101020a0001070208

# Slot -> level id order (level_order @ 0x8110) and the two bytes after it
xxd -s 0x7110 -l 52 "$BIN"
# 00007110: 0d20 150f 0901 2928 050a 0c08 0412 111e  . ....)(........
# 00007120: 1023 2422 212f 0e03 1726 272e 2c1c 132d  .#$"!/...&'.,..-
# 00007130: 1432 0b16 2a19 1d2b 1f1a 311b 0730 3302  .2..*..+..1..03.
# 00007140: 2506 0600                                %...

# Name table and hint table headers
xxd -s 0x9a08 -l 16 "$BIN"
# 00009a08: 0000 0000 0000 708c 0000 7098 0000 70a8  ......p...p...p.
xxd -s 0x9adc -l 16 "$BIN"
# 00009adc: 0000 0000 0000 6ec0 0000 6ec0 0000 6ec0  ......n...n...n.

# num_images, first image-table entries, and the first image "header" (no header: RLE stream D9 06 D7 08 06 ...)
xxd -s 0x111e4 -l 4 "$BIN"
# 000111e4: 0000 0080                                ....
xxd -s 0x10ce4 -l 8 "$BIN"
# 00010ce4: 0000 abb0 0000 abf8                      ........
xxd -s 0x9bb0 -l 16 "$BIN"
# 00009bb0: d906 d708 06d7 0806 d708 06d7 0806 d708  ................

# Palette (BGR) entries 0..7 and beam colours: identical to C1
xxd -s 0x10ee4 -l 24 "$BIN"
# 00010ee4: 0000 00a8 0000 00a8 00a8 a800 0000 a8a8  ................
# 00010ef4: 00a8 2c44 58a8 a8a8                      ..,DX...
xxd -s 0x70d4 -l 32 "$BIN"
# 000070d4: 0000 0000 0000 00ff 0000 c800 0000 c8ff  ................
# 000070e4: 00ff 0000 00ff 00ff 00ff c800 00ff ffff  ................

# nl_symbol_ptr slots: level_table, level_name_table, hint_table, image_table, palette, num_images, level_order, board, hint_text, level_name_text
r2 -qc 'p8 4 @ 0x12388; p8 4 @ 0x12380; p8 4 @ 0x12384; p8 4 @ 0x1238c; p8 4 @ 0x12348; p8 4 @ 0x12324; p8 4 @ 0x12328; p8 4 @ 0x12374; p8 4 @ 0x12318; p8 4 @ 0x1231c' "$BIN" </dev/null 2>/dev/null
# 0000a934
# 0000aa08
# 0000aadc
# 00011ce4
# 00011ee4
# 000121e4
# 00008110
# 000140d0
# 000136a0
# 00016000

# New image cases in cell_image_index: type 13 -> 100+(dir&3), 14 -> 15, 15 -> 0x74+(dir&1)
r2 -e scr.color=0 -qc 'pi 7 @ 0x4ad8' "$BIN" </dev/null 2>/dev/null
# addi r10, r8, 0x64
# b 0x4af0
# li r10, 0xf
# b 0x4af0
# clrlwi r2, r11, 0x1f
# addi r10, r2, 0x74
# mr r3, r10
# is_movable: 13, 15, or 3..9
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
# 28-byte cell stride in load_level_data (memset 0x1be4, x*0x1c, y*0x1a4)
r2 -e scr.color=0 -qc 'pi 1 @ 0x6d98; pi 2 @ 0x6dcc' "$BIN" </dev/null 2>/dev/null
# li r5, 0x1be4
# mulli r2, r2, 0x1c
# mulli r0, r0, 0x1a4

# Full checks: 52 blobs well-formed, all 128 images decode, which images differ from C1, palette equal
python3 -I -c '
import struct,sys
def ld(p,it,ni):
    B=open(p,"rb").read(); u=lambda v:struct.unpack(">I",B[v-0x1000:v-0xffc])[0]
    P=[u(it+4*i) for i in range(u(ni))]; I=[]; ok=0
    for a,nx in zip(P,P[1:]+[it]):
        o=[];v=a
        while len(o)<576:
            b=B[v-0x1000]
            if b<=0xc0: o.append(b);v+=1
            else: o+=[B[v-0xfff]]*(b-0xc0);v+=2
        ok+=len(o)==576 and nx-v<4 and not any(B[v-0x1000:nx-0x1000]); I.append(bytes(o))
    return B,u,I,ok
B,u,I2,ok=ld(sys.argv[1],0x11ce4,0x121e4); B1,_,I1,_=ld(sys.argv[2],0x126dc,0x12bdc)
L=[u(0xa934+4*i) for i in range(53)]; good=0
for i in range(1,53):
    a=L[i]
    while B[a-0x1000]: a+=5
    nx=min(x for x in L+[0xa934] if x>L[i]); good+= nx-(a+1)<4 and not any(B[a-0xfff:nx-0x1000])
print(good, len(I2), ok, [i for i in range(128) if I1[i]!=I2[i]], B[0x10ee4:0x111e4]==B1[0x118dc:0x11bdc])' "$BIN" "$B1"
# 52 128 128 [15, 100, 101, 102, 103, 116, 117] True
```
