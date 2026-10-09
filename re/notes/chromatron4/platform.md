# Chromatron 4 v1.0 — Platform layer (diff against Chromatron 2)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 4.app/Contents/MacOS/chromatron4` (Mach-O, big-endian PowerPC, Carbon).
All addresses are vmaddrs. File offset = vmaddr − 0x1000. Function and data names are the ones in
`re/notes/chromatron4/*.names.tsv` (each C4 function keeps its C2/C1 name). This note cites code by **address**.
Baseline: `re/notes/chromatron2/platform.md`. It builds on `chromatron4/data.md` (tables, sprites, `__bss`/`__common` map)
and `chromatron4/logic.md`. Slot numbering: "slot N" = 1-based strip position, `current_slot` (0x803c) = N − 1.
"Same as C2" means the C4 function's decompiled body equals C2's after normalising addresses, Ghidra temporaries and
global names (each function was checked this way). All `__bss` globals are at their C2 address + 0x3230.

**Import set**: byte-for-byte the same 79 imported symbols as C2 (and C1) (`r2 -qc ir`, sorted names, `diff` empty).
Non-lazy imports: 0x15538 `kCFAllocatorDefault`, 0x1553c `kCFBooleanFalse`, 0x15540 `kCFBooleanTrue`,
0x15544 `kCFPreferencesCurrentApplication`.

**Startup** (`app_main` @ 0x2670, same sequence as C2): `chdir_to_prefs_folder()` @ 0x2614; `game_init()` @ 0x6184
(`load_save_file()`, `decode_images()`, `fb_width = 640`, `fb_height = 480`, `framebuffer = malloc(0xe1000)`,
`current_level_id = level_order[0]` (= **id 49**), `start_level(1)`); `load_menu_bar()` @ 0x23d4; `load_prefs()` @ 0x2448;
`create_main_window()` @ 0x259c; `install_event_handlers()` @ 0x21f0; `RepositionWindow(…, kWindowCenterOnMainScreen)`,
`ShowWindow`, `RunApplicationEventLoop()`; then `save_on_quit()` @ 0x6b18 and `CFPreferencesAppSynchronize`.

**What breaks C2 platform code** (summary): the window title; the save-file name; the saved-state id range (0–127
instead of 0–51; `saved_states` has 128 entries); the clipboard prefix `4-`; the force-win cheat is **char 0x0F
(Ctrl-O)** instead of `z`; the bundle strings. Everything else (window, framebuffer, layout, text boxes, menus,
preferences, the save bitmap including `save_bit_perm`, timers) is identical to C2.

## Findings

