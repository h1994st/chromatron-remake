# Chromatron 3 v1.1 — Platform layer (diff against Chromatron 2)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 3.app/Contents/MacOS/chromatron3` (Mach-O, big-endian PowerPC, Carbon).
All addresses are vmaddrs. File offset = vmaddr − 0x1000. Function and data names are the ones in
`re/notes/chromatron3/*.names.tsv` (each C3 function keeps its C2 name). This note cites code by **address**.
Baseline: `re/notes/chromatron2/platform.md` (itself a diff against C1); it also builds on `chromatron3/data.md`
(tables, sprites, globals) and `chromatron3/logic.md`. "Same as C2" means a normalised textual diff (addresses and
identifiers masked) of the decompiled function against C2's is empty, or differs only in line wrapping, `__nl_symbol_ptr`
slot addresses or the constants listed here. "Slot N" = 1-based strip position; `current_slot` (0x803c) = N − 1.

**Import set**: byte-for-byte the same **79** imported symbols as C2 (and C1) (`r2 -qc ir`, sorted, `diff` empty).
Non-lazy imports: 0x16504 `kCFAllocatorDefault`, 0x16508 `kCFBooleanFalse`, 0x1650c `kCFBooleanTrue`,
0x16510 `kCFPreferencesCurrentApplication`, 0x1658c `__sF`.

**Startup** (`app_main` @ 0x28fc, same as C2): `chdir_to_prefs_folder()` @ 0x28a0; `game_init()` @ 0x642c
(`load_save_file()` @ 0x44e4, `decode_images()` @ 0x6de8, `fb_width = 640`, `fb_height = 480`, `framebuffer =
malloc(0xe1000)`, `current_level_id = level_order[0]` (= id 11), `start_level(1)` @ 0x5b5c); `load_menu_bar()` @ 0x2660;
`load_prefs()` @ 0x26d4; `create_main_window()` @ 0x2828; `install_event_handlers()` @ 0x247c;
`RepositionWindow(…, kWindowCenterOnMainScreen)`, `ShowWindow`, `RunApplicationEventLoop()`; then `save_on_quit()` @ 0x6dc0
and `CFPreferencesAppSynchronize`.

**What breaks C2 platform code** (summary): window title; save-file name; **save-file layout** (a 0xff marker byte after
the bitmap, records for ids up to 127, and a completion-flag migration for files without the marker); clipboard prefix
`3-`; force-win cheat key **`^`** instead of `z`; bundle strings. **Unchanged**: window size, framebuffer, layout rects,
render order, every text box (rects, conditions, wording), menus nib, prefs key, `save_bit_perm`, event handling.

## Findings

### P1. Window and framebuffer
**Answer:** **Same as C2 except the title.** `CreateNewWindow(kDocumentWindowClass 6, attributes 0x02000009, &rect,
&main_window)`; the content rect at **0x7fec** = {top 0, left 0, bottom 480, right 640} → **640 × 480**, close + collapse
boxes, not resizable. Title **"Chromatron 3 (v1.1)"** (string @ 0x7974). `main_window` @ 0x165ac. Centred on the main
screen at every launch.
- Framebuffer: `framebuffer` @ 0x165dc = `malloc(0xe1000)`, `fb_width` @ 0x165e0 = 640, `fb_height` @ 0x165e4 = 480, packed
  RGB 24 bpp, row 0 at the top.
- Present: `draw_rgb_image` @ 0x2e08 is C2's (`CGDataProviderCreateWithData(NULL, buf, 921600, NULL)`;
  `CGImageCreate(640, 480, 8, 24, 1920, DeviceRGB, kCGImageAlphaNone, provider, NULL, false, 0)`; `QDBeginCGContext` /
  `CGContextDrawImage` / `QDEndCGContext`). The int→float constant `4330000080000000` is in `__TEXT,__literal8` @ 0x7ff8,
  as in C2.
- Redraw: no timer. `request_redraw` @ 0x31f4 sets `redraw_all_flag` @ 0x8040 = 1 and calls `inval_window_rect` @ 0x297c;
  `on_draw_content` @ 0x2b28 → `paint_window()` @ 0x6bd4 + `CGContextFlush`. As in C2, the flag is never cleared.

**Evidence:** `create_main_window @ 0x2828`: `_CreateNewWindow(6,0x2000009,&local_20,&main_window);`
`___CFStringMakeConstantString("Chromatron 3 (v1.1)")`, `_SetWindowTitleWithCFString` (disassembly: `lis r4,0x200; ori r4,r4,9;
li r3,6`; rect from `bcl` base 0x2830 + 0x57bc = 0x7fec; title 0x2830 + 0x5144 = 0x7974). `game_init @ 0x642c`:
`DAT_000165e0 = 0x280; DAT_000165e4 = 0x1e0; DAT_000165dc = _malloc(0xe1000);`.
**Confidence:** confirmed.

