# Chromatron 4 v1.0 — Data formats (diff against Chromatron 2)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 4.app/Contents/MacOS/chromatron4` (Mach-O, big-endian PowerPC, SHA-256
`ee66f35da2e9d4e60f3af4c2b3423c0906cf178b3b7e4cb9e8d660a89adbc8c3`). All addresses are vmaddrs. File offset = vmaddr − 0x1000.
All multi-byte values are big-endian. Function names are the ones in `re/notes/chromatron4/*.names.tsv`; each C4 function
keeps the name of its C2 (and C1) equivalent. This note cites functions by **address**, not by decompiled.c line.
Baseline: `re/notes/chromatron2/data.md` (C2), which is itself a diff against `re/notes/chromatron1/data.md`. Only
differences are spelled out; "same as C2" means byte-identical data, or the same algorithm with only addresses changed.
Slot numbering: "slot N" = 1-based position in the level strip; `current_slot` (0x803c) = N − 1. "Id" = level id (index
into `level_table`).

**Method.** The C4 binary has the same function layout as C2 plus one new function (0x405c, logic.md). Every C4 function
was paired with its C2 counterpart by order and size, and the decompiled bodies were compared after normalising
addresses, Ghidra temporaries and global names. Functions called "same as C2" below differ only in addresses (and
string literals where stated). The C3 v1.1 binary was used for comparison where noted.

Sections: `__TEXT,__cstring` 0x6df0–0x778b, `__TEXT,__const` 0x7fac–0x7ff3 (byte-identical to C2's), `__TEXT,__literal8` 0x7ff8–0x7fff (`43300000 80000000`, the int→float constant used by `draw_rgb_image`), `__DATA,__data`
0x8000–0x15417, `__nl_symbol_ptr` 0x15528–0x155c3, `__bss` 0x155e0–0x16261, `__common` 0x16270–0x19b8f.

`game.c` reaches its globals through these local `__nl_symbol_ptr` slots (`r2 -qc 'p8 4 @ <slot>'`). The slot order is
the same as C2's (C2 slot + 0x3230):

| slot | → target | meaning | C2 target |
|---|---|---|---|
| 0x15548 | 0x16ed0 | `hint_text` (char[2048], common) | 0x136a0 |
| 0x1554c | 0x19830 | `level_name_text` (char[256], common) | 0x16000 |
| 0x15550 | 0x19b50 | `site_url_text` | 0x161f0 |
| 0x15554 | 0x15414 | `num_images` | 0x121e4 |
| 0x15558 | 0x8110 | `level_order` | 0x8110 |
| 0x15560 | 0x176d0 | `current_level_id` | 0x13ea0 |
| 0x1556c | 0x16ad0 | `decoded_images` | 0x132a0 |
| 0x15578 | 0x15114 | `palette` | 0x11ee4 |
| 0x155a4 | 0x17900 | `board` | 0x140d0 |
| 0x15588 | 0x19940 | `saved_states` | 0x16110 |
| 0x155b0 | 0xbc48 | `level_name_table` | 0xaa08 |
| 0x155b4 | 0xbd24 | `hint_table` | 0xaadc |
| 0x155b8 | 0xbb6c | `level_table` | 0xa934 |
| 0x155bc | 0x14e94 | `image_table` | 0x11ce4 |

Terms (colour mask, direction 0–7 with 0 = N clockwise, `dx`/`dy` at 0x8044/0x8064) are exactly as in C1/C2.

**What breaks a C2 decoder/renderer** (summary; details below):
1. **New table addresses and sizes**: `level_table` @ 0xbb6c with **55** entries (54 encoded levels; C2: 53 @ 0xa934),
   name table @ 0xbc48, hint table @ 0xbd24, `image_table` @ 0x14e94 with **160** entries (C2: 128), palette @ 0x15114,
   `num_images` @ 0x15414. Still **50 playable** slots with a new `level_order`; **4 ids are unreachable** (5, 11, 35, 46).
   The game starts on id **49** (C2: 13).
2. **Seven new piece types, 16–22**: 16 logic gate, 17 starburst, 18 mobile teleporter, 19 dual-reflector, 20 quad-bender,
   21 occupied square, 22 primary logic gate. Movable: 16, 18, 19, 20, 22; fixed: 17, 21.
   `is_movable` = types 3–9, 13, 15, **16, 18, 19, 20, 22**. A C2 renderer would draw nothing for them (and the C2 beam
   router asserts on type > 15).
3. **New sprites**: 128–135 logic gate, 136–139 dual-reflector, 140–143 quad-bender, 144 starburst, 145 mobile
   teleporter, 146 occupied square, 150–157 primary logic gate (147–149, 158, 159 unused). **Image 14 (teleporter) was
   retouched**: its black and dark-grey pixels moved from palette indices 0/10 to 252/28. Those pixels are now ≥ 16, so they
   are drawn by the min-0x10 overlay pass and cover beams. Images 0–13 and 15–127, the palette and the beam colours are
   byte-identical to C2.
4. The 5-byte record, 28-byte cell, 15 × 15 board, 6 × 4 toolbox, loader, decoder and blitter are **unchanged**.
5. Save file: new name, saved states for ids **0–127** (C2: 0–51), the **same** `save_bit_perm` as C2. Clipboard prefix
   `4-`. Force-win cheat is **Ctrl-O (char 0x0F)**, not `z`. (platform.md.)

## Findings

### D1. Level table: address, count, record layout, loader
**Answer:**
- **`level_table` @ 0xbb6c**: **55** `u32` pointers indexed by level id 0–54. Entry 0 is NULL. Ids 1–54 point to blobs in
  `level_data` 0x81d0–0xbb6b (id 1 @ 0x81d0, highest blob id 54 @ 0xb97c). The table ends where `level_name_table`
  starts (0xbc48). (C2: 53 entries @ 0xa934.)
- **Count**: 54 levels are encoded and **50 are playable**. `level_order` @ 0x8110 (same address as C1/C2) is a `u8[50]`
  mapping slot → id:
  `31 13 0c 01 0a 02 0f 22 04 09 27 07 1c 17 1d 06 2c 16 10 15 1e 0d 11 12 18 33 28 21 08 25 20 2a 14 2d 24 35 2b 2f 36 1a 1b 0e 30 19 32 29 34 26 1f 03`
  (decimal: 49 19 12 1 10 2 15 34 4 9 39 7 28 23 29 6 44 22 16 21 30 13 17 18 24 51 40 33 8 37 32 42 20 45 36 53 43 47
  54 26 27 14 48 25 50 41 52 38 31 3). The slot cap is the same as C2's: `next_level` @ 0x5a44 is C2's code, and the strip
  draws 50 numbers. Each id appears at most once. **Four ids are in no slot and can't be reached**: **5** ("level005",
  8 records), **11** ("level011", 51 records), **35** ("level035", 104 records, 24 movable) and **46** ("level046",
  40 records). All four have the placeholder hint "blah" (0x73c4). The byte after `level_order` (0x8142) is `03`, a copy of
  the last entry (C2 had `06` after its last `06`). It is followed by 141 zero bytes up to 0x81d0 and is unreferenced
  (D8).