### P1. Window and framebuffer
**Answer:** **Same as C2 except the title.** `CreateNewWindow(kDocumentWindowClass 6, attributes 0x02000009, &rect,
&main_window)` with the content rect at **0x7fec** = {top 0, left 0, bottom 480, right 640} → **640 × 480** content, close +
collapse boxes, not resizable. Title **"Chromatron 4 (v1.0)"** (string @ 0x6eb8). Centred on the main screen at every launch.
`main_window` @ 0x155e0. Framebuffer: `framebuffer` @ 0x15610 = `malloc(0xe1000)`, `fb_width` @ 0x15614 = 640,
`fb_height` @ 0x15618 = 480, packed RGB 24 bpp, row 0 at the top. Present: `draw_rgb_image` @ 0x2b7c is C2's
(`CGImageCreate(640, 480, 8, 24, 1920, DeviceRGB, kCGImageAlphaNone, provider, NULL, false, 0)`, then
`CGContextDrawImage` inside `QDBeginCGContext`/`QDEndCGContext`). Redraw model as C2: no timer; `request_redraw` @ 0x2f68 sets
`redraw_all_flag` @ 0x8040 = 1 and calls `inval_window_rect` @ 0x26f0; `on_draw_content` @ 0x289c → `paint_window` @ 0x692c
+ `CGContextFlush`.
**Evidence:** `create_main_window @ 0x259c`: `_CreateNewWindow(6,0x2000009,&local_20,&main_window);
___CFStringMakeConstantString("Chromatron 4 (v1.0)"); _SetWindowTitleWithCFString(…)`. `__TEXT,__const` 0x7fac–0x7ff3 is
byte-identical to C2's (`xxd -s 0x6fec -l 8` → `0000 0000 01e0 0280`). `game_init @ 0x6184`: `fb_width = 0x280;
fb_height = 0x1e0; framebuffer = _malloc(0xe1000);`. Disassembly at 0x2be8–0x2c1c equals C2's at 0x3008.
**Confidence:** confirmed.

### P2. Screen layout and render order
**Answer:** **Identical to C2.** Rects are `[x0, x1) × [y0, y1)` in window-content pixels:

| region | rect |
|---|---|
| window / framebuffer | 0,0 – 640,480, cleared to a4a4a4 |
| board | 48,18 – 408,378 (15 × 15 × 24 px; cell centre (60 + 24x, 30 + 24y)) |
| toolbox | 448,8 – 602,110 (6 × 4 slots, 26 px pitch; slot centre (460 + 26(s mod 6), 20 + 26(s div 6))) |
| level strip | glyphs inside 10,410 – 510,442 (2 × 25 numbers) |
| hint / tutorial text | 450,125 – 620,475 |
| win status text | 330,385 – 400,420 |
| level name text | 50,385 – 300,420 |
| footer | y 450 – 480: "Thank you…" 0–400 (some slots), URL 470–640 |

**Render order** — `paint_window` @ 0x692c:
1. first call only: decode `site_url_text` (0x19b50) from `site_url_xor` @ 0x80f4 (same bytes as C2 → "silverspaceship.com").
2. `render_scene` @ 0x681c (C2's): clear to a4; draw board cells opaque via `draw_slot_cell` @ 0x6374; draw the drag source from `empty_cell`
   @ 0x155f0; draw per-cell beams via `draw_cell_beams` @ 0x6438, re-blitting the overlay at min index 0x10; draw toolbox slots 0–23
   opaque; draw strip numbers 1–50 when `game_mode == 2`.
3. `present_frame` @ 0x509c (C2's): save-under 24 × 24 into `drag_save_under` @ 0x1561c, drag sprite at pointer − 12 with
   min 0x0e, `draw_rgb_image`, then a tail call to the unnamed restore routine at 0x4fb4 (C2 0x5098), which calls
   `drag_sprite_rect` and `copy_rgb_rows`.
4. text boxes in the order of P3.

The only rendering difference is in the data: the retouched teleporter sprite (image 14) and the new sprites
128–157 (data.md D5). Image 14's outline now survives the min-0x10 overlay, so it is drawn over beams.
**Evidence:** normalised diffs: `render_scene`, `draw_slot_cell`, `draw_cell_beams`, `present_frame`, `save_under_drag`,
`draw_drag_sprite`, `drag_sprite_rect`, `copy_rgb_rows`, `draw_line` and the four line helpers, `draw_cell_image`,
`blit_indexed` and `paint_window` are all identical to C2 (differences are only line wrapping and switch-label addresses).
**Confidence:** confirmed.

### P3. Text boxes (`DrawThemeTextBox`)
**Answer:** `draw_text_box(str, left, top, right, bottom)` @ 0x2cd8 is C2's (`CFStringCreateWithCString(…, 0x0600)`,
`DrawThemeTextBox(cf, kThemeSystemFont 0, kThemeStateActive 1, wrap true, &Rect{top, left, bottom, right}, teFlushDefault 0,
ctx)`). `paint_window` @ 0x692c is identical to C2's. Every call, in draw order:

| # | string | string @ | left, top, right, bottom | when drawn |
|---|---|---|---|---|
| 1a | "You win!" | 0x6f54 | 330, 385, 400, 420 | `level_won != 0` |
| 1b | "(won)" | 0x6f60 | 350, 385, 400, 420 | `level_won == 0` and `completed[current_slot] != 0` |
| 2 | `level_name_text` (0x19830) | name table 0xbc48 | 50, 385, 300, 420 | `game_mode == 2` (always after startup) |
| 2′ | `level_name_text` | – | 350, 420, 450, 460 | `game_mode != 2` — never happens |
| 3 | `hint_text` (0x16ed0), or "Click on a level or press spacebar for next." (0x6f68) when `game_mode == 2 && level_won` | hint table 0xbd24 | 450, 125, 620, 475 | always |
| 4 | "Thank you for purchasing this game." | 0x6f98 | 0, 450, 400, 480 | `current_slot % 6 == 4` (slots 5, 11, 17, 23, 29, 35, 41, 47) |
| 5 | `site_url_text` = "silverspaceship.com" | decoded from 0x80f4 | 470, 450, 640, 480 | always |

Fit: the longest name (30 chars, "Choice: It's What's For Dinner") fits the 250-px name box on one line. The longest hint
(145 chars, 0x742c) is shorter than C2's longest (195), so it fits the hint box.
**Evidence:** `paint_window @ 0x692c` (globals renamed):
```c
  if (level_won == 0) { if (completed[current_slot] == 0) goto L; uVar4 = 0x15e; pcVar3 = "(won)"; }
  else { uVar4 = 0x14a; pcVar3 = "You win!"; }
  draw_text_box(pcVar3,uVar4,0x181,400,0x1a4);