### P2. Screen layout and render order
**Answer:** **Identical to C2.** Rects are `[x0, x1) × [y0, y1)`:

| region | drawn rect | vs C2 |
|---|---|---|
| window / framebuffer | 0,0 – 640,480, cleared to a4a4a4 | same |
| board | 48,18 – 408,378 (15 × 15 × 24 px, centres (60 + 24x, 30 + 24y)) | same |
| toolbox | 448,8 – 602,110 (6 × 4 slots, 26 px pitch, centres (460 + 26(s mod 6), 20 + 26(s div 6))) | same |
| level strip | glyphs inside 10,410 – 510,442 (numbers 1–50 in 2 rows of 25) | same |
| hint text | 450,125 – 620,475 | same |
| win status | 330,385 – 400,420 | same |
| level name | 50,385 – 300,420 (`game_mode == 2`) | same |
| footer | y 450 – 480: "Thank you…" 0–400 (some slots), URL 470–640 | same |

Render order (`paint_window` @ 0x6bd4): decode `site_url_text` @ 0x1a510 from `site_url_xor` @ 0x80f4 on first call →
`render_scene` @ 0x6ac4 (if `redraw_all_flag`) → `present_frame` @ 0x534c → text boxes in P3 order. All of these, plus
`draw_slot_cell` @ 0x661c, `draw_cell_beams` @ 0x66e0, `save_under_drag` @ 0x51e4, `draw_drag_sprite` @ 0x52e4,
`drag_sprite_rect` @ 0x50d0, `draw_line` @ 0x500c (assert line now game.c 0x4c7) and its helpers 0x4dec–0x4f7c, are C2's.
The only renderer change is the sprite mapping in `cell_image_index` @ 0x4a54 (data.md D2/D5).

**Evidence:** `render_scene @ 0x6ac4` (decompiled, same as C2): 15 × 15 opaque cells, then beam overlays
`draw_slot_cell(x, y, 0x10)` where `draw_cell_beams` returns non-zero, toolbox `draw_slot_cell(s, 0xf, 0)` for
`s < 0x18`, strip `draw_digit_string((n−1 − 25·((n−1)/25))·20 + 10, ((n−1)/25)·20 + 0x19a, …)` for `n < 0x33`
when `game_mode == 2`.
**Confidence:** confirmed.

### P3. Text boxes (`DrawThemeTextBox`)
**Answer:** **Identical to C2: nothing added, nothing removed; only the string addresses moved.**
`draw_text_box(str, left, top, right, bottom)` @ 0x2f64 = C2's (`CFStringCreateWithCString(…, 0x0600)`,
`DrawThemeTextBox(cf, kThemeSystemFont 0, kThemeStateActive 1, wrap true, &Rect, teFlushDefault 0, ctx)`).

| # | string | string @ | left, top, right, bottom | when drawn |
|---|---|---|---|---|
| 1a | "You win!" | 0x7a10 | 330, 385, 400, 420 | `level_won != 0` |
| 1b | "(won)" | 0x7a1c | 350, 385, 400, 420 | `level_won == 0` and `completed[current_slot] != 0` |
| 2 | `level_name_text` (0x1a1f0; table 0xd6cc) | — | 50, 385, 300, 420 | `game_mode == 2` (always after startup) |
| 2′ | `level_name_text` | — | 350, 420, 450, 460 | `game_mode != 2` — never happens |
| 3 | `hint_text` (0x17890), or "Click on a level or press spacebar for next." when `game_mode == 2 && level_won` | 0x7a24 | 450, 125, 620, 475 | always |
| 4 | "Thank you for purchasing this game." | 0x7a54 | 0, 450, 400, 480 | `current_slot % 6 == 4`, i.e. slots 5, 11, 17, 23, 29, 35, 41, 47 |
| 5 | `site_url_text` = "silverspaceship.com" (decoded from 0x80f4 into 0x1a510) | — | 470, 450, 640, 480 | always |

The only strings in `__cstring` that are drawn are 0x7a10, 0x7a1c, 0x7a24, 0x7a54, the level names (0x7a78–0x7ec4) and
the hints (0x7ed0–0x7f6c). "freeware" / "more levels @" are still absent. Fit: the longest playable name (29 chars) and
the longest hint (90 chars) are shorter than C2's longest (31 / 195).

