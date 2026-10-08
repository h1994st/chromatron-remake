# Chromatron 1 v1.15 — Data formats

decompiled.c line numbers valid for Ghidra 12.1.4 with re/notes/chromatron1/*.names.tsv as of this commit.

Binary: `chromatron_mac/Chromatron.app/Contents/MacOS/chromatron1` (Mach-O, big-endian PowerPC).
All addresses are vmaddrs. File offset = vmaddr − 0x1000. All multi-byte values are big-endian.
Function names are the ones in `data.names.tsv` (applied by `just decompile 1`); in
`re/out/chromatron1/decompiled.c` each function starts with a line `// <name> @ <8-hex-digit addr>`.

`game.c` reaches its globals through the non-lazy pointer table `__DATA,__nl_symbol_ptr`
(0x12cf0–0x12d87). The pointers this note relies on (`r2 -qc 'pxw 4 @ <addr>'`):

| slot | → target | meaning |
|---|---|---|
| 0x12d10 | 0x14080 | `hint_text` (char buffer, bss) |
| 0x12d18 | 0x16420 | `site_url_text` (char buffer, bss) |
| 0x12d1c | 0x12bdc | `num_images` |
| 0x12d20 | 0x8110 | `level_order` |
| 0x12d28 | 0x14880 | `current_level_id` (int, bss) |
| 0x12d34 | 0x13c80 | `decoded_images` (128 × `u8*`, bss) |
| 0x12d40 | 0x128dc | `palette` |
| 0x12d6c | 0x14ab0 | `board` (bss) |
| 0x12d78 | 0xb700 | `hint_table` |
| 0x12d7c | 0xb604 | `level_table` |
| 0x12d80 | 0x126dc | `image_table` |

Terms used below:
- **level id**: the index into `level_table` (1–62).
- **slot**: the 0-based position in the level-select strip (0–49). The game shows it as slot+1. `current_slot` is the int at 0x803c, and `level_order[slot]` gives the id.
- **Colour mask**: bit 0 (1) = blue, bit 1 (2) = green, bit 2 (4) = red. So 3 = cyan, 5 = magenta, 6 = yellow, 7 = white and 0 = black/none.
- **Direction**: 0–7 = N, NE, E, SE, S, SW, W, NW (clockwise in 45° steps). The step tables are `dx` = [0,1,1,1,0,−1,−1,−1] (int32 at 0x8044) and `dy` = [−1,−1,0,1,1,1,0,−1] (int32 at 0x8064). The screen y axis points down. (Rotation and beam rules belong to `logic.md`.)

## Findings

### D1. Level table: address, count, record layout, loader
**Answer:**
- **`level_table` @ 0xb604**: 63 big-endian `u32` pointers, indexed by level id 0–62. Entry 0 is NULL (0x00000000). Ids 1–62 point to level blobs in 0x81d0–0xb603. The table ends where `hint_table` starts (0xb700).
- **Count**: 62 levels are encoded, but only **50 are playable**. `level_order` @ 0x8110 is a `u8[50]` that maps slot → level id:
  `01 02 0e 1a 1b 0c 1d 27 37 38 25 03 36 2a 05 20 10 1c 06 0d 29 11 22 1e 23 24 26 07 16 14 17 09 13 08 15 2c 2d 2e 0b 2b 39 31 3a 30 19 18 34 0a 3e 04`.
  The slot index is capped at 49 (`FUN_00005800`: `if (current_slot < 0x31)` before incrementing). The strip draws 50 numbers. Twelve ids appear in no slot and can't be reached: **15, 18, 31, 33, 40, 47, 50, 51, 53, 59, 60, 61**.
- **Level blob**: a list of **5-byte records**, ended by a single `0x00` byte where the next record's `type` would be. Zero padding then brings it to 4-byte alignment (the next blob or table is 4-aligned). So a blob is 5·n + 1 bytes plus 0–3 zero pad bytes. Checked for all 62 blobs: each terminator is followed only by zero bytes up to the next pointer.

  | rec offset | size | field | stored into cell byte |
  |---|---|---|---|
  | +0 | u8 | `type` (piece kind, see D2). 0 ends the list | +0 |
  | +1 | u8 | `dir` 0–7 | +2 |
  | +2 | u8 | `colour` mask | +1 |
  | +3 | u8 | `x` = column 0–14 | — (cell index) |
  | +4 | u8 | `y` = row 0–14 | — (cell index) |

  The field order in the record is **type, dir, colour, x, y**. In the cell it is **type, colour, dir**. Across all 62 levels, every x and y is 0–14 and no two records share a cell.
- **Loader**: `load_level_data` @ 0x69fc does the following:
  1. `strcpy(hint_text, hint_table[current_level_id])`.
  2. `memset(board, 0, 0x13ec)`, which clears rows 0–16 (see D2).
  3. For each record: `board[y*300 + x*20 + 0] = type; [+1] = colour; [+2] = dir`.

  It's called by `start_level` @ 0x5668(restore). That function calls `load_level_data()` first. If `restore == 0`, or there's no saved state for this id, it calls `move_pieces_to_toolbox` @ 0x54f8 (D3). Otherwise it `memcpy`s the saved 0x1644-byte state over `board`. Then it re-traces the beams and sets mode = 2.
- Per-level table (slot is 1-based as displayed; `records` doesn't count the terminator):

| slot | id | data @ | records | hint ptr |
|---|---|---|---|---|
| 1 | 1 | 0x81d0 | 3 | 0x6d40 |
| 2 | 2 | 0x81e0 | 7 | 0x6e08 |
| 3 | 14 | 0x8738 | 9 | 0x6f28 |
| 4 | 26 | 0x9218 | 9 | 0x709c |
| 5 | 27 | 0x9248 | 7 | 0x7134 |
| 6 | 12 | 0x85f4 | 13 | 0x6b78 |
| 7 | 29 | 0x94a4 | 65 | 0x7260 |
| 8 | 39 | 0x9b8c | 7 | 0x6b78 |
| 9 | 55 | 0xad5c | 71 | 0x7728 |
| 10 | 56 | 0xaec0 | 83 | 0x6b78 |
| 11 | 37 | 0x9a50 | 46 | 0x7460 |
| 12 | 3 | 0x8204 | 12 | 0x6b78 |
| 13 | 54 | 0xac34 | 59 | 0x6b78 |
| 14 | 42 | 0x9c6c | 15 | 0x752c |
| 15 | 5 | 0x82c8 | 22 | 0x6e74 |
| 16 | 32 | 0x9784 | 33 | 0x6b78 |
| 17 | 16 | 0x87c4 | 25 | 0x6b78 |
| 18 | 28 | 0x926c | 113 | 0x71f0 |
| 19 | 6 | 0x8338 | 9 | 0x6b78 |
| 20 | 13 | 0x8638 | 51 | 0x6ec4 |
| 21 | 41 | 0x9be4 | 27 | 0x6b78 |
| 22 | 17 | 0x8844 | 92 | 0x6b78 |
| 23 | 34 | 0x98e8 | 26 | 0x72e8 |
| 24 | 30 | 0x95ec | 20 | 0x6b78 |
| 25 | 35 | 0x996c | 36 | 0x6b78 |
| 26 | 36 | 0x9a24 | 8 | 0x7368 |
| 27 | 38 | 0x9b38 | 16 | 0x6b78 |
| 28 | 7 | 0x8368 | 27 | 0x6b78 |
| 29 | 22 | 0x8fb4 | 16 | 0x6ff8 |
| 30 | 20 | 0x8cdc | 119 | 0x6b78 |
| 31 | 23 | 0x9008 | 11 | 0x6b78 |
| 32 | 9 | 0x844c | 14 | 0x6b78 |
| 33 | 19 | 0x8ac4 | 107 | 0x6b78 |
| 34 | 8 | 0x83f0 | 18 | 0x6b78 |
| 35 | 21 | 0x8f30 | 26 | 0x6b78 |
| 36 | 44 | 0x9d68 | 32 | 0x7664 |
| 37 | 45 | 0x9e0c | 13 | 0x6b78 |
| 38 | 46 | 0x9e50 | 29 | 0x6b78 |
| 39 | 11 | 0x84e8 | 53 | 0x6b78 |
| 40 | 43 | 0x9cb8 | 35 | 0x75d8 |
| 41 | 57 | 0xb060 | 55 | 0x6b78 |
| 42 | 49 | 0xa060 | 143 | 0x6b78 |
| 43 | 58 | 0xb174 | 39 | 0x6b78 |
| 44 | 48 | 0x9f8c | 42 | 0x6b78 |
| 45 | 25 | 0x90bc | 69 | 0x6b78 |
| 46 | 24 | 0x9040 | 24 | 0x6b78 |
| 47 | 52 | 0xa964 | 83 | 0x6b78 |
| 48 | 10 | 0x8494 | 16 | 0x6b78 |
| 49 | 62 | 0xb57c | 27 | 0x6b78 |
| 50 | 4 | 0x8244 | 26 | 0x6b78 |

  The unreachable ids, as id (data @, records): 15 (0x8768, 18), 18 (0x8a14, 35), 31 (0x9654, 60), 33 (0x982c, 37), 40 (0x9bb0, 10), 47 (0x9ee4, 33), 50 (0xa32c, 188), 51 (0xa6dc, 129), 53 (0xab04, 60), 59 (0xb238, 85), 60 (0xb3e4, 30), 61 (0xb47c, 51). Their hint pointers are all 0x6b78 (the empty string).
  Movable pieces (types 3–9) in the unreachable ids: 15 → 11, 18 → 15, 31 → 9, 33 → 10, 40 → 5, 47 → 6, **50 → 80** (27 mirrors, 53 benders), **51 → 57** (24 mirrors, 33 benders), 53 → 8, 59 → 16, 60 → 7, 61 → 8. The most in any playable level is 20 (id 11). Ids 50 and 51 don't fit the original toolbox (D2/D3), so a decoder must **not** bound-check movable counts against the toolbox size. Treat the blob as valid and leave capacity to the game layer.

**Evidence:** `load_level_data @ 0x69fc` (decompiled.c, `// load_level_data @ 000069fc`):
```c
  pcVar4 = *(char **)(PTR_level_table_00012d7c + *(int *)PTR_current_level_id_00012d28 * 4);
  _strcpy(PTR_hint_text_00012d10,
          *(char **)(PTR_hint_table_00012d78 + *(int *)PTR_current_level_id_00012d28 * 4));
  puVar7 = _memset(PTR_board_00012d6c,0,0x13ec);
  cVar1 = *pcVar4;
  while (cVar1 != '\0') {
    iVar6 = (uint)(byte)pcVar4[4] * 300 + (uint)(byte)pcVar4[3] * 0x14;
    puVar5[iVar6] = cVar1;  puVar5[iVar6 + 1] = pcVar4[2];  puVar5[iVar6 + 2] = pcVar4[1];
    pcVar4 = pcVar4 + 5;  cVar1 = *pcVar4; }
```
(Condensed from the decompiled lines. The disassembly at 0x6a74–0x6a9c confirms the byte order: `lbzu r11,1(r10)` (dir) → `stb r11,2(r2)`; `lbzu r9` (colour) → `stb r9,1(r2)`; `r2` = x·0x14; `r0` = y·0x12c.)
Slot → id: `start_level` callers `FUN_00005714`, `FUN_0000576c` and `FUN_000057ac` do `current_level_id = level_order[current_slot]`. `FUN_00005f38` starts with `level_order[0]`.
Data: `xxd -s 0xa604 -l 16` shows `0000 0000 0000 81d0 0000 81e0 0000 8204`.
Worked check (level id 1 @ 0x81d0, `0a06040802 0202040207 0301040807 00`): red (4) laser at (2,7) facing E. A mirror at (8,7) with dir 1 sends the beam N into the red pinwheel at (8,2). That's the solution (see D3).
**Confidence:** confirmed.

### D2. Board: grid size and cell encoding
**Answer:**
- **Grid**: the board is a fixed **15 × 15** on every level. In memory, `board` @ 0x14ab0 (bss) is **19 rows × 15 columns × 20-byte cells** = 0x1644 bytes. Cell (x, y) is at `board + y*300 + x*20`. Rows 0–14 are the board. Rows 15–18 hold the toolbox as a flat list: toolbox slot s is at `board + 0x1194 + 20*s` (D3). Rows 15–18 give 60 addressable toolbox slots (0–59). The loader's `memset` clears only rows 0–16 (0x13ec bytes), i.e. toolbox slots 0–29. Only slots 0–23 are drawn or hit-tested (D3). Every **playable** level fills at most 20 slots (id 11). The unreachable ids don't all fit: **id 50 has 80 movable pieces and id 51 has 57**. Loading id 50 would make `move_pieces_to_toolbox` write slots 60–79 past the end of the 0x1644-byte `board`, and its `qsort` would read them too. Id 51 would fill slots 24–56, which can't be seen. So the original toolbox can't represent ids 50 or 51.
- **Cell layout (20 bytes)**:

  | off | size | field |
  |---|---|---|
  | +0 | u8 | `type` |
  | +1 | u8 | `colour` mask (lasers, filters, pinwheels; ignored for other types) |
  | +2 | u8 | `dir` 0–7 |
  | +3 | u8 | unused. Only the dead function `FUN_00003dd4` (no callers) clears it |
  | +4..+11 | u8[8] | runtime: colour mask of beams **entering** this cell, indexed by travel direction |
  | +12..+19 | u8[8] | runtime: colour mask of beams **leaving** this cell, indexed by travel direction |

  Bytes +4..+19 are zero after loading. The beam tracer (`FUN_00003a58` / `FUN_00003568`) fills them in; see `logic.md`.
- **`type` values**. Movable = `FUN_000034d8`: `type − 3 < 7`, i.e. types 3–9. Image indices refer to D5.

  | type | kind | uses `colour` | uses `dir` | image index |
  |---|---|---|---|---|
  | 0 | empty | – | – | 0 |
  | 1 | wall | – | – | 1 |
  | 2 | laser (emitter), fixed | 1/2/4/7 = blue/green/red/white | yes, 0–7 = beam direction | red 56+d, green 72+d, blue 88+d, white 104+d |
  | 3 | mirror ("REFLECTOR"), movable | – | 0–7 | 32+d |
  | 4 | bender, movable | – | 0–7 | 48+d |
  | 5 | filter, movable | 1/2/4 | `dir & 3` drawn | blue 8+(d&3), red 96+(d&3), green 112+(d&3) |
  | 6 | prism, movable | – | 0–7 | 24+d |
  | 7 | doppler, movable | – | 0–7 | 64+d |
  | 8 | splitter, movable | – | `dir & 3` drawn | 40+(d&3) |
  | 9 | quantum tangler, movable | – | 0–7 | 80+d |
  | 10 | pinwheel (target), fixed | required mask 0–7 | – | 0→12, 1→6, 2→4, 3→18, 4→2, 5→20, 6→16, 7→22; +1 when satisfied |
  | 11 | conduit, fixed | – | `dir & 3` = axis: 0 = N–S, 1 = NE–SW, 2 = E–W, 3 = NW–SE | 44+(d&3) |
  | 12 | teleporter, fixed | – | – | 14 |

  The tracer passes a beam through a conduit only when `(beam_dir − dir) & 7` is 0 or 4. A **pinwheel is satisfied** when `colour == OR(cell[+4..+11])` (`FUN_00003ecc`). So **black (0)** means no light at all may arrive, and **white (7)** needs red, green and blue. (Full semantics: `logic.md`.)
- **No fixed movable pieces exist in C1.** Every type 3–9 record is moved to the toolbox when the level starts (D3), so the fixed elements of a level are only walls, lasers, pinwheels, conduits and teleporters.
- **Data noise**: the `colour` byte of types that ignore it is usually 4 (sometimes 1, 2, 5 or 7). The `dir` byte of walls, pinwheels and teleporters is usually 0 (sometimes 1–7). Neither has any effect, but both are copied into cells and the save file, so keep them byte-exact.
- Record counts over all 62 blobs: wall 1049, laser 108, mirror 221, bender 269, filter 9, prism 15, doppler 45, splitter 46, tangler 14, pinwheel 557, conduit 171, teleporter 140.
- **Screen position** (from `FUN_00006128`): board cell (x, y) is centred at (60 + 24x, 30 + 24y). Toolbox slot s is centred at (460 + 26·(s mod 6), 20 + 26·(s div 6)). Each 24×24 image is drawn with its top-left at centre − 12.

**Evidence:** `cell_image_index @ 0x45d4` (type switch → image index). For example:
```c
  case 2:  bVar3 = param_1[1];  if (bVar3 == 2) lVar5 = uVar6 + 0x48;  ... (bVar3 == 4) lVar5 = uVar6 + 0x38; ...
  case 5:  ... (bVar3 == 2) lVar5 = uVar2 + 0x70; ... (bVar3 == 1) lVar5 = uVar2 + 8; ... (bVar3 == 4) lVar5 = uVar2 + 0x60;
  case 10: switch(param_1[1]) { case 0: lVar5 = 0xc; ... }  ...  if (param_1[1] == bVar3) lVar5 = lVar5 + 1;
  case 0xb: lVar5 = uVar2 + 0x2c;      case 0xc: lVar5 = 0xe;
```
(`uVar6` = dir, `uVar2` = dir & 3, `bVar3` in case 10 = OR of +4..+11.) `FUN_00003d60` clears +4..+11 and +12..+19 of 15 × 15 cells. `FUN_00003568` reads `PTR_board_00012d6c[param_2 * 300 + param_1 * 0x14]` and ORs the colour into `[param_3 + iVar4 + 4]`. `move_pieces_to_toolbox` uses `board + 0x1194` as the toolbox. The save code copies 0x1644 bytes (`start_level`, `FUN_000055f4`).
**Confidence:** confirmed for layout, types, colour/dir usage and image mapping. Piece names come from the matching tutorial strings and the sprite artwork (D5). Behaviour is in `logic.md`.

### D3. Toolbox: how available pieces and counts are stored
**Answer:** The toolbox has **no separate table**. A level's available pieces are exactly its records whose type is movable (3–9). Their stored `x`, `y` and `dir` are the **solution placement**. Exception: level id 32 (slot 16), whose mirror is stored at (12,2) dir 6 but the level is solved with it at (12,4) dir 0 (logic.md simulator check; two independent tracers (Task 7 implementer and reviewer, throwaway scripts) agree). There are no counts: each record is one piece in its own toolbox slot. The visible toolbox is 6 × 4 = 24 slots. The evidence is the draw loop in `FUN_000065d0` (decompiled.c lines 2919–2923, `FUN_00006128(lVar4,0xf,0)` while `lVar4 < 0x18`) and the 6-wide × 4-high hit-test in `FUN_000050d4`. The most any playable level uses is 20 (id 11). The unreachable ids 50 (80 pieces) and 51 (57) overflow it (see D2). A decoder must report their pieces as-is and not reject or clamp them.
When a level starts without saved state (or on restart via the `r` key / "Reset Level" (command `rvrt`)), `move_pieces_to_toolbox` @ 0x54f8 runs this:
1. `next = 0` (toolbox slot). For each board cell `c = 0 .. 224` in row-major order (y outer, x inner): if `type(c)` is in 3..9, then:
   - while `toolbox[next].type != 0`, do `next += 1`;
   - copy all 20 bytes of the cell to `toolbox[next]`;
   - set `toolbox[next].dir = 0` and `board[c].type = 0` (the other board bytes stay).
2. `qsort(toolbox, n, 20, cmp)`, where `n` = the index of the last slot filled, plus 1. If nothing moved, `n` = 1. `cmp` @ 0x54e8 is `return a.type − b.type` (it compares **only** `type`).
3. Re-trace the beams and redraw.

**Ordering caveat:** `qsort` isn't stable and comes from the host libSystem. The order of same-type pieces with different `colour` bytes therefore depends on the qsort implementation. You can only see this for **filters**, in slots 15 (id 5) and 17 (id 16). With a stable sort, the filter order is 4, 1, 2 (red, blue, green) for slot 15 and 4, 1, 2 for slot 17. My Python re-implementation of the 4.4BSD/FreeBSD `qsort` (med3 pivot, swap_cnt insertion-sort fallback) gives 1, 2, 4 for slot 15 and 1, 4, 2 for slot 17. Other levels also differ, but only in invisible colour-noise bytes (D2).

**Ruling for the port:** follow BSD qsort (Mac OS X 10.2/10.3 libc is FreeBSD-derived) — filter order 1,2,4 in slot 15 and 1,4,2 in slot 17.

**Evidence:** `move_pieces_to_toolbox @ 0x54f8`:
```c
  pcVar15 = PTR_board_00012d6c + 0x1194;  lVar12 = 0xe0;
  do { iVar9 = FUN_000034d8(PTR_board_00012d6c[iVar14]);
    if (iVar9 != 0) { if (*pcVar15 != '\0') { do { ...; iVar10++; pcVar15 += 0x14; iVar11 += 0x14; } while (*pcVar7 != '\0'); }
      /* copy 5 words board[iVar14] -> board[iVar11 + 0x1194] */
      puVar6[iVar11 + 0x1196] = 0;  puVar6[iVar14] = 0; }
    lVar12 = lVar12 + -1;  iVar14 = iVar14 + 0x14; } while (-1 < lVar12);
  _qsort(PTR_board_00012d6c + 0x1194,iVar10 + 1,0x14,DAT_0000810c);