L: if (game_mode == 2) {0x32,0x181,300,0x1a4} else {0x15e,0x1a4,0x1c2,0x1cc} -> draw_text_box(level_name_text,…);
  pcVar3 = hint_text; if ((game_mode == 2) && (level_won != 0)) pcVar3 = "Click on a level or press spacebar for next.";
  draw_text_box(pcVar3,0x1c2,0x7d,0x26c,0x1db);
  if (current_slot % 6 == 4) draw_text_box("Thank you for purchasing this game.",0,0x1c2,400,0x1e0);
  draw_text_box(site_url_text,0x1d6,0x1c2,0x280,0x1e0);
```
The `DrawThemeTextBox` call site at 0x2d54 passes `r4 = 0, r5 = 1, r6 = 1, r7 = &rect, r8 = 0`, as C2's 0x3174.
**Confidence:** confirmed for the arguments, rects, conditions and order; the font rendering details are **likely** (system theme font, as in C1/C2).

### P4. Input and keys
**Answer:** **Same as C2 except the cheat key.** `install_event_handlers` @ 0x21f0 installs the same six handlers with the
same type specs (`__TEXT,__const` 0x7fac–0x7feb, byte-identical to C2's). The handler table at 0x8024 is 0x274c (menu),
0x2908 (key), 0x2af4 (wheel), 0x2d9c (mouse), 0x289c (draw), 0x2878 (close).
- Mouse (`on_mouse_event` @ 0x2d9c), wheel (`on_wheel_event` @ 0x2af4) and keyboard (`on_key_event` @ 0x2908: MacRoman
  char from `kEventParamKeyMacCharCodes`, ignored when Cmd (0x100) is held, → `handle_key` @ 0x60a8) are C2's.
- `handle_key` @ 0x60a8: Space / `=` / `+` → next level if `unlocked[current_slot + 1]`; `-` → previous; `r` → restart;
  Esc → swallowed (returns 1); all other keys → 0. The **force-win cheat is char 0x0F** (`force_win = 1; check_win()`).
  C2 used `z` (0x7a); `z` now does nothing. With Control held, the Mac character code for O is 0x0F, so
  the keystroke is **Ctrl-O** (Ctrl-Shift-O too). Cmd-O is filtered out before `handle_key`.
  **Port note:** in a modern macOS shell, Ctrl+O arrives as `NSEvent.characters == "\x0F"` (winit:
  `KeyEventExtModifierSupplement::text_with_all_modifiers()`). winit's `KeyEvent.text` strips Control and yields `"o"`.
  A port must read the all-modifiers text, or map Control + physical KeyO to 0x0F, or the C4 cheat can't be typed.
  Cmd-O stays filtered.
**Evidence:** `handle_key @ 0x60a8`: `if (param_1 == 0x1b) return 1; if (param_1 < 0x1c) { if (param_1 != 0xf) return 0;
force_win = 1; check_win(); return 0; }`; disassembly 0x60dc `cmpwi cr7,r3,0xf; beq 0x6120`, 0x6120–0x6130
`li r0,1; lwz r2,-0xb34(r2); stw r0,0(r2); bl 0x4644`. No `cmpwi …,0x7a` remains.
**Confidence:** confirmed for char 0x0F; that this char comes from Ctrl-O is **likely** (standard Carbon/KCHR control-key
translation; pressing it in the original game would settle it).

### P5. Menus and bundle
**Answer:** **Menus identical to C2 (and C1).** `load_menu_bar` @ 0x23d4 loads `Resources/menus.nib` ("menus", "MenuBar").
`objects.xib`, `classes.nib` and `info.nib` are byte-identical to C2's (`cmp` reports no difference). The menus are: application menu,
**File** (Reset Level ⌘R `rvrt`, Next Level ⌘= `Next`, Previous Level ⌘- `Prev`, Mouse Wheel Changes Level `Whee`, Close ⌘W),
**Edit** (Copy Level ⌘C `copy`, Paste Level ⌘V `past`, others disabled) and **Window**. `on_menu_command` @ 0x274c is C2's.
The application menu shows `CFBundleName` = "Chromatron 4" (**likely**, standard Carbon behaviour).
**Bundle** (`Contents/Info.plist`, `plutil -p`): `CFBundleName` "Chromatron 4", `CFBundleExecutable` chromatron4,
`CFBundleIconFile` chromatron4.icns (45539 bytes), `CFBundleIdentifier` com.silverspaceship.chromatron4, `CFBundleVersion`
"1.0", `CFBundleShortVersionString` "Chromatron 4 (v1.0)", `CFBundleGetInfoString` "Chromatron 4 (v1.0), Copyright © 2003
by Sean Barrett", `CFBundleSignature` Crm4 (`PkgInfo` = `APPLCrm4`), `CFBundlePackageType` APPL, `CFBundleDevelopmentRegion`
English, `LSRequiresCarbon` true. `Contents/` holds `Info.plist`, `MacOS/chromatron4`, `PkgInfo` and `Resources/`
(`chromatron4.icns`, `menus.nib`).
**Evidence:** `cmp` of the three nib files against C2's; `on_menu_command @ 0x274c` uses the same constants 0x57686565,
0x4e657874, 0x50726576, 0x70617374, 0x636f7079, 0x72767274.
**Confidence:** confirmed (system-provided items **likely**).

### P6. Preferences
**Answer:** **Same as C2**: one key, `UseMouseWheelToChangeLevel` (string @ 0x6e9c), CFBoolean, default **true**.
`load_prefs` @ 0x2448 reads it and `store_wheel_pref` @ 0x24fc writes it on each toggle; it is flushed at quit. The domain
is `kCFPreferencesCurrentApplication` = **`com.silverspaceship.chromatron4`**, so the file is
`~/Library/Preferences/com.silverspaceship.chromatron4.plist`. `use_wheel_pref` @ 0x8020 (initial 1), `wheel_menu_mark` @ 0x155e8.
**Evidence:** both functions are C2's (normalised diff identical).
**Confidence:** confirmed.

### P7. Save file `com.silverspaceship.chromatron4.dat`
**Answer:**
- **Name**: `com.silverspaceship.chromatron4.dat` (string @ 0x6f10, used by `fopen "rb"` in `load_save_file` @ 0x42e8 and
  `fopen "wb"` in `write_save_file` @ 0x44ec). **Folder** as C2: `chdir_to_prefs_folder` @ 0x2614 (`FSFindFolder(kUserDomain,
  'pref', kCreateFolder)` → `FSRefMakePath` → `chdir`), so the file is `~/Library/Preferences/com.silverspaceship.chromatron4.dat`.
- **Layout: same as C2**. A 32-byte scrambled bitmap, then `k` records of **856** bytes each: `u8 level_id`, then 285 ×
  (`type`, `colour`, `dir`) for cells 0..284 (board rows 0–14, then toolbox slots 0–59). File size is `32 + 856·k`. The in-memory state is
  0x1f2c bytes (28-byte cells), of which only bytes +0..+2 of each cell are stored.
- **Saved-state range changed back to 128**: `saved_states` @ 0x19940 has **128** entries (0x200 bytes, up to 0x19b3f).
  `write_save_file` writes ids **0..127** in ascending order (`cmpwi cr7,r29,0x7f; ble`), as C1 did; C2 wrote 0..51. Real ids are 1–54, of
  which 5, 11, 35 and 46 are unreachable and can never be saved. On load each record does `malloc(0x1f2c)`, zeroes it and
  reads 285 triples into +0/+1/+2 of each 28-byte cell. A short triple frees the record and stops. `level_id` is
  **not validated**: id 128 writes its pointer into `cursor_col` 0x19b40, 129 into `cursor_row` 0x19b44, 130–131 into
  0x19b48–0x19b4f, 132–147 into `site_url_text` 0x19b50–0x19b8f, and ids ≥ 148 write past the end of `__common` (0x19b90).
  A port should reject ids > 54 (or > 127 to match the file format exactly).
- **Bitmap: identical to C2**: the same `save_slot_check` @ 0x4214 (`a = 9s; a += a<<8; a += a>>4; return a % 15 + 1`), `save_bit_index` @
  0x41d0 and `save_bit_scramble` @ 0x4108 (offsets 13/+0x60, 20/+0x20, 5/+0x40; `n < 128` assert at game.c 0x2cc).
  The scheme is 64 slots × 4 bits, LSB-first, and any value other than 0 or `check(s)` aborts with `game.c:842 failed assertion '0'`.
  **`save_bit_perm` @ 0x80ac is the same table as C2's** (and C3's):
  ```
  14 1f 07 08 1e 0e 03 1c 02 16 05 1b 10 0b 19 0c 00 09 0d 13 17 04 11 1d 15 0a 06 18 12 1a 01 0f
  ```
  So C2's examples hold: only slot 0 completed → bytes `00 00 10` + 29 zeros; all 50 completed →
  `9dd4579637ca28db4eacb9525cf3512d284b114840059064286a091840228622`. (A C2-model bitmap codec works unchanged; only the
  file name and the record-id range differ.)
- **When**: as C2. The file is read once in `game_init`, written on first completion of a slot (`check_win` @ 0x4644) and at quit
  (`save_on_quit` @ 0x6b18 = `save_level_state(); write_save_file();`), whole file each time, errors ignored.
  `load_save_file` ends by calling `update_unlocked` @ 0x4258 (logic.md).
- **No marker byte**: unlike C3 v1.1, C4 v1.0 writes no 0xFF marker after the bitmap and does no migration of
  markerless files. The byte after the bitmap is the first record's id. `write_save_file` @ 0x44ec makes exactly three
  `fwrite` calls (0x45bc bitmap, 0x45f0 id byte, 0x4610 triples), and `load_save_file` @ 0x42e8 never compares with 0xff.
**Evidence:** `load_save_file @ 0x42e8`: `_fopen("com.silverspaceship.chromatron4.dat","rb")`, `_fread(abStack_50,0x20,1,…)`,
`pvVar7 = _malloc(0x1f2c); … _memset(…,0,0x1f2c); do { _fread(… + iVar10,3,1,pFVar3); … iVar10 += 0x1c; } while (uVar5 < 0x11d);`.
`write_save_file @ 0x44ec`: `_fwrite(abStack_50,1,0x20,…)`; per id `_fwrite(local_30,1,1,…)` then triples to `+0x1f10`;
`} while (iVar6 < 0x80);`. The normalised diff against C2 shows only the file name and `0x34` → `0x80`.
`save_bit_scramble @ 0x4108` reads `(&DAT_000080ac)[…]`.
**Confidence:** confirmed (bitmap examples recomputed with the C4 table, Verification).

### P8. Clipboard (Copy Level / Paste Level)
**Answer:** **Same as C2 except the game digit and the movable-type set.** Scrap Manager, flavour `'TEXT'`, flags 0, plain ASCII,
no terminator. `copy_solution` @ 0x29a8 and `paste_solution` @ 0x2a28 are identical to C2's.
- `encode_solution` @ 0x5b58 writes into `clip_text` @ 0x1622c (54 bytes):
  ```
  "4-" + NN + "-" + pairs     # clip_text[0] = 0x34 ('4'); C2 stored '2'
  ```
  As C2 otherwise. Entries `{u16 cell index, u8 dir, u8 type, u8 colour, pad}` are collected for every `is_movable` cell
  of rows 0–18 (now including types **16, 18, 19, 20, 22**). They are `qsort`ed with 6-byte elements by `cmp` @ 0x5ae4,
  type then colour (unsigned). Copy refuses (returns NULL) if `2n + 6 ≥ 55`, i.e. n ≥ 25. C4's largest level has 21
  movable pieces (24 in unreachable id 35), so every level can be copied. Then for each entry
  `v = index + 285·dir` is written as `b52(v / 52) b52(v % 52)` with `base52_char` @ 0x5b28 ('a'–'z' = 0–25, 'A'–'Z' = 26–51).
  Example: slot 29 = id 8 "Pot Luck", stored placement mirror (7,1) dir 5 and doppler (6,7) dir 7 → **`4-29-BROA`**.
- `decode_solution` @ 0x5d18 requires `'4' '-' d d '-'` (after leading spaces) with `dd == current_slot + 1`. Everything else
  is C2's (trailing-space strip, `strlen == 2n`, target `v % 285` empty or movable, apply by sorted (type, colour),
  `dir = v / 285` unmasked, `check_win_and_redraw`). **C2 (`2-…`) and C3 (`3-…`) solutions are rejected by C4 and vice
  versa.**
**Evidence:** disassembly 0x5c44–0x5c84: `li r11,0x2d; stb r11,4(r24); stb r11,1(r24); stb r2,2(r24); li r0,0x34;
stb r0,0(r24); stb r9,3(r24)` (r24 = `clip_text`); paste checks 0x5d64 `cmpwi cr7,r0,0x34`, 0x5d74 and 0x5dac
`cmpwi cr7,r0,0x2d`. The normalised diffs against C2 show only `0x32` → `0x34` / `'2'` → `'4'` and a reordering of
the byte loads that build an entry (same fields).
Comparator pointer 0x8108 → 0x5ae4 (`lbz 3(r3)` vs `lbz 3(r4)`, then `lbz 4`, `cmplw`), same code as C2's 0x5bb8.
**Confidence:** confirmed.

### P9. Timers and animation
**Answer:** **None**, as in C1/C2. The import set is identical to C2's (no timer, clock, random, sound or idle API).
**Evidence:** `r2 -qc ir` lists 79 imports, none matching timer/tick/time/clock/rand/sound/idle; sorted names equal C2's.
**Confidence:** confirmed.

## Unclassified functions

After both C4 name files are applied, the remaining `FUN_` functions should be the C runtime / dyld glue (as in C1/C2):

| address | role (C2 equivalent) |
|---|---|
| `FUN_00001da4` | crt `_start` (C2 `FUN_000021c4`); Ghidra's `entry` @ 0x1d70 jumps here |
| `FUN_00001f24` | `__dyld_make_delayed_module_initializer_calls` caller (C2 0x2344) |
| `FUN_00001f6c` | dyld kernel-support check (C2 0x238c) |
| `FUN_00001fc0` | dyld function-lookup trampoline (C2 0x23e0) |
| `FUN_00002054` | keymgr image-removal callback (C2 0x2474; pointer at 0x8018) |
| `FUN_00002170` | keymgr init (C2 0x2590) |

Not functions in Ghidra's list, and so left unnamed (as in C2): the drag save-under restore routine at 0x4fb4 (tail call
from `present_frame` @ 0x5114; C2 0x5098) and the two qsort comparators 0x5ae4 (clipboard entries) and 0x572c (toolbox).
`assert_fail` @ 0x6d38 is named here.

## Verification
```bash
BIN="chromatron_mac/Chromatron 4.app/Contents/MacOS/chromatron4"
B2="chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2"