**Evidence:** `paint_window @ 0x6bd4` is C2's (normalised diff empty). Disassembly: 0x6cac `li r5,0x181; li r6,0x190;
li r7,0x1a4; bl 0x2f64`; 0x6cd4/0x6cec the two name rects; 0x6d38 the hint rect; 0x6d4c–0x6d74 the
`mulhw … 0x2aaaaaab` mod-6 test and `addi r3,r3,0xe78` (→ 0x7a54); 0x6da8 the URL rect; the `DrawThemeTextBox` call
site at 0x2fe0 passes `r4 = 0, r5 = 1, r6 = 1, r7 = &rect, r8 = 0`.
**Confidence:** confirmed for arguments, rects, conditions and order; font rendering details **likely** (system theme
font, as C1/C2).

### P4. Input
**Answer:** **Same as C2 except the cheat key.** `install_event_handlers` @ 0x247c installs the same six handlers with
the same type specs (`__TEXT,__const` 0x7fac–0x7feb byte-identical to C2's: `'wind' 0x48`, `'wind' 2`, `'mous' 1, 2, 6`,
`'mous' 0xa`, `'keyb' 1`, `'cmds' 1`); handler table @ 0x8024 = 0x29d8 (menu), 0x2b94 (key), 0x2d80 (wheel),
0x3028 (mouse), 0x2b28 (draw), 0x2b04 (close). `on_mouse_event` @ 0x3028, `on_wheel_event` @ 0x2d80 and
`on_key_event` @ 0x2b94 are C2's.
- `handle_key` @ 0x6358 (MacRoman char code): Space / `=` / `+` → next slot if `unlocked[current_slot + 1]`; `-` →
  previous; `r` → restart (`start_level(0)`); Esc → swallowed (returns 1); **`^` (0x5e) → force win** (`force_win = 1;
  check_win()`). C2 used `z` (0x7a), C1 `L` (0x4c). `z` now does nothing. Everything else returns 0 (not handled).
  (Semantics of the cheat: logic.md.)

**Evidence:** `handle_key @ 0x6358`: `if (param_1 == 0x5e) { *(undefined4 *)PTR_force_win_00016540 = 1; check_win(); return 0; }
if (0x5e < param_1) { if (param_1 != 0x72) return 0; start_level(0); return 0; }`. The compare chain at 0x6358–0x63b8 is
0x2d, 0x20, 0x1b, 0x2b, **0x5e**, 0x3d, 0x72.
**Confidence:** confirmed.

### P5. Menus and bundle
**Answer:** **Menus identical to C2 (and C1).** `load_menu_bar` @ 0x2660 loads `Resources/menus.nib` ("menus" @ 0x7948,
"MenuBar" @ 0x7950). `objects.xib`, `classes.nib` and `info.nib` are byte-identical to C2's (`cmp` silent). Menu bar:
application menu (nib title "Chromatron", shown by the system as `CFBundleName` = **"Chromatron 3"**, also in
Hide/Quit; system items incl. Quit ⌘Q); **File**: Reset Level ⌘R `rvrt`, —, Next Level ⌘= `Next`, Previous Level ⌘-
`Prev`, —, Mouse Wheel Changes Level `Whee` (checked), —, Close ⌘W `clos`; **Edit**: Undo ⌘Z (disabled), Redo ⇧⌘Z
(disabled), —, Cut ⌘X (disabled), Copy Level ⌘C `copy`, Paste Level ⌘V `past`, Delete (disabled), Select All ⌘A
(disabled); **Window**: Minimize Window ⌘M, Minimize All Windows ⌥⌘M, —, Bring All to Front, Arrange in Front.
`on_menu_command` @ 0x29d8 is C2's (`Whee` toggle + `store_wheel_pref` @ 0x2788; `Next` → `'='`, `Prev` → `'-'`,
`rvrt` → `'r'` via `handle_key`; `copy` → `copy_solution` @ 0x2c34; `past` → `paste_solution` @ 0x2cb4; else
`eventNotHandledErr` −9874).
**Bundle** (`Contents/Info.plist`, `plutil -p`):

| key | C3 value | C2 value |
|---|---|---|
| `CFBundleName` (menu-bar app title) | **Chromatron 3** | Chromatron 2 |
| `CFBundleExecutable` | chromatron3 | chromatron2 |
| `CFBundleIconFile` | chromatron3.icns (37807 bytes: `it32` + `t8mk`, i.e. one 128 × 128 icon with mask) | chromatron2.icns (31082 bytes, same chunk types) |
| `CFBundleVersion` | **1.1** | 1.4 |
| `CFBundleShortVersionString` | Chromatron 3 (v1.1) | Chromatron 2 (v1.4) |
| `CFBundleGetInfoString` | Chromatron 3 (v1.1), Copyright © **2004** by Sean Barrett | … © 2003 … |
| `CFBundleSignature` | Crm3 (`PkgInfo` = `APPLCrm3`) | Crm2 |
| `CFBundleIdentifier` | com.silverspaceship.chromatron3 | com.silverspaceship.chromatron2 |
| `CFBundlePackageType`, `LSRequiresCarbon`, `CFBundleDevelopmentRegion`, `CFBundleDictionaryVersion` | APPL, true, English, 6.0 | same |

`Contents/` holds `Info.plist`, `MacOS/chromatron3`, `PkgInfo` and `Resources/` (`chromatron3.icns`, `menus.nib`).
**Evidence:** `cmp` of the nib files; `on_menu_command @ 0x29d8` constants `lis/ori` 0x57686565, 0x4e657874,
0x50726576, 0x70617374, 0x636f7079, 0x72767274 at 0x2a1c–0x2a88.
**Confidence:** confirmed (system-provided items and the displayed app-menu title **likely**, standard Carbon
behaviour, as C1/C2).

### P6. Preferences
**Answer:** **Same as C2**: one key `UseMouseWheelToChangeLevel` (string @ 0x7958), CFBoolean, default **true**, read by
`load_prefs` @ 0x26d4 (`CFPreferencesGetAppBooleanValue`; missing → 1; sets the menu check mark), written by
`store_wheel_pref` @ 0x2788 on each toggle, flushed at quit. Domain `kCFPreferencesCurrentApplication` = bundle id
**`com.silverspaceship.chromatron3`** → `~/Library/Preferences/com.silverspaceship.chromatron3.plist`.
**Evidence:** normalised diffs of `load_prefs` / `store_wheel_pref` / `app_main` against C2 differ only in the
`__nl_symbol_ptr` slot of the CF constants.
**Confidence:** confirmed.

### P7. Save file `com.silverspaceship.chromatron3.dat`
**Answer:**
- **Name**: `com.silverspaceship.chromatron3.dat` (string @ 0x79cc, used by `fopen "rb"` in `load_save_file` @ 0x44e4 and
  `fopen "wb"` in `write_save_file` @ 0x47a0). Folder as C2: `chdir_to_prefs_folder` @ 0x28a0 →
  `FSFindFolder(kUserDomain, 'pref', kCreateFolder)` → `~/Library/Preferences/com.silverspaceship.chromatron3.dat`.
- **Layout (CHANGED)**:
  ```
  u8  bitmap[32]                      # scrambled completion bitmap, same algorithm and table as C2
  u8  0xFF                            # NEW: format marker, always written right after the bitmap
  k × { u8 level_id;                  # ascending ids 0..127 that have a saved state
        285 × (u8 type, u8 colour, u8 dir) }   # cells 0..284 (board rows 0-14, then toolbox slots 0-59), as C2
  ```
  File size `33 + 856·k` (C2: `32 + 856·k`). The in-memory state is still 0x1f2c bytes (28-byte cells), only bytes
  +0..+2 are stored.
- **Saved-state range**: `saved_states` @ 0x1a300 now has **128** entries; `write_save_file` writes ids **0..127**
  (`cmpwi r29,0x7f; ble`), C2 0..51. Real ids are 1–78 (only the 50 reachable ones can ever be saved).
- **Reader** (`load_save_file` @ 0x44e4): after a successful 32-byte bitmap read it decodes `completed[0..63]` exactly as C2
  (any value other than 0 or `check(s)` aborts with `game.c:848 failed assertion '0'`; C2 printed 842). It then reads
  records in a loop: read 1 byte; EOF → stop; **byte 0xff → set "marker seen" and continue**; any other byte = level id →
  `malloc(0x1f2c)`, zero it, read 285 triples into +0/+1/+2 of each cell; a short triple frees that record and moves
  on to the next id byte (as C2). The marker is accepted anywhere in the stream, but the writer emits it only first.
  `level_id` is still not validated: id 128 → writes its pointer into cursor col 0x1a500, 129 → cursor row 0x1a504,
  130–131 → 0x1a508–0x1a50f, 132–147 → `site_url_text` 0x1a510–0x1a54f, ≥ 148 → past the end of `__common`. A port should
  accept ids 1–78 only (0xff being the marker).
- **NEW: migration when no marker was seen** (no file, a file shorter than 32 bytes, or a bitmap with no 0xff byte after
  it, presumably written by an earlier C3 build): the bitmap-derived flags are **discarded and recomputed from the saved
  boards**:
  ```c
  memset(completed, 0, 0x100);
  for (slot = 0; slot < 64; slot++) {
      current_level_id = level_order[slot];          // reads 0x8110+slot; slots 50..63 read 0x39, 0, 0, ...
      if (saved_states[current_level_id]) {
          current_slot = slot;
          load_level_data();                         // 0x6efc
          memcpy(board, saved_states[current_level_id], 0x1f2c);
          game_mode = 2;
          check_win();                               // 0x4914: retrace, test pinwheels; if won and !completed[slot]:
      }                                              //   completed[slot]=1; save_level_state(); write_save_file(); update_unlocked()
  }
  current_slot = 0;
  ```
  then `update_unlocked()` (as on every load). So a level counts as completed after migration only if its **saved board
  is currently a solved position**. The first slot found solved rewrites the file in the new format, including the
  marker. If none is solved, the file stays markerless until the next write (first completion or quit). Quirks a
  port can ignore: slot index 50 reads `level_order[50]` = 0x39 = id 57 (slot 50's level again), so a solved id 57 also
  sets `completed[50]` (a bit beyond the strip, saved in the bitmap); slots 51–63 read id 0, and loading id 0 would
  dereference `level_table[0]` = NULL. That only happens if the file contains a record with id byte 0.
- **Bitmap**: same algorithm **and the same `save_bit_perm` as C2** (0x80ac: `14 1f 07 08 1e 0e 03 1c 02 16 05 1b 10 0b
  19 0c 00 09 0d 13 17 04 11 1d 15 0a 06 18 12 1a 01 0f`; `save_bit_scramble` @ 0x4304, `save_bit_index` @ 0x43cc,
  `save_slot_check` @ 0x4410 are C2's). C2 bitmap examples hold: only slot 0 completed → `00 00 10` + 29 zero bytes;
  all 50 slots completed → `9dd4579637ca28db4eacb9525cf3512d284b114840059064286a091840228622`. A C2 bitmap therefore
  decodes without tripping the abort, but C2 and C3 files have different names and layouts.
- **When**: read once in `game_init`; written on first completion of a slot (`check_win`) and at quit (`save_on_quit`
  @ 0x6dc0 = `save_level_state(); write_save_file();`), whole file each time, errors ignored — as C2, plus the writes
  that the migration triggers.

**Evidence:** `write_save_file @ 0x47a0`: `local_30 = 0xff; … _fwrite(abStack_50,1,0x20,f); _fwrite(&local_30,1,1,f);
do { if (saved_states[i]) { _fwrite(&id,1,1,f); …285 × _fwrite(cell,3,1,f)… } } while (++i < 0x80);` (disassembly
0x47c4 `li r0,-1` … `stb r0,0x60(r1)`; 0x487c–0x4890 `addi r3,r1,0x60; li r4,1; li r5,1; … bl fwrite`; 0x48f0
`cmpwi r29,0x7f`). `load_save_file @ 0x44e4`: `if (local_30[0] == 0xff) bVar2 = true; else { …malloc(0x1f2c)… }`
(0x45f4 `cmpwi cr7,r28,0xff`); after `fclose`, `if (!bVar2) { memset(completed,0,0x100); … }` (0x46dc `cmpwi r25,0;
bne 0x4780`; loop 0x46fc–0x4770 calling 0x6efc, `memcpy(…,0x1f2c)`, `game_mode = 2`, 0x4914; bound `cmpwi r30,0x3f`).
**Confidence:** confirmed for the format and the reader/writer algorithms. That the markerless format came from a
C3 v1.0 build is **likely** (no v1.0 binary is available; one would settle it).

### P8. Clipboard (Copy Level / Paste Level)
**Answer:** **Same as C2 except the game digit.** Scrap Manager, flavour `'TEXT'`, plain ASCII, no terminator;
`copy_solution` @ 0x2c34 / `paste_solution` @ 0x2cb4 are C2's.
- `encode_solution` @ 0x5e08 → `clip_text` @ 0x171f8 (54 bytes): **`"3-" + NN + "-" + pairs`** (`clip_text[0] = 0x33`).
  The rest is C2's: entries `{u16 cell index, u8 dir, u8 type, u8 colour, pad}` for every movable cell of rows 0–18
  (`is_movable` now includes gate 16, mobile teleporter 18, DUAL-REFLECTOR 19 and QUAD-BENDER 20),
  `qsort(…, 6, cmp @ 0x5d94)` by type then colour (unsigned), refuse if `2n + 6 ≥ 55` (n ≥ 25; the largest C3 level has 24 movable pieces, so every level can be copied), then per entry
  `v = index + 285·dir` → `b52(v / 52) b52(v % 52)` with `base52_char` @ 0x5dd8 ('a'–'z' = 0–25, 'A'–'Z' = 26–51).
  Examples (stored placements, no equal (type, colour) keys so the order doesn't depend on qsort): slot 1 (id 11) →
  **`3-01-oCcmcO`**; slot 17 (id 55: mirror (0,7) dir 5 colour 4, mirror (14,7) dir 1 colour 1, DUAL-REFLECTOR (7,7)
  dir 1) → **`3-17-hODwhH`**.
- `decode_solution` @ 0x5fc8: requires `'3' '-' d d '-'` (after leading spaces) with `dd == current_slot + 1`; otherwise
  C2's. **C2 (`2-…`) and C1 (`1-…`) strings are rejected by C3, and vice versa.**

**Evidence:** disassembly 0x5ef4–0x5f34: `li r11,0x2d; stb r11,4(r24); stb r11,1(r24); stb r2,2(r24); li r0,0x33;
stb r0,0(r24); stb r9,3(r24)`; paste checks 0x6014 `cmpwi cr7,r0,0x33`, 0x6024 and 0x605c `cmpwi cr7,r0,0x2d`.
Comparator pointer 0x8108 → 0x5d94 (`lbz 3(r3)` vs `lbz 3(r4)`, then `lbz 4`, `cmplw`), C2's code.
**Confidence:** confirmed.

### P9. Timers and animation
**Answer:** **None**, as C2/C1. The import set equals C2's (no timer, clock, random, sound or idle API).
**Evidence:** `r2 -qc ir` lists 79 imports, none matching timer/tick/time/clock/rand/sound/idle; sorted names equal C2's.
**Confidence:** confirmed.

## Unclassified functions

After both C3 name files are applied, the remaining `FUN_` functions should be the C runtime / dyld glue (same six as
C2), plus anything logic.md leaves unnamed:

| address | role (C2 equivalent) |
|---|---|
| `FUN_00002024` | crt `_start` (C2 0x21c4); `entry` @ 0x1ff0 jumps here |
| `FUN_000021b0` | `__dyld_make_delayed_module_initializer_calls` caller (C2 0x2344) |
| `FUN_000021f8` | dyld kernel-support check (C2 0x238c) |
| `FUN_0000224c` | dyld function-lookup trampoline (C2 0x23e0) |
| `FUN_000022e0` | keymgr image-removal callback (C2 0x2474; pointer at 0x8018) |
| `FUN_000023fc` | keymgr init (C2 0x2590; pointer at 0x8014) |

`assert_fail` @ 0x6fe0 is named here. The new function `emit_gates` @ 0x4198 (the gate pass called from `retrace_beams` @ 0x4258)
is named in logic.names.tsv.

## Verification
```bash
BIN="chromatron_mac/Chromatron 3.app/Contents/MacOS/chromatron3"
B2="chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2"

