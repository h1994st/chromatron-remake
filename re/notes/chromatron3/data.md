# Chromatron 3 v1.1 — Data formats (diff against Chromatron 2)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 3.app/Contents/MacOS/chromatron3` (Mach-O, big-endian PowerPC, 94064 bytes, SHA-256
`a4e22d83e4d25d7d4379c449a23b7201b74eb0d5bd1a2ad2811c1aa9cfb9f7bb`). All addresses are vmaddrs. File offset = vmaddr − 0x1000.
All multi-byte values are big-endian. Function names are the ones in `re/notes/chromatron3/*.names.tsv`; each C3 function
keeps the name of its C2 equivalent. This note cites functions by **address**, not by decompiled.c line.
Baseline: `re/notes/chromatron2/data.md` (C2), which is itself a diff against `re/notes/chromatron1/data.md` (C1). Only
differences are spelled out; "same as C2" means byte-identical data or the same algorithm with only addresses changed.
"Slot N" = 1-based position on the level strip; `current_slot` (0x803c) = N − 1.

Sections: `__TEXT,__cstring` 0x78ac–0x7fab, `__TEXT,__const` 0x7fac–0x7ff3, `__TEXT,__literal8` 0x7ff8,
`__DATA,__data` 0x8000–0x163e3, `__nl_symbol_ptr` 0x164f4–0x1658f, `__bss` 0x165ac–0x1722d, `__common` 0x17230–0x1a54f.

Function correspondence: every C2 function has a C3 counterpart of the same size in the same order (except the
logic functions listed in logic.md). A normalised textual diff (addresses and identifiers masked) of all 79 pairs shows
these as **identical to C2**: `decode_images` 0x6de8, `load_level_data` 0x6efc (only line-wrapping differs),
`move_pieces_to_toolbox` 0x59ec, `start_level` 0x5b5c, `blit_indexed` 0x4cc4, `draw_slot_cell` 0x661c,
`draw_cell_beams` 0x66e0, `draw_digit_string` 0x68f0, `strip_pal_offset` 0x6994, `render_scene` 0x6ac4,
`paint_window` 0x6bd4. `draw_cell_image` 0x653c differs only in its assert line numbers (game.c 0xa7e / 0xa7f instead
of 0xa5c / 0xa5d). **`cell_image_index` 0x4a54** changed (D2, D5).

`game.c` reaches its globals through these `__nl_symbol_ptr` slots (`r2 -qc 'p8 4 @ <slot>'`). The slot **order**
changed against C2, so a tool that hard-codes C2's slot list has to be redone:

| slot | → target | meaning | C2 target |
|---|---|---|---|
| 0x16514 | 0x17890 | `hint_text` (char[2048]) | 0x136a0 |
| 0x16518 | 0x1a1f0 | `level_name_text` (char[256]) | 0x16000 |
| 0x1651c | 0x1a510 | `site_url_text` | 0x161f0 |
| 0x16520 | 0x163e0 | `num_images` | 0x121e4 |
| 0x16524 | 0x17480 | `level_started` | 0x13290 |
| 0x16528 / 0x1652c | 0x1a504 / 0x1a500 | cursor row / col (logic.md) | 0x161e4 / 0x161e0 |
| 0x16530 | 0x17490 | `decoded_images` | 0x132a0 |
| 0x16534 / 0x16538 | 0x17278 / 0x17274 | drag sprite y / x | 0x13088 / 0x13084 |
| 0x1653c | 0x160e0 | `palette` | 0x11ee4 |
| 0x16540 | 0x181a0 | `force_win` | 0x13fb0 |
| 0x16544 | 0x182b0 | `level_won` | 0x140c0 |
| 0x16548 | 0x17484 | `game_mode` | 0x13294 |
| 0x1654c | 0x8110 | `level_order` | 0x8110 |
| 0x16550 | 0x18090 | `current_level_id` | 0x13ea0 |
| 0x16554 | 0x1a300 | `saved_states` (**128** entries) | 0x16110 (52) |
| 0x16558 | 0x180a0 | `unlocked[64]` | 0x13eb0 |
| 0x1655c | 0x181b0 | `completed[64]` | 0x13fc0 |
| 0x16560–0x1656c | 0x17270, 0x17380, 0x17280, 0x1726c | beam lists (logic.md) | 0x13080, 0x13190, 0x13090, 0x1307c |
| 0x16570 | 0x182c0 | `board` | 0x140d0 |
| 0x16574 / 0x16578 | 0x1a2f0 / 0x1a2f4 | beam next list / count | 0x16100 / 0x16104 |
| 0x1657c | 0xd6cc | `level_name_table` | 0xaa08 |
| 0x16580 | 0xd808 | `hint_table` | 0xaadc |
| 0x16584 | 0xd590 | `level_table` | 0xa934 |
| 0x16588 | 0x15e60 | `image_table` | 0x11ce4 |

**What breaks a C2 decoder/renderer** (summary; details below):
1. **Board and cell layout are UNCHANGED**: still 15 × 15 plus toolbox rows 15–18, 28-byte cells (row stride 0x1a4,
   state size 0x1f2c), 5-byte level records. A C2 cell/record decoder works as is.
2. **79-entry level table (78 encoded levels, ids 1–78), not 53**, at new addresses (`level_table` 0xd590,
   `level_name_table` 0xd6cc, `hint_table` 0xd808). Still **50 playable** slots; different `level_order`;
   **28 unreachable ids**.
3. **Five new piece types 16–20**, including **19 = DUAL-REFLECTOR** and **20 = QUAD-BENDER** (plus 16 gate, 17 starburst, 18 mobile
   teleporter, used only by unreachable levels; names of 16–18 are descriptive, shared with logic.md). `is_movable` = types
   3–9, 13, 15, **16, 18, 19, 20**. C2's `beam_enter_cell` asserts on any type > 15.
4. **`num_images` = 160, not 128.** New sprites 128–135 (gate 16), 136–139 (DUAL-REFLECTOR), 140–143 (QUAD-BENDER),
   144 (starburst 17); mobile teleporter 18 reuses sprite 14. Images 145–159 are unused copies of the floor tile. Images 0–127, the
   palette and the beam colours are byte-identical to C2.