# P1: content rect {0,0,480,640}; title, save name, thank-you line, prefs key
xxd -s 0x6fec -l 8 "$BIN"
# 00006fec: 0000 0000 01e0 0280                      ........
r2 -qc 'ps @ 0x6eb8; ps @ 0x6f10; ps @ 0x6f98; ps @ 0x6e9c' "$BIN" </dev/null 2>/dev/null
# Chromatron 4 (v1.0)
# com.silverspaceship.chromatron4.dat
# Thank you for purchasing this game.
# UseMouseWheelToChangeLevel

# P1: CGImageCreate(width, height, 8, 24, stride*3, DeviceRGB, 0, provider; decode/interp/intent = 0)
r2 -e scr.color=0 -qc 'pi 14 @ 0x2be8' "$BIN" </dev/null 2>/dev/null
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
r2 -e scr.color=0 -qc 'pi 8 @ 0x2d54' "$BIN" </dev/null 2>/dev/null
# lwz r9, 0x50(r1)
# addi r7, r1, 0x40
# li r5, 1
# li r6, 1
# li r8, 0
# mr r3, r29
# li r4, 0
# bl sym.imp.DrawThemeTextBox
# P3: paint_window rects: won box, level name (else / mode 2), hint box, slot%6==4 thank-you box, URL box
r2 -e scr.color=0 -qc 'pi 4 @ 0x6a04; pi 4 @ 0x6a30; pi 4 @ 0x6a48; pi 5 @ 0x6a90; pi 1 @ 0x6ac8; pi 5 @ 0x6ad4; pi 4 @ 0x6b00' "$BIN" </dev/null 2>/dev/null
# li r5, 0x181
# li r6, 0x190
# li r7, 0x1a4
# bl 0x2cd8
# li r4, 0x15e
# li r5, 0x1a4
# li r6, 0x1c2
# li r7, 0x1cc
# li r4, 0x32
# li r5, 0x181
# li r6, 0x12c
# li r7, 0x1a4
# li r4, 0x1c2
# li r5, 0x7d
# li r6, 0x26c
# li r7, 0x1db
# bl 0x2cd8
# cmpwi cr7, r2, 4
# li r4, 0
# addi r3, r3, 0x664
# li r5, 0x1c2
# li r6, 0x190
# li r7, 0x1e0
# li r4, 0x1d6
# li r5, 0x1c2
# li r6, 0x280
# li r7, 0x1e0

