# Chromatron 2 v1.4 — Platform layer (diff against Chromatron 1)

Cites addresses only; no decompiled.c line numbers.

Binary: `chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2` (Mach-O, big-endian PowerPC, Carbon).
All addresses are vmaddrs. File offset = vmaddr − 0x1000. Function and data names are the ones in
`re/notes/chromatron2/*.names.tsv` (each C2 function keeps its C1 name). This note cites code by **address**.
Baseline: `re/notes/chromatron1/platform.md`; it also builds on `chromatron2/data.md` (cell stride, tables, sprites)
and `chromatron2/logic.md`. "Same as C1" means the decompiled function is C1's with only addresses, the 28-byte cell
stride and the constants listed here changed (checked function by function with a normalised textual diff).

**Import set**: byte-for-byte the same 79 imported symbols as C1 (`r2 -qc ir` on both binaries, sorted, `diff` empty).
Non-lazy imports: 0x12308 `kCFAllocatorDefault`, 0x1230c `kCFBooleanFalse`, 0x12310 `kCFBooleanTrue`,
0x12314 `kCFPreferencesCurrentApplication`, 0x12390 `__sF`.
Local slots used here: 0x1231c → 0x16000 `level_name_text`, 0x12320 → 0x161f0 `site_url_text`, 0x1232c → 0x13290
`level_started`, 0x12340 → 0x13088 and 0x12344 → 0x13084 (drag sprite y and x).

**Startup** (`app_main` @ 0x2a90, same sequence as C1): `chdir_to_prefs_folder()` @ 0x2a34; `game_init()` @ 0x6254
(`load_save_file()`, `decode_images()`, `fb_width = 640`, `fb_height = 480`, `framebuffer = malloc(0xe1000)`,
`current_level_id = level_order[0]` (= id 13), `start_level(1)`); `load_menu_bar()` @ 0x27f4; `load_prefs()` @ 0x2868;
`create_main_window()` @ 0x29bc; `install_event_handlers()` @ 0x2610; `RepositionWindow(…, kWindowCenterOnMainScreen)`,
`ShowWindow`, `RunApplicationEventLoop()`; then `save_on_quit()` @ 0x6be8 and `CFPreferencesAppSynchronize`.

**What breaks C1 platform code** (summary): window title; two text boxes ("freeware", "more levels @") gone, two new ones
(level name, "Thank you for purchasing this game."); save-file name, `save_bit_perm`, in-memory state size 0x1f2c and the
saved-state id range 0–51; clipboard prefix `2-`; the force-win cheat key is `z` instead of `L`.

## Findings

### P1. Window and framebuffer
**Answer:** **Same as C1 except the title.**
- `CreateNewWindow(kDocumentWindowClass 6, attributes 0x02000009, &rect, &main_window)` with the content rect at **0x7fec**
  = {top 0, left 0, bottom 480, right 640} → **640 × 480** content, close + collapse boxes, not resizable, no zoom box.
  Title **"Chromatron 2 (v1.4)"** (string @ 0x6f88). Centred on the main screen at every launch. `main_window` @ 0x123b0.
- Framebuffer: `framebuffer` @ 0x123e0 = `malloc(0xe1000)`, `fb_width` @ 0x123e4 = 640, `fb_height` @ 0x123e8 = 480,
  packed RGB 24 bpp, row 0 at the top.
- Present: `draw_rgb_image` @ 0x2f9c(0, 0, buf, 640, 480, 640) →
  `CGDataProviderCreateWithData(NULL, buf, 921600, NULL)`;
  `CGImageCreate(640, 480, bitsPerComponent 8, bitsPerPixel 24, bytesPerRow 1920, CGColorSpaceCreateDeviceRGB(),
  bitmapInfo 0 (kCGImageAlphaNone), provider, decode NULL, shouldInterpolate false, intent 0)`;
  `QDBeginCGContext` / `CGContextDrawImage(ctx, {0, 0, 640, 480}, image)` / `QDEndCGContext`; release both.
- Redraw: no timer. `request_redraw` @ 0x3388 sets `redraw_all_flag` @ 0x8040 = 1 and calls `inval_window_rect` @ 0x2b10
  (`InvalWindowRect`). `on_draw_content` @ 0x2cbc → `paint_window()` @ 0x69fc + `CGContextFlush`. Callers and rects are as
  in C1 (whole window from `start_level`, `move_pieces_to_toolbox`, `check_win_and_redraw`, `update_unlocked`; the 60 × 60
  old / 25 × 25 new drag boxes from `handle_mouse`). The flag is never cleared, so every paint re-renders everything.