5. Save file: new name, a **new 0xff marker byte after the bitmap**, saved states for ids **0–127**, and a one-time
   recomputation of completion flags for files without the marker (platform.md P7). `save_bit_perm` is **the same as
   C2's**. Clipboard prefix `3-` (platform.md P8).

## Findings

### D1. Level table: address, count, record layout, loader
**Answer:**
- **`level_table` @ 0xd590**: **79** big-endian `u32` pointers, indexed by level id 0–78. Entry 0 is NULL. Ids 1–78 point
  to blobs in `level_data` 0x81d0–0xd58f (id 1 @ 0x81d0, id 78 @ 0xd56c, the highest address). The table ends where
  `level_name_table` starts (0xd6cc = 0xd590 + 79·4). Blobs are stored in id order. (C2: 53 entries @ 0xa934.)
- **Count**: 78 levels are encoded, **50 are playable**. `level_order` @ 0x8110 (same address as C1/C2) is a `u8[50]`
  mapping slot → id:
  `0b 13 3b 4c 4b 16 3a 49 1f 31 43 22 44 4a 1b 32 37 3c 42 1e 47 2c 36 3e 3f 38 2d 23 21 48 3d 28 30 40 1d 25 2a 2b 33 41 34 2f 17 24 29 19 18 26 2e 39`
  (decimal 11 19 59 76 75 22 58 73 31 49 67 34 68 74 27 50 55 60 66 30 71 44 54 62 63 56 45 35 33 72 61 40 48 64 29 37
  42 43 51 65 52 47 23 36 41 25 24 38 46 57). The slot cap is unchanged (`next_level` @ 0x5cf4: `if (current_slot < 0x31)`,
  disassembly `cmpwi cr7,r2,0x30; bgt`; `render_scene` @ 0x6ac4 draws numbers 1..50). The byte after `level_order`
  (0x8142) is `39` (= the last entry again), followed by 141 zero bytes up to 0x81d0 — the same shape as C2's `06`
  + zeros (unreferenced by the game's slot code; but see platform.md P7 for a read of 0x8142.. by the save-file
  migration loop).
- **Unreachable ids (28)**: **1–10, 12–18, 20, 21, 26, 28, 32, 39, 53, 69, 70, 77, 78**. Exactly these ids have placeholder
  names `levelNNN` and placeholder hints ("foobar" for id 1, "blah" for the rest); every playable id has a real name.
  Types 16, 17 and 18 (and the older types 7, 9, 14, 15) occur **only** in unreachable levels.