# P1: content rect {0,0,480,640}, title, save name, the "thank you" string, prefs key
xxd -s 0x6fec -l 8 "$BIN"
# 00006fec: 0000 0000 01e0 0280                      ........
r2 -qc 'ps @ 0x7974; ps @ 0x79cc; ps @ 0x7a54; ps @ 0x7958' "$BIN" </dev/null 2>/dev/null
# Chromatron 3 (v1.1)
# com.silverspaceship.chromatron3.dat
# Thank you for purchasing this game.
# UseMouseWheelToChangeLevel
# P1: CGImageCreate(width, height, 8, 24, stride*3, DeviceRGB, bitmapInfo 0, provider; decode/interp/intent = 0)
r2 -e scr.color=0 -qc 'pi 14 @ 0x2e74' "$BIN" </dev/null 2>/dev/null
# mulli r7, r24, 3
# mr r10, r23
# li r9, 0
# mr r8, r3
# li r5, 8
# li r6, 0x18
# mr r4, r28
# mr r3, r27
# stw r29, 0x40(r1)
# xoris r26, r26, sym._NXArgc
# stw r29, 0x38(r1)
# xoris r27, r27, sym._NXArgc
# stw r29, 0x3c(r1)
# bl sym.imp.CGImageCreate

# P3: DrawThemeTextBox(str, 0, 1, 1, &rect, 0, ctx)
r2 -e scr.color=0 -qc 'pi 8 @ 0x2fe0' "$BIN" </dev/null 2>/dev/null
# lwz r9, 0x50(r1)
# addi r7, r1, 0x40
# li r5, 1
# li r6, 1
# li r8, 0
# mr r3, r29
# li r4, 0
# bl sym.imp.DrawThemeTextBox
# P3: paint_window rects: won box, level name (else-branch / mode 2), hint box
r2 -e scr.color=0 -qc 'pi 4 @ 0x6cac; pi 5 @ 0x6cd4; pi 5 @ 0x6cec; pi 5 @ 0x6d38' "$BIN" </dev/null 2>/dev/null
# li r5, 0x181
# li r6, 0x190
# li r7, 0x1a4
# bl 0x2f64
# lwz r3, -0x6c4(r9)
# li r4, 0x15e
# li r5, 0x1a4
# li r6, 0x1c2
# li r7, 0x1cc
# lwz r3, -0x6c4(r9)
# li r4, 0x32
# li r5, 0x181
# li r6, 0x12c
# li r7, 0x1a4
# li r4, 0x1c2
# li r5, 0x7d
# li r6, 0x26c
# li r7, 0x1db
# bl 0x2f64
# P3: "Thank you..." only when current_slot % 6 == 4, rect 0,450,400,480; then the URL box 470,450,640,480
r2 -e scr.color=0 -qc 'pi 6 @ 0x6d4c; pi 11 @ 0x6d64; pi 4 @ 0x6da8' "$BIN" </dev/null 2>/dev/null
# lis r0, 0x2aaa
# addis r2, r31, 0
# ori r0, r0, 0xaaab
# lwz r2, 0x1460(r2)
# mulhw r0, r2, r0
# srawi r9, r2, 0x1f
# subf r0, r9, r0
# mulli r0, r0, 6
# subf r2, r0, r2
# cmpwi cr7, r2, 4
# bne cr7, 0x6d94
# addis r3, r31, 0
# li r4, 0
# addi r3, r3, 0xe78
# li r5, 0x1c2
# li r6, 0x190
# li r7, 0x1e0
# li r4, 0x1d6
# li r5, 0x1c2
# li r6, 0x280
# li r7, 0x1e0
r2 -qc 'ps @ 0x7a10; ps @ 0x7a1c; ps @ 0x7a24' "$BIN" </dev/null 2>/dev/null
# You win!
# (won)
# Click on a level or press spacebar for next.
strings -a "$BIN" | grep -c -E '^(freeware|more levels @)$'
# 0