```
`0x810c` holds 0x000054e8, and the disassembly there is `lbz r0,0(r3); lbz r3,0(r4); subf r3,r3,r0; blr`. Level 1 check: the solution mirror at (8,7) with dir 1 reflects the eastbound red beam (rel = (2−1)&7 = 1 → out dir (1+7)&7 = 0 = N) into the pinwheel at (8,2).
**Confidence:** confirmed for the storage model and algorithm. **likely** for the filter order in slots 15 and 17 (it depends on the original host's libc qsort). Watching those two toolboxes in the original game would settle it.

### D4. Level names and tutorial/hint text
**Answer:** Levels have **no names**. The only identifier shown is the slot number, 1–50 (level strip and clipboard prefix `1-NN-`). Hint text comes from **`hint_table` @ 0xb700**: 63 big-endian `u32` pointers into `__cstring`, indexed by **level id** (parallel to `level_table`). Entry 0 is NULL, and every level without a hint points to 0x6b78, which is an empty string (the `__cstring` section starts with a NUL byte). `load_level_data` copies the string into `hint_text` @ 0x14080, a 2048-byte bss buffer. `FUN_000066e0` draws it with `DrawThemeTextBox` in rect left 450, top 125, right 620, bottom 475. When the level is won (mode 2 and win flag set), it draws "Click on a level or press spacebar for next." there instead.

Slots that show a hint (all others show nothing):

| slot | id | string @ | starts with |
|---|---|---|---|
| 1 | 1 | 0x6d40 | Drag the REFLECTOR in the toolbox above onto the board and p… |
| 2 | 2 | 0x6e08 | To complete a level, every pinwheel must be hit by light of … |
| 3 | 14 | 0x6f28 | Some pinwheels require multiple lasers to light them up. You… |
| 4 | 26 | 0x709c | If a laser hits a SPLITTER at the correct angle, it bounces … |
| 5 | 27 | 0x7134 | Mirrors and lenses generally work in both directions. The sp… |
| 7 | 29 | 0x7260 | The BENDER is an angled reflector. Because of its angle, it … |
| 9 | 55 | 0x7728 | The conduits on the map will only allow light to pass in one… |
| 11 | 37 | 0x7460 | You can now try two different levels by clicking on the numb… |
| 14 | 42 | 0x752c | The white laser combines red, blue, and green light in a sin… |
| 15 | 5 | 0x6e74 | The FILTER only allows one color of light through--try them … |
| 18 | 28 | 0x71f0 | The PRISM bends red, green, and blue light differently when … |
| 20 | 13 | 0x6ec4 | Like splitters, you can also send light backwards through a … |
| 23 | 34 | 0x72e8 | The doppler turns red beams into green, green into blue, and… |
| 26 | 36 | 0x7368 | The quantum tangler takes a beam in on one side and outputs … |
| 29 | 22 | 0x6ff8 | When a quantum-entangled beam goes through a splitter or ano… |
| 36 | 44 | 0x7664 | The TELEPORTER causes laser beams to jump instantaneously to… |
| 40 | 43 | 0x75d8 | If there are any black pinwheel targets, you don't win the l… |

That's 17 hint strings, each referenced exactly once. Their full text is in `re/out/chromatron1/strings.tsv` (rows 0x6d40–0x7728; the `xrefs` column there gives the `hint_table` entry address, i.e. 0xb700 + 4·id). The text is plain ASCII: the `CFStringCreateWithCString` call in `FUN_00003228` uses encoding 0x0600, which is ASCII.
**Evidence:** the `load_level_data` excerpt in D1 (the `_strcpy` from `PTR_hint_table_00012d78`). Data: `xxd -s 0xa700 -l 16` shows `0000 0000 0000 6d40 0000 6e08 0000 6b78` (id 0 NULL, id 1 REFLECTOR, id 2 "To complete…", id 3 empty). `FUN_000066e0`:
```c
  pcVar4 = PTR_hint_text_00012d10;
  if ((*(int *)puVar3 == 2) && (*(int *)PTR_DAT_00012d4c != 0)) {
    pcVar4 = "Click on a level or press spacebar for next.";
  }
  FUN_00003228(pcVar4,0x1c2,0x7d,0x26c,0x1db);