**Evidence:** `create_main_window @ 0x29bc`: `_CreateNewWindow(6,0x2000009,&local_20,&main_window);` then
`___CFStringMakeConstantString("Chromatron 2 (v1.4)")`, `_SetWindowTitleWithCFString`. `xxd -s 0x6fec -l 8` →
`0000 0000 01e0 0280`. `draw_rgb_image @ 0x2f9c`: `_CGImageCreate(param_4,param_5,8,0x18,param_6 * 3,uVar3,0,uVar2);`
and the disassembly at 0x3008–0x303c (`li r9,0`, three `stw r29` of 0 to 0x38/0x3c/0x40), identical to C1's 0x3138.
`game_init @ 0x6254`: `fb_width = 0x280; fb_height = 0x1e0; framebuffer = _malloc(0xe1000);`.
**Confidence:** confirmed.

### P2. Screen layout and render order
**Answer:** Rects are `[x0, x1) × [y0, y1)`, window-content pixels.

| region | drawn rect | vs C1 |
|---|---|---|
| window / framebuffer | 0,0 – 640,480, cleared to a4a4a4 | same |
| board | 48,18 – 408,378 (15 × 15 × 24 px) | same |
| toolbox | 448,8 – 602,110 (6 × 4 slots, 26 px pitch) | same |
| level strip | glyphs inside 10,410 – 510,442 (2 × 25 numbers) | same |
| hint / tutorial text | 450,125 – 620,475 | same |
| win status text | 330,385 – 400,420 | same |
| **level name text** | **50,385 – 300,420** when `game_mode == 2` (always after startup); 350,420 – 450,460 otherwise (unreachable) | **new** |
| footer | y 450 – 480: "Thank you…" 0–400 (some slots), URL 470–640 | **changed** (C1: "freeware", "more levels @") |

**Render order** — `paint_window` @ 0x69fc:
1. first call only: decode `site_url_text` from `site_url_xor` (same bytes as C1 → "silverspaceship.com");
2. `render_scene` @ 0x68ec (identical to C1: clear a4; board cells opaque via `draw_slot_cell` @ 0x6444, drag source drawn
   from `empty_cell` @ 0x123c0 (28 zero bytes); per-cell beams via `draw_cell_beams` @ 0x6508 with overlay re-blit at
   min 0x10; toolbox slots 0–23 opaque; strip numbers 1–50 when `game_mode == 2`);
3. `present_frame` @ 0x5180 (identical to C1: save-under 24 × 24 into `drag_save_under` @ 0x123ec (stride 32 px), drag
   sprite at pointer − 12 with min 0x0e, `draw_rgb_image`, restore);
4. text boxes, in the order of P3 (on top of everything, including the drag sprite).

**Evidence:** normalised diffs of `render_scene`, `draw_slot_cell`, `draw_cell_beams`, `present_frame`, `save_under_drag`,
`draw_drag_sprite`, `drag_sprite_rect`, `draw_line` and its four helpers against C1 show only address/stride changes
(e.g. `draw_cell_beams @ 0x6508`: `iVar3 = param_2 * 0x1a4 + param_1 * 0x1c;` vs C1 `* 300 + * 0x14`; segment ends
`iVar2 + 0x49` / `+ 0x2f` / `iVar6 + 0x2b` / `+ 0x11` unchanged). `draw_slot_cell @ 0x6444`: `param_2 * 0x18 + 0x1e`,
`param_1 * 0x18 + 0x3c`; toolbox `… * 0x1a + 0x14`, `… * 0x1a + 0x1cc`.
**Confidence:** confirmed.

### P3. Text boxes (`DrawThemeTextBox`)
**Answer:** `draw_text_box(str, left, top, right, bottom)` @ 0x30f8 is identical to C1:
`CFStringCreateWithCString(kCFAllocatorDefault, str, ASCII 0x0600)`, `DrawThemeTextBox(cf, kThemeSystemFont 0,
kThemeStateActive 1, wrap true, &Rect{top, left, bottom, right}, teFlushDefault 0, ctx)`. Font, colour (black, theme
default), left justification, word wrap, no truncation and no background fill are as in C1.

Every call, in draw order (`paint_window` @ 0x69fc):