# P4: handler table; event type specs identical to C2
xxd -s 0x7024 -l 24 "$BIN"
# 00007024: 0000 274c 0000 2908 0000 2af4 0000 2d9c  ..'L..)...*...-.
# 00007034: 0000 289c 0000 2878                      ..(...(x
cmp <(xxd -s 0x6fac -l 0x48 "$BIN") <(xxd -s 0x6fac -l 0x48 "$B2") && echo same-const
# same-const
# P4: cheat key is char 0x0F: force_win = 1; check_win()
r2 -e scr.color=0 -qc 'pi 2 @ 0x60dc; pi 5 @ 0x6120' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r3, 0xf
# beq cr7, 0x6120
# addis r2, r31, 1
# li r0, 1
# lwz r2, -0xb34(r2)
# stw r0, 0(r2)
# bl 0x4644

# P5: bundle metadata and menus.nib identical to C2
plutil -p "chromatron_mac/Chromatron 4.app/Contents/Info.plist" | grep -E 'CFBundle(Name|Executable|IconFile|Identifier|Version|ShortVersionString|GetInfoString|Signature)"'
#   "CFBundleExecutable" => "chromatron4"
#   "CFBundleGetInfoString" => "Chromatron 4 (v1.0), Copyright © 2003 by Sean Barrett"
#   "CFBundleIconFile" => "chromatron4.icns"
#   "CFBundleIdentifier" => "com.silverspaceship.chromatron4"
#   "CFBundleName" => "Chromatron 4"
#   "CFBundleShortVersionString" => "Chromatron 4 (v1.0)"
#   "CFBundleSignature" => "Crm4"
#   "CFBundleVersion" => "1.0"
for f in objects.xib classes.nib info.nib; do cmp "chromatron_mac/Chromatron 4.app/Contents/Resources/menus.nib/$f" "chromatron_mac/Chromatron 2.app/Contents/Resources/menus.nib/$f" && echo same; done
# same
# same
# same