```
**Confidence:** confirmed.

### D5. Images/sprites
**Answer:**
- **`num_images` @ 0x12bdc** = `u32` **128**.
- **`image_table` @ 0x126dc**: 128 big-endian `u32` pointers to compressed images in **`image_data` 0xb7fc–0x126db**. Image 0 is at 0xb7fc and image 127 at 0x1264c. Each stream is followed by 0–3 zero pad bytes up to the next 4-aligned pointer, and image 127 ends at the table. In total that's 28384 bytes including padding.
- **No header.** Every image is **24 × 24, 8 bits per pixel, palette-indexed**, row-major and top row first: 576 = 0x240 decoded bytes. The palette is `palette` (D6).
- **Compression: PCX-style RLE** (`decode_images` @ 0x68e8, which runs once at startup and goes from image 127 down to 0):
  ```
  decode(src) -> [u8; 576]:
      out = []; i = 0
      while len(out) < 576:
          b = src[i]
          if b <= 0xC0:            # literal (0xC0 itself is a literal)
              out.push(b); i += 1
          else:                    # run of (b - 0xC0) copies of src[i+1], 1..63
              v = src[i+1]
              repeat (b - 0xC0) times: if len(out) == 576: break; out.push(v)
              i += 2
      return out
  ```
  Pixel values ≥ 0xC1 can only be stored as a run, e.g. `C1 FC` for one pixel of value 252. In the shipped data no run crosses the 576-byte end, no literal 0xC0 occurs, and the longest run is 25 (0xD9). Each decoded image is `malloc(0x240)` and its pointer stored in `decoded_images[i]` @ 0x13c80.
- **Blit**: `blit_indexed` @ 0x47e0 takes `(dx, dy, src, w, h, stride, min_index, pal_offset)` and writes into the 640 × 480 RGB24 framebuffer (`fb` = `*(0x12dcc)`, width `*(0x12dd0)` = 640, height `*(0x12dd4)` = 480):
  ```
  if dx < 0: w += dx; dx = 0        # src NOT advanced (left/top clip shifts the image)
  if dy < 0: h += dy; dy = 0
  if dx + w > 640: w = 640 - dx
  if dy + h > 480: h = 480 - dy
  for r in 0..h: for c in 0..w:
      p = src[r*stride + c]
      if p >= min_index:                      # smaller indices are transparent
          fb[((dy+r)*640 + dx + c)*3 .. +3] = palette_rgb[p + pal_offset]
  ```
- **Callers and their parameters**:
  - `draw_cell_image` @ 0x6048(cell, cx, cy, min) does `i = cell_image_index(cell)`. If `i < 0` it returns. Otherwise it asserts `i < num_images` (game.c line 0xa5d) and calls `blit(cx-12, cy-12, decoded[i], 24, 24, 24, min, 0)`. The board pass uses min = 0 (opaque). Cells with beams are drawn again with min = 0x10, so only pixels ≥ 16 cover the beams.
  - The dragged piece is drawn with min = 0x0e (`FUN_00004e00`).
  - Digits are drawn with w = h = 12, stride 24, min = 0x0e (D7).
- **Index map** (from `cell_image_index` and visual inspection of a decoded sheet). 
  - 0: empty floor tile. Pixels are index 8, plus index 6 on the top row and left column (the grid line).
  - 1: wall.
  - Pinwheels (off/on): 2/3 red, 4/5 green, 6/7 blue, 12/13 black, 16/17 yellow, 18/19 cyan, 20/21 magenta, 22/23 white.
  - Filters: 8–11 blue, 96–99 red, 112–115 green (`dir & 3`).
  - Pieces: 14 teleporter, 24–31 prism, 32–39 mirror, 40–43 splitter, 44–47 conduit, 48–55 bender, 64–71 doppler, 80–87 tangler.
  - Lasers: 56–63 red, 72–79 green, 88–95 blue, 104–111 white.
  - 118–127: digit glyphs '0'–'9'.
  - **Unused**: 15 (white spoked disc), 100/102/103/116/117 (floor tile + one pixel 252) and 101 (copy of image 2 + one pixel 252).
**Evidence:** `decode_images @ 0x68e8`:
```c
      pbVar11 = *(byte **)(PTR_image_table_00012d80 + iVar3);
      pvVar7 = _malloc(0x240);
        bVar2 = *pbVar11;  uVar8 = (uint)bVar2;  pbVar12 = pbVar11 + 1;
        if (bVar2 < 0xc1) { *(byte *)(*(int *)(puVar4 + iVar3) + iVar9) = bVar2; iVar9 = iVar9 + 1; }
        else { do { uVar8 = uVar8 - 1; *(byte *)(... + iVar9) = *pbVar12; iVar9 = iVar9 + 1;
                    if ((int)uVar8 < 0xc1) break; } while (iVar9 < 0x240);
               pbVar12 = pbVar11 + 2; }
        bVar1 = 0x23f < iVar9;
