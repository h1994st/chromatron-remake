# Chromatron 1 v1.15 — Platform layer

decompiled.c line numbers valid for Ghidra 12.1.4 with re/notes/chromatron1/*.names.tsv as of this commit.

Binary: `chromatron_mac/Chromatron.app/Contents/MacOS/chromatron1` (Mach-O, big-endian PowerPC, Carbon).
All addresses are vmaddrs. File offset = vmaddr − 0x1000. Function and data names are the ones in
`data.names.tsv`, `logic.names.tsv` and `platform.names.tsv` (applied by `just decompile 1`). Line numbers refer to
`re/out/chromatron1/decompiled.c` as produced by `just decompile 1` with all three name files. `data.md` and `logic.md`
cite some of the functions below by their old `FUN_<addr>` names; the address gives the new name.

This builds on `data.md` (cell layout, sprites, palette, `beam_colors`, board/toolbox/strip positions, D4/D7 text strings)
and `logic.md` (hit test, drag/rotate state machine, keys, win display). It doesn't repeat them.

Imported non-lazy symbols used here (`__nl_symbol_ptr`, read from the indirect symbol table):
0x12d00 `kCFAllocatorDefault`, 0x12d04 `kCFBooleanFalse`, 0x12d08 `kCFBooleanTrue`,
0x12d0c `kCFPreferencesCurrentApplication`, 0x12d84 `__sF` (stdio; `assert_fail` writes to `stderr` = `__sF + 0xb0`).
Local slots used here: 0x12d14 → 0x16100 `mode_text`, 0x12d24 → 0x13c70 `level_started`, 0x12d38 → 0x13a68 and
0x12d3c → 0x13a64 (the drag sprite's y and x; left unnamed so that the decompiled line numbers don't shift).

**Startup** (`app_main` @ 0x2bc0, lines 354–369, called from the crt start `FUN_000022f4`):
1. `chdir_to_prefs_folder()` (P7).
2. `game_init()` @ 0x5f38: `load_save_file()` (P7), `decode_images()`, `fb_width = 640`, `fb_height = 480`,
   `framebuffer = malloc(0xe1000)`, `current_level_id = level_order[0]`, `start_level(1)`. The game always starts on slot 0.
3. `load_menu_bar()` (P5), `load_prefs()` (P6), `create_main_window()` (P1), `install_event_handlers()` (P4).
4. `RepositionWindow(main_window, NULL, kWindowCenterOnMainScreen)`, `ShowWindow`, `RunApplicationEventLoop()`.
5. After the loop returns (Quit or window close): `save_on_quit()` = `save_level_state(); write_save_file();`, then
   `CFPreferencesAppSynchronize(kCFPreferencesCurrentApplication)`, return 0 → `exit`.

## Findings

### P1. Window and framebuffer
**Answer:**
- **Window**: `CreateNewWindow(kDocumentWindowClass = 6, attributes 0x02000009, &rect, &main_window)` with the content
  rect at 0x7ff0 = {top 0, left 0, bottom 480, right 640}, so the **content area is 640 × 480**.
  Attributes = `kWindowCloseBoxAttribute (1) | kWindowCollapseBoxAttribute (8) | kWindowStandardHandlerAttribute (1<<25)`.
  So it has a close box and a minimise box, and it is **not resizable** and has **no zoom box**.
  Title: **"Chromatron 1 (v1.15)"** (string @ 0x6c40, `SetWindowTitleWithCFString`).
  Position: `RepositionWindow(w, NULL, 1 = kWindowCenterOnMainScreen)` at every launch. The position isn't saved.
  `main_window` @ 0x12da4.
- **Framebuffer**: `framebuffer` @ 0x12dcc = `malloc(0xe1000)` (921 600 bytes), `fb_width` @ 0x12dd0 = 640,
  `fb_height` @ 0x12dd4 = 480. Packed **RGB, 3 bytes per pixel, 8 bits per component, no alpha, no padding**,
  row-major with **row 0 at the top**. Byte order in memory: R, G, B.
- **Present** (`draw_rgb_image` @ 0x30cc(x = 0, y = 0, buf, w = 640, h = 480, stride = 640), called from `present_frame`):
  ```
  provider = CGDataProviderCreateWithData(NULL, buf, h * stride * 3 = 921600, NULL)
  image    = CGImageCreate(width 640, height 480, bitsPerComponent 8, bitsPerPixel 24, bytesPerRow 1920,
                           CGColorSpaceCreateDeviceRGB(), bitmapInfo 0 (kCGImageAlphaNone, default byte order),
                           provider, decode NULL, shouldInterpolate false, intent 0 (default))
  QDBeginCGContext(GetWindowPort(main_window), &ctx)
  CGContextDrawImage(ctx, CGRect(0, 0, 640, 480), image)    # whole content area, 1:1
  QDEndCGContext; CGImageRelease; CGDataProviderRelease
  ```
  The colour space and image are created on every present and released right away.
- **When a redraw happens**: there is no timer and no periodic redraw. The game only invalidates.
  `request_redraw(x, y, w, h)` @ 0x34b8 sets `redraw_all_flag` @ 0x8040 = 1 and calls
  `inval_window_rect` @ 0x2c40 → `InvalWindowRect(main_window, {top y, left x, bottom y+h, right x+w})`.
  The Window Manager then sends `kEventWindowDrawContent`, and `on_draw_content` @ 0x2dec calls `paint_window()` and
  `CGContextFlush`. Callers of `request_redraw`:
  - the whole window (0, 0, 640, 480): `start_level`, `move_pieces_to_toolbox`, `check_win_and_redraw` (after every drop,
    rotate and paste), and `update_unlocked` (load and first-time completion);
  - while dragging, on every drag event: the old 60 × 60 box (`drag_inval_x − 30`, `drag_inval_y − 30`) and the new
    25 × 25 box (pointer − 12).
  The system also sends `kEventWindowDrawContent` on its own (show, expose, un-minimise).
  `redraw_all_flag` starts at 1 and is **never cleared**, so `paint_window` always re-renders the whole framebuffer
  (P2) and presents all of it. A partial invalidation only limits what reaches the screen; that clipping comes from
  the Carbon update region, not from game code.

**Evidence:** `create_main_window @ 0x2aec` line 325 `_CreateNewWindow(6,0x2000009,&local_20,&main_window);` (rect copied from
`DAT_00007ff0`, bytes `0000 0000 01e0 0280`), line 326 the title string. `app_main` line 363 `_RepositionWindow(main_window,0,1);`.
`game_init` lines 2676–2678. `draw_rgb_image` lines 584–593:
```c
  uVar2 = _CGDataProviderCreateWithData(0,param_3,(longlong)(int)(uint)param_5 * (longlong)(int)param_6 * 3,0);
  uVar3 = _CGColorSpaceCreateDeviceRGB();
  uVar3 = _CGImageCreate(param_4,param_5,8,0x18,param_6 * 3,uVar3,0,uVar2);
  ...
  _CGContextDrawImage(local_38[0],(float)dVar4,(float)dVar5,(float)dVar6,(float)dVar7,uVar3);
```
The disassembly at 0x3138–0x316c gives the arguments Ghidra drops: `li r9, 0` (bitmapInfo) and three `stw r29` of 0 to the
stack slots 0x38/0x3c/0x40 (decode, shouldInterpolate, intent). `request_redraw` lines 699–705 (`DAT_00008040 = 1`, now
`redraw_all_flag`); a Ghidra reference scan finds only that write and the read in `paint_window` (line 2977).
**Confidence:** confirmed. The clipping of a partial invalidation to the update region is Carbon behaviour, **likely**.
A port that always repaints the whole window looks the same.

### P2. Screen layout and render order
**Answer:** All coordinates are window-content pixels, origin top-left, y down. Rects are `[x0, x1) × [y0, y1)`.

| region | drawn rect | hit-test rect (logic.md L7) | layout |
|---|---|---|---|
| window / framebuffer | 0,0 – 640,480 | – | cleared to RGB a4a4a4 |
| board | 48,18 – 408,378 | same | 15 × 15 cells of 24 px, no gaps. Cell (x, y) top-left = (48 + 24x, 18 + 24y), centre = (60 + 24x, 30 + 24y) |
| toolbox | 448,8 – 602,110 | 448,8 – 604,112 | 6 × 4 slots of 24 px at a 26 px pitch (2 px gaps show the background). Slot s top-left = (448 + 26(s mod 6), 8 + 26(s div 6)) |
| level strip | glyphs inside 10,410 – 510,442 | 10,410 – 510,450 | 2 rows × 25 numbers, 20 px pitch. Number n = 1..50 starts at (10 + 20((n−1) mod 25), 410 + 20((n−1) div 25)); 12 × 12 digit glyphs (data.md D7) |
| hint / tutorial text | 450,125 – 620,475 | – | P3 |
| win status text | 330,385 – 400,420 | – | P3 |
| footer text | y 450 – 480 | – | P3 |

The window has no other chrome, buttons or separators. Everything not listed is background a4a4a4.

**Render order** (`paint_window` @ 0x66e0 → `render_scene` @ 0x65d0 → `present_frame` @ 0x4e68 → text):
1. (first call only) decode `site_url_text` (data.md D8).
2. `render_scene` (runs every time, see P1):
   1. `memset(framebuffer, 0xa4, 640·480·3)`.
   2. **Board cells**, row-major (y outer, x inner): `draw_slot_cell(x, y, min 0)` blits the cell sprite opaque
      (`draw_cell_image`, data.md D5). Empty cells use sprite 0, the floor tile with its grid line on the top row and
      left column. While a drag is in progress (`drag_state == 2`), the source cell (`cursor_col`, `cursor_row`) is drawn
      from `empty_cell` @ 0x12db4 (20 zero bytes, so sprite 0) instead.
   3. **Beams**, row-major again. For each cell `draw_cell_beams(x, y)` @ 0x61ec draws, for each side s = 0..7 with
      `c = out[s] | in[(s+4)&7]` non-zero, a 1-px line in `beam_colors[c]` from the centre (cx, cy) to
      (cx + 13·dx[s], cy + 13·dy[s]), both ends included. Then it draws the one-pixel centre dot (logic.md L4 has the rule).
      If it drew anything, the cell's sprite is blitted again with `min_index 0x10`, so only sprite pixels ≥ 16 cover the beams
      (floor pixels 6 and 8 stay transparent). This happens cell by cell: the overlay of cell N comes before the beams of
      cell N+1. A segment reaches 1 px into the W/N neighbour (cx − 13, cy − 13) and 2 px into the E/S neighbour (cx + 12,
      cx + 13), so a later cell's beam can draw over an earlier cell's overlay, and a later overlay can cover an earlier
      cell's overhang.
   4. **Toolbox** slots 0..23: `draw_slot_cell(s, 15, min 0)`, opaque. Toolbox cells never show beams.
   5. **Level strip** (only when `game_mode == 2`, i.e. always): numbers 1..50 via `draw_digit_string`, coloured by
      `strip_pal_offset` @ 0x64a0 (data.md D6/D7).
3. `present_frame` @ 0x4e68:
   1. If a piece is being dragged (`drag_image >= 0`): copy the clipped 24 × 24 box under the sprite into
      `drag_save_under` @ 0x12dd8 (stride 32 px) and blit `decoded_images[drag_image]` at (drag_x − 12, drag_y − 12) with
      `min_index 0x0e`. The sprite is centred on the pointer, not offset by the grab point.
   2. Present the framebuffer (P1).
   3. Restore the save-under, so the framebuffer itself never keeps the drag sprite.
4. **Text boxes**, drawn into the window over the presented image, in this order: "You win!" / "(won)", `mode_text`
   (unreachable), the hint box, "freeware", "more levels @", the site URL (P3). So text is on top of everything,
   including the drag sprite.

**Evidence:** `render_scene` lines 2898–2943 (`_memset(framebuffer,0xa4,fb_width * fb_height * 3)`, the three cell loops,
`draw_slot_cell(lVar6,lVar4,0x10)` only if `draw_cell_beams` returns non-zero, `draw_slot_cell(lVar4,0xf,0)` for
`lVar4 < 0x18`, and the strip loop `while ((int)lVar4 < 0x33)` with x = `… * 0x14 + 10`, y = `… * 0x14 + 0x19a`).
`draw_slot_cell` lines 2728–2745 (board: `param_2 * 0x18 + 0x1e`, `param_1 * 0x18 + 0x3c`; toolbox: `… * 0x1a + 0x14`,
`… * 0x1a + 0x1cc`; `&empty_cell` when `drag_state == 2` and the cell is the cursor). Segment ends in `draw_cell_beams`
lines 2783–2811 (`iVar2 + 0x49` = cx + 13, `iVar2 + 0x2f` = cx − 13, `iVar6 + 0x2b` = cy + 13, `iVar6 + 0x11` = cy − 13). The
disassembly at 0x62cc–0x6350 uses `addi r6/r7, r30/r29, ±0xd`. `draw_line` @ 0x4b28 sends x0 == x1 to `draw_vline`,
y0 == y1 to `draw_hline`, ±1 slopes to `draw_diag_se`/`draw_diag_sw`, and anything else to an assertion (game.c 0x4a4).
Each one loops `max − min + 1` pixels. `present_frame` lines 1966–1975. `paint_window` lines 2964–3005.
**Confidence:** confirmed.

### P3. Text boxes (`DrawThemeTextBox`)
**Answer:** All text goes through `draw_text_box(str, left, top, right, bottom)` @ 0x3228:
```
cf = CFStringCreateWithCString(kCFAllocatorDefault, str, kCFStringEncodingASCII = 0x0600)
QDBeginCGContext(GetWindowPort(main_window), &ctx)
DrawThemeTextBox(cf, inFontID 0 = kThemeSystemFont, inState 1 = kThemeStateActive, inWrapToWidth true,
                 &Rect{top, left, bottom, right}, inJust 0 = teFlushDefault, ctx)
QDEndCGContext; CFRelease(cf)
```
- **Font**: `kThemeSystemFont`, which on Mac OS X 10.3 (Aqua) is **Lucida Grande, 13 pt, regular**. All boxes use the same
  font. (`kThemeSmallSystemFont` would be Lucida Grande 11 pt; it isn't used.)
- **Justification**: `teFlushDefault` = left-aligned for this (Roman, left-to-right) text. Nothing is centred or right-aligned.
- **Wrapping**: word-wrap to the rect width (`inWrapToWidth = true`). No truncation: `TruncateThemeText` isn't imported and
  no ellipsis is ever added. The strings contain no line breaks. The longest hint is 245 characters and all of them fit
  their boxes, so vertical overflow never comes up.
- **State**: always `kThemeStateActive`. The text doesn't dim when the window is inactive.
- **Colour**: none is set. The CG context from `QDBeginCGContext` starts with a black fill, so the text is black (data.md D6).
  There is no background fill: the text is drawn straight over the presented framebuffer (background a4a4a4).
- **Encoding**: ASCII. All strings are 7-bit.

Every call, in draw order (`paint_window` lines 2981–3005):

| string | string @ | left, top, right, bottom | width × height | when drawn |
|---|---|---|---|---|
| "You win!" | 0x6ce0 | 330, 385, 400, 420 | 70 × 35 | `level_won != 0` |
| "(won)" | 0x6cec | 350, 385, 400, 420 | 50 × 35 | `level_won == 0` and `completed[current_slot] != 0` |
| `mode_text` (0x16100, bss, never written, so "") | – | 350, 420, 450, 460 | 100 × 40 | `game_mode != 2`, which never happens after startup (logic.md L7). Unreachable |
| `hint_text` (data.md D4; bss 0x14080), or "Click on a level or press spacebar for next." (0x6cf4) when `game_mode == 2 && level_won` | – | 450, 125, 620, 475 | 170 × 350 | always. Empty hint → nothing visible |
| "freeware" | 0x6d24 | 0, 450, 100, 480 | 100 × 30 | always |
| "more levels @" | 0x6d30 | 350, 450, 465, 480 | 115 × 30 | `current_slot > 39` and (`current_slot` even or `== 49`), i.e. displayed levels 41, 43, 45, 47, 49, 50 |
| `site_url_text` = "silverspaceship.com" (data.md D8) | decoded at runtime (xor data @ 0x80f4), buffer 0x16420 | 470, 450, 640, 480 | 170 × 30 | always |

**Evidence:** `draw_text_box` lines 615–625:
```c
  uVar2 = _CFStringCreateWithCString(*(undefined4 *)PTR_00012d00,param_1,0x600);
  local_40 = param_3;  local_3e = param_2;  local_3c = param_5;  local_3a = param_4;
  _QDBeginCGContext(uVar1,local_30);
  _DrawThemeTextBox(uVar2,0,1,1,&local_40,0,local_30[0]);
```
(condensed). The disassembly at 0x3290–0x329c stores `left → 0x42`, `top → 0x40`, `right → 0x46`, `bottom → 0x44` (a QD
`Rect` is top, left, bottom, right), and at 0x32a8–0x32c0 passes `r4 = 0, r5 = 1, r6 = 1, r7 = &rect, r8 = 0, r9 = ctx`.
The `paint_window` calls (lines 2990–3005), e.g. `draw_text_box(pcVar4,0x1c2,0x7d,0x26c,0x1db);` and
`if ((0x27 < (int)current_slot) && (((current_slot & 1) == 0 || (current_slot == 0x31))))`.
**Confidence:** confirmed for the call arguments, rects, conditions and order. **likely** for "Lucida Grande 13 pt" and black:
these are what Mac OS X 10.3 uses for `kThemeSystemFont` and a fresh CG context, not values stored in the binary.
How the box looks pixel for pixel (anti-aliasing, line height) can only be settled on a 10.3 machine. Core Text with
Lucida Grande 13 pt, left-aligned, word-wrapped to the rect width, top line at the rect's top, is the intended match.

### P4. Input
**Answer:** `install_event_handlers` @ 0x2740 installs six handlers, all on `GetWindowEventTarget(main_window)`
(type specs in `__TEXT,__const` 0x7fb0–0x7fef; handler pointers at 0x8024–0x803b):

| class | kind | handler | effect |
|---|---|---|---|
| `kEventClassWindow` 'wind' | `kEventWindowClose` (72) | `on_window_close` @ 0x2dc8 | `QuitApplicationEventLoop()`, returns noErr (close box or Cmd-W quits the game) |
| 'wind' | `kEventWindowDrawContent` (2) | `on_draw_content` @ 0x2dec | `paint_window()` (P2) and `CGContextFlush` |
| `kEventClassMouse` 'mous' | `kEventMouseDown` (1), `kEventMouseUp` (2), `kEventMouseDragged` (6) | `on_mouse_event` @ 0x32ec | → `handle_mouse` (logic.md L7) |
| 'mous' | `kEventMouseWheelMoved` (10) | `on_wheel_event` @ 0x3044 | level change |
| `kEventClassKeyboard` 'keyb' | `kEventRawKeyDown` (1) | `on_key_event` @ 0x2e58 | → `handle_key` (logic.md L7) |
| `kEventClassCommand` 'cmds' | `kEventCommandProcess` (1) | `on_menu_command` @ 0x2c9c | P5 |

Not registered: `kEventMouseMoved` (5), so move events arrive **only while a button is held**.
`kEventRawKeyRepeat` isn't registered either, so holding a key acts once. There are no activate/deactivate, resize or
mouse-entered handlers.

**Mouse** (`on_mouse_event`):
- Reads `kEventParamMouseButton` ('mbtn', `typeMouseButton`, 2 bytes), `kEventParamKeyModifiers` ('kmod', `typeUInt32`)
  and `kEventParamMouseLocation` ('mloc', `typeQDPoint`, global coordinates).
- If button == 1: on a mouse-down, latch `mouse_alt_button = (modifiers & 0x1a00) != 0`
  (0x1a00 = `shiftKey 0x0200 | optionKey 0x0800 | controlKey 0x1000`; Cmd isn't included). If the latch is set, the event
  counts as button 2. Button 2 (right) counts as button 2. Any other button number (3 = middle, …) counts as primary.
- Code: down → 1 (primary) / 3 (button 2, ignored by `handle_mouse`); up → 4 / 6; dragged → 0.
- The event is handled only if the point lies in the window's content rect (`GetWindowBounds(kWindowContentRgn = 33)`,
  `PtInRect`) **and** the window is active (`IsWindowHilited`). Then `handle_mouse(code, h − content.left, v − content.top)`
  runs and noErr is returned. Otherwise it returns `eventNotHandledErr` (−9874). So the click that activates the window does
  nothing, drag events outside the content area are dropped, and a mouse-up outside the window never reaches the game.
  A drag released outside the window therefore leaves `drag_state = 2`, with the sprite still shown, until the next primary
  mouse-up inside. A primary mouse-down on a non-movable board/toolbox cell in between moves `cursor` there without
  clearing `drag_state`, and the next drop onto an empty cell then moves *that* cell, even a wall or laser (the drop
  code doesn't check `is_movable`, logic.md L7). A port should cancel the drag instead.
- `handle_mouse` itself (hit test, drag threshold of 1 px, drop, rotate, strip clicks, quirks) is in logic.md L7.
  There is no double-click or click-count logic.

**Wheel** (`on_wheel_event`): only when `use_wheel_pref != 0` (P6); otherwise `eventNotHandledErr`. It reads
`kEventParamMouseWheelDelta` ('mwdl', `typeLongInteger`). delta > 0 → `handle_key('-')` (previous level); delta ≤ 0 →
`handle_key('=')` (next level, only if unlocked). It's one level per event whatever the magnitude, and
`kEventParamMouseWheelAxis` isn't read, so a horizontal wheel or trackpad swipe also changes level.

**Keyboard** (`on_key_event`): reads `kEventParamKeyMacCharCodes` ('kchr', `typeChar`, 1 byte, a MacRoman character from
the current keyboard layout) and `kEventParamKeyModifiers`. If `cmdKey` (0x0100) is held the key isn't handled, so it goes
to the menus. Otherwise `handle_key(char)` runs. The keys (Space / `=` / `+` next, `-` previous, `r` restart, `L` force-win
cheat, Esc swallowed) are in logic.md L7. Because they are characters, `L` means Shift+L and `+` means Shift+= on a US
layout. Arrow keys and all other characters do nothing. `handle_key` returns 1 only for Esc, so only Esc reports noErr.
Every other key acts but still returns `eventNotHandledErr`, which has no visible effect.

**Evidence:** type specs, `xxd -s 0x6fb0 -l 0x40` (Verification): `'wind' 0x48`, `'wind' 2`, `'mous' 1, 2, 6`, `'mous' 0xa`,
`'keyb' 1`, `'cmds' 1`. `install_event_handlers` lines 222–239 (`_InstallEventHandler(…,3,&local_80,…)` for the three
mouse kinds). Handler table 0x8024 = 0x2c9c, 0x2e58, 0x3044, 0x32ec, 0x2dec, 0x2dc8. `on_mouse_event` lines 645–692:
```c
  _GetEventParameter(param_2,0x6d62746e,0x6d62746e,0,2,0,local_20);
  if (local_20[0] == 1) { ...
    if (uVar2 == 1) { mouse_alt_button = -(local_1c & 0x1a00) >> 0x1f; }
    if (mouse_alt_button != 0) { local_20[0] = 2; } }
  ...  _GetWindowBounds(main_window,0x21,&local_30);  iVar3 = _PtInRect(local_18,&local_30);
  ...  iVar3 = _IsWindowHilited(main_window); ...
       handle_mouse(uVar4,(longlong)local_18._2_2_ - (longlong)local_2e, (longlong)local_18._0_2_ - (longlong)local_30,0,0);
```
`on_wheel_event` lines 554–560. `on_key_event` lines 476–481 (`(local_1c[0] & 0x100) == 0`). `handle_key` lines 2636–2639.
**Confidence:** confirmed.

### P5. Menus
**Answer:** `load_menu_bar` @ 0x2924 does `CreateNibReference("menus")` and `SetMenuBarFromNib(nib, "MenuBar")`, using
`Resources/menus.nib` (an IBCarbonFramework nib). The menu bar (`objects.xib`, in order):

| menu | item | key equivalent | command | enabled | handled by |
|---|---|---|---|---|---|
| **Chromatron** (`_NSAppleMenu`) | (no items in the nib; the system adds the standard application menu items, including Quit ⌘Q `quit`) | | | | system |
| **File** (menuID 128) | Reset Level | ⌘R | `rvrt` | yes | game: `handle_key('r')` → `start_level(0)` |
| | — | | | | |
| | Next Level | ⌘= | `Next` | yes | game: `handle_key('=')` → next level if unlocked |
| | Previous Level | ⌘- | `Prev` | yes | game: `handle_key('-')` → previous level |
| | — | | | | |
| | Mouse Wheel Changes Level | none | `Whee` | yes, **checked** in the nib | game: toggles the preference (P6) |
| | — | | | | |
| | Close | ⌘W | `clos` | yes | system (standard window handler) → `kEventWindowClose` → quit |
| **Edit** | Undo | ⌘Z | `undo` | **disabled** | – |
| | Redo | ⇧⌘Z (key "Z") | `redo` | **disabled** | – |
| | — | | | | |
| | Cut | ⌘X | `cut ` | **disabled** | – |
| | Copy Level | ⌘C | `copy` | yes | game: `copy_solution` (P8) |
| | Paste Level | ⌘V | `past` | yes | game: `paste_solution` (P8) |
| | Delete | none | `clea` | **disabled** | – |
| | Select All | ⌘A | `sall` | **disabled** | – |
| **Window** (`_NSWindowsMenu`) | Minimize Window (dynamic) | ⌘M | `mini` | yes | system |
| | Minimize All Windows (dynamic alternate) | ⌥⌘M (modifier 1572864 = 0x180000) | `mina` | yes | system |
| | — | | | | |
| | Bring All to Front (dynamic) | none | `bfrt` | yes | system |
| | Arrange in Front (dynamic alternate) | ⌥ (modifier 0x180000, no key) | `frnt` | yes | system |

`on_menu_command` reads `kEventParamDirectObject` as `typeHICommand` (14 bytes) and switches on `commandID`:
- `'Whee'`: `use_wheel_pref = !use_wheel_pref`, then `store_wheel_pref()` (writes the preference and sets the check mark). noErr.
- `'Next'` → `handle_key('=')`, `'Prev'` → `handle_key('-')`, `'rvrt'` → `handle_key('r')`. noErr.
- `'copy'` → `copy_solution()`, `'past'` → `paste_solution()`. noErr.
- anything else → `eventNotHandledErr`, so the standard handlers deal with Close, Minimize, Quit and the rest.

The menu commands don't depend on game state: Copy, Paste, Next and Previous are never disabled or greyed out. Next does
nothing on a locked level and Paste does nothing on invalid text. Quit (⌘Q, from the system application menu) ends
`RunApplicationEventLoop`, which saves (startup step 5). There's no About box and no Preferences item.
The check mark: `load_prefs` reads the nib's mark character with `GetMenuCommandMark(NULL, 'Whee', &wheel_menu_mark)` and
then sets `SetMenuCommandMark(NULL, 'Whee', use_wheel_pref ? wheel_menu_mark : 0)`. `store_wheel_pref` does the same.

**Evidence:** `objects.xib` (`grep -o 'title="[^"]*"'` doesn't match this file, because titles are elements:
`<string name="title">…</string>`, see Verification). `on_menu_command` lines 400–434 (`0x57686565` 'Whee', `0x4e657874`
'Next', `0x50726576` 'Prev', `0x70617374` 'past', `0x636f7079` 'copy', `0x72767274` 'rvrt'; default `0xffffffffffffd96e` = −9874).
`load_menu_bar` lines 253–258. `load_prefs` lines 278–283.
**Confidence:** confirmed for the nib contents and the game's handlers. **likely** for the system-provided items (the application
menu's contents and the Close → `kEventWindowClose` path come from the Carbon standard handlers, not from the binary).

### P6. Preferences
**Answer:** There is exactly **one** CFPreferences key:

| key | type | default | domain |
|---|---|---|---|
| `UseMouseWheelToChangeLevel` | CFBoolean | **true** | `kCFPreferencesCurrentApplication` = bundle id `com.silverspaceship.chromatron1` (Info.plist), i.e. `~/Library/Preferences/com.silverspaceship.chromatron1.plist` |

- **Read** once at startup (`load_prefs` @ 0x2998): `use_wheel_pref = CFPreferencesGetAppBooleanValue(key, app, &valid)`.
  If the key is missing or not a valid boolean (`valid == false`), it is set to 1.
  `use_wheel_pref` @ 0x8020 is also statically 1.
- **Written** on every toggle of the menu item (`store_wheel_pref` @ 0x2a4c):
  `CFPreferencesSetAppValue(key, use_wheel_pref ? kCFBooleanTrue : kCFBooleanFalse, app)`.
- **Flushed** only at quit: `CFPreferencesAppSynchronize(app)` after the event loop. A crash loses the change.
- Nothing else is a preference. Window position, current level and sound don't exist as settings; progress is in the
  save file (P7).

**Evidence:** `load_prefs` lines 273–277:
```c
  uVar2 = ___CFStringMakeConstantString("UseMouseWheelToChangeLevel");
  use_wheel_pref = _CFPreferencesGetAppBooleanValue(uVar2,*(undefined4 *)PTR_00012d0c,local_20);
  if (local_20[0] == '\0') { use_wheel_pref = 1; }
```
`store_wheel_pref` lines 298–304 (`PTR_00012d04` = `kCFBooleanFalse`, `PTR_00012d08` = `kCFBooleanTrue`). `app_main` line 367.
`strings.tsv` has no other key string, and the imports have no other CFPreferences or NSUserDefaults call.
**Confidence:** confirmed.

### P7. Save file `com.silverspaceship.chromatron.dat`
**Answer:**
- **Folder**: at startup `chdir_to_prefs_folder` @ 0x2b64 calls
  `FSFindFolder(kUserDomain = −32763 (0x8005), kPreferencesFolderType = 'pref', kCreateFolder = true, &ref)`,
  `FSRefMakePath(&ref, path, 1024)` and `chdir(path)`. Every file access then uses the **relative** name
  `com.silverspaceship.chromatron.dat`, so the file is `~/Library/Preferences/com.silverspaceship.chromatron.dat`
  (no "1" in the name). If any step fails the error is ignored and the file goes to the inherited working directory.
- **Layout** (no header, magic or version; byte-oriented, so endianness doesn't matter):

  | offset | size | content |
  |---|---|---|
  | 0 | 32 | scrambled progress bitmap (256 bits = 64 slots × 4 check bits) |
  | 32 | 856 × k | k saved board states, in ascending level-id order: `u8 level_id`, then **285 × 3 bytes** = (`type`, `colour`, `dir`) of cells 0..284 (cell i at `board + 20·i`: board rows 0–14 row-major, then toolbox slots 0–59) |

  The file is `32 + 856·k` bytes. Runtime bytes (`+3`, `in[]`, `out[]`) aren't stored.
- **Bitmap**: bit p (0..255) is byte `p >> 3`, mask `1 << (p & 7)` (LSB first). For each slot s = 0..63:
  ```
  check(s):                       # save_slot_check @ 0x400c, result 1..15
      a = 9*s;  a = a + (a << 8);  a = a + (a >> 4);  return a % 15 + 1
  bit_index(s, k), k = 0..3:      # save_bit_index @ 0x3fc8
      return scramble((s & 31) + 32*k) + 128*(s >> 5)
  scramble(n), n = 0..127:        # save_bit_scramble @ 0x3f00 (asserts n < 128, game.c line 0x2cc)
      n < 32: perm[n];  n < 64: perm[(n+13) & 31] + 0x60;  n < 96: perm[(n+20) & 31] + 0x20;  else perm[(n+5) & 31] + 0x40
  perm = save_bit_perm @ 0x80ac = 00 09 0d 13 15 0a 06 18 1e 0e 03 1c 10 0b 19 0c
                                  17 04 11 1d 12 1a 01 0f 14 1f 07 08 02 16 05 1b   (a permutation of 0..31)
  ```
  (s, k) → `bit_index` is a bijection onto 0..255.
  **Write**: start from 32 zero bytes. For each slot with `completed[s] != 0`, set bit `bit_index(s, k)` for every k where
  bit k of `check(s)` is 1. **Read**: for each slot, gather v = Σ (bit `bit_index(s, k)` set) << k. If v == `check(s)`,
  `completed[s] = 1`. If v == 0, the slot isn't completed. **Any other value aborts the game** with
  `../game.c:842: failed assertion '0'` (`assert_fail`), so a corrupt bitmap crashes at launch. `unlocked[]` is recomputed
  (logic.md L6).
  Examples: only slot 0 completed → `01` followed by 31 zero bytes. All 50 slots completed →
  `4776c1cf380fb07fe10837ebd1d54b5c590c291081806c80580c033196800602`.
- **Saved states**: `saved_states[id]` (logic.md L6) is written for every id 0..127 that has one. On load, each record:
  `malloc(0x1644)`, zero it, read 285 triples into bytes +0/+1/+2 of each cell. If a triple is short (truncated file), that
  record is freed and dropped and loading stops. `level_id` isn't validated (a byte ≥ 128 would index past `saved_states[128]`).
  The cell contents aren't validated either.
- **When**: **read** once, at startup in `game_init` (before the window exists). If the file is missing or shorter than 32
  bytes, nothing is completed and no states are loaded. **Written** (`fopen "wb"`, the whole file every time) in two cases:
  1. in `check_win`, on the **first completion** of a slot (`completed = 1; save_level_state(); write_save_file(); update_unlocked()`);
  2. **at quit** (`save_on_quit` = `save_level_state(); write_save_file();`).
  Level switches only update the in-memory `saved_states`. A write failure (fopen NULL) is silently ignored.

**Evidence:** `chdir_to_prefs_folder` line 341 `_FSFindFolder(0xffffffffffff8005,0x70726566,1,auStack_460);` (disassembly
0x2b68–0x2b80: `lis r4, 0x7072; ori r4, r4, 0x6566; li r3, -0x7ffb; li r5, 1`). `load_save_file @ 0x4138` lines 1350–1404
(`_fread(abStack_50,0x20,1,…)`, the `uVar8 == uVar5` / `else if (uVar8 != 0)` assertion with line 0x34a, the record loop
`while (uVar5 < 0x11d)` reading 3 bytes per 0x14-byte cell). `write_save_file @ 0x433c` lines 1426–1462:
```c
    _fwrite(abStack_50,1,0x20,pFVar3);
    ...  if (*(int *)(PTR_saved_states_00012d50 + iVar6 * 4) != 0) {
        local_30[0] = (undefined1)iVar6;  _fwrite(local_30,1,1,pFVar3);
        ...  do { _fwrite(pvVar8,3,1,pFVar3); pvVar8 += 0x14; } while (pvVar9 <= (void *)((int)pvVar1 + 0x1630U));
    } while (iVar6 < 0x80);
```
(condensed). `save_slot_check` disassembly 0x400c–0x4048 (`slwi 3; add; slwi 8; add; srawi 4; add; mod 15 via mulhw 0x88888889;
addi 1`). `save_bit_scramble` disassembly 0x3f40–0x3fb0 (table at `0x3f0c + 0x41a0` = 0x80ac). `check_win` lines 1510–1516.
`save_on_quit` lines 3012–3017. `game_init` line 2674.
**Confidence:** confirmed. The bitmap examples and the bijection were computed with a Python model of the three helpers
(Verification).

### P8. Clipboard (Copy Level / Paste Level)
**Answer:** The Carbon **Scrap Manager**, flavour **`'TEXT'`** (`kScrapFlavorTypeText`), flags 0, plain ASCII with no
terminator, no newline and no other flavour.
- **Copy** (`copy_solution` @ 0x2ef8): `s = encode_solution()`. If it isn't NULL or empty: `ClearCurrentScrap()`,
  `GetCurrentScrap(&scrap)`, `PutScrapFlavor(scrap, 'TEXT', 0, strlen(s), s)`. If it is, the clipboard is left alone.
- **Text format** (`encode_solution` @ 0x5914, static buffer `clip_text` @ 0x139e8):
  ```
  "1-" + NN + "-" + pairs          NN = current_slot + 1 as two digits ("01".."50"); the leading "1" is the game number
  pieces = every cell i in 0..284 (board rows 0–14, then toolbox slots 0–59 as i = 225 + slot) whose type is movable (3–9),
           collected in index order as {u16 i, u8 dir, u8 type, u8 colour}
  qsort(pieces, 6-byte entries, cmp @ 0x58a0)   # by type, then colour, both unsigned ascending
  if 2·n + 6 >= 55 (n >= 25 pieces): return NULL (copy does nothing)
  for each piece: v = i + 285·dir (0..2279);  pairs += b52(v / 52) + b52(v % 52)
  b52(d) = d < 26 ? 'a' + d : 'A' + (d − 26)        # base52_char @ 0x58e4: 'a'..'z' = 0..25, 'A'..'Z' = 26..51
  ```
  So a solution is `1-NN-` plus 2 letters per movable piece, wherever it is (on the board or still in the toolbox).
  Example: slot 1 with its mirror at (8,7) dir 1: i = 7·15 + 8 = 113, v = 113 + 285 = 398 = 7·52 + 34 → **`1-01-hI`**.
  `qsort` isn't stable, so pieces with equal (type, colour) can come out in either order, and the copied text for such a
  level depends on the libc qsort. Paste accepts every order (below).
- **Paste** (`paste_solution` @ 0x2f78): `GetCurrentScrap`, `GetScrapFlavorSize('TEXT')`, `calloc(size + 1)`,
  `GetScrapFlavorData`. If all of that succeeds, `decode_solution(text)` @ 0x5ad4 runs:
  1. Skip leading spaces (0x20 only).
  2. Require `'1' '-' digit digit '-'` with the two digits == `current_slot + 1`, and the next character not a space.
     So **only the current level's solution is accepted**.
  3. Strip trailing spaces (0x20 only; a trailing CR/LF is **not** stripped and makes step 4 fail).
  4. n = number of movable pieces on the board plus toolbox. Require `strlen(rest) == 2n` exactly.
  5. For each pair: `v = val(c1)·52 + val(c2)`, `val(c) = c > 'Z' ? c − 'a' : c − 0x27` (signed char). The target is
     `cell = v % 285`. If that cell's type is non-zero **and not movable** (wall, laser, pinwheel, conduit, teleporter),
     reject.
  **Any failure is a silent no-op**: no beep, no message, the board is unchanged.
  6. Apply: build the current pieces' list and sort it as in Copy. Clear the `type` byte of every movable cell (the other
     bytes stay). Then for the k-th pair (in text order) and the k-th sorted piece: `board[v % 285] = {type, colour}` of that
     piece, `dir = v / 285`. Then `check_win_and_redraw()`.
  Because pieces are matched to pairs by sorted (type, colour), pairs only need to be grouped in that order. Pieces with the
  same type and colour are interchangeable.
  Not validated (a port should reject these): characters other than a–z/A–Z (they give `val` outside 0..51; a negative
  `v` indexes before the board); `v / 285 > 7`, which stores `dir` 8 or 9 unmasked (letters allow v up to 2703); two pairs
  naming the same cell (the later one wins and a piece disappears); targets 249..284 (toolbox slots 24–59, which exist but
  are never drawn).

**Evidence:** `copy_solution` lines 496–503 (`_PutScrapFlavor(local_20[0],0x54455854,0,sVar3,pcVar1);`). `paste_solution`
lines 524–539. `encode_solution` lines 2419–2474 (entry build `*(short *)… = sVar11 + (short)iVar14` with `sVar11 += 0xf` per row
over `0x13` rows, `_qsort(&local_1a0,sVar13,6,…)`, `if (sVar13 * 2 + 6 < 0x37)`, `(ulonglong)*puVar3 + (ulonglong)*pbVar4 * 0x11d`,
`/ 0x34`). Comparator disassembly 0x58a0–0x58e0 (`lbz 3(r3)` vs `lbz 3(r4)`, then `lbz 4`, `cmplw`). `base52_char` /
`base52_value` lines 2363–2390. `decode_solution` lines 2502–2614; the validation at 2538–2543 is shown above. Disassembly
0x5c44–0x5ca8 confirms `extsb` (signed chars) and the `% 0x11d` / `is_movable` reject.
**Confidence:** confirmed.

### P9. Timers and animation
**Answer:** **None.** The import table has no timer API (`InstallEventLoopTimer`, `CFRunLoopTimer`, `SetTimer`, …), no clock
or time call (`time`, `clock`, `TickCount`, `gettimeofday`, `Microseconds`), no random-number call, no sound and no
idle/null-event handler. The complete import list is: process/runtime (`exit`, `atexit`, `abort`, keymgr/dyld glue), memory
and stdio (`malloc`, `calloc`, `free`, `memcpy`, `memset`, `strcpy`, `strlen`, `qsort`, `fopen`, `fread`, `fwrite`,
`fclose`, `fflush`, `fprintf`, `chdir`), CoreFoundation strings and preferences, CoreGraphics image drawing,
`DrawThemeTextBox`, the Scrap Manager, `FSFindFolder`/`FSRefMakePath`, and window, menu, nib and event-loop calls.
Nothing in the game moves on its own: no beam animation, no pinwheel spin, no win effect (logic.md L8). Only input and
system events change the screen. Nothing else is time-based either. The click-versus-drag decision is spatial (the pointer
must move more than 1 px), not a delay, and there's no double-click, key repeat or hover handling.

**Evidence:** `r2 -qc 'ir' "$BIN"` (lazy pointers 0x12be0–0x12cec) and the non-lazy symbols listed at the top. Together they
are the whole symbol import set (`strings.tsv` rows 0x17a0f–0x17f63 list the same names).
**Confidence:** confirmed.

## Unclassified functions

After `just decompile 1` with all three name files, seven `FUN_` functions remain. Six are C runtime / dyld glue from
`crt1.o`, and one is dead game code:

| address | role |
|---|---|
| `FUN_000022f4` | crt `_start`: sets `NXArgc/NXArgv/environ`, registers keymgr/dyld callbacks, computes `__progname`, calls `app_main`, `exit` |
| `FUN_00002474` | calls `__dyld_make_delayed_module_initializer_calls` |
| `FUN_000024bc` | checks dyld kernel support ("The kernel support for the dynamic linker is not present…") |
| `FUN_00002510` | dyld function-lookup trampoline (`(*DAT_00012d8c)()`) |
| `FUN_000025a4` | keymgr image-removal callback (unregisters a 0x28-byte EH object, `abort` on mismatch) |
| `FUN_000026c0` | keymgr init: `__init_keymgr`, registers the add/remove-image callbacks |
| `FUN_00003dd4` | game code, **dead**: clears cell byte +3 for the 15 × 15 board. No callers (data.md D2) |

`assert_fail` @ 0x6ac0 (named here) is the libgcc-style `__eprintf`: `fprintf(stderr, fmt, file, line, expr)`, `fflush`, `abort`.

## Verification
```bash
BIN=chromatron_mac/Chromatron.app/Contents/MacOS/chromatron1
D=re/out/chromatron1/decompiled.c

# Names applied, and only runtime glue + one dead function left as FUN_
grep -c FUN_ re/out/chromatron1/functions.tsv
# 7
grep -c -E '^// (app_main|game_init|create_main_window|install_event_handlers|draw_rgb_image|draw_text_box|paint_window|render_scene|present_frame|load_save_file|write_save_file|encode_solution|decode_solution) @ ' $D
# 13

# P1: window content rect {top 0, left 0, bottom 480, right 640} and CreateNewWindow(kDocumentWindowClass, 0x02000009)
xxd -s 0x6ff0 -l 8 "$BIN"
# 00006ff0: 0000 0000 01e0 0280                      ........
sed -n '325,326p' $D
#   _CreateNewWindow(6,0x2000009,&local_20,&main_window);
#   uVar1 = ___CFStringMakeConstantString("Chromatron 1 (v1.15)");

# P1: CGImageCreate call site: r3 = width (640), r4 = height (480), r5 = 8 bpc, r6 = 24 bpp, r7 = stride*3 (1920),
#     r8 = DeviceRGB, r9 = bitmapInfo 0, r10 = provider; stack 0x38/0x3c/0x40 = decode NULL, interpolate false, intent 0
r2 -e scr.color=0 -qc 'pi 14 @ 0x3138' "$BIN" </dev/null 2>/dev/null
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
sed -n '587p' $D
#   uVar3 = _CGImageCreate(param_4,param_5,8,0x18,param_6 * 3,uVar3,0,uVar2);
sed -n '1970p' $D
#   draw_rgb_image(0,0,framebuffer,fb_width,fb_height,fb_width);

# P3: DrawThemeTextBox(str, kThemeSystemFont 0, kThemeStateActive 1, wrap 1, &rect, teFlushDefault 0, ctx)
r2 -e scr.color=0 -qc 'pi 8 @ 0x32a4' "$BIN" </dev/null 2>/dev/null
# lwz r9, 0x50(r1)
# addi r7, r1, 0x40
# li r5, 1
# li r6, 1
# li r8, 0
# mr r3, r29
# li r4, 0
# bl sym.imp.DrawThemeTextBox
grep -n 'draw_text_box(' $D | sed -n '2,8p'
# 2990:  draw_text_box(pcVar4,uVar5,0x181,400,0x1a4);
# 2994:    draw_text_box(PTR_mode_text_00012d14,0x15e,0x1a4,0x1c2,0x1cc);
# 3000:  draw_text_box(pcVar4,0x1c2,0x7d,0x26c,0x1db);
# 3001:  draw_text_box("freeware",0,0x1c2,100,0x1e0);
# 3003:    draw_text_box("more levels @",0x15e,0x1c2,0x1d1,0x1e0);
# 3005:  draw_text_box(PTR_site_url_text_00012d18,0x1d6,0x1c2,0x280,0x1e0);

# P4: event type specs (class, kind) installed by install_event_handlers
xxd -s 0x6fb0 -l 0x40 "$BIN"
# 00006fb0: 7769 6e64 0000 0048 7769 6e64 0000 0002  wind...Hwind....
# 00006fc0: 6d6f 7573 0000 0001 6d6f 7573 0000 0002  mous....mous....
# 00006fd0: 6d6f 7573 0000 0006 6d6f 7573 0000 000a  mous....mous....
# 00006fe0: 6b65 7962 0000 0001 636d 6473 0000 0001  keyb....cmds....

# P5: menu titles and command ids from the nib (titles are <string name="title"> elements)
grep -o '<string name="title">[^<]*' chromatron_mac/Chromatron.app/Contents/Resources/menus.nib/objects.xib | sed 's/.*>//' | paste -sd'|' -
# menus|Chromatron|Chromatron|File|File|Reset Level|Next Level|Previous Level|Mouse Wheel Changes Level|Close|Edit|Edit|Undo|Redo|Cut|Copy Level|Paste Level|Delete|Select All|Window|Window|Minimize Window|Minimize All Windows|Bring All to Front|Arrange in Front
grep -o '<ostype name="command">[^<]*' chromatron_mac/Chromatron.app/Contents/Resources/menus.nib/objects.xib | sed 's/.*>//' | paste -sd'|' -
# rvrt|Next|Prev|Whee|clos|undo|redo|cut |copy|past|clea|sall|mini|mina|bfrt|frnt

# P7: save file read call sites (fopen "rb", 32-byte bitmap, 1-byte id, 3-byte cell triples)
r2 -e scr.color=0 -qc 'pi 1 @ 0x4168; pi 4 @ 0x417c; pi 5 @ 0x42dc; pi 1 @ 0x42a4; pi 1 @ 0x42ac; pi 1 @ 0x42c0' "$BIN" </dev/null 2>/dev/null
# bl sym.imp.fopen
# li r4, 0x20
# li r5, 1
# mr r6, r27
# bl sym.imp.fread
# addi r3, r1, 0x60
# li r4, 1
# li r5, 1
# mr r6, r27
# bl sym.imp.fread
# li r4, 3
# li r5, 1
# bl sym.imp.fread
# P7: save file write call sites (fopen "wb", 32-byte bitmap, then id byte and 3-byte triples up to +0x1630)
r2 -e scr.color=0 -qc 'pi 1 @ 0x4364; pi 5 @ 0x43fc; pi 7 @ 0x4448' "$BIN" </dev/null 2>/dev/null
# bl sym.imp.fopen
# addi r3, r1, 0x40
# li r4, 1
# li r5, 0x20
# mr r6, r27
# bl sym.imp.fwrite
# addi r28, r30, 0x1630
# mr r3, r30
# li r4, 3
# addi r30, r30, 0x14
# li r5, 1
# mr r6, r27
# bl sym.imp.fwrite
sed -n '1350p;1426p' $D
#   pFVar3 = _fopen("com.silverspaceship.chromatron.dat","rb");
#   pFVar3 = _fopen("com.silverspaceship.chromatron.dat","wb");
# P7: folder = FSFindFolder(kUserDomain -32763, 'pref', kCreateFolder)
r2 -e scr.color=0 -qc 'pi 9 @ 0x2b64' "$BIN" </dev/null 2>/dev/null
# mflr r0
# lis r4, 0x7072
# stw r0, 8(r1)
# ori r4, r4, 0x6566
# stwu r1, -0x4a0(r1)
# li r3, -0x7ffb
# li r5, 1
# addi r6, r1, 0x40
# bl sym.imp.FSFindFolder

# P7: save_bit_perm, and a model of the bitmap helpers (bijection, two example bitmaps)
xxd -s 0x70ac -l 32 "$BIN"
# 000070ac: 0009 0d13 150a 0618 1e0e 031c 100b 190c  ................
# 000070bc: 1704 111d 121a 010f 141f 0708 0216 051b  ................
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
print(sorted(ix(s,k) for s in range(64) for k in range(4))==list(range(256)), bm([0])[:4], bm(range(50)))' "$BIN"
# True 0100 4776c1cf380fb07fe10837ebd1d54b5c590c291081806c80580c033196800602

# P8: scrap flavour 'TEXT' on copy and paste, and the paste validation
sed -n '502p;534p' $D
#       _PutScrapFlavor(local_20[0],0x54455854,0,sVar3,pcVar1);
#       iVar4 = _GetScrapFlavorData(local_20,0x54455854,local_1c,pvVar3);
sed -n '2509,2512p' $D
#     if (((((*param_1 == '1') && (param_1[1] == '-')) && ((byte)param_1[2] - 0x30 < 10)) &&
#         (((byte)param_1[3] - 0x30 < 10 && (param_1[4] == '-')))) &&
#        ((param_1[2] * 10 + (int)param_1[3] + -0x210 == current_slot + 1 &&
#         (param_1 = param_1 + 5, *param_1 != ' ')))) {
# P8: worked example, slot 1 (level id 1 @ 0x81d0): the mirror at (8,7) dir 1 -> "1-01-hI"
python3 -I -c '
import sys
B=open(sys.argv[1],"rb").read(); a=0x81d0-0x1000; out=[]
b52=lambda d: chr(97+d) if d<26 else chr(65+d-26)
while B[a]:
    t,d,c,x,y=B[a:a+5]
    if 3<=t<=9: v=y*15+x+285*d; out.append(b52(v//52)+b52(v%52))
    a+=5
print("1-01-"+"".join(out))' "$BIN"
# 1-01-hI

# P9: no timer, clock, random or sound imports
r2 -e scr.color=0 -qc 'ir' "$BIN" </dev/null 2>/dev/null | grep -c -i -E 'timer|tick|time|clock|rand|sound|idle'
# 0
# positive control: the import list itself is non-empty
r2 -e scr.color=0 -qc 'ir' "$BIN" </dev/null 2>/dev/null | wc -l
# 79
```