- **Level blob and record**: **unchanged from C2/C1**: a list of 5-byte records `type, dir, colour, x, y`, ended by a
  single `0x00` type byte, then 0–3 zero pad bytes to 4-byte alignment. Checked for all 78 blobs (each terminator is
  followed only by zeros up to the next blob, and id 78's padding ends exactly at 0xd590). Every x and y is 0–14 and no
  two records share a cell. No new record fields: the new pieces are new `type` values (D2).
- **Loader**: `load_level_data` @ 0x6efc is **identical to C2's** 0x6d24: `strcpy(hint_text, hint_table[id])`,
  `strcpy(level_name_text, level_name_table[id])`, `memset(board, 0, 0x1be4)`, then for each record
  `board[y*0x1a4 + x*0x1c + {0,1,2}] = {type, colour, dir}`. Callers as in C2 (`start_level` @ 0x5b5c copies a 0x1f2c-byte
  saved state; the new save-file migration in `load_save_file` @ 0x44e4 also calls it, platform.md P7).
- Per-slot table (slot 1-based; movable = types 3–9, 13, 15, 16, 18, 19, 20; new-type abbreviations: DR = DUAL-REFLECTOR
  19, QB = QUAD-BENDER 20, GT = gate 16, SB = starburst 17, MT = mobile teleporter 18 (16–18 named as in logic.md),
  and C2's MF 13, TW 14, CO 15;
  hint "–" = the empty string 0x78ac, the first byte of `__cstring`):

| slot | id | data @ | records | movable | new types | name (string @) | hint @ |
|---|---|---|---|---|---|---|---|
| 1 | 11 | 0x8674 | 71 | 3 | – | Easy Does It (0x7af0) | – |
| 2 | 19 | 0x8db4 | 22 | 9 | – | Bounce (0x7b54) | – |
| 3 | 59 | 0xbb60 | 102 | 10 | – | Pathfinder (0x7d9c) | – |
| 4 | 76 | 0xd4d4 | 12 | 5 | – | Splitz (0x7eb0) | – |
| 5 | 75 | 0xd3a4 | 60 | 8 | – | Impenetrable (0x7ea0) | – |
| 6 | 22 | 0x9134 | 24 | 8 | – | Symmetric Hexagon (0x7b74) | – |
| 7 | 58 | 0xb9fc | 71 | 7 | – | Doubletastic (0x7d8c) | – |
| 8 | 73 | 0xd0c8 | 60 | 11 | – | Squishy (0x7e8c) | – |
| 9 | 31 | 0x9770 | 46 | 9 | – | Reconstruction (0x7bfc) | – |
| 10 | 49 | 0xaec4 | 36 | 10 | – | To & Fro (0x7d1c) | – |
| 11 | 67 | 0xc694 | 58 | 12 | – | Blue vs. Green III: Interlock (0x7e24) | – |
| 12 | 34 | 0x9934 | 50 | 10 | – | Crossing Over (0x7c2c) | – |
| 13 | 68 | 0xc7b8 | 10 | 5 | – | Prism Schism (0x7e44) | – |
| 14 | 74 | 0xd1f8 | 85 | 11 | – | Your Choice (0x7e94) | – |
| 15 | 27 | 0x93f0 | 71 | 14 | – | Alternator (0x7bc8) | – |
| 16 | 50 | 0xaf7c | 127 | 12 | – | Amazing (0x7d28) | – |
| 17 | 55 | 0xb694 | 17 | 3 | DR | Over & Out (0x7d60) | 0x7f6c |
| 18 | 60 | 0xbd60 | 145 | 19 | DR | Interference Patterns (0x7da8) | – |
| 19 | 66 | 0xc558 | 63 | 12 | – | Router (0x7e1c) | – |
| 20 | 30 | 0x9714 | 18 | 5 | – | Latticework (0x7bf0) | – |
| 21 | 71 | 0xcaf4 | 193 | 10 | – | Recon (0x7e6c) | – |
| 22 | 44 | 0xa7c8 | 61 | 5 | QB | Flexible (0x7cd8) | 0x7f10 |
| 23 | 54 | 0xb528 | 72 | 8 | QB | Double-Jointed (0x7d50) | – |
| 24 | 62 | 0xc0ac | 29 | 13 | QB | Retrograde Action (0x7ddc) | – |
| 25 | 63 | 0xc140 | 39 | 11 | – | Opposing Forces (0x7df0) | – |
| 26 | 56 | 0xb6ec | 81 | 12 | – | Circular Fashion (0x7d6c) | – |
| 27 | 45 | 0xa8fc | 128 | 17 | – | Parity Error (0x7ce4) | – |
| 28 | 35 | 0x9a30 | 20 | 9 | – | All In A Row (0x7c3c) | 0x7ee0 |
| 29 | 33 | 0x98b8 | 24 | 8 | – | In This Corner... (0x7c18) | – |
| 30 | 72 | 0xcebc | 104 | 17 | – | There and Back Again (0x7e74) | – |
| 31 | 61 | 0xc038 | 23 | 11 | – | Line 'Em Up, Knock 'Em Down (0x7dc0) | – |
| 32 | 40 | 0x9f98 | 92 | 11 | – | Warp & Weave (0x7c80) | – |
| 33 | 48 | 0xad9c | 59 | 10 | DR | Perimeter Walk (0x7d0c) | – |
| 34 | 64 | 0xc204 | 99 | 16 | – | Two Right Turns (0x7e00) | – |
| 35 | 29 | 0x96cc | 14 | 5 | – | Sure You Can (0x7be0) | – |
| 36 | 37 | 0x9b8c | 68 | 13 | – | Broken Symmetry (0x7c58) | – |
| 37 | 42 | 0xa288 | 151 | 20 | DR | Interference Patterns II (0x7cac) | – |
| 38 | 43 | 0xa57c | 117 | 9 | QB | Space Invader (0x7cc8) | – |
| 39 | 51 | 0xb1f8 | 33 | 11 | DR | Obvious (0x7d30) | – |
| 40 | 65 | 0xc3f4 | 71 | 20 | – | Good Fences (0x7e10) | – |
| 41 | 52 | 0xb2a0 | 54 | 17 | – | Work It In (0x7d38) | – |
| 42 | 47 | 0xac10 | 79 | 23 | – | Overload (0x7d00) | – |
| 43 | 23 | 0x91b0 | 18 | 6 | – | Careful Alignment (0x7b88) | – |
| 44 | 36 | 0x9a98 | 48 | 12 | – | Doubled Up (0x7c4c) | – |
| 45 | 41 | 0xa168 | 57 | 16 | – | To Get To The Other Side (0x7c90) | – |
| 46 | 25 | 0x9314 | 29 | 13 | – | Erratic Message (0x7bac) | – |
| 47 | 24 | 0x920c | 52 | 10 | – | Tight Squeeze (0x7b9c) | – |
| 48 | 38 | 0x9ce4 | 67 | 24 | – | The Race (0x7c68) | – |
| 49 | 46 | 0xab80 | 28 | 11 | QB | Rectangles (0x7cf4) | – |
| 50 | 57 | 0xb884 | 75 | 24 | – | Bullseye (0x7d80) | – |
| – | 1 | 0x81d0 | 18 | 5 | GT | level001 (0x7a78) | 0x7ed0 ("foobar") |
| – | 2 | 0x822c | 17 | 8 | GT | level002 (0x7a84) | 0x7ed8 ("blah") |
| – | 3 | 0x8284 | 39 | 20 | GT | level003 (0x7a90) | 0x7ed8 ("blah") |
| – | 4 | 0x8348 | 14 | 3 | SB | level004 (0x7a9c) | 0x7ed8 ("blah") |
| – | 5 | 0x8390 | 8 | 2 | SB | level005 (0x7aa8) | 0x7ed8 ("blah") |
| – | 6 | 0x83bc | 17 | 7 | SB | level006 (0x7ab4) | 0x7ed8 ("blah") |
| – | 7 | 0x8414 | 13 | 3 | CO, SB | level007 (0x7ac0) | 0x7ed8 ("blah") |
| – | 8 | 0x8458 | 17 | 1 | SB | level008 (0x7acc) | 0x7ed8 ("blah") |
| – | 9 | 0x84b0 | 36 | 9 | SB | level009 (0x7ad8) | 0x7ed8 ("blah") |
| – | 10 | 0x8568 | 53 | 7 | GT | level010 (0x7ae4) | 0x7ed8 ("blah") |
| – | 12 | 0x87d8 | 7 | 4 | – | level012 (0x7b00) | 0x7ed8 ("blah") |
| – | 13 | 0x87fc | 39 | 6 | – | level013 (0x7b0c) | 0x7ed8 ("blah") |
| – | 14 | 0x88c0 | 86 | 15 | GT | level014 (0x7b18) | 0x7ed8 ("blah") |
| – | 15 | 0x8a70 | 24 | 14 | – | level015 (0x7b24) | 0x7ed8 ("blah") |
| – | 16 | 0x8aec | 63 | 7 | MT | level016 (0x7b30) | 0x7ed8 ("blah") |
| – | 17 | 0x8c28 | 29 | 6 | TW, MT | level017 (0x7b3c) | 0x7ed8 ("blah") |
| – | 18 | 0x8cbc | 49 | 12 | MT | level018 (0x7b48) | 0x7ed8 ("blah") |
| – | 20 | 0x8e24 | 73 | 19 | MT | level020 (0x7b5c) | 0x7ed8 ("blah") |
| – | 21 | 0x8f94 | 83 | 20 | MT | level021 (0x7b68) | 0x7ed8 ("blah") |
| – | 26 | 0x93a8 | 14 | 3 | GT, SB | level026 (0x7bbc) | 0x7ed8 ("blah") |
| – | 28 | 0x9554 | 75 | 16 | – | level028 (0x7bd4) | 0x7ed8 ("blah") |
| – | 32 | 0x9858 | 19 | 8 | – | level032 (0x7c0c) | 0x7ed8 ("blah") |
| – | 39 | 0x9e34 | 71 | 19 | – | level039 (0x7c74) | 0x7ed8 ("blah") |
| – | 53 | 0xb3b0 | 75 | 24 | QB | level053 (0x7d44) | 0x7ed8 ("blah") |
| – | 69 | 0xc7ec | 81 | 8 | – | level069 (0x7e54) | 0x7ed8 ("blah") |
| – | 70 | 0xc984 | 73 | 3 | – | level070 (0x7e60) | 0x7ed8 ("blah") |
| – | 77 | 0xd514 | 17 | 4 | – | level077 (0x7eb8) | 0x7ed8 ("blah") |
| – | 78 | 0xd56c | 7 | 2 | – | level078 (0x7ec4) | 0x7ed8 ("blah") |

**Evidence:** `load_level_data @ 0x6efc` (identical to C2's):
```c
  pcVar4 = *(char **)(PTR_level_table_00016584 + current_level_id * 4);
  _strcpy(hint_text /*0x17890*/, *(char **)(PTR_hint_table_00016580 + current_level_id * 4));
  _strcpy(level_name_text /*0x1a1f0*/, *(char **)(PTR_level_name_table_0001657c + current_level_id * 4));
  puVar7 = _memset(board /*0x182c0*/, 0, 0x1be4);
  while (cVar1 != '\0') { iVar6 = (uint)(byte)pcVar4[4] * 0x1a4 + (uint)(byte)pcVar4[3] * 0x1c;
    puVar5[iVar6] = cVar1;  puVar5[iVar6 + 1] = cVar3 /*rec[2]*/;  puVar5[iVar6 + 2] = cVar2 /*rec[1]*/;  pcVar4 += 5; }
```
Slot → id: `goto_slot` @ 0x5c08, `slot_next_id` @ 0x5c60, `slot_prev_id` @ 0x5ca0 (identical to C2's); `game_init` @ 0x642c
starts with `current_level_id = level_order[0]` (= 11). Worked checks: slot 1 = id 11 @ 0x8674 starts `01 00 04 00 00`
(wall at (0,0)); its three movable pieces are a mirror (6,12) dir 2, a bender (11,7) dir 0 and a splitter (9,9) dir 0.
Slot 17 = id 55 @ 0xb694 holds the first DUAL-REFLECTOR record `13 01 04 07 07` at 0xb6bc (type 19, dir 1, colour 4,
cell (7,7)); slot 22 = id 44 holds `14 01 04 07 07` (QUAD-BENDER) at 0xa85e.
**Confidence:** confirmed.

### D2. Board: grid size and cell encoding
**Answer:**
- **Grid and memory layout: same as C2.** `board` @ 0x182c0 (common) is 19 rows × 15 columns × 28-byte cells = 0x1f2c
  bytes; cell (x, y) at `board + y*0x1a4 + x*0x1c`; toolbox slot s at `board + 0x189c + 28*s`. Cell bytes +0 `type`,
  +1 `colour`, +2 `dir`, +3..+27 runtime (as C2; logic.md describes any new runtime use). Screen positions unchanged
  (`draw_slot_cell` @ 0x661c is C2's).
- **`type` values** (`cell_image_index` @ 0x4a54, switch bound now `cmplwi r9,0x14`; `is_movable` @ 0x3214, now a jump
  table at 0x324c over `type − 3` ≤ 0x11):

  | type | kind | movable | uses `colour` | uses `dir` | image index |
  |---|---|---|---|---|---|
  | 0–15 | as C2 (… 12 teleporter, 13 multi-filter, 14 twister, 15 complementor) | 3–9, 13, 15 | as C2 | as C2 | as C2 |
  | **16** | **gate** (descriptive name; only in unreachable ids 1, 2, 3, 10, 14, 26) | **yes** | data 4 (once 2), not drawn | yes, 0–7 | **128 + dir** |
  | **17** | **starburst**, fixed (descriptive name; only in unreachable ids 4–9, 26) | **no** | data 4, not drawn | data 0, not drawn | **144** |
  | **18** | **mobile teleporter** (descriptive name; unreachable ids 16, 17, 18, 20, 21) | **yes** | data 4, not drawn | data 0 or 7, not drawn | **14** (the teleporter sprite) |
  | **19** | **DUAL-REFLECTOR** | **yes** | data 4, not drawn | yes; drawn `dir & 3` | **136 + (dir & 3)** |
  | **20** | **QUAD-BENDER** | **yes** | data 4, not drawn | yes; drawn `dir & 3` | **140 + (dir & 3)** |

  Any other type still makes `cell_image_index` return −1. Names: 19 and 20 come from the hint strings 0x7f6c "This new
  piece, the DUAL-REFLECTOR, is a double-sided mirror." (slot 17) and 0x7f10 "The QUAD-BENDER is a double-sided bender.
  You can bounce up to four separate beams off it." (slot 22). The binary contains no names for 16, 17 and 18; "gate",
  "starburst" and "mobile teleporter" are the descriptive names used in logic.md, which owns their beam behaviour. In
  short, from `beam_enter_cell` @ 0x3320: 18 is handled together with teleporter 12; 17 re-emits an entering beam in all
  four diagonal or all four orthogonal directions; 16 absorbs and is serviced by a new pass `emit_gates` @ 0x4198.
- Record counts over all 78 blobs: wall 2294, laser 148, mirror 412, bender 309, **filter 0**, prism 6, doppler 18,
  splitter 29, tangler 12, pinwheel 866, conduit 96, **teleporter 0**, multi-filter 0, twister 2, complementor 1,
  **gate 11, starburst 14, mobile teleporter 20, DUAL-REFLECTOR 6, QUAD-BENDER 6**. The **playable** levels use only walls,
  lasers, mirrors, benders, prisms, splitters, pinwheels, conduits, DUAL-REFLECTORs (6) and QUAD-BENDERs (5).
- Data noise as in C2 (walls/mirrors/benders with colour 1, 2, 5 or 7; wall `dir` ≠ 0; a gate in id 3 with colour 2).
  Keep it byte-exact.

**Evidence:** `cell_image_index @ 0x4a54`, new cases (disassembly 0x4c7c–0x4cb8):
```
addi r10,r8,0x64     ; 13: 100 + (dir&3)          (r8 = dir & 3, r11 = dir)
li   r10,0xf         ; 14: 15
clrlwi r2,r11,0x1f; addi r10,r2,0x74   ; 15: 116 + (dir&1)
addi r10,r11,0x80    ; 16: 128 + dir
li   r10,0x90        ; 17: 144
clrlwi r2,r11,0x1e; addi r10,r2,0x88   ; 19: 136 + (dir&3)
clrlwi r2,r11,0x1e; addi r10,r2,0x8c   ; 20: 140 + (dir&3)
```
Type 18 shares the `li r10,0xe` case at 0x4b70 with type 12 (decompiled `case 0xc: case 0x12: lVar5 = 0xe;`).
`is_movable @ 0x3214`: `switch(type) { case 3..9, 0xd, 0xf, 0x10, 0x12, 0x13, 0x14: return 1; default: return 0; }`;
jump table at 0x324c: offsets 0x48 (→ 1) for types 3–9, 13, 15, 16, 18, 19, 20 and 0x50 (→ 0) for 10, 11, 12, 14, 17.
**Confidence:** confirmed for layout, type ids, movability and image mapping; the names of 16/17/18 are descriptive
only (**unknown** real names; only original design notes would settle them).

### D3. Toolbox: how available pieces and counts are stored
**Answer:** **Same model and code as C2** (`move_pieces_to_toolbox` @ 0x59ec identical; comparator pointer 0x810c →
0x59dc `lbz r0,0(r3); lbz r3,0(r4); subf r3,r3,r0; blr`). The toolbox is the level's movable records (now also types 16,
18, 19, 20), sorted by type: … tangler 9, multi-filter 13, complementor 15, **gate 16, mobile teleporter 18,
DUAL-REFLECTOR 19, QUAD-BENDER 20**. Visible toolbox 6 × 4 = 24 slots.
- **Maximum movable pieces: 24** — slot 48 (id 38) and slot 50 (id 57), plus unreachable id 53. They exactly fill the
  24 visible slots; nothing overflows.
- Ordering caveat (unstable qsort among equal types): playable levels with differently coloured pieces of one movable
  type are ids 38, 47, 52, 55, 65 and 67, all mirrors (colour 1/2/4/5), which don't draw their colour. So, as in C2, no
  visible toolbox order depends on the qsort implementation. (Unreachable id 3 has gates with colours 2 and 4; the gate
  sprite ignores colour too.)

**Evidence:** `r2 -qc 'pi 4 @ 0x59dc'` (Verification); normalised diff of `move_pieces_to_toolbox` against C2's 0x5810
is empty.
**Confidence:** confirmed.

### D4. Level names and tutorial/hint text
**Answer:** **Same mechanism as C2**, new tables:
- **`level_name_table` @ 0xd6cc**: 79 `u32` pointers into `__cstring`, indexed by level id, entry 0 NULL. Copied into
  `level_name_text` @ 0x1a1f0 (256 bytes) and drawn at 50,385–300,420 (platform.md P3). All 50 playable names are
  distinct; the longest is "Blue vs. Green III: Interlock" (29 chars, slot 11). Unreachable ids have `levelNNN`
  (8 chars).
- **`hint_table` @ 0xd808**: 79 `u32` pointers, entry 0 NULL; the empty hint is **0x78ac** (first byte of `__cstring`, a
  NUL — C2 used its own first byte 0x6ec0). Copied into `hint_text` @ 0x17890 (2048 bytes), drawn at 450,125–620,475,
  replaced by "Click on a level or press spacebar for next." (0x7a24) when the level is won.
- Only **three** playable slots have a hint:

  | slot | id | string @ | text |
  |---|---|---|---|
  | 17 | 55 | 0x7f6c | This new piece, the DUAL-REFLECTOR, is a double-sided mirror. |
  | 22 | 44 | 0x7f10 | The QUAD-BENDER is a double-sided bender. You can bounce up to four separate beams off it. |
  | 28 | 35 | 0x7ee0 | (Hint: you only need to hit each bender once) |

  Unreachable: id 1 → "foobar" (0x7ed0), the other 27 → "blah" (0x7ed8). The longest hint is 90 characters (0x7f10).
  All strings are 7-bit ASCII without line breaks (`draw_text_box` @ 0x2f64 uses encoding 0x0600, as C2).

**Evidence:** `load_level_data @ 0x6efc` (D1). `xxd -s 0xc6cc -l 16` → `0000 0000 0000 7a78 0000 7a84 0000 7a90`
(NULL, "level001", "level002", "level003"); `xxd -s 0xc808 -l 16` → `0000 0000 0000 7ed0 0000 7ed8 0000 7ed8`.
**Confidence:** confirmed.

### D5. Images/sprites
**Answer:**
- **`num_images` @ 0x163e0 = 160** (C2: 128). **`image_table` @ 0x15e60**: 160 `u32` pointers into **`image_data`
  0xd944–0x15e5f** (0x851c bytes incl. padding). Image 0 @ 0xd944, image 128 @ 0x14a78, image 159 @ 0x15e18.
- **Encoding: identical to C2/C1** (no header, 24 × 24, 8-bit palette indices, PCX-style RLE: byte ≤ 0xC0 literal, else
  run of `b − 0xC0` copies of the next byte). `decode_images` @ 0x6de8 is C2's (decodes `num_images − 1` → 0,
  `malloc(0x240)` each into `decoded_images` @ 0x17490; the array is still declared 256 entries, 0x17490–0x1788f).
  All 160 streams decode to exactly 576 bytes and end at their zero padding.
- **Images 0–127 are byte-identical to C2** — not just the decoded pixels: the first 0x7134 bytes of `image_data` equal
  C2's whole `image_data` block. **New images:**

  | index | used by | content (decoded sheet) |
  |---|---|---|
  | 128–135 | gate (16), `128 + dir` | a Y-shaped three-armed grey piece with a rainbow-striped core, rotated through 8 directions |
  | 136–139 | **DUAL-REFLECTOR** (19), `136 + (dir & 3)` | a straight grey bar with black faces on **both** sides, at 4 orientations (vertical, diagonal, horizontal, other diagonal) |
  | 140–143 | **QUAD-BENDER** (20), `140 + (dir & 3)` | a short grey bar with black faces on both sides, slightly tilted, at 4 orientations |
  | 144 | starburst (17, fixed) | a multicoloured spiked disc (red/green/blue rings) |
  | 145–159 | — (unreferenced) | 15 copies of the floor tile: decoded pixels equal image 0 |

- **Full C3 index map**: C2's map for 0–127 (0 floor; 1 wall; pinwheels 2–7, 12/13, 16–23; filters 8–11, 96–99, 112–115;
  14 teleporter **and mobile teleporter**; 15 twister; 24–31 prism; 32–39 mirror; 40–43 splitter; 44–47 conduit; 48–55
  bender; lasers 56–63/72–79/88–95/104–111; 64–71 doppler; 80–87 tangler; 100–103 multi-filter; 116–117 complementor;
  118–127 digits) plus **128–135 gate, 136–139 DUAL-REFLECTOR, 140–143 QUAD-BENDER, 144 starburst**.
- **Blit** paths are unchanged (`blit_indexed` 0x4cc4; `draw_cell_image` 0x653c asserts `b != -1` and `b < num_images`, now
  at game.c 0xa7e / 0xa7f). A renderer built for C2 must widen its sprite array to 160 (or at least 145) and add the five
  new `cell_image_index` cases.

**Evidence:** `decode_images @ 0x6de8` uses `PTR_num_images_00016520`, `PTR_image_table_00016588`,
`PTR_decoded_images_00016530`, `PTR_palette_0001653c`; `xxd -s 0x14e60 -l 8` → `0000 d944 0000 d98c`; image 0 starts
`d9 06 d7 08 06 …` as in C2. The Verification script prints the image count, the differing indices against C2 (none)
and the floor-tile copies.
**Confidence:** confirmed (format, table, which images are new); the art descriptions come from a decoded sheet.

### D6. Colours
**Answer:** **All identical to C2 (and C1).** `palette` @ 0x160e0 (256 × BGR) is byte-equal to C2's 0x11ee4 and is
swapped to RGB in place by `decode_images`. `beam_colors` @ 0x80d4 (same address and bytes: 000000, 0000ff, 00c800,
00c8ff, ff0000, ff00ff, ffc800, ffffff). Background `memset(fb, 0xa4, …)` in `render_scene` @ 0x6ac4; strip digit palette
offsets `strip_pal_offset` @ 0x6994 = 0 / 0x48 / 0x30 / 0x10 (current / completed / unlocked / locked) as C2.
The new sprites use only existing palette entries.
**Evidence:** byte comparison of the palettes (Verification `True`); `xxd -s 0x70d4 -l 32` equals C2's.
**Confidence:** confirmed.

### D7. Fonts
**Answer:** **Same as C2.** Digit glyphs are images 118–127 (byte-identical), drawn 12 × 12 by `draw_digit_string` @ 0x68f0
(`blit_indexed(x, y, decoded_images[c − '0' + 118], 12, 12, 24, 0x0e, pal)`); strip numbers 1..50 at
x = 10 + 20((n−1) mod 25), y = 410 + 20((n−1) div 25), leading '0' → ' ' (`render_scene` @ 0x6ac4). All other text goes
through `draw_text_box` @ 0x2f64 (`DrawThemeTextBox`, theme system font).
**Evidence:** normalised diffs of `draw_digit_string`, `render_scene` and `draw_text_box` against C2 show only line
wrapping and slot addresses.
**Confidence:** confirmed.

### D8. Map of `__DATA,__data` (0x8000–0x163e3)
**Answer:**

| range | size | purpose | vs C2 |
|---|---|---|---|
| 0x8000–0x800f | 16 | `_NXArgc`, `_NXArgv`, `_environ`, `___progname` (→ 0x78ac) | pointer differs |
| 0x8010–0x801f | 16 | crt/dyld glue pointers 0x17234, 0x23fc, 0x22e0, 0x225c | addresses differ |
| 0x8020 | 4 | `use_wheel_pref` = 1 | same |
| 0x8024–0x803b | 24 | event handlers 0x29d8 (menu), 0x2b94 (key), 0x2d80 (wheel), 0x3028 (mouse), 0x2b28 (draw), 0x2b04 (close) | addresses differ |
| 0x803c | 4 | `current_slot` = 0 | same |
| 0x8040 | 4 | `redraw_all_flag` = 1 | same |
| 0x8044–0x80ab | 104 | `dir_dx[8]`, `dir_dy[8]`, two `int32[5]` tables (logic.md) | same bytes |
| 0x80ac–0x80cb | 32 | `save_bit_perm` `14 1f 07 08 1e 0e 03 1c 02 16 05 1b 10 0b 19 0c 00 09 0d 13 17 04 11 1d 15 0a 06 18 12 1a 01 0f` | **same bytes as C2** (not C1) |
| 0x80cc | 4 | −1, unreferenced | same |
| 0x80d0 | 4 | `drag_image` = −1 | same |
| 0x80d4–0x80f3 | 32 | `beam_colors` (D6) | same bytes |
| 0x80f4–0x8107 | 20 | `site_url_xor` → "silverspaceship.com" | same bytes |
| 0x8108 | 4 | clipboard-entry comparator → 0x5d94 (type, then colour, unsigned) | address differs |
| 0x810c | 4 | toolbox comparator → 0x59dc (type) | address differs |
| 0x8110–0x8141 | 50 | `level_order` u8[50] (D1) | **different contents** |
| 0x8142–0x81cf | 142 | `39` + 141 zero bytes (the tail of `level_order`'s declared storage) | C2: `06` + zeros |
| 0x81d0–0xd58f | 0x53c0 | `level_data`: 78 blobs (D1) | C2: 52 blobs, 0x2764 |
| 0xd590–0xd6cb | 316 | `level_table` u32[79] | C2: u32[53] @ 0xa934 |
| 0xd6cc–0xd807 | 316 | `level_name_table` u32[79] (D4) | C2: u32[53] @ 0xaa08 |
| 0xd808–0xd943 | 316 | `hint_table` u32[79] (D4) | C2: u32[53] @ 0xaadc |
| 0xd944–0x15e5f | 0x851c | `image_data`: 160 RLE images (D5) | C2: 128 images, 0x7134 |
| 0x15e60–0x160df | 640 | `image_table` u32[160] | C2: u32[128] |
| 0x160e0–0x163df | 768 | `palette` 256 × BGR | moved, same bytes |
| 0x163e0–0x163e3 | 4 | `num_images` = **160** | C2: 128 |

The bytes 0x8000–0x810f differ from C2's only at 0x800e–0x801f, 0x8026–0x803b and 0x810a–0x810f (code addresses).

Runtime (not in `__data`) — same declarations as C2, rebased. `__bss` (C2 + 0x41fc): `main_window` 0x165ac,
mouse-alt latch 0x165b0, `wheel_menu_mark` 0x165b4, drag state 0x165b8, `empty_cell` 0x165bc (28 bytes), entangle
counter 0x165d8, `framebuffer` 0x165dc, `fb_width` 0x165e0, `fb_height` 0x165e4, `drag_save_under` 0x165e8 (0xc00),
press x/y 0x171e8/0x171ec, `drag_inval_x/y` 0x171f0/0x171f4, `clip_text` 0x171f8 (54 bytes, to the end of `__bss`).
`__common` (C2 + 0x41f0 up to `saved_states`): beam lists 0x1726c–0x1747f, drag sprite x/y 0x17274/0x17278,
`level_started` 0x17480, `game_mode` 0x17484, `decoded_images` 0x17490 (256 × 4; 160 used), `hint_text` 0x17890 (2048),
`current_level_id` 0x18090, `unlocked[64]` 0x180a0, `force_win` 0x181a0, `completed[64]` 0x181b0, `level_won` 0x182b0,
`board` 0x182c0 (0x1f2c), `level_name_text` 0x1a1f0 (256), beam next list 0x1a2f0/0x1a2f4, **`saved_states[128]`
0x1a300–0x1a4ff** (C2: 52 entries), cursor col/row 0x1a500/0x1a504, `site_url_text` 0x1a510 (to 0x1a54f, the end of
`__common`). `__common` is 0x130 bytes larger than C2's: exactly the 76 extra `saved_states` pointers.

**Evidence:** the `__nl_symbol_ptr` table above; `create_main_window` @ 0x2828 stores to 0x165ac; `game_init` @ 0x642c
writes `DAT_000165dc/e0/e4`; `encode_solution` @ 0x5e08 writes `DAT_000171f8..fc`; `write_save_file` @ 0x47a0 loops ids
0..0x7f over `saved_states`. Byte diff of 0x8000–0x810f against C2 in Verification.
**Confidence:** confirmed, except the purpose of 0x80cc and 0x8142–0x81cf (**unknown**, unreferenced by game code; only
the original source would settle them; irrelevant to a reimplementation).

## Verification
```bash
BIN="chromatron_mac/Chromatron 3.app/Contents/MacOS/chromatron3"
B2="chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2"

# Level-table header (level_table @ 0xd590 = file 0xc590): id0 NULL, id1 0x81d0, id2 0x822c, id3 0x8284
xxd -s 0xc590 -l 16 "$BIN"
# 0000c590: 0000 0000 0000 81d0 0000 822c 0000 8284  ...........,....
# ... its last entry (id 78 @ 0xd56c), directly followed by level_name_table @ 0xd6cc
xxd -s 0xc6c8 -l 4 "$BIN"
# 0000c6c8: 0000 d56c                                ...l

# First level record: id 1 @ 0x81d0 (file 0x71d0): 03 06 01 06 02 = mirror, dir 6, colour 1, at (6,2)
xxd -s 0x71d0 -l 16 "$BIN"
# 000071d0: 0306 0106 0203 0604 0802 0a00 0205 0310  ................
# Slot 1 = id 11 @ 0x8674 (file 0x7674): 01 00 04 00 00 = wall at (0,0) ...
xxd -s 0x7674 -l 16 "$BIN"
# 00007674: 0100 0400 0001 0004 0100 0100 0402 0001  ................
# First DUAL-REFLECTOR record (slot 17, id 55) and first QUAD-BENDER record (slot 22, id 44)
xxd -s 0xa6bc -l 5 "$BIN"; xxd -s 0x985e -l 5 "$BIN"
# 0000a6bc: 1301 0407 07                             .....
# 0000985e: 1401 0407 07                             .....

# Slot -> level id order (level_order @ 0x8110) and the two bytes after it
xxd -s 0x7110 -l 52 "$BIN"
# 00007110: 0b13 3b4c 4b16 3a49 1f31 4322 444a 1b32  ..;LK.:I.1C"DJ.2
# 00007120: 373c 421e 472c 363e 3f38 2d23 2148 3d28  7<B.G,6>?8-#!H=(
# 00007130: 3040 1d25 2a2b 3341 342f 1724 2919 1826  0@.%*+3A4/.$)..&
# 00007140: 2e39 3900                                .99.

# Name table and hint table headers, and the last hint entry right before image_data
xxd -s 0xc6cc -l 16 "$BIN"
# 0000c6cc: 0000 0000 0000 7a78 0000 7a84 0000 7a90  ......zx..z...z.
xxd -s 0xc808 -l 16 "$BIN"
# 0000c808: 0000 0000 0000 7ed0 0000 7ed8 0000 7ed8  ......~...~...~.
xxd -s 0xc940 -l 4 "$BIN"
# 0000c940: 0000 7ed8                                ..~.
r2 -qc 'ps @ 0x7f6c; ps @ 0x7f10; ps @ 0x7ee0' "$BIN" </dev/null 2>/dev/null
# This new piece, the DUAL-REFLECTOR, is a double-sided mirror.
# The QUAD-BENDER is a double-sided bender. You can bounce up to four separate beams off it.
# (Hint: you only need to hit each bender once)

# num_images (= 0xa0 = 160), first image-table entries, and the first image "header" (none: RLE stream D9 06 D7 08 06 ...)
xxd -s 0x153e0 -l 4 "$BIN"
# 000153e0: 0000 00a0                                ....
xxd -s 0x14e60 -l 8 "$BIN"
# 00014e60: 0000 d944 0000 d98c                      ...D....
xxd -s 0xc944 -l 16 "$BIN"
# 0000c944: d906 d708 06d7 0806 d708 06d7 0806 d708  ................

# Beam colours: identical to C2
xxd -s 0x70d4 -l 32 "$BIN"
# 000070d4: 0000 0000 0000 00ff 0000 c800 0000 c8ff  ................
# 000070e4: 00ff 0000 00ff 00ff 00ff c800 00ff ffff  ................

# nl_symbol_ptr slots: level_table, level_name_table, hint_table, image_table, palette, num_images, level_order, board, hint_text, level_name_text
r2 -qc 'p8 4 @ 0x16584; p8 4 @ 0x1657c; p8 4 @ 0x16580; p8 4 @ 0x16588; p8 4 @ 0x1653c; p8 4 @ 0x16520; p8 4 @ 0x1654c; p8 4 @ 0x16570; p8 4 @ 0x16514; p8 4 @ 0x16518' "$BIN" </dev/null 2>/dev/null
# 0000d590
# 0000d6cc
# 0000d808
# 00015e60
# 000160e0
# 000163e0
# 00008110
# 000182c0
# 00017890
# 0001a1f0

# New image cases in cell_image_index: 13 -> 100+(dir&3), 14 -> 15, 15 -> 0x74+(dir&1), 16 -> 0x80+dir, 17 -> 0x90,
# 19 -> 0x88+(dir&3), 20 -> 0x8c+(dir&3); switch bound 0x14
r2 -e scr.color=0 -qc 'pi 1 @ 0x4a64; pi 16 @ 0x4c7c' "$BIN" </dev/null 2>/dev/null
# cmplwi cr7, r9, 0x14
# addi r10, r8, 0x64
# b 0x4cbc
# li r10, 0xf
# b 0x4cbc
# clrlwi r2, r11, 0x1f
# addi r10, r2, 0x74
# b 0x4cbc
# addi r10, r11, 0x80
# b 0x4cbc
# li r10, 0x90
# b 0x4cbc
# clrlwi r2, r11, 0x1e
# addi r10, r2, 0x88
# b 0x4cbc
# clrlwi r2, r11, 0x1e
# addi r10, r2, 0x8c
# is_movable jump table (types 3..20): 0x48 -> return 1, 0x50 -> return 0
r2 -e scr.color=0 -qc 'pxw 72 @ 0x324c' "$BIN" </dev/null 2>/dev/null
# 0x0000324c  0x00000048 0x00000048 0x00000048 0x00000048  ...H...H...H...H
# 0x0000325c  0x00000048 0x00000048 0x00000048 0x00000050  ...H...H...H...P
# 0x0000326c  0x00000050 0x00000050 0x00000048 0x00000050  ...P...P...H...P
# 0x0000327c  0x00000048 0x00000048 0x00000050 0x00000048  ...H...H...P...H
# 0x0000328c  0x00000048 0x00000048                        ...H...H
# Comparators: 0x8108 -> clipboard 0x5d94, 0x810c -> toolbox 0x59dc
r2 -e scr.color=0 -qc 'pxw 8 @ 0x8108; pi 4 @ 0x59dc' "$BIN" </dev/null 2>/dev/null
# 0x00008108  0x00005d94 0x000059dc                        ..]...Y.
# lbz r0, 0(r3)
# lbz r3, 0(r4)
# subf r3, r3, r0
# blr
# Slot cap: next_level advances only while current_slot <= 0x30
r2 -e scr.color=0 -qc 'pi 2 @ 0x5d18' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r2, 0x30
# bgt cr7, 0x5d34
# Header bytes 0x8000-0x810f differing from C2 (code addresses only)
python3 -I -c 'import sys;a=open(sys.argv[1],"rb").read();b=open(sys.argv[2],"rb").read();print([hex(0x8000+i) for i in range(0x110) if a[0x7000+i]!=b[0x7000+i]])' "$BIN" "$B2"
# ['0x800e', '0x800f', '0x8012', '0x8013', '0x8016', '0x8017', '0x801a', '0x801b', '0x801e', '0x801f', '0x8026', '0x8027', '0x802a', '0x802b', '0x802e', '0x802f', '0x8032', '0x8033', '0x8036', '0x8037', '0x803a', '0x803b', '0x810a', '0x810b', '0x810e', '0x810f']

# Full checks: 78 blobs well-formed, all 160 images decode, images 0-127 vs C2 (no differences),
# images equal to the floor tile among 128-159, palette equal to C2's
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
B,u,I3,ok=ld(sys.argv[1],0x15e60,0x163e0); B2,_,I2,_=ld(sys.argv[2],0x11ce4,0x121e4)
L=[u(0xd590+4*i) for i in range(79)]; good=0
for i in range(1,79):
    a=L[i]
    while B[a-0x1000]: a+=5
    nx=min(x for x in L+[0xd590] if x>L[i]); good+= nx-(a+1)<4 and not any(B[a-0xfff:nx-0x1000])
print(good, len(I3), ok, [i for i in range(128) if I3[i]!=I2[i]], [i for i in range(128,160) if I3[i]==I3[0]], B[0x150e0:0x153e0]==B2[0x10ee4:0x111e4])' "$BIN" "$B2"
# 78 160 160 [] [145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159] True
```