```
`draw_cell_image @ 0x6048`: `FUN_000047e0(param_2 + -0xc,param_3 + -0xc, *(...)(PTR_decoded_images_00012d34 + idx*4),0x18,0x18,0x18,param_4,0)`.
Data: `xxd -s 0x116dc -l 8` shows `0000 b7fc 0000 b844`. Image 0 starts `d9 06 d7 08 06 d7 08 06 …`, i.e. 25×6 (the whole top row plus the first pixel of row 1), then 23×8, then a literal 6 for the start of the next row, and so on. I decoded all 128 images with this algorithm: each stream ends exactly at the start of its zero padding before the next pointer.
**Confidence:** confirmed (algorithm, sizes, table). The meanings of the unused images are unknown; nothing references them.

### D6. Colours
**Answer:**
- **`palette` @ 0x128dc**: 256 entries × 3 bytes (0x300 bytes, up to 0x12bdc) stored as **B, G, R**. At startup `decode_images` swaps bytes 0 and 2 of every entry in place, so the in-memory palette is **R, G, B**. (Each step of the swap loop at 0x69c0 copies exactly 3 bytes (`lswi r0,r2,3` / `stswi r0,r9,3` into a stack temp), then writes them back reversed (`lbz 0x42/0x41/0x40` → `stb 0/1/2(r2)`). It never reads past the palette. The `& 0xffffff00` in the decompiled C is an artifact of how Ghidra models `lswi`.) Every value is a multiple of 4 (6-bit VGA DAC × 4). Some RGB entries: 0 = 000000, 1 = 0000a8, 2 = 00a800, 4 = a80000, 6 = 58442c (grid line), 8 = 5c5c5c (floor), 16–31 = grey ramp fcfcfc → 0c0c0c (step 0x10), 32–47 red ramp fc0000 → 0c0000, 48–63 yellow ramp, 64–79 green ramp 00fc00 → 000c00, 80–95 cyan ramp 00fcfc → 000c0c, 96–111 blue ramp 2828fc → 00000c, 112–127 magenta ramp, 252 = 000000, 255 = fcfcfc. To see all 256, run `xxd -s 0x118dc -l 768 "$BIN"` and swap bytes 0 and 2 of each triple.
- **Beam colours: `beam_colors` @ 0x80d4**, `u32[8]` 0x00RRGGBB indexed by colour mask: 0 000000, 1 **0000ff** (blue), 2 **00c800** (green), 3 00c8ff (cyan), 4 **ff0000** (red), 5 ff00ff (magenta), 6 ffc800 (yellow), 7 ffffff (white). Green is 200, not 255. The beam renderer (`FUN_000061ec`) draws each half-segment of a cell in `beam_colors[out[d] | in[(d+4)&7]]`. The line routine `FUN_00004b28` writes R, G, B = `c>>16`, `c>>8`, `c`.
- **Pinwheels, lasers, pieces**: no separate colour data. Their colours come from the sprite pixels through `palette` (pinwheel colour → image, see D2/D5).
- **Background**: the framebuffer is cleared with `memset(fb, 0xa4, 640*480*3)`, i.e. RGB **a4a4a4** (`FUN_000065d0`). Board and toolbox cells are opaque floor tiles (palette 8 = 5c5c5c with grid line palette 6 = 58442c).
- **Level-number strip** (UI): digits are drawn with `pal_offset` from `FUN_000064a0(slot)`. Pixels of a digit glyph with index ≥ 14 (in practice 16–31) are drawn as `palette[p + off]`:

  | slot state | off | effect |
  |---|---|---|
  | current slot | 0x00 | grey ramp |
  | completed (`completed[slot]` @ 0x149a0 ≠ 0) | 0x48 | indices 88–103: dark cyan → blue |
  | unlocked (`unlocked[slot]` @ 0x14890 ≠ 0) | 0x30 | indices 64–79: green ramp |
  | locked | 0x10 | indices 32–47: red ramp |

  This matches the tutorial text "blue levels are complete, green levels are open, and red levels are inaccessible".
- **Text** (`DrawThemeTextBox`): no colour is set anywhere in the binary, so it uses the Carbon theme's default text colour (black on Aqua).
**Evidence:** `decode_images @ 0x68e8` (palette swap):
```c
  lVar13 = 0x100;  puVar6 = (uint *)PTR_palette_00012d40;
  do { uVar8 = *puVar6 & 0xffffff00;
    *(undefined1 *)puVar6 = (undefined1)(uVar8 >> 8);
    *(undefined1 *)((int)puVar6 + 1) = (undefined1)(uVar8 >> 0x10);
    *(undefined1 *)((int)puVar6 + 2) = (undefined1)(uVar8 >> 0x18);
    puVar6 = (uint *)((int)puVar6 + 3);  lVar13 = lVar13 + -1; } while (lVar13 != 0);