# P7: folder = FSFindFolder(kUserDomain -32763, 'pref', kCreateFolder)
r2 -e scr.color=0 -qc 'pi 9 @ 0x2614' "$BIN" </dev/null 2>/dev/null
# mflr r0
# lis r4, 0x7072
# stw r0, 8(r1)
# ori r4, r4, 0x6566
# stwu r1, -0x4a0(r1)
# li r3, -0x7ffb
# li r5, 1
# addi r6, r1, 0x40
# bl sym.imp.FSFindFolder
# P7: read: fopen, 32-byte bitmap, 64 slots, malloc/memset(0x1f2c) per record, 3-byte triples
r2 -e scr.color=0 -qc 'pi 1 @ 0x4318; pi 1 @ 0x432c; pi 1 @ 0x43e4; pi 1 @ 0x441c; pi 1 @ 0x4440; pi 1 @ 0x4454' "$BIN" </dev/null 2>/dev/null
# bl sym.imp.fopen
# li r4, 0x20
# cmpwi cr7, r30, 0x3f
# li r3, 0x1f2c
# li r5, 0x1f2c
# li r4, 3
# P7: write: per id 1 id byte + triples at 28-byte steps up to +0x1f10; ids 0..0x7f (C2: 0..0x33)
r2 -e scr.color=0 -qc 'pi 1 @ 0x45f8; pi 1 @ 0x4600; pi 1 @ 0x4620' "$BIN" </dev/null 2>/dev/null
# addi r28, r30, 0x1f10
# li r4, 3
# cmpwi cr7, r29, 0x7f
# P7: save permutation table (same bytes as C2)
xxd -s 0x70ac -l 32 "$BIN"
# 000070ac: 141f 0708 1e0e 031c 0216 051b 100b 190c  ................
# 000070bc: 0009 0d13 1704 111d 150a 0618 121a 010f  ................
cmp <(xxd -s 0x70ac -l 32 -p "$BIN") <(xxd -s 0x70ac -l 32 -p "$B2") && echo same-as-C2
# same-as-C2
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