| # | string | string @ | left, top, right, bottom | w × h | when drawn | vs C1 |
|---|---|---|---|---|---|---|
| 1a | "You win!" | 0x7024 | 330, 385, 400, 420 | 70 × 35 | `level_won != 0` | same |
| 1b | "(won)" | 0x7030 | 350, 385, 400, 420 | 50 × 35 | `level_won == 0` and `completed[current_slot] != 0` | same |
| 2 | `level_name_text` (common 0x16000; data.md D4) | name table 0xaa08 | **50, 385, 300, 420** | 250 × 35 | `game_mode == 2` (always after startup) | **new** (replaces C1's empty `mode_text`) |
| 2′ | `level_name_text` | – | 350, 420, 450, 460 | 100 × 40 | `game_mode != 2` — never happens | same rect as C1's unreachable `mode_text` |
| 3 | `hint_text` (common 0x136a0), or "Click on a level or press spacebar for next." (0x7038) when `game_mode == 2 && level_won` | – | 450, 125, 620, 475 | 170 × 350 | always | same |
| 4 | **"Thank you for purchasing this game."** | **0x7068** | **0, 450, 400, 480** | 400 × 30 | **`current_slot % 6 == 4`**, i.e. displayed levels **5, 11, 17, 23, 29, 35, 41, 47** | **new** |
| 5 | `site_url_text` = "silverspaceship.com" | decoded from 0x80f4 into 0x161f0 | 470, 450, 640, 480 | 170 × 30 | always | same |

**Removed**: C1's "freeware" (0, 450, 100, 480) and "more levels @" (350, 450, 465, 480). Neither string exists in C2's
`__cstring`. The only strings in `__cstring` that are drawn are 0x7024, 0x7030, 0x7038, 0x7068, the level names
(0x708c–0x73c4) and the hints (0x73d0–0x7784).

Fit: the longest name (31 chars) fits one line of the 250-px name box; the longest hint (195 chars, 0x7400) fits the
hint box as in C1.

**Evidence:** `paint_window @ 0x69fc`:
```c
  if (*(int *)PTR_level_won_00012354 == 0) { if (completed[current_slot] == 0) goto LAB_00006ae4;
    uVar4 = 0x15e; pcVar3 = "(won)"; } else { uVar4 = 0x14a; pcVar3 = "You win!"; }
  draw_text_box(pcVar3,uVar4,0x181,400,0x1a4);
  if (game_mode == 2) { 0x32,0x181,300,0x1a4 } else { 0x15e,0x1a4,0x1c2,0x1cc }  -> draw_text_box(level_name_text, …);
  pcVar3 = hint_text; if ((game_mode == 2) && (level_won != 0)) pcVar3 = "Click on a level or press spacebar for next.";
  draw_text_box(pcVar3,0x1c2,0x7d,0x26c,0x1db);
  if (current_slot % 6 == 4) draw_text_box("Thank you for purchasing this game.",0,0x1c2,400,0x1e0);
  draw_text_box(site_url_text,0x1d6,0x1c2,0x280,0x1e0);
```
(condensed). The disassembly at 0x6b74–0x6bb8 computes `slot − 6·mulhw(slot, 0x2aaaaaab)` and compares with 4; the
`DrawThemeTextBox` call site at 0x3174 passes `r4 = 0, r5 = 1, r6 = 1, r7 = &rect, r8 = 0` exactly as C1's 0x32a4.
**Confidence:** confirmed for arguments, rects, conditions and order; font rendering details are **likely** (system theme
font, as for C1).

### P4. Input
**Answer:** **Same as C1.** `install_event_handlers` @ 0x2610 installs the same six handlers on the window target with the
same type specs (`__TEXT,__const` 0x7fac–0x7feb: `'wind' 0x48`, `'wind' 2`, `'mous' 1, 2, 6`, `'mous' 0xa`,
`'keyb' 1`, `'cmds' 1`); the handler table at 0x8024 is 0x2b6c (menu), 0x2d28 (key), 0x2f14 (wheel), 0x31bc (mouse),
0x2cbc (draw), 0x2c98 (close).
- Mouse (`on_mouse_event` @ 0x31bc): identical (button 1 + Shift/Option/Control latch → button 2; down 1/3, up 4/6,
  dragged 0; content-rect and `IsWindowHilited` checks; local coordinates to `handle_mouse` @ 0x53ec). `kEventMouseMoved`
  is still not registered (the handler's `kind 5` branch is dead, as in C1).
- Wheel (`on_wheel_event` @ 0x2f14): identical (only if `use_wheel_pref`; delta > 0 → `'-'`, else `'='`).
- Keyboard (`on_key_event` @ 0x2d28): identical (MacRoman char, ignored when Cmd held, → `handle_key` @ 0x617c).
  `handle_key` keys: Space / `=` / `+` next (if `unlocked[slot+1]`), `-` previous, `r` restart, Esc swallowed (returns 1)
  — as C1 — but the **force-win cheat is `z` (0x7a)**, where C1 used `L` (0x4c). Details: logic.md.

**Evidence:** `xxd -s 0x6fac -l 0x40` equals C1's `xxd -s 0x6fb0 -l 0x40`; `xxd -s 0x7024 -l 24` → handler table above.
`handle_key @ 0x617c`: `if (0x72 < param_1) { if (param_1 != 0x7a) return 0; force_win = 1; check_win(); return 0; }`.
**Confidence:** confirmed.

### P5. Menus
**Answer:** **Identical to C1.** `load_menu_bar` @ 0x27f4 loads `Resources/menus.nib` ("menus", "MenuBar").
`menus.nib/objects.xib`, `classes.nib` and `info.nib` are byte-identical to C1's (`cmp` reports no difference), so the
menu bar is: application menu (nib title "Chromatron", shown as the bundle name, see below; no nib items; system items incl. Quit ⌘Q); **File**: Reset Level ⌘R
`rvrt`, —, Next Level ⌘= `Next`, Previous Level ⌘- `Prev`, —, Mouse Wheel Changes Level `Whee` (checked), —, Close ⌘W
`clos`; **Edit**: Undo ⌘Z `undo` (disabled), Redo ⇧⌘Z `redo` (disabled), —, Cut ⌘X `cut ` (disabled), Copy Level ⌘C
`copy`, Paste Level ⌘V `past`, Delete `clea` (disabled), Select All ⌘A `sall` (disabled); **Window**: Minimize Window ⌘M
`mini`, Minimize All Windows ⌥⌘M `mina`, —, Bring All to Front `bfrt`, Arrange in Front `frnt`.
`on_menu_command` @ 0x2b6c switches on the same command ids (`Whee` toggle + `store_wheel_pref` @ 0x291c; `Next` → `'='`,
`Prev` → `'-'`, `rvrt` → `'r'`; `copy` → `copy_solution` @ 0x2dc8; `past` → `paste_solution` @ 0x2e48; else
`eventNotHandledErr`).
The nib's application-menu title is still "Chromatron", but the system shows the bundle name, `CFBundleName` = "Chromatron 2", in the menu bar (and in Hide/Quit) (likely; standard Carbon behaviour, not visible in the binary).
**Bundle** (`Contents/Info.plist`, `plutil -p`): `CFBundleName` "Chromatron 2", `CFBundleExecutable` chromatron2,
`CFBundleIconFile` chromatron2.icns, `CFBundleVersion` "1.4", `CFBundleShortVersionString` "Chromatron 2 (v1.4)",
`CFBundleGetInfoString` "Chromatron 2 (v1.4), Copyright © 2003 by Sean Barrett", `CFBundleSignature` Crm2,
`CFBundleIdentifier` com.silverspaceship.chromatron2, `CFBundlePackageType` APPL, `LSRequiresCarbon` true.
`Contents/` holds `Info.plist`, `MacOS/chromatron2`, `PkgInfo` and `Resources/` (`chromatron2.icns`, `menus.nib`).
**Evidence:** `cmp` of the three nib files; `on_menu_command @ 0x2b6c` constants 0x57686565, 0x4e657874, 0x50726576,
0x70617374, 0x636f7079, 0x72767274.
**Confidence:** confirmed (system-provided items **likely**, as in C1).

### P6. Preferences
**Answer:** **Same as C1**: one key, `UseMouseWheelToChangeLevel` (string @ 0x6f6c), CFBoolean, default **true**, read by
`load_prefs` @ 0x2868 (`CFPreferencesGetAppBooleanValue`; invalid/missing → 1; also sets the menu check mark), written by
`store_wheel_pref` @ 0x291c on each toggle, flushed only at quit. The domain is `kCFPreferencesCurrentApplication` =
bundle id **`com.silverspaceship.chromatron2`** (Info.plist `CFBundleIdentifier`), i.e.
`~/Library/Preferences/com.silverspaceship.chromatron2.plist`.
**Evidence:** `load_prefs @ 0x2868` / `store_wheel_pref @ 0x291c` are C1's code (normalised diff: names only).
**Confidence:** confirmed.

### P7. Save file `com.silverspaceship.chromatron2.dat`
**Answer:**
- **Name**: `com.silverspaceship.chromatron2.dat` (string @ 0x6fe0, used by both `fopen "rb"` in `load_save_file` @ 0x4428
  and `fopen "wb"` in `write_save_file` @ 0x462c). **Folder**: as C1 — `chdir_to_prefs_folder` @ 0x2a34 does
  `FSFindFolder(kUserDomain −32763, 'pref', kCreateFolder)` → `FSRefMakePath` → `chdir`, so the file is
  `~/Library/Preferences/com.silverspaceship.chromatron2.dat`.
- **Layout**: same structure and record size as C1: `32` bytes scrambled bitmap, then `k` records of **856** bytes
  (`u8 level_id` + 285 × (`type`, `colour`, `dir`) for cells 0..284 = board rows 0–14 then toolbox slots 0–59). File size
  `32 + 856·k`. The in-memory state is now 0x1f2c bytes (28-byte cells; cell i at `+28·i`), but only bytes +0..+2 are
  stored, so **the file format doesn't depend on the stride**.
- **Saved-state range changed**: `saved_states` @ 0x16110 has **52** entries; `write_save_file` writes ids **0..51**
  (`cmpwi r29,0x33; ble`), in ascending order (C1: 0..127). On load each record does `malloc(0x1f2c)`, zero it, read 285
  triples into +0/+1/+2 of each 28-byte cell; a short triple frees the record and stops. `level_id` is **not validated**:
  a record for id 52 writes its pointer into `cursor_col` 0x161e0, id 53 into `cursor_row` 0x161e4, ids 54–55 into
  0x161e8–0x161ef, ids 56–71 into `site_url_text` 0x161f0–0x1622f, and ids ≥ 72 write past the end of `__common`. A port should reject ids > 51. (Real ids are 1–52; id 52 is unreachable, so it can never be saved.)
- **Bitmap**: same algorithm as C1 (`save_slot_check` @ 0x42e4 `a = 9s; a += a<<8; a += a>>4; return a % 15 + 1`;
  `save_bit_index` @ 0x42a0 `scramble((s & 31) + 32k) + 128(s >> 5)`; `save_bit_scramble` @ 0x41d8 with the same offsets
  13/+0x60, 20/+0x20, 5/+0x40 and the same `n < 128` assert at game.c 0x2cc; 64 slots × 4 bits; LSB-first bit order;
  any value other than 0 or `check(s)` aborts with `game.c:842 failed assertion '0'`), **but the permutation table is
  different**:
  ```
  save_bit_perm @ 0x80ac (C2) = 14 1f 07 08 1e 0e 03 1c 02 16 05 1b 10 0b 19 0c
                                00 09 0d 13 17 04 11 1d 15 0a 06 18 12 1a 01 0f   (a permutation of 0..31)
  C1                         = 00 09 0d 13 15 0a 06 18 1e 0e 03 1c 10 0b 19 0c
                                17 04 11 1d 12 1a 01 0f 14 1f 07 08 02 16 05 1b
  ```
  (C2 is C1's eight 4-byte groups reordered 6, 2, 7, 3, 0, 4, 1, 5.) `(s, k) → bit_index` is still a bijection onto
  0..255. Examples with the C2 table: only slot 0 completed → bytes `00 00 10` then 29 zero bytes (C1: `01` …); all 50
  slots completed → `9dd4579637ca28db4eacb9525cf3512d284b114840059064286a091840228622`.
  **A C1 bitmap decoded with the C2 table (or vice versa) would usually trip the abort**, so the table must be per game.
- **When**: read once in `game_init`; written on first completion of a slot (`check_win`) and at quit
  (`save_on_quit` @ 0x6be8 = `save_level_state(); write_save_file();`), whole file each time, errors ignored — as C1.
  `load_save_file` ends by calling `update_unlocked` (C2's unlock rule differs from C1's; logic.md).

**Evidence:** `load_save_file @ 0x4428`: `_fopen("com.silverspaceship.chromatron2.dat","rb")`, `_fread(abStack_50,0x20,1,…)`,
`pvVar7 = _malloc(0x1f2c); … _memset(…,0,0x1f2c); do { _fread(… + iVar10,3,1,pFVar3); … iVar10 = iVar10 + 0x1c; } while (uVar5 < 0x11d);`.
`write_save_file @ 0x462c`: `_fwrite(abStack_50,1,0x20,…)`; `do { … _fwrite(pvVar8,3,1,…) } while (pvVar9 <= pvVar1 + 0x1f10U);`
`} while (iVar6 < 0x34);`. `save_bit_scramble @ 0x41d8` reads `(&save_bit_perm)[…]` at 0x80ac.
**Confidence:** confirmed (bitmap examples and bijection computed with a model of the helpers, Verification).

### P8. Clipboard (Copy Level / Paste Level)
**Answer:** **Same as C1 except the game digit.** Scrap Manager, flavour `'TEXT'`, flags 0, plain ASCII, no terminator.
- `copy_solution` @ 0x2dc8 / `paste_solution` @ 0x2e48: identical.
- `encode_solution` @ 0x5c2c writes into `clip_text` @ 0x12ffc (54 bytes):
  ```
  "2-" + NN + "-" + pairs     # clip_text[0] = 0x32 ('2'); C1 stored 0x31 ('1')
  ```
  Everything else is C1's: entries `{u16 cell index, u8 dir, u8 type, u8 colour, pad}` for every movable cell of rows
  0–18 (now including types **13 and 15**), `qsort(…, 6, cmp @ 0x5bb8)` by type then colour (unsigned), refuse (NULL) if
  `2n + 6 ≥ 55` (n ≥ 25 — C2's largest level has 24 movable pieces, so every level can be copied), then for each entry
  `v = index + 285·dir`, `b52(v / 52) b52(v % 52)` with `base52_char` @ 0x5bfc ('a'–'z' = 0–25, 'A'–'Z' = 26–51).
  Example (slot 7 = id 41, stored placement: mirror (11,7) dir 7, bender (7,1) dir 6, multi-filter (7,7) dir 4):
  **`2-07-OFHqye`**.
- `decode_solution` @ 0x5dec: requires `'2' '-' d d '-'` (after leading spaces) with `dd == current_slot + 1`; everything
  else (trailing-space strip, `strlen == 2n`, target cell `v % 285` must be empty or movable, apply by sorted
  (type, colour), `dir = v / 285` unmasked, `check_win_and_redraw`) is C1's, including the same unvalidated cases.
  **A C1 solution (`1-NN-…`) is rejected by C2 and vice versa.**

**Evidence:** disassembly at 0x5d18–0x5d58: `li r11,0x2d; stb r11,4(r24); stb r11,1(r24); stb r2,2(r24); li r0,0x32;
stb r0,0(r24); stb r9,3(r24)` (r24 = `clip_text`); paste check at 0x5e38 `cmpwi cr7,r0,0x32`, 0x5e48 and 0x5e80
`cmpwi cr7,r0,0x2d`. Decompiled: `if (((((*param_1 == '2') && (param_1[1] == '-')) && …`. Comparator pointer 0x8108 →
0x5bb8 (`lbz 3(r3)` vs `lbz 3(r4)`, then `lbz 4`, `cmplw`), same code as C1's 0x58a0.
**Confidence:** confirmed.

### P9. Timers and animation
**Answer:** **None**, as in C1. The import set is identical to C1's (no timer, clock, random, sound or idle API), so nothing
moves on its own; only input and system events repaint.
**Evidence:** `r2 -qc ir` lists 79 lines, none matching timer/tick/time/clock/rand/sound/idle; sorted import names equal C1's.
**Confidence:** confirmed.

## Unclassified functions

After both C2 name files are applied, the remaining `FUN_` functions should be the C runtime / dyld glue (C1 had the
same six), with C2 addresses:

| address | role (C1 equivalent) |
|---|---|
| `FUN_000021c4` | crt `_start` (C1 `FUN_000022f4`) — references "__dyld_mod_term_funcs"; Ghidra's `entry` @ 0x2190 jumps here |
| `FUN_00002344` | `__dyld_make_delayed_module_initializer_calls` caller (C1 0x2474) |
| `FUN_0000238c` | dyld kernel-support check (C1 0x24bc) |
| `FUN_000023e0` | dyld function-lookup trampoline (C1 0x2510) |
| `FUN_00002474` | keymgr image-removal callback (C1 0x25a4; pointer at 0x8018) |
| `FUN_00002590` | keymgr init (C1 0x26c0) |

C1's dead `FUN_00003dd4` (clear cell +3) has a live counterpart in C2 at 0x3d5c (complementor flag reset); it belongs to
logic.md. `assert_fail` @ 0x6e08 is named here.

## Verification
```bash
BIN="chromatron_mac/Chromatron 2.app/Contents/MacOS/chromatron2"
B1=chromatron_mac/Chromatron.app/Contents/MacOS/chromatron1

# P1: content rect {0,0,480,640}, title, save name and the new text string
xxd -s 0x6fec -l 8 "$BIN"
# 00006fec: 0000 0000 01e0 0280                      ........
r2 -qc 'ps @ 0x6f88; ps @ 0x6fe0; ps @ 0x7068' "$BIN" </dev/null 2>/dev/null
# Chromatron 2 (v1.4)
# com.silverspaceship.chromatron2.dat
# Thank you for purchasing this game.

# P1: CGImageCreate(width, height, 8, 24, stride*3, DeviceRGB, bitmapInfo 0, provider; decode/interp/intent = 0)
r2 -e scr.color=0 -qc 'pi 14 @ 0x3008' "$BIN" </dev/null 2>/dev/null
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
r2 -e scr.color=0 -qc 'pi 8 @ 0x3174' "$BIN" </dev/null 2>/dev/null
# lwz r9, 0x50(r1)
# addi r7, r1, 0x40
# li r5, 1
# li r6, 1
# li r8, 0
# mr r3, r29
# li r4, 0
# bl sym.imp.DrawThemeTextBox
# P3: paint_window rects: won/You win (r4 = 330 or 350), level name (mode 2: 50,385,300,420; else 350,420,450,460), hint box
r2 -e scr.color=0 -qc 'pi 4 @ 0x6ad4; pi 5 @ 0x6afc; pi 5 @ 0x6b14; pi 5 @ 0x6b60' "$BIN" </dev/null 2>/dev/null
# li r5, 0x181
# li r6, 0x190
# li r7, 0x1a4
# bl 0x30f8
# lwz r3, -0x46e8(r9)
# li r4, 0x15e
# li r5, 0x1a4
# li r6, 0x1c2
# li r7, 0x1cc
# lwz r3, -0x46e8(r9)
# li r4, 0x32
# li r5, 0x181
# li r6, 0x12c
# li r7, 0x1a4
# li r4, 0x1c2
# li r5, 0x7d
# li r6, 0x26c
# li r7, 0x1db
# bl 0x30f8
# P3: "Thank you..." only when current_slot % 6 == 4, rect 0,450,400,480; then the URL box 470,450,640,480
r2 -e scr.color=0 -qc 'pi 6 @ 0x6b74; pi 11 @ 0x6b8c; pi 4 @ 0x6bd0' "$BIN" </dev/null 2>/dev/null
# lis r0, 0x2aaa
# addis r2, r31, 0
# ori r0, r0, 0xaaab
# lwz r2, 0x1638(r2)
# mulhw r0, r2, r0
# srawi r9, r2, 0x1f
# subf r0, r9, r0
# mulli r0, r0, 6
# subf r2, r0, r2
# cmpwi cr7, r2, 4
# bne cr7, 0x6bbc
# addis r3, r31, 0
# li r4, 0
# addi r3, r3, 0x664
# li r5, 0x1c2
# li r6, 0x190
# li r7, 0x1e0
# li r4, 0x1d6
# li r5, 0x1c2
# li r6, 0x280
# li r7, 0x1e0
# "freeware" and "more levels @" are gone
strings -a "$BIN" | grep -c -E '^(freeware|more levels @)$'
# 0

# P4: event type specs and handler table
xxd -s 0x6fac -l 0x40 "$BIN"
# 00006fac: 7769 6e64 0000 0048 7769 6e64 0000 0002  wind...Hwind....
# 00006fbc: 6d6f 7573 0000 0001 6d6f 7573 0000 0002  mous....mous....
# 00006fcc: 6d6f 7573 0000 0006 6d6f 7573 0000 000a  mous....mous....
# 00006fdc: 6b65 7962 0000 0001 636d 6473 0000 0001  keyb....cmds....
xxd -s 0x7024 -l 24 "$BIN"
# 00007024: 0000 2b6c 0000 2d28 0000 2f14 0000 31bc  ..+l..-(../...1.
# 00007034: 0000 2cbc 0000 2c98                      ..,...,.

# P5: bundle metadata (menu-bar app name = CFBundleName)
plutil -p "chromatron_mac/Chromatron 2.app/Contents/Info.plist" | grep -E 'CFBundle(Name|Executable|IconFile|Version|ShortVersionString|GetInfoString|Signature)"'
#   "CFBundleExecutable" => "chromatron2"
#   "CFBundleGetInfoString" => "Chromatron 2 (v1.4), Copyright © 2003 by Sean Barrett"
#   "CFBundleIconFile" => "chromatron2.icns"
#   "CFBundleName" => "Chromatron 2"
#   "CFBundleShortVersionString" => "Chromatron 2 (v1.4)"
#   "CFBundleSignature" => "Crm2"
#   "CFBundleVersion" => "1.4"
ls "chromatron_mac/Chromatron 2.app/Contents/Resources" "chromatron_mac/Chromatron 2.app/Contents" | paste -sd' ' -
# chromatron_mac/Chromatron 2.app/Contents: Info.plist MacOS PkgInfo Resources  chromatron_mac/Chromatron 2.app/Contents/Resources: chromatron2.icns menus.nib
# P5: menus.nib identical to C1
for f in objects.xib classes.nib info.nib; do cmp "chromatron_mac/Chromatron 2.app/Contents/Resources/menus.nib/$f" "chromatron_mac/Chromatron.app/Contents/Resources/menus.nib/$f" && echo same; done
# same
# same
# same

# P7: folder = FSFindFolder(kUserDomain -32763, 'pref', kCreateFolder)
r2 -e scr.color=0 -qc 'pi 9 @ 0x2a34' "$BIN" </dev/null 2>/dev/null
# mflr r0
# lis r4, 0x7072
# stw r0, 8(r1)
# ori r4, r4, 0x6566
# stwu r1, -0x4a0(r1)
# li r3, -0x7ffb
# li r5, 1
# addi r6, r1, 0x40
# bl sym.imp.FSFindFolder
# P7: read sites: fopen, 32-byte bitmap, malloc(0x1f2c) per record, 3-byte triples, 1-byte id
r2 -e scr.color=0 -qc 'pi 1 @ 0x4458; pi 4 @ 0x446c; pi 1 @ 0x455c; pi 1 @ 0x4564; pi 1 @ 0x4594; pi 1 @ 0x45b0; pi 4 @ 0x45d0' "$BIN" </dev/null 2>/dev/null
# bl sym.imp.fopen
# li r4, 0x20
# li r5, 1
# mr r6, r27
# bl sym.imp.fread
# li r3, 0x1f2c
# bl sym.imp.malloc
# li r4, 3
# bl sym.imp.fread
# li r4, 1
# li r5, 1
# mr r6, r27
# bl sym.imp.fread
# P7: write sites: 32-byte bitmap, then per id: 1 id byte + triples at 28-byte steps up to +0x1f10, ids 0..0x33
r2 -e scr.color=0 -qc 'pi 1 @ 0x4654; pi 4 @ 0x46f0; pi 7 @ 0x4738; pi 2 @ 0x4760' "$BIN" </dev/null 2>/dev/null
# bl sym.imp.fopen
# li r4, 1
# li r5, 0x20
# mr r6, r27
# bl sym.imp.fwrite
# addi r28, r30, 0x1f10
# mr r3, r30
# li r4, 3
# addi r30, r30, 0x1c
# li r5, 1
# mr r6, r27
# bl sym.imp.fwrite
# cmpwi cr7, r29, 0x33
# ble cr7, 0x4704
# P7: save permutation table (DIFFERENT from C1's 00 09 0d 13 ...)
xxd -s 0x70ac -l 32 "$BIN"
# 000070ac: 141f 0708 1e0e 031c 0216 051b 100b 190c  ................
# 000070bc: 0009 0d13 1704 111d 150a 0618 121a 010f  ................
xxd -s 0x70ac -l 32 "$B1"
# 000070ac: 0009 0d13 150a 0618 1e0e 031c 100b 190c  ................
# 000070bc: 1704 111d 121a 010f 141f 0708 0216 051b  ................
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

# P8: clipboard header construction: '-' at [1] and [4], tens digit at [2], '2' (0x32) at [0], units at [3]
r2 -e scr.color=0 -qc 'pi 1 @ 0x5d18; pi 1 @ 0x5d20; pi 1 @ 0x5d2c; pi 1 @ 0x5d44; pi 4 @ 0x5d4c' "$BIN" </dev/null 2>/dev/null
# li r11, 0x2d
# stb r11, 4(r24)
# stb r11, 1(r24)
# stb r2, 2(r24)
# li r0, 0x32
# addi r9, r9, 0x30
# stb r0, 0(r24)
# stb r9, 3(r24)
# P8: paste requires '2', '-', ..., '-'
r2 -e scr.color=0 -qc 'pi 1 @ 0x5e38; pi 1 @ 0x5e48; pi 1 @ 0x5e80' "$BIN" </dev/null 2>/dev/null
# cmpwi cr7, r0, 0x32
# cmpwi cr7, r0, 0x2d
# cmpwi cr7, r0, 0x2d
# P8: comparators (clipboard 0x5bb8, toolbox 0x5800)
r2 -e scr.color=0 -qc 'pxw 8 @ 0x8108' "$BIN" </dev/null 2>/dev/null
# 0x00008108  0x00005bb8 0x00005800                        ..[...X.
# P8: worked example, slot 7 (id 41 @ 0xa12c) stored placement -> 2-07-OFHqye
python3 -I -c '
import sys
B=open(sys.argv[1],"rb").read(); a=0xa12c-0x1000; m=[]
b52=lambda d: chr(97+d) if d<26 else chr(65+d-26)
while B[a]:
    t,d,c,x,y=B[a:a+5]
    if t in (3,4,5,6,7,8,9,13,15): m.append((t,c,y*15+x+285*d))
    a+=5
print("2-07-"+"".join(b52(v//52)+b52(v%52) for t,c,v in sorted(m)))' "$BIN"
# 2-07-OFHqye

# P9: no timer/clock/random/sound imports; import set equals C1's
r2 -e scr.color=0 -qc 'ir' "$BIN" </dev/null 2>/dev/null | grep -c -i -E 'timer|tick|time|clock|rand|sound|idle'
# 0
r2 -e scr.color=0 -qc 'ir' "$BIN" </dev/null 2>/dev/null | wc -l | tr -d ' '
# 79
diff <(r2 -e scr.color=0 -qc ir "$B1" </dev/null 2>/dev/null | awk '{print $NF}' | sort) <(r2 -e scr.color=0 -qc ir "$BIN" </dev/null 2>/dev/null | awk '{print $NF}' | sort) && echo same-imports
# same-imports
```