```
(Condensed: the decompiler's `uStack_2e` etc. are inlined.) `blit_indexed` writes `fb[0..2] = palette[(p+off)*3 + 0..2]`. `CGImageCreate(w, h, 8, 0x18, w*3, DeviceRGB, 0, …)` in `FUN_000030cc` makes the framebuffer RGB, so the in-memory palette is RGB. A decoded sprite sheet rendered with the swapped palette shows red/green/blue lasers and pinwheels in their named colours. `FUN_000064a0` returns 0, 0x48, 0x30 or 0x10. `FUN_000065d0` calls `_memset(DAT_00012dcc,0xa4,DAT_00012dd0 * DAT_00012dd4 * 3)`.
**Confidence:** confirmed for palette, beam table, background and digit offsets. **likely** for the text colour: the binary sets none, so it's the system theme default.

### D7. Fonts
**Answer:** Only one glyph set is stored in data: the **digits 0–9 = images 118–127**, a 12 × 12 glyph in the top-left corner of each 24 × 24 image. The rest of each image is the floor tile, which isn't drawn. `draw_digit_string` @ 0x63fc(x, y, str, pal_offset) handles them:
- for each char `ch`: if `'0' ≤ ch ≤ '9'`, it calls `blit(x, y, decoded[118 + ch−'0'], 12, 12, 24, min=0x0e, pal_offset)` and does `x += 8`;
- otherwise (a space, which replaces a leading '0') it does `x += 4`.

It's used only for the level strip. Number n = 1..50 is formatted as two chars (tens digit '0' → ' ') and drawn at x = 10 + 20·((n−1) mod 25), y = 410 + 20·((n−1) div 25).
**All other text** (hint, "You win!", "(won)", "Click on a level…", "freeware", "more levels @", and the decoded "silverspaceship.com") goes through `FUN_00003228` → `DrawThemeTextBox(str, fontID 0 = kThemeSystemFont, state 1 = active, wrapToWidth = true, rect, just 0, ctx)`. The rects are a platform topic (`platform.md`).
**Evidence:** `draw_digit_string @ 0x63fc`:
```c
    if ((int)cVar1 - 0x30U < 10) {
      FUN_000047e0(param_1,param_2,
                   *(undefined4 *)(PTR_decoded_images_00012d34 + ((int)cVar1 - 0x30U) * 4 + 0x1d8),0xc,0xc,0x18
                   ,0xe,param_4);
      param_1 = param_1 + 8;
    }
    else { param_1 = param_1 + 4; }