# P8: clipboard prefix: '-' at [1] and [4], tens digit at [2], '4' (0x34) at [0], units at [3]
r2 -e scr.color=0 -qc 'pi 1 @ 0x5c44; pi 1 @ 0x5c4c; pi 1 @ 0x5c58; pi 1 @ 0x5c70; pi 1 @ 0x5c78; pi 1 @ 0x5c80; pi 1 @ 0x5c84' "$BIN" </dev/null 2>/dev/null
# li r11, 0x2d
# stb r11, 4(r24)
# stb r11, 1(r24)
# stb r2, 2(r24)
# li r0, 0x34
# stb r0, 0(r24)
# stb r9, 3(r24)
# P8: paste requires '4', '-', ..., '-'
r2 -e scr.color=0 -qc 'pi 1 @ 0x5d64; pi 1 @ 0x5d74; pi 1 @ 0x5dac' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r0, 0x34
# cmpwi cr7, r0, 0x2d
# cmpwi cr7, r0, 0x2d
# P8: comparators (clipboard 0x5ae4, toolbox 0x572c)
r2 -e scr.color=0 -qc 'pxw 8 @ 0x8108; pi 4 @ 0x572c' "$BIN" </dev/null 2>/dev/null
# 0x00008108  0x00005ae4 0x0000572c                        ..Z...W,
# lbz r0, 0(r3)
# lbz r3, 0(r4)
# subf r3, r3, r0
# blr
# P8: worked example, slot 29 (id 8 @ 0x8480) stored placement -> 4-29-BROA
python3 -I -c '
import sys
B=open(sys.argv[1],"rb").read(); a=0x8480-0x1000; m=[]
b52=lambda d: chr(97+d) if d<26 else chr(65+d-26)
while B[a]:
    t,d,c,x,y=B[a:a+5]
    if t in (3,4,5,6,7,8,9,13,15,16,18,19,20,22): m.append((t,c,y*15+x+285*d))
    a+=5
print("4-29-"+"".join(b52(v//52)+b52(v%52) for t,c,v in sorted(m)))' "$BIN"
# 4-29-BROA

# P9: same 79 imports as C2, no timer/clock/random/sound
r2 -e scr.color=0 -qc 'ir' "$BIN" </dev/null 2>/dev/null | wc -l | tr -d ' '
# 79
diff <(r2 -e scr.color=0 -qc ir "$B2" </dev/null 2>/dev/null | awk '{print $NF}' | sort) <(r2 -e scr.color=0 -qc ir "$BIN" </dev/null 2>/dev/null | awk '{print $NF}' | sort) && echo same-imports
# same-imports
r2 -e scr.color=0 -qc 'ir' "$BIN" </dev/null 2>/dev/null | grep -c -i -E 'timer|tick|time|clock|rand|sound|idle'
# 0
```