# P4: event type specs equal C2's; handler table
cmp <(xxd -s 0x6fac -l 0x40 "$BIN") <(xxd -s 0x6fac -l 0x40 "$B2") && echo same-specs
# same-specs
xxd -s 0x7024 -l 24 "$BIN"
# 00007024: 0000 29d8 0000 2b94 0000 2d80 0000 3028  ..)...+...-...0(
# 00007034: 0000 2b28 0000 2b04                      ..+(..+.
# P4: handle_key compare chain; the cheat is '^' (0x5e)
r2 -e scr.color=0 -qc 'pd 25 @ 0x6358' "$BIN" </dev/null 2>/dev/null | grep -o 'cmpwi cr7, r3, 0x[0-9a-f]*'
# cmpwi cr7, r3, 0x2d
# cmpwi cr7, r3, 0x20
# cmpwi cr7, r3, 0x1b
# cmpwi cr7, r3, 0x2b
# cmpwi cr7, r3, 0x5e
# cmpwi cr7, r3, 0x3d
# cmpwi cr7, r3, 0x72

# P5: bundle metadata (menu-bar app name = CFBundleName)
plutil -p "chromatron_mac/Chromatron 3.app/Contents/Info.plist" | grep -E 'CFBundle(Name|Executable|IconFile|Version|ShortVersionString|GetInfoString|Signature|Identifier)"'
#   "CFBundleExecutable" => "chromatron3"
#   "CFBundleGetInfoString" => "Chromatron 3 (v1.1), Copyright © 2004 by Sean Barrett"
#   "CFBundleIconFile" => "chromatron3.icns"
#   "CFBundleIdentifier" => "com.silverspaceship.chromatron3"
#   "CFBundleName" => "Chromatron 3"
#   "CFBundleShortVersionString" => "Chromatron 3 (v1.1)"
#   "CFBundleSignature" => "Crm3"
#   "CFBundleVersion" => "1.1"
cat "chromatron_mac/Chromatron 3.app/Contents/PkgInfo"; echo
# APPLCrm3
ls "chromatron_mac/Chromatron 3.app/Contents/Resources" | paste -sd' ' -
# chromatron3.icns menus.nib
# P5: menus.nib identical to C2
for f in objects.xib classes.nib info.nib; do cmp "chromatron_mac/Chromatron 3.app/Contents/Resources/menus.nib/$f" "chromatron_mac/Chromatron 2.app/Contents/Resources/menus.nib/$f" && echo same; done
# same
# same
# same