```
(0x1d8 / 4 = 118.) `FUN_00003228` calls `_DrawThemeTextBox(uVar2,0,1,1,&local_40,0,local_30[0])`.
**Confidence:** confirmed.

### D8. Map of `__DATA,__data` (0x8000–0x12bdf)
**Answer:**

| range | size | purpose |
|---|---|---|
| 0x8000–0x800f | 16 | C runtime: `_NXArgc`, `_NXArgv`, `_environ`, `___progname` (→ 0x6b78) |
| 0x8010–0x801f | 16 | crt/dyld glue pointers: 0x13a24, 0x26c0, 0x25a4, 0x2520 (used by `FUN_000022f4` / `FUN_000026c0`) |
| 0x8020 | 4 | int `UseMouseWheelToChangeLevel` pref value, initially 1 |
| 0x8024–0x803b | 24 | 6 Carbon event-handler function pointers: 0x2c9c, 0x2e58, 0x3044, 0x32ec, 0x2dec, 0x2dc8 (installed by `FUN_00002740`) |
| 0x803c | 4 | int `current_slot`, initially 0 |
| 0x8040 | 4 | int "full redraw needed" flag, initially 1 (set by `FUN_000034b8`, tested by `FUN_000066e0`) |
| 0x8044–0x8063 | 32 | int32 `dx[8]` = 0,1,1,1,0,−1,−1,−1 |
| 0x8064–0x8083 | 32 | int32 `dy[8]` = −1,−1,0,1,1,1,0,−1 |
| 0x8084–0x8097 | 20 | int32[5] = 0,2,4,0,1: colour-bit map for an entangled twin (one rotation; used by `FUN_00003a58` via 0x8087 + 4·mask) — see `logic.md` |
| 0x8098–0x80ab | 20 | int32[5] = 0,4,1,0,2: the opposite rotation (0x809b + 4·mask) — see `logic.md` |
| 0x80ac–0x80cb | 32 | u8 permutation of 0–31 (`00 09 0d 13 15 0a …`): bit scrambler for the save file's 32-byte progress bitmap (`FUN_00003f00`) — see `platform.md` |
| 0x80cc | 4 | int −1 (0xffffffff), **unknown**: no references |
| 0x80d0 | 4 | int dragged-piece image index, initially −1 (`FUN_00004f4c` / `FUN_000050d4` / `FUN_00004e00`) |
| 0x80d4–0x80f3 | 32 | `beam_colors` u32[8] (D6) |
| 0x80f4–0x8107 | 20 | `site_url_xor`: XOR-chained string. `out[i] = in[i] ^ in[i−1]` (with `in[−1] = 0`), stop when `out[i] == 0`. It decodes to "silverspaceship.com" (19 chars + NUL) into `site_url_text` @ 0x16420 the first time `FUN_000066e0` runs, which draws it at bottom right |
| 0x8108 | 4 | qsort comparator pointer → 0x58a0, used by the clipboard copy/paste code `FUN_00005914`/`FUN_00005ad4` (see `platform.md`). Its elements are **6-byte clipboard entries**, not 20-byte board cells: {u16 cell index = row·15 + col, u8 dir, u8 type, u8 colour, u8 pad}, built at decompiled.c lines 2420–2445 (`qsort(&local_1a0, n, 6, …)`). It compares `type` (byte 3), then `colour` (byte 4), unsigned |
| 0x810c | 4 | qsort comparator pointer → 0x54e8 (by type; toolbox sort, D3) |
| 0x8110–0x8141 | 50 | `level_order` u8[50] (D1) |
| 0x8142–0x81cf | 142 | `01 01` followed by 140 zero bytes. **Unknown/unreferenced**. 0x8110 + 0xc0 = 0x81d0, so `level_order` was probably declared as a 192-byte array with 52 initialisers |
| 0x81d0–0xb603 | 0x3434 | `level_data`: 62 level blobs (D1) |
| 0xb604–0xb6ff | 252 | `level_table` u32[63] (D1) |
| 0xb700–0xb7fb | 252 | `hint_table` u32[63] (D4) |
| 0xb7fc–0x126db | 0x6ee0 | `image_data`: 128 RLE images (D5) |
| 0x126dc–0x128db | 512 | `image_table` u32[128] (D5) |
| 0x128dc–0x12bdb | 768 | `palette` 256 × BGR (D6) |
| 0x12bdc–0x12bdf | 4 | `num_images` = 128 |

Nothing else is in the section. Related runtime (bss/common, not in `__data`):
- `board` 0x14ab0 (0x1644 bytes);
- `decoded_images` 0x13c80 (128 × 4);
- `hint_text` 0x14080 (2048 bytes);
- `current_level_id` 0x14880;
- `unlocked[64]` 0x14890 and `completed[64]` 0x149a0, both int, indexed by slot;
- `win flag` 0x14aa0;
- per-level saved-state pointers `saved[128]` 0x16210, indexed by level id, each a 0x1644-byte board copy;
- `site_url_text` 0x16420;
- framebuffer pointer 0x12dcc (malloc 0xe1000 = 640·480·3).

**Evidence:** I dumped every Ghidra reference with a target in 0x8000–0x13a20 (an ad hoc headless script). 0x80cc and 0x8142–0x81cf have no incoming references. 0x8087/0x809b are referenced from `FUN_00003a58` @ 0x3d08 (`iVar10 = mask*4 + 0x8098` / `+ 0x8084`, low byte at +3). The other in-section references (e.g. 0xc319 ← 0xd304) are false pointer hits inside image bytes. `FUN_000066e0` decoder:
```c
    pbVar8 = &site_url_xor;  pbVar6 = PTR_site_url_text_00012d18;  bVar7 = 0;
    do { bVar1 = *pbVar8;  pbVar8 = pbVar8 + 1;
      *pbVar6 = bVar1 ^ bVar7;  bVar2 = *pbVar6;  pbVar6 = pbVar6 + 1;  bVar7 = bVar1;
    } while (bVar2 != 0);