- **Level blob and record**: **unchanged from C1/C2**. A blob is a list of 5-byte records `type, dir, colour, x, y`,
  ended by a single `0x00` type byte, then 0–3 zero pad bytes to 4-byte alignment. This was checked for all 54 blobs
  (id 54's padding ends exactly at 0xbb6c). Every x and y is 0–14 and no two records share a cell. The new pieces are new
  `type` values (D2); there are no new fields.
- **Loader**: `load_level_data` @ 0x6c54 is C2's (normalised diff: identical): copy `hint_table[id]` → `hint_text`,
  `level_name_table[id]` → `level_name_text`, `memset(board, 0, 0x1be4)`, then for each record
  `board[y*0x1a4 + x*0x1c + {0,1,2}] = {type, colour, dir}`. Callers are as in C2. `start_level` @ 0x58ac copies a saved
  state of 0x1f2c bytes. `game_init` @ 0x6184 starts on `level_order[0]` = **id 49** (slot 1).
- Per-slot table (`movable` = types 3–9, 13, 15, 16, 18, 19, 20, 22; types new since C1: MF multi-filter 13,
  TW twister 14, CO complementor 15, and new in C4: LG logic gate 16, SB starburst 17, MT mobile teleporter 18,
  DR dual-reflector 19, QB quad-bender 20, OS occupied square 21, PL primary logic gate 22. Hint "–" = the empty string
  0x6df0):

| slot | id | data @ | records | movable | types ≥ 13 | name (string @) | hint @ |
|---|---|---|---|---|---|---|---|
| 1 | 49 | 0xb1d8 | 148 | 4 | – | Crosswards (0x72d0) | – |
| 2 | 19 | 0x8db8 | 17 | 7 | QB | Green vs. Blue IV: The Duel (0x70d8) | 0x7590 |
| 3 | 12 | 0x87a0 | 7 | 4 | – | Quantum Warmup (0x706c) | – |
| 4 | 1 | 0x81d0 | 18 | 5 | LG | Down the Hatch (0x6fbc) | 0x7334 |
| 5 | 10 | 0x8594 | 53 | 7 | LG | The Right Pairs (0x7050) | – |
| 6 | 2 | 0x822c | 25 | 8 | LG | Merge and Merge Again (0x6fcc) | – |
| 7 | 15 | 0x8a90 | 19 | 8 | – | Upwards and Onwards (0x7090) | – |
| 8 | 34 | 0x99bc | 63 | 8 | – | The Big Eight (0x71d8) | – |
| 9 | 4 | 0x8370 | 14 | 3 | SB | Starburst (0x6ff8) | 0x7384 |
| 10 | 9 | 0x84dc | 36 | 9 | SB | Spiral Starburst (0x703c) | – |
| 11 | 39 | 0x9f30 | 189 | 9 | – | Conduit Maze Practice (0x7228) | – |
| 12 | 7 | 0x843c | 13 | 3 | CO, SB | The Call of Duty (0x701c) | 0x742c |
| 13 | 28 | 0x94e8 | 13 | 8 | CO | Secondary Forces (0x7180) | – |
| 14 | 23 | 0x91c0 | 37 | 12 | – | Obstacle Course (0x712c) | – |
| 15 | 29 | 0x952c | 11 | 7 | CO, LG | Color Challenge I (0x7194) | – |
| 16 | 6 | 0x83e4 | 17 | 7 | SB | Riff Raff (0x7010) | 0x73cc |
| 17 | 44 | 0xacd8 | 51 | 13 | – | Partition Dynamics (0x7284) | – |
| 18 | 22 | 0x9160 | 19 | 8 | SB | Wheels Within Wheels (0x7114) | – |
| 19 | 16 | 0x8af0 | 63 | 7 | MT | Leverage (0x70a4) | 0x74c0 |
| 20 | 21 | 0x8fb8 | 84 | 20 | MT | Destination Unknown (0x7100) | – |
| 21 | 30 | 0x9564 | 69 | 13 | MT | Plaid (0x71a8) | – |
| 22 | 13 | 0x87c4 | 54 | 6 | – | Sea Colors (0x707c) | – |
| 23 | 17 | 0x8c2c | 29 | 6 | MT, TW | Right Back At You (0x70b0) | 0x7520 |
| 24 | 18 | 0x8cc0 | 49 | 12 | MT | Locked Room Mystery (0x70c4) | – |
| 25 | 24 | 0x927c | 35 | 14 | – | Blue vs. Green V: Symmetry (0x713c) | – |
| 26 | 51 | 0xb5d0 | 139 | 12 | OS | Exchange Rate (0x72f0) | 0x772c |
| 27 | 40 | 0xa2e4 | 45 | 10 | – | The Big Seven (0x7240) | – |
| 28 | 33 | 0x9940 | 24 | 9 | – | Buddy System (0x71c8) | – |
| 29 | 8 | 0x8480 | 18 | 2 | SB | Pot Luck (0x7030) | – |
| 30 | 37 | 0x9d8c | 21 | 4 | SB | Choice: It's What's For Dinner (0x71fc) | – |
| 31 | 32 | 0x9810 | 60 | 21 | MF | Cubbyholes (0x71bc) | 0x7654 |
| 32 | 42 | 0xa518 | 198 | 9 | OS | Shortest Distance (0x725c) | – |
| 33 | 20 | 0x8e10 | 84 | 19 | MT | Loose Nut (0x70f4) | – |
| 34 | 45 | 0xadd8 | 23 | 5 | SB, TW | Gears (0x7298) | – |
| 35 | 36 | 0x9d04 | 27 | 9 | DR, SB | King Me (0x71f4) | 0x76c4 |
| 36 | 53 | 0xb8e0 | 31 | 13 | DR | Tower of Flower (0x7314) | – |
| 37 | 43 | 0xa8f8 | 198 | 8 | OS | Longest Distance (0x7270) | – |
| 38 | 47 | 0xaf18 | 25 | 6 | SB | Chained Reaction (0x72ac) | – |
| 39 | 54 | 0xb97c | 99 | 13 | TW | Black Diamond (0x7324) | – |
| 40 | 26 | 0x940c | 14 | 3 | LG, SB | Undesirable (0x7168) | – |
| 41 | 27 | 0x9454 | 29 | 13 | PL | Star Castle (0x7174) | 0x75ec |
| 42 | 14 | 0x88d4 | 88 | 15 | PL | Arena (0x7088) | – |
| 43 | 48 | 0xaf98 | 115 | 9 | – | Free as a Bird (0x72c0) | – |
| 44 | 25 | 0x932c | 44 | 7 | – | Duelling Prisms (0x7158) | – |
| 45 | 50 | 0xb4c0 | 54 | 8 | CO, DR, TW | Foolish Consistency (0x72dc) | – |
| 46 | 41 | 0xa3c8 | 67 | 14 | – | The Big Six (0x7250) | – |
| 47 | 52 | 0xb888 | 17 | 8 | CO, TW | Color Challenge II (0x7300) | – |
| 48 | 38 | 0x9df8 | 62 | 16 | SB | No Outlet (0x721c) | – |
| 49 | 31 | 0x96c0 | 67 | 18 | MT | Four Rooms (0x71b0) | – |
| 50 | 3 | 0x82ac | 39 | 20 | LG | Easier For a Camel (0x6fe4) | – |
| – | 5 | 0x83b8 | 8 | 2 | SB | level005 (0x7004) | 0x73c4 ("blah") |
| – | 11 | 0x86a0 | 51 | 8 | MT | level011 (0x7060) | 0x73c4 ("blah") |
| – | 35 | 0x9af8 | 104 | 24 | SB | level035 (0x71e8) | 0x73c4 ("blah") |
| – | 46 | 0xae4c | 40 | 2 | SB | level046 (0x72a0) | 0x73c4 ("blah") |

  The most movable pieces in a playable level is **21** (slot 31, id 32), and **24** in any level (unreachable id 35).
  Neither overflows the 24 visible toolbox slots.

**Evidence:** `load_level_data @ 0x6c54`:
```c
  pcVar4 = *(char **)(PTR_level_table_000155b8 + *(int *)PTR_current_level_id_00015560 * 4);
  _strcpy(hint_text,*(char **)(PTR_hint_table_000155b4 + current_level_id * 4));
  _strcpy(level_name_text,*(char **)(PTR_level_name_table_000155b0 + current_level_id * 4));
  _memset(board,0,0x1be4);
  while (cVar1 != '\0') { iVar6 = (uint)(byte)pcVar4[4] * 0x1a4 + (uint)(byte)pcVar4[3] * 0x1c;
    board[iVar6] = cVar1;  board[iVar6 + 1] = pcVar4[2] /*colour*/;  board[iVar6 + 2] = pcVar4[1] /*dir*/;  pcVar4 += 5; }
```
(Condensed, globals renamed; disassembly 0x6cc8 `li r5,0x1be4`, 0x6cfc `mulli r2,r2,0x1c`, 0x6d00 `mulli r0,r0,0x1a4`.)
`game_init @ 0x6184`: `current_level_id = (uint)*level_order;` (= 0x31). Worked check, first record of id 1 @ 0x81d0:
`03 06 01 06 02` = mirror, dir 6, colour 1 (noise), at (6, 2); the 4th record `10 04 04 07 03` = logic gate facing S at (7, 3).
**Confidence:** confirmed.

### D2. Board: grid size and cell encoding
**Answer:**
- **Grid and memory layout: same as C2**. The board is 15 × 15, plus toolbox rows 15–18. `board` @ 0x17900 (common) is
  19 × 15 cells of **28 bytes** = 0x1f2c bytes. Cell (x, y) is at `board + y*0x1a4 + x*0x1c` and toolbox slot s at
  `board + 0x189c + 28*s`. The cell layout (+0 type, +1 colour, +2 dir, +3 complementor flag, +4..+11 beams in,
  +12..+19 beams out, +20..+27 complementor snapshot) is unchanged. Bytes +3..+27 are runtime-only. How the new pieces
  use them is in logic.md.
- **`type` values**: 0–15 as C2 (0 empty, 1 wall, 2 laser, 3 mirror, 4 bender, 5 filter, 6 prism, 7 doppler, 8 splitter,
  9 tangler, 10 pinwheel, 11 conduit, 12 teleporter, 13 multi-filter, 14 twister, 15 complementor), plus **seven new ones**.
  `cell_image_index` @ 0x4784 now switches on 0–**22** (`cmplwi cr7,r9,0x16`). `is_movable` @ 0x2f88 is a 20-entry jump table
  over `type − 3`. The type names come from the hint strings in D4. Each one was matched to its id because the
  level that shows the hint is the first slot that uses that type (D1 table):

  | type | piece (hint string @) | movable | image index | `dir` in data | `colour` in data |
  |---|---|---|---|---|---|
  | **16** | **logic gate** ("The LOGIC GATE outputs a color if it sees the same color at both of its inputs." 0x7334; slot 4) | **yes** | **128 + dir** (0–7) | 0, 1, 2, 3, 4, 7 | 4 (one record 2: noise) |
  | **17** | **starburst** ("The STARBURST produces four beams…" 0x7384; slot 9) | **no** | **144** | 0 | 4 (one record 1) |
  | **18** | **mobile teleporter** ("The MOBILE TELEPORTER works just like the teleporter, but you can place it anywhere you like." 0x74c0; slot 19) | **yes** | **145** | 0 (one record 2, two 7) | 4 (one record 2) |
  | **19** | **dual-reflector** ("The DUAL-REFLECTOR is double-sided…" 0x76c4; slot 35) | **yes** | **136 + (dir & 3)** | 0, 1, 2, 5 | 4 |
  | **20** | **quad-bender** ("The QUAD-BENDER is a double-sided BENDER…" 0x7590; slot 2) | **yes** | **140 + (dir & 3)** | 0 | 4 |
  | **21** | **occupied square** ("The OCCUPIED SQUARE passes light like an empty square, but you cannot place any pieces there." 0x772c; slot 26) | **no** | **146** | noise (0–5, 7; mostly 0) | noise (1, 2, 4; mostly 4) |
  | **22** | **primary logic gate** ("The PRIMARY LOGIC GATE is just like the regular logic gate, but only works for a single primary color." 0x75ec; slot 41) | **yes** | **150 + dir** (0–7) | 0, 1, 2, 7 | 4 |

  No colour byte affects any new piece's image. Types > 22 give −1 (nothing drawn), and the beam router
  `beam_enter_cell` @ 0x309c asserts (`game.c:0x1a6`) on them. The router places the new types as follows: 21 with 0
  and 10 (passes the beam straight through); 16 and 22 with 1, 2 and 15 (absorb; a separate pass, 0x3f1c, emits their
  output); 17 emits beams on four directions; 18 with 12 (teleports, and pairs with either 12 or 18); 19 and 20 are
  mirror-like. Full semantics: logic.md.
- **Pieces named by the tutorial, checked against the data**: all ten pieces that the hints introduce exist (logic gate,
  primary logic gate, starburst, mobile teleporter, occupied square, dual-reflector, quad-bender, complementor,
  multi-filter, twister). No hint mentions conduits, but they exist (type 11, unchanged from C1, 182 records; e.g. slot 11
  "Conduit Maze Practice").
  The multi-filter (13) survives in **one** record only (slot 31, id 32, whose hint 0x7654 introduces it). The
  quad-bender (20) also has one record (slot 2, id 19).
- **Record counts** over all 54 blobs (2924 records): wall 1049, laser 112, mirror 188, bender 212, filter 9, prism 11,
  doppler 10, splitter 15, tangler 7, pinwheel 642 (93 black), conduit 182, teleporter 10, multi-filter 1, twister 7,
  complementor 5, **logic gate 9, starburst 51, mobile teleporter 37, dual-reflector 4, quad-bender 1, occupied square 356,
  primary logic gate 6**. Laser colours are only 1, 2, 4 and 7, and filter colours only 1, 2 and 4, so
  `cell_image_index` covers every record.
- **Data noise** as in C1/C2 (walls, mirrors and occupied squares carry arbitrary `colour`/`dir` bytes). Keep it byte-exact
  (save file, clipboard sort).
- **Screen positions**: `draw_slot_cell` @ 0x6374 is identical to C2's.

**Evidence:** `cell_image_index @ 0x4784` new cases:
```c
  case 0x10: lVar5 = uVar6 + 0x80;          /* uVar6 = dir */
  case 0x11: lVar5 = 0x90;
  case 0x12: lVar5 = 0x91;
  case 0x13: lVar5 = (uVar6 & 3) + 0x88;
  case 0x14: lVar5 = (uVar6 & 3) + 0x8c;
  case 0x15: lVar5 = 0x92;
  case 0x16: lVar5 = uVar6 + 0x96;
```
(disassembly 0x48a8 `li r10,0x91`; 0x49d8–0x4a08). `is_movable @ 0x2f88`: `addi r3,r3,-3; cmplwi cr7,r3,0x13; bgt → 0`,
then the jump table at 0x2fc0 (20 words; 0x50 → 0x3010 `li r3,1`, 0x58 → 0x3018 `li r3,0`), giving 1 for types
3–9, 13, 15, 16, 18, 19, 20, 22. `beam_enter_cell @ 0x309c`: `case 0: case 10: case 0x15:` (pass-through);
`case 1: case 2: case 0xf: case 0x10: case 0x16:` (absorb); `case 0xc: case 0x12:` the teleporter scan, which looks for
`'\f'` or `'\x12'`. C3 comparison: C3's `cell_image_index` (0x4a54) has cases 16, 17, 19, 20 with the same images
and draws type 18 with **image 14** (the fixed teleporter's sprite). It has no types 21 or 22.
**Confidence:** confirmed for ids, movability and image mapping; the names are **likely** but well supported (hint
text, the level that shows the hint, and sprite art); the semantics belong to logic.md.

### D3. Toolbox: how available pieces and counts are stored
**Answer:** **Same model and code as C2**: there is no piece table and there are no counts. The toolbox holds exactly
the level's movable records (now types 3–9, 13, 15, 16, 18, 19, 20, 22), and their stored `x, y, dir` are the intended
placement. `move_pieces_to_toolbox` @ 0x573c is identical to C2's: scan cells 0–224, copy each movable cell (all 28
bytes) to the next empty toolbox slot with `dir = 0`, clear the board cell's `type`, then `qsort(board + 0x189c, n, 0x1c, cmp)`,
where `cmp` @ 0x572c = `a.type − b.type` (pointer at 0x810c).
- Sort order of new types: … tangler 9, multi-filter 13, complementor 15, **logic gate 16, mobile teleporter 18,
  dual-reflector 19, quad-bender 20, primary logic gate 22**.
- **Visible qsort ties**: filters of different colours share a level in slots **7** (id 15: colours 4, 1),
  **30** (id 37: 1, 4, 2) and **47** (id 52: 2, 4). For these three toolboxes, a stable sort and the 4.4BSD/FreeBSD
  `qsort` (med3 pivot, insertion-sort fallback; the C1 ruling) give the **same** order. Slot 7 shows filters 4, 1 in
  toolbox positions 0, 1; slot 30 shows 1, 4, 2 in positions 1–3; slot 47 shows 2, 4 in positions 5, 6. So, as in C2,
  no visible toolbox order depends on the qsort implementation. Other ties are colour noise on mirrors, benders, prisms,
  mobile teleporters (slot 49) or logic gates (slot 50) and can't be seen.
- **Maximum movable pieces**: 21 in a playable level (slot 31, id 32); 24 overall (unreachable id 35). The visible
  toolbox is still 6 × 4 = 24 (`render_scene` @ 0x681c and `handle_mouse` @ 0x5318 are C2's), so nothing overflows.

**Evidence:** normalised diff of `move_pieces_to_toolbox` C2 0x5810 vs C4 0x573c: identical. `r2 -qc 'pi 4 @ 0x572c'` →
`lbz r0,0(r3); lbz r3,0(r4); subf r3,r3,r0; blr`; `pxw 8 @ 0x8108` → `0x00005ae4 0x0000572c`. The tie analysis used a
Python model of the BSD qsort on each level's row-major movable list.
**Confidence:** confirmed (model, code, counts); the tie result is confirmed under both sort models.

### D4. Level names and tutorial/hint text
**Answer:**
- **`level_name_table` @ 0xbc48**: 55 `u32` pointers into `__cstring` (entry 0 NULL), parallel to `level_table`. Every id
  has its own string (no sharing, unlike C2's ids 22/33). The unreachable ids are named "level005", "level011",
  "level035" and "level046". The longest name is "Choice: It's What's For Dinner" (30 characters). It is drawn exactly as
  in C2 (50, 385, 300, 420; platform.md P3).
- **`hint_table` @ 0xbd24**: 55 pointers, entry 0 NULL, empty hint → **0x6df0** (first byte of `__cstring`, a NUL).
  Copied into `hint_text` @ 0x16ed0 (2048 bytes) and drawn in 450,125–620,475; it is replaced by "Click on a level or
  press spacebar for next." (0x6f68) once the level is won.
- 11 real hints, each used once, and all of them introduce a piece. "blah" (0x73c4) is used by the 4 unreachable ids.

  | slot | id | string @ | starts with |
  |---|---|---|---|
  | 2 | 19 | 0x7590 | The QUAD-BENDER is a double-sided BENDER; it can bounce beams in four directions at once. |
  | 4 | 1 | 0x7334 | The LOGIC GATE outputs a color if it sees the same color at both of its inputs. |
  | 9 | 4 | 0x7384 | The STARBURST produces four beams when a single beam enters it. |
  | 12 | 7 | 0x742c | The COMPLEMENTOR inverts the color of a beam going through it; red becomes cyan/teal… |
  | 16 | 6 | 0x73cc | Like the SPLITTER and TANGLER, a beam going through a STARBURST is no longer quantum entangled. |
  | 19 | 16 | 0x74c0 | The MOBILE TELEPORTER works just like the teleporter, but you can place it anywhere you like. |
  | 23 | 17 | 0x7520 | The TWISTER sends an incoming beam back out 90 degrees clockwise; it is one of the only non-reversible pieces. |
  | 26 | 51 | 0x772c | The OCCUPIED SQUARE passes light like an empty square, but you cannot place any pieces there. |
  | 31 | 32 | 0x7654 | The MULTI-FILTER acts as either a red, blue, or green filter, depending on the direction light goes through it. |
  | 35 | 36 | 0x76c4 | The DUAL-REFLECTOR is double-sided, and can reflect beams 90 degrees (or 180 degrees) on both sides. |
  | 41 | 27 | 0x75ec | The PRIMARY LOGIC GATE is just like the regular logic gate, but only works for a single primary color. |

  The longest hint has 145 characters (0x742c), shorter than C2's 195. All names and hints are 7-bit ASCII without line
  breaks.

**Evidence:** `load_level_data @ 0x6c54` (D1). `xxd -s 0xac48 -l 16` → `0000 0000 0000 6fbc 0000 6fcc 0000 6fe4` (NULL,
"Down the Hatch", "Merge and Merge Again", "Easier For a Camel"); `xxd -s 0xad24 -l 16` → `0000 0000 0000 7334 0000 6df0 …`.
`paint_window` @ 0x692c is identical to C2's 0x69fc (normalised diff).
**Confidence:** confirmed.

### D5. Images/sprites
**Answer:**
- **`num_images` @ 0x15414** = **160** (C2: 128).
- **`image_table` @ 0x14e94**: 160 `u32` pointers into **`image_data` 0xbe00–0x14e93** (0x9094 = 37012 bytes including
  padding). Image 0 @ 0xbe00, image 159 @ 0x14e4c.
- **Encoding, decoder and blit: identical to C1/C2**: no header, 24 × 24, 8-bit palette indices, PCX-style RLE (byte ≤ 0xC0
  is a literal; otherwise a run of `b − 0xC0` copies of the next byte). `decode_images` @ 0x6b40 decodes 159 → 0, `malloc(0x240)` each,
  into `decoded_images` @ 0x16ad0. All 160 streams decode to exactly 576 bytes and end at their zero padding.
  `blit_indexed` @ 0x4a14, `draw_cell_image` @ 0x6294 (asserts `b != -1` / `b < num_images`), `draw_drag_sprite` @ 0x5034
  and `draw_digit_string` @ 0x6648 are C2's. `decoded_images` has room for 256 pointers (0x16ad0–0x16ecf), consistent with
  C2's unreferenced gap after 128 entries.
- **Diff against C2's sheet** (decoded pixels, image by image): images **0–13 and 15–127 are pixel-identical**.
  **Image 14 (teleporter) changed**: the art looks the same, but 97 pixels moved from index 0 to 252 (both black), 32 from
  10 (404040) to 28 (3c3c3c), 4 from 31 to 252 and 2 from 21 to 18.
  Effect: the beam overlay re-blit (min index 0x10) and the drag sprite (min 0x0e) now draw the teleporter's black
  outline and dark rim. In C2 those pixels were transparent, so beams showed through them. A renderer must use C4's own
  image 14.
- **New images 128–159**:

  | index | content | used by |
  |---|---|---|
  | 128–135 | logic gate: three-armed grey "Y" with RGB dots at the hub, rotated by dir 0–7 | type 16 |
  | 136–139 | dual-reflector: straight grey double-sided bar with black end caps, dir & 3 = vertical, /, horizontal, \ | type 19 |
  | 140–143 | quad-bender: tilted grey double-sided bar, dir & 3 | type 20 |
  | 144 | starburst: octagonal burst of red/blue/green rings | type 17 |
  | 145 | mobile teleporter: white eight-spoked asterisk with a dark centre (a smaller variant of image 14) | type 18 |
  | 146 | occupied square: grey checkered tile with a faint spoke pattern | type 21 |
  | 147 | white disc with red/green/blue centre — **unused** | – |
  | 148 | thin red/green/blue outlined eight-petal flower — **unused** | – |
  | 149 | black pinwheel with red/green/blue petal tips — **unused** | – |
  | 150–157 | primary logic gate: the "Y" with red/green/blue striped arms, dir 0–7 | type 22 |
  | 158, 159 | copies of floor tile 0 — **unused** | – |

- **C3 comparison**: C3's `num_images` (0x163e0) is also 160 and its palette is equal. Its images 0–143 equal C4's, except
  that C3 keeps C2's image 14. C3's 144 is different art, and its 145–159 are floor tiles.
- **Full C4 index map**: C2's map (0 floor; 1 wall; pinwheels 2–7, 12/13, 16–23; filters 8–11, 96–99, 112–115;
  14 teleporter (**retouched**); 15 twister; 24–31 prism; 32–39 mirror; 40–43 splitter; 44–47 conduit; 48–55 bender;
  lasers 56–63/72–79/88–95/104–111; 64–71 doppler; 80–87 tangler; 100–103 multi-filter; 116–117 complementor;
  118–127 digits) **plus** 128–146 and 150–157 as above.

**Evidence:** `decode_images @ 0x6b40` uses `num_images` (slot 0x15554), `image_table` (0x155bc), `decoded_images`
(0x1556c), the same RLE loop (`cmpwi …,0xc0`, `cmpwi …,0x23f`). `xxd -s 0x13e94 -l 8` → `0000 be00 0000 be48`; image 0
starts `d9 06 d7 08 06 …` (= C2's image 0). The comparison script is in Verification.
**Confidence:** confirmed (format, table, which images changed and which piece uses which); the art descriptions come
from a decoded sheet.

### D6. Colours
**Answer:** **All identical to C2 (and C1).** The `palette` @ 0x15114 (256 × BGR) is byte-equal to C2's 0x11ee4 and is swapped
to RGB in place by `decode_images`. `beam_colors` @ 0x80d4 (u32[8] 000000, 0000ff, 00c800, 00c8ff, ff0000, ff00ff,
ffc800, ffffff) is the same address and bytes. The background is `memset(fb, 0xa4, …)` in `render_scene` @ 0x681c, the
strip digit palette offsets from `strip_pal_offset` @ 0x66ec are 0 / 0x48 / 0x30 / 0x10, and text colour is the theme
default, all as C2. `draw_cell_beams` @ 0x6438 is C2's routine.
**Evidence:** the palette byte comparison in Verification prints `True`; `xxd -s 0x70d4 -l 32` equals C2's.
**Confidence:** confirmed.

### D7. Fonts
**Answer:** **Same as C2.** Digit glyphs are images 118–127 (pixel-identical to C2), drawn 12 × 12 from the top-left of each
24 × 24 image by `draw_digit_string` @ 0x6648 (x += 8 per digit, += 4 for a space, min index 0x0e). `render_scene` @ 0x681c draws the strip
as numbers 1..50 at x = 10 + 20((n−1) mod 25), y = 410 + 20((n−1) div 25), with a leading '0' drawn as ' '. All other text uses
`draw_text_box` @ 0x2cd8 (`DrawThemeTextBox`, `kThemeSystemFont`).
**Evidence:** `draw_digit_string @ 0x6648`: `blit_indexed(…, *(decoded_images + (c - 0x30) * 4 + 0x1d8), 0xc, 0xc, 0x18, 0xe, …)`
(0x1d8 / 4 = 118).
**Confidence:** confirmed.

### D8. Map of `__DATA,__data` (0x8000–0x15417)
**Answer:**

| range | size | purpose | vs C2 |
|---|---|---|---|
| 0x8000–0x800f | 16 | `_NXArgc`, `_NXArgv`, `_environ`, `___progname` (→ 0x6df0) | same layout |
| 0x8010–0x801f | 16 | crt/dyld glue pointers 0x16274, 0x2170, 0x2054, 0x1fd0 | addresses differ |
| 0x8020 | 4 | `use_wheel_pref` = 1 | same |
| 0x8024–0x803b | 24 | event handlers 0x274c (menu), 0x2908 (key), 0x2af4 (wheel), 0x2d9c (mouse), 0x289c (draw), 0x2878 (close) | addresses differ |
| 0x803c | 4 | `current_slot` = 0 | same |
| 0x8040 | 4 | `redraw_all_flag` = 1 | same |
| 0x8044–0x8063 | 32 | `dir_dx[8]` | same bytes |
| 0x8064–0x8083 | 32 | `dir_dy[8]` | same bytes |
| 0x8084–0x80ab | 40 | two int32[5] doppler maps (logic.md) | same bytes |
| 0x80ac–0x80cb | 32 | `save_bit_perm` `14 1f 07 08 1e 0e 03 1c 02 16 05 1b 10 0b 19 0c 00 09 0d 13 17 04 11 1d 15 0a 06 18 12 1a 01 0f` | **same as C2** (also C3) |
| 0x80cc | 4 | −1, unreferenced | same |
| 0x80d0 | 4 | `drag_image` = −1 | same |
| 0x80d4–0x80f3 | 32 | `beam_colors` | same bytes |
| 0x80f4–0x8107 | 20 | `site_url_xor` → "silverspaceship.com" | same bytes |
| 0x8108 | 4 | clipboard-entry comparator → 0x5ae4 (type, then colour, unsigned) | address differs |
| 0x810c | 4 | toolbox comparator → 0x572c (type) | address differs |
| 0x8110–0x8141 | 50 | `level_order` u8[50] (D1) | **different contents** |
| 0x8142–0x81cf | 142 | `03` + 141 zero bytes, unreferenced | C2: `06` + 141 zeros |
| 0x81d0–0xbb6b | 0x399c | `level_data`: 54 blobs (D1) | C2: 52 blobs, 0x2764 |
| 0xbb6c–0xbc47 | 220 | `level_table` u32[55] | C2: u32[53] @ 0xa934 |
| 0xbc48–0xbd23 | 220 | `level_name_table` u32[55] | C2: u32[53] @ 0xaa08 |
| 0xbd24–0xbdff | 220 | `hint_table` u32[55] | C2: u32[53] @ 0xaadc |
| 0xbe00–0x14e93 | 0x9094 | `image_data`: 160 RLE images (D5) | C2: 128 images, 0x7134 |
| 0x14e94–0x15113 | 640 | `image_table` u32[160] | C2: u32[128] @ 0x11ce4 |
| 0x15114–0x15413 | 768 | `palette` 256 × BGR | moved, same bytes |
| 0x15414–0x15417 | 4 | `num_images` = **160** | moved, C2 = 128 |

Runtime (not in `__data`). `__bss` is the same size as C2's (0xc82) and every variable is at C2 address + 0x3230:
`main_window` 0x155e0, `mouse_alt_button` 0x155e4, `wheel_menu_mark` 0x155e8, `drag_state` 0x155ec, `empty_cell`
0x155f0 (28 bytes), `entangle_counter` 0x1560c, `framebuffer` 0x15610, `fb_width` 0x15614, `fb_height` 0x15618,
`drag_save_under` 0x1561c (0x900), `press_x/y` 0x1621c/0x16220, `drag_inval_x/y` 0x16224/0x16228, `clip_text` 0x1622c
(54 bytes, to 0x16261). `__common`: beam lists `beam_cur_list` 0x162ac / `beam_cur_count` 0x162b0, drag sprite x/y
0x162b4/0x162b8, `beam_buf_a` 0x162c0 and `beam_buf_b` 0x166c0 (**0x400 bytes each**, C2 0x100; logic.md),
`level_started` 0x16ac0, `game_mode` 0x16ac4, `decoded_images` 0x16ad0 (256 × 4; 160 used), `hint_text` 0x16ed0 (2048),
`current_level_id` 0x176d0, `unlocked[64]` 0x176e0, `force_win` 0x177e0, `completed[64]` 0x177f0, `level_won` 0x178f0,
`board` 0x17900 (0x1f2c), `level_name_text` 0x19830 (256), `beam_next_list/count` 0x19930/0x19934,
**`saved_states[128]` 0x19940** (0x200 bytes; C2 had 52 entries), `cursor_col/row` 0x19b40/0x19b44, `site_url_text`
0x19b50 (to 0x19b8f, the end of `__common`).

**Evidence:** byte diff of 0x8044–0x810f against C2 differs only at 0x810a–0x810f (the comparator pointers). Every
`DAT_`/`PTR_` reference in `re/out/chromatron4/decompiled.c` falls in one of these ranges. The `__nl_symbol_ptr` targets
are listed at the top of this note.
**Confidence:** confirmed. The purpose of 0x80cc and 0x8142–0x81cf is **unknown** (unreferenced; only the source would
settle it; irrelevant to a port).

## Verification
```bash
BIN="chromatron_mac/Chromatron 4.app/Contents/MacOS/chromatron4"
B2="chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2"

# Level-table header (level_table @ 0xbb6c = file 0xab6c): id0 NULL, id1 0x81d0, id2 0x822c, id3 0x82ac
xxd -s 0xab6c -l 16 "$BIN"
# 0000ab6c: 0000 0000 0000 81d0 0000 822c 0000 82ac  ...........,....
# ... its last entry (id 54 @ 0xb97c), directly followed by level_name_table @ 0xbc48
xxd -s 0xac44 -l 4 "$BIN"
# 0000ac44: 0000 b97c                                ...|

# First level record: id 1 @ 0x81d0 (file 0x71d0): 03 06 01 06 02 = mirror dir 6 at (6,2); 03 06 04 08 02 = mirror at (8,2);
# 0a 00 02 05 03 = green pinwheel at (5,3); 10 04 04 07 03 = logic gate (type 16) dir 4 at (7,3)
xxd -s 0x71d0 -l 20 "$BIN"
# 000071d0: 0306 0106 0203 0604 0802 0a00 0205 0310  ................
# 000071e0: 0404 0703                                ....

# Slot -> level id order (level_order @ 0x8110) and the byte after it
xxd -s 0x7110 -l 52 "$BIN"
# 00007110: 3113 0c01 0a02 0f22 0409 2707 1c17 1d06  1......"..'.....
# 00007120: 2c16 1015 1e0d 1112 1833 2821 0825 202a  ,........3(!.% *
# 00007130: 142d 2435 2b2f 361a 1b0e 3019 3229 3426  .-$5+/6...0.2)4&
# 00007140: 1f03 0300                                ....

# Name table and hint table headers
xxd -s 0xac48 -l 16 "$BIN"
# 0000ac48: 0000 0000 0000 6fbc 0000 6fcc 0000 6fe4  ......o...o...o.
xxd -s 0xad24 -l 16 "$BIN"
# 0000ad24: 0000 0000 0000 7334 0000 6df0 0000 6df0  ......s4..m...m.

# num_images = 160, first image-table entries, first image "header" (no header: RLE stream D9 06 D7 08 06 ...)
xxd -s 0x14414 -l 4 "$BIN"
# 00014414: 0000 00a0                                ....
xxd -s 0x13e94 -l 8 "$BIN"
# 00013e94: 0000 be00 0000 be48                      .......H
xxd -s 0xae00 -l 16 "$BIN"
# 0000ae00: d906 d708 06d7 0806 d708 06d7 0806 d708  ................

# Palette (BGR) entries 0..7 and beam colours: identical to C2
xxd -s 0x14114 -l 24 "$BIN"
# 00014114: 0000 00a8 0000 00a8 00a8 a800 0000 a8a8  ................
# 00014124: 00a8 2c44 58a8 a8a8                      ..,DX...
xxd -s 0x70d4 -l 32 "$BIN"
# 000070d4: 0000 0000 0000 00ff 0000 c800 0000 c8ff  ................
# 000070e4: 00ff 0000 00ff 00ff 00ff c800 00ff ffff  ................

# nl_symbol_ptr slots: level_table, level_name_table, hint_table, image_table, palette, num_images, level_order, board, hint_text, level_name_text, saved_states
r2 -qc 'p8 4 @ 0x155b8; p8 4 @ 0x155b0; p8 4 @ 0x155b4; p8 4 @ 0x155bc; p8 4 @ 0x15578; p8 4 @ 0x15554; p8 4 @ 0x15558; p8 4 @ 0x155a4; p8 4 @ 0x15548; p8 4 @ 0x1554c; p8 4 @ 0x15588' "$BIN" </dev/null 2>/dev/null
# 0000bb6c
# 0000bc48
# 0000bd24
# 00014e94
# 00015114
# 00015414
# 00008110
# 00017900
# 00016ed0
# 00019830
# 00019940

# cell_image_index: switch bound 22; type 18 -> 0x91; types 16,17,19,20,21,22 -> 0x80+dir, 0x90, 0x88+(dir&3), 0x8c+(dir&3), 0x92, 0x96+dir
r2 -e scr.color=0 -qc 'pi 1 @ 0x4794; pi 2 @ 0x48a8; pi 13 @ 0x49d8' "$BIN" </dev/null 2>/dev/null
# cmplwi cr7, r9, 0x16
# li r10, 0x91
# b 0x4a0c
# addi r10, r11, 0x80
# b 0x4a0c
# li r10, 0x90
# b 0x4a0c
# clrlwi r2, r11, 0x1e
# addi r10, r2, 0x88
# b 0x4a0c
# clrlwi r2, r11, 0x1e
# addi r10, r2, 0x8c
# b 0x4a0c
# li r10, 0x92
# b 0x4a0c
# addi r10, r11, 0x96

# is_movable: jump table over type-3 (0x50 -> return 1, 0x58 -> return 0): 3-9 yes, 10-12 no, 13 yes, 14 no, 15 16 yes, 17 no, 18 19 20 yes, 21 no, 22 yes
r2 -e scr.color=0 -qc 'pi 3 @ 0x2f88; pxw 80 @ 0x2fc0; pi 4 @ 0x3010' "$BIN" </dev/null 2>/dev/null
# addi r3, r3, -3
# mflr r0
# cmplwi cr7, r3, 0x13
# 0x00002fc0  0x00000050 0x00000050 0x00000050 0x00000050  ...P...P...P...P
# 0x00002fd0  0x00000050 0x00000050 0x00000050 0x00000058  ...P...P...P...X
# 0x00002fe0  0x00000058 0x00000058 0x00000050 0x00000058  ...X...X...P...X
# 0x00002ff0  0x00000050 0x00000050 0x00000058 0x00000050  ...P...P...X...P
# 0x00003000  0x00000050 0x00000050 0x00000058 0x00000050  ...P...P...X...P
# li r3, 1
# blr
# li r3, 0
# blr

# 28-byte cell stride in load_level_data (memset 0x1be4, x*0x1c, y*0x1a4)
r2 -e scr.color=0 -qc 'pi 1 @ 0x6cc8; pi 2 @ 0x6cfc' "$BIN" </dev/null 2>/dev/null
# li r5, 0x1be4
# mulli r2, r2, 0x1c
# mulli r0, r0, 0x1a4

# Full checks: 54 blobs well-formed, 160 images decode, images differing from C2, images equal to the floor tile, palette equal to C2
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
        ok+=len(o)==576 and 0<=nx-v<4 and not any(B[v-0x1000:nx-0x1000]); I.append(bytes(o))
    return B,u,I,ok
B,u,I4,ok=ld(sys.argv[1],0x14e94,0x15414); B2,_,I2,_=ld(sys.argv[2],0x11ce4,0x121e4)
L=[u(0xbb6c+4*i) for i in range(55)]; good=0
for i in range(1,55):
    a=L[i]
    while B[a-0x1000]: a+=5
    nx=min(x for x in L+[0xbb6c] if x>L[i]); good+= nx-(a+1)<4 and not any(B[a-0xfff:nx-0x1000])
print(good, len(I4), ok, [i for i in range(128) if I2[i]!=I4[i]], [i for i in range(160) if I4[i]==I4[0]], B[0x14114:0x14414]==B2[0x10ee4:0x111e4])' "$BIN" "$B2"
# 54 160 160 [14] [0, 158, 159] True

# Unreachable ids and per-type record counts
python3 -I -c '
import struct,sys
from collections import Counter
B=open(sys.argv[1],"rb").read(); u=lambda v:struct.unpack(">I",B[v-0x1000:v-0xffc])[0]
order=list(B[0x7110:0x7142]); c=Counter()
for i in range(1,55):
    a=u(0xbb6c+4*i)-0x1000
    while B[a]: c[B[a]]+=1; a+=5
print(sorted(set(range(1,55))-set(order)), [c[t] for t in range(13,23)], sum(c.values()))' "$BIN"
# [5, 11, 35, 46] [1, 7, 5, 9, 51, 37, 4, 1, 356, 6] 2924
```