# P7: folder = FSFindFolder(kUserDomain -32763, 'pref', kCreateFolder)
r2 -e scr.color=0 -qc 'pi 9 @ 0x28a0' "$BIN" </dev/null 2>/dev/null
# mflr r0
# lis r4, 0x7072
# stw r0, 8(r1)
# ori r4, r4, 0x6566
# stwu r1, -0x4a0(r1)
# li r3, -0x7ffb
# li r5, 1
# addi r6, r1, 0x40
# bl sym.imp.FSFindFolder
# P7: writer: marker byte 0xff prepared, then fwrite(bitmap, 1, 32), fwrite(&marker, 1, 1), ids 0..0x7f
r2 -e scr.color=0 -qc 'pi 3 @ 0x47c4; pi 6 @ 0x4874; pi 6 @ 0x487c; pi 2 @ 0x48f0' "$BIN" </dev/null 2>/dev/null
# li r0, -1
# addi r4, r4, 0x324c
# stb r0, 0x60(r1)
# addi r3, r1, 0x40
# bl sym.imp.fwrite
# addi r3, r1, 0x60
# li r4, 1
# li r5, 1
# mr r6, r27
# addi r3, r1, 0x60
# li r4, 1
# li r5, 1
# mr r6, r27
# li r29, 0
# bl sym.imp.fwrite
# cmpwi cr7, r29, 0x7f
# ble cr7, 0x4894
# P7: reader: id byte 0xff = marker; bitmap assert line 0x350 (848); migration if no marker (memset completed, 64 slots)
r2 -e scr.color=0 -qc 'pi 2 @ 0x45f4; pi 1 @ 0x45d8; pi 2 @ 0x46dc; pi 3 @ 0x46ec; pi 2 @ 0x4730; pi 1 @ 0x4764; pi 2 @ 0x476c' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r28, 0xff
# bne cr7, 0x4628
# li r5, 0x350
# cmpwi cr7, r25, 0
# bne cr7, 0x4780
# lwz r3, 0x2070(r3)
# li r5, 0x100
# bl sym.imp.memset
# stw r30, 0x3b50(r2)
# bl 0x6efc
# bl 0x4914
# cmpwi cr7, r30, 0x3f
# ble cr7, 0x46fc
# P7: save permutation table: identical to C2's
xxd -s 0x70ac -l 32 "$BIN"
# 000070ac: 141f 0708 1e0e 031c 0216 051b 100b 190c  ................
# 000070bc: 0009 0d13 1704 111d 150a 0618 121a 010f  ................
cmp <(xxd -s 0x70ac -l 32 "$BIN") <(xxd -s 0x70ac -l 32 "$B2") && echo same-perm
# same-perm
# P7: model: permutation?, bijection, slot-0-only bitmap prefix, all-50 bitmap
python3 -I -c '
import sys
B=open(sys.argv[1],"rb").read(); P=B[0x70ac:0x70cc]
sc=lambda n: P[n] if n<32 else P[(n+13)&31]+0x60 if n<64 else P[(n+20)&31]+0x20 if n<96 else P[(n+5)&31]+0x40
ix=lambda s,k: sc((s&31)+32*k)+128*(s>>5)
def ck(s): a=9*s; a+=a<<8; a+=a>>4; return a%15+1
def bm(slots):
    b=bytearray(32)
    for s in slots:
        for k in range(4):
            if ck(s)>>k&1: p=ix(s,k); b[p>>3]|=1<<(p&7)
    return b.hex()