```
**Confidence:** confirmed for every range except 0x80cc and 0x8142–0x81cf, which are **unknown**. They're unreferenced, so they don't matter to a reimplementation. Reading the original source would settle them. The 0x8084/0x8098 semantics are only summarised here; `logic.md` owns them.

## Verification
```bash
BIN=chromatron_mac/Chromatron.app/Contents/MacOS/chromatron1

# Level-table header (level_table @ 0xb604 = file 0xa604): id0 NULL, id1 0x81d0, id2 0x81e0, id3 0x8204
xxd -s 0xa604 -l 16 "$BIN"
# 0000a604: 0000 0000 0000 81d0 0000 81e0 0000 8204  ................

# First level record(s): id 1 @ 0x81d0 = file 0x71d0 -> 3 records + 0x00 terminator
#   0a 06 04 08 02 = pinwheel, dir 6, red, (8,2)
#   02 02 04 02 07 = laser, E, red, (2,7)
#   03 01 04 08 07 = mirror, dir 1, (8,7)      00 = end
xxd -s 0x71d0 -l 16 "$BIN"
# 000071d0: 0a06 0408 0202 0204 0207 0301 0408 0700  ................

# Same bytes via r2 (vmaddr)
r2 -qc 'p8 16 @ 0x81d0' "$BIN" 2>/dev/null
# 0a060408020202040207030104080700

# Slot -> level id order (level_order @ 0x8110)
xxd -s 0x7110 -l 50 "$BIN"
# 00007110: 0102 0e1a 1b0c 1d27 3738 2503 362a 0520  .......'78%.6*. 
# 00007120: 101c 060d 2911 221e 2324 2607 1614 1709  ....).".#$&.....
# 00007130: 1308 152c 2d2e 0b2b 3931 3a30 1918 340a  ...,-..+91:0..4.
# 00007140: 3e04                                     >.

# Hint table header (hint_table @ 0xb700)
xxd -s 0xa700 -l 16 "$BIN"
# 0000a700: 0000 0000 0000 6d40 0000 6e08 0000 6b78  ......m@..n...kx

# num_images and the first image-table entries (image_table @ 0x126dc)
xxd -s 0x11bdc -l 4 "$BIN"
# 00011bdc: 0000 0080                                ....
xxd -s 0x116dc -l 8 "$BIN"
# 000116dc: 0000 b7fc 0000 b844                      .......D

# First image "header" = start of image 0's RLE stream (no header; 24x24 implied): D9 06 = 25 x idx 6, D7 08 = 23 x idx 8, 06 = literal
xxd -s 0xa7fc -l 16 "$BIN"
# 0000a7fc: d906 d708 06d7 0806 d708 06d7 0806 d708  ................

# Palette (BGR on disk): entries 0..7
xxd -s 0x118dc -l 24 "$BIN"
# 000118dc: 0000 00a8 0000 00a8 00a8 a800 0000 a8a8  ................
# 000118ec: 00a8 2c44 58a8 a8a8                      ..,DX...

# Beam colours (u32 0x00RRGGBB by mask 0..7)
xxd -s 0x70d4 -l 32 "$BIN"
# 000070d4: 0000 0000 0000 00ff 0000 c800 0000 c8ff  ................
# 000070e4: 00ff 0000 00ff 00ff 00ff c800 00ff ffff  ................

# The nl_symbol_ptr slots used by game.c point at these tables
r2 -qc 'p8 4 @ 0x12d7c; p8 4 @ 0x12d78; p8 4 @ 0x12d80; p8 4 @ 0x12d40; p8 4 @ 0x12d1c; p8 4 @ 0x12d20; p8 4 @ 0x12d6c' "$BIN" 2>/dev/null
# 0000b604
# 0000b700
# 000126dc
# 000128dc
# 00012bdc
# 00008110
# 00014ab0

# Names applied (after `just decompile 1`)
grep -c -E '^// (load_level_data|start_level|move_pieces_to_toolbox|decode_images|blit_indexed|cell_image_index|draw_cell_image|draw_digit_string) @ ' re/out/chromatron1/decompiled.c
# 8

# Optional full decode check: all 128 images RLE-decode to 576 bytes ending exactly at their zero padding
python3 -I -c '
import struct,sys
B=open(sys.argv[1],"rb").read(); u=lambda v:struct.unpack(">I",B[v-0x1000:v-0xffc])[0]
P=[u(0x126dc+4*i) for i in range(u(0x12bdc))]+[0x126dc]; ok=0
for a,nx in zip(P,P[1:]):
    n=0;v=a
    while n<576:
        b=B[v-0x1000]
        if b<=0xc0: n+=1;v+=1
        else: n+=b-0xc0;v+=2
    ok+= n==576 and nx-v<4 and not any(B[v-0x1000:nx-0x1000])
print(len(P)-1,ok)' "$BIN"
# 128 128
```