print(sorted(P)==list(range(32)), sorted(ix(s,k) for s in range(64) for k in range(4))==list(range(256)), bm([0])[:8], bm(range(50)))' "$BIN"
# True True 00001000 9dd4579637ca28db4eacb9525cf3512d284b114840059064286a091840228622

# P8: clipboard prefix: '-' at [1] and [4], tens digit at [2], '3' (0x33) at [0], units at [3]
r2 -e scr.color=0 -qc 'pi 1 @ 0x5ef4; pi 1 @ 0x5efc; pi 1 @ 0x5f08; pi 1 @ 0x5f20; pi 4 @ 0x5f28' "$BIN" </dev/null 2>/dev/null
# li r11, 0x2d
# stb r11, 4(r24)
# stb r11, 1(r24)
# stb r2, 2(r24)
# li r0, 0x33
# addi r9, r9, 0x30
# stb r0, 0(r24)
# stb r9, 3(r24)
# P8: paste requires '3', '-', ..., '-'
r2 -e scr.color=0 -qc 'pi 1 @ 0x6014; pi 1 @ 0x6024; pi 1 @ 0x605c' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r0, 0x33
# cmpwi cr7, r0, 0x2d
# cmpwi cr7, r0, 0x2d
# P8: worked examples from the stored placements of slot 1 (id 11 @ 0x8674) and slot 17 (id 55 @ 0xb694)
python3 -I -c '
import sys
B=open(sys.argv[1],"rb").read()
b52=lambda d: chr(97+d) if d<26 else chr(65+d-26)
for slot,a in ((1,0x8674),(17,0xb694)):
    a-=0x1000; m=[]
    while B[a]:
        t,d,c,x,y=B[a:a+5]
        if t in (3,4,5,6,7,8,9,13,15,16,18,19,20): m.append((t,c,y*15+x+285*d))
        a+=5
    print("3-%02d-"%slot+"".join(b52(v//52)+b52(v%52) for t,c,v in sorted(m)))' "$BIN"
# 3-01-oCcmcO
# 3-17-hODwhH

# P9: no timer/clock/random/sound imports; import set equals C2's
r2 -e scr.color=0 -qc 'ir' "$BIN" </dev/null 2>/dev/null | grep -c -i -E 'timer|tick|time|clock|rand|sound|idle'
# 0
diff <(r2 -e scr.color=0 -qc ir "$B2" </dev/null 2>/dev/null | awk '{print $NF}' | sort) <(r2 -e scr.color=0 -qc ir "$BIN" </dev/null 2>/dev/null | awk '{print $NF}' | sort) && echo same-imports
# same-imports
```
