// `just web-check`: the modern look's budgets in the built page's wasm, driven through the Session
// API as the page drives it (Node runs V8, Chrome's engine; putImageData is not included): the
// memory a phone rotation loop and a stepped live resize take; then per game and level, at the spec's web size
// (2450 × 1671 @1.75), against its budget:
// - a cold paint: the best of 5 medians of 7 paints, each after a resize (with that
//   `Session::resize`'s own time too: reported, not gated);
// - the edits (drops and rotations on the board), each its cue's first frame: the best of 5 runs'
//   medians;
// - the animation frames of the rotations' cues (their sweeps; the first 3 rotations of each run,
//   every 16.7 ms until the session is idle): the p90 of every frame, with the worst reported;
//   and the p90 of the frames of the turns that swept: those rotations' that did (most turn a
//   piece off the beams), and SWEEP_TURNS more driven onto the beams (driveSweeps), so every
//   row has sweeps;
// - a drop back onto a tray slot (the board and the tray repaint): the best of 5;
// - a cheat-key win (it moves no piece, so the card repaints, and the first time the picker): the
//   best of 5, and the first alone; then each win's animation frames (packets, pulses, the header
//   written on): their p90, with the worst reported;
// - a drag frame: the median of every frame;
// and per game, on its solution fixture's level, a winning drop (the board and the card repaint
// in one frame): the best of 5; and per game at slot 0, opening the level sheet (timed as an
// edit) on a desktop and a phone sheet layout, with its frames fading in and out (their p90 at
// the animation cap), and a resize with its first paint at the spec size
// (it builds a new lightbox: reported, not gated); and a level change from slot 0 (the cheat key,
// then `=`): its first frame, the best of 5, at the cold cap, and its cross-fade's and sweep's
// frames' p90 at the animation cap. Noise only ever adds time, hence the bests.
// With `light-memory`, it only measures the light stepped live resize's memory, in its own fresh
// process (wasm memory never shrinks), and exits.
// Usage: node lightbox_budgets.mjs <dist/web> <tests/solutions> [light-memory]

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [webDir, solutionsDir, mode] = process.argv.slice(2);
assert.ok(webDir && solutionsDir, "usage: node lightbox_budgets.mjs <dist/web> <tests/solutions>");

// The user's web budgets (2026-10-12), in ms; `anim` is the user's motion budget (2026-10-13).
// Tightened (phase-4 Task 13; spec "Revision: motion budgets": a cap whose worst row over three
// local runs is ≤ 0.6 × it becomes max(1.5 × worst, worst + 5) rounded up to 5 ms): `edit` 45 → 40
// (worst 24.0 ms, C4 slot 20's drops), `anim` 45 → 35 (worst 21.5 ms, C3 slot 41's sweeps).
// web_shots.mjs's ANIM_CAP (40) is derived from `anim`: it adds Chrome's measured putImageData
// p90 and rounds up to 5 ms (spec "Revision: motion budgets"); a change to `anim` moves it too.
const BUDGETS = { cold: 60, edit: 40, drag: 4, anim: 35 };
// Timings depend on the machine: CI's shared runners are slower and noisier, so only local runs
// (GitHub Actions sets CI=true) assert them. Size, behaviour and memory checks always run.
const TIMING = !process.env.CI;
const REPEATS = 5;

const glue = await import(pathToFileURL(join(webDir, "chromatron_web.js")).href);
const wasm = glue.initSync({ module: readFileSync(join(webDir, "chromatron_web_bg.wasm")) });
const { Flag, Look, Session } = glue;
const CHEATS = { 1: ["L", false], 2: ["z", false], 3: ["^", false], 4: ["o", true] };
const median = (times) => [...times].sort((a, b) => a - b)[times.length >> 1];
const timed = (f) => {
  const start = performance.now();
  f();
  return performance.now() - start;
};
// A monotonic clock (ruling M7): every step renders 10 s after the last, past every cue it left;
// `frames` renders every 16.7 ms from the clock's now until next_wake is −1, timing each, and
// returns how many it rendered.
let clockNow = 0;
const later = () => (clockNow += 10_000);
const render = (session) => session.render_at(later());
function frames(session, into) {
  let k = 0;
  for (; session.next_wake(clockNow) >= 0; k++) {
    assert.ok(k < 600, "an animation that never settles");
    clockNow += 1000 / 60;
    into.push(timed(() => session.render_at(clockNow)));
  }
  return k;
}
// A sweep runs at least 250 ms (15 frames); a turn that changes no beam animates for 150 ms.
const SWEEP_FRAMES = 15;
// The driven cues that must sweep on every row (driveSweeps): 3 locally, as the bench's
// `sweep_frames`; 1 on CI, which skips the timing asserts but still checks every row sweeps.
const SWEEP_TURNS = TIMING ? 3 : 1;
// The 90th percentile of `times` (null for none).
const p90 = (times) => (times.length === 0 ? null : [...times].sort((a, b) => a - b)[Math.floor(times.length * 0.9)]);

// Memory first, in this fresh wasm instance. wasm memory never shrinks, so its size is the
// high-water mark over the classic baseline (one classic game painted), and the smaller budget is
// measured first. A phone turned between portrait and landscape 40 times (780 × 1688 ↔ 1688 × 780
// @2, touch), the level sheet's paint every fourth turn: spec §14's phone row, 20 MiB.
const MIB = 1 << 20;
let baseline;
// A live desktop resize from 1400 × 900 to the 5.5 Mpx limit (2880 × 1910 @1) in 60 steps, then
// back and forth, as the page would drive it without its resize debounce.
function steppedResize(session) {
  const step = (t) => {
    session.resize(Math.round(1400 + 1480 * t), Math.round(900 + 1010 * t), 1, [0, 0, 0, 0], false);
    render(session);
  };
  for (let k = 0; k <= 60; k++) step(k / 60);
  for (let k = 0; k <= 120; k++) step((Math.cos(k / 7) + 1) / 2);
  step(1);
}
if (mode === "light-memory") {
  // Memory never shrinks, so the light stepped resize is told apart only in a fresh process:
  // its own classic baseline, the stepped desktop resize with the light theme, <= 96 MiB; then exit.
  const budget = 96 * MIB;
  const session = new Session();
  session.select(1, undefined, undefined);
  render(session);
  baseline = wasm.memory.buffer.byteLength;
  session.set_theme(false);
  session.set_look(Look.Modern);
  steppedResize(session);
  const used = wasm.memory.buffer.byteLength - baseline;
  console.log(`memory: ${(used / MIB).toFixed(1)} MiB over the classic baseline (${(baseline / MIB).toFixed(1)} MiB) after a light stepped live resize`);
  assert.ok(used <= budget, `a light stepped live resize takes ${(used / MIB).toFixed(1)} MiB, over ${budget / MIB}`);
  session.free();
  process.exit(0);
}
{
  const budget = 20 * MIB;
  const session = new Session();
  session.select(1, undefined, undefined);
  render(session);
  baseline = wasm.memory.buffer.byteLength;
  session.set_look(Look.Modern);
  for (let k = 0; k < 40; k++) {
    const [width, height] = k % 2 === 0 ? [780, 1688] : [1688, 780];
    session.resize(width, height, 2, [0, 0, 0, 0], true);
    render(session);
    if (k % 4 === 3) {
      session.command("levels");
      render(session);
    }
  }
  const used = wasm.memory.buffer.byteLength - baseline;
  console.log(`memory: ${(used / MIB).toFixed(1)} MiB over the classic baseline (${(baseline / MIB).toFixed(1)} MiB) after 40 phone rotations`);
  assert.ok(used <= budget, `40 phone rotations take ${(used / MIB).toFixed(1)} MiB, over ${budget / MIB}`);

  // Ruling L2: 20 in-place flips in portrait, then 20 in landscape, rendering after each, with
  // no resize between flips: the swap of the dark planes for the wash and back, alone.
  for (const [width, height] of [[780, 1688], [1688, 780]]) {
    session.resize(width, height, 2, [0, 0, 0, 0], true);
    render(session);
    for (let k = 0; k < 20; k++) {
      session.set_theme(k % 2 === 1);
      render(session);
    }
  }
  const inPlace = wasm.memory.buffer.byteLength - baseline;
  console.log(`memory: ${(inPlace / MIB).toFixed(1)} MiB over the classic baseline after 40 in-place theme flips`);
  assert.ok(inPlace <= budget, `in-place theme flips take ${(inPlace / MIB).toFixed(1)} MiB, over ${budget / MIB}`);

  // Ruling L2: the theme flipped every third step while the phone keeps turning, in the same
  // wasm instance (memory never shrinks, so this is the high-water of both loops): <= 20 MiB.
  for (let k = 0; k < 40; k++) {
    if (k % 3 === 0) session.set_theme(k % 6 !== 0);
    const [width, height] = k % 2 === 0 ? [780, 1688] : [1688, 780];
    session.resize(width, height, 2, [0, 0, 0, 0], true);
    render(session);
    if (k % 4 === 3) {
      session.command("levels");
      render(session);
    }
  }
  session.set_theme(true);
  render(session);
  const flipped = wasm.memory.buffer.byteLength - baseline;
  console.log(`memory: ${(flipped / MIB).toFixed(1)} MiB over the classic baseline after 40 more rotations with the theme flipped every third`);
  assert.ok(flipped <= budget, `rotations with theme flips take ${(flipped / MIB).toFixed(1)} MiB, over ${budget / MIB}`);
  session.free();
}

// Then the stepped live resize (steppedResize): the classic baseline plus the modern buffers,
// which spec §14 holds to 96 MiB with the RGBA canvas.
{
  const budget = 96 * MIB;
  const session = new Session();
  session.select(1, undefined, undefined);
  render(session);
  session.set_look(Look.Modern);
  steppedResize(session);
  const used = wasm.memory.buffer.byteLength - baseline;
  console.log(`memory: ${(used / MIB).toFixed(1)} MiB over the classic baseline (${(baseline / MIB).toFixed(1)} MiB) after a stepped live resize`);
  assert.ok(used <= budget, `a stepped live resize takes ${(used / MIB).toFixed(1)} MiB, over ${budget / MIB}`);

  // Ruling L2: 10 in-place flips at the web reference size, after the stepped resize: <= 96 MiB.
  session.resize(2450, 1671, 1.75, [0, 0, 0, 0], false);
  render(session);
  for (let k = 0; k < 10; k++) {
    session.set_theme(k % 2 === 1);
    render(session);
  }
  const desk = wasm.memory.buffer.byteLength - baseline;
  console.log(`memory: ${(desk / MIB).toFixed(1)} MiB over the classic baseline after 10 desktop theme flips`);
  assert.ok(desk <= budget, `desktop theme flips take ${(desk / MIB).toFixed(1)} MiB, over ${budget / MIB}`);
  session.free();
}

// The tableau at 2450 × 1671 @1.75 (halation's Tableau::arrange): the board's top-left and cell
// side, and the tray's top-left and slot pitch (6 columns). A layout change fails the self-checks
// below (no tray piece found, or no drop lands), and these must follow it.
const [WIDTH, HEIGHT, SCALE] = [2450, 1671, 1.75];
const BOARD = { left: 79, top: 78, cell: 101 };
const TRAY = { left: 1725, top: 754, pitch: 105 };
const boardCell = (i) => [BOARD.left + BOARD.cell * (i % 15) + 50, BOARD.top + BOARD.cell * Math.floor(i / 15) + 50];
const traySlot = (k) => [TRAY.left + TRAY.pitch * (k % 6) + 52, TRAY.top + TRAY.pitch * Math.floor(k / 6) + 52];
// Renders; whether it repainted board light (a drop's or a rotation's, with its glow and bloom,
// redrawn only where it changed), not just a piece: some damage reaches past one cell.
const lightRepainted = (session) => {
  const n = render(session);
  const rects = new Int32Array(wasm.memory.buffer, session.dirty_ptr(), 4 * n);
  for (let i = 0; i < n; i++) {
    if (rects[4 * i + 2] - rects[4 * i] > BOARD.cell) return true;
  }
  return false;
};

// Drives sweeps: each tray piece in turn is carried across up to 6 of the board's lit cells (a
// cell whose centre pixel is bright in the settled canvas: a beam crosses it, or a fixed piece
// shines there, where no drop lands) and turned on each up to 4 times, until SWEEP_TURNS of these
// moves and turns have swept (a cue of SWEEP_FRAMES frames or more: beams swept on, or a target
// lit and burst; a piece that only moves, as on C4's level 21, unblocks the beams it leaves).
// Pushes those cues' frame times onto `into` and returns how many swept.
function driveSweeps(session, into, dark) {
  const lifts = ([x, y]) => {
    session.pointer_down(x, y, 0, false, false, false);
    session.pointer_move(x + 30, y + 30);
    const lifted = session.dragging();
    if (!lifted) {
      session.pointer_cancel();
      render(session);
    }
    return lifted;
  };
  const litCells = () => {
    render(session);
    const width = session.canvas_width();
    const rgba = new Uint8Array(wasm.memory.buffer, session.rgba_ptr(), width * session.canvas_height() * 4);
    const cells = [];
    // Dark: lit is bright at the centre. Light: the page is bright everywhere, so lit is a beam's
    // pure white core (the wash tops out near 250) or its saturated colour (far from grey).
    for (let i = 0; i < 225; i++) {
      const [x, y] = boardCell(i);
      const at = (y * width + x) * 4;
      const c = [rgba[at], rgba[at + 1], rgba[at + 2]];
      const lit = dark ? Math.max(...c) >= 96 : Math.min(...c) >= 253 || Math.max(...c) - Math.min(...c) >= 120;
      if (lit) cells.push(i);
    }
    return cells;
  };
  let swept = 0;
  // Renders an input's first frame (an edit, untimed) and times its cue's frames, kept if it ran
  // SWEEP_FRAMES or more (counted in `swept`); returns how many it ran, or −1 for an input that
  // won the level, whose frames (the win's packets and header, cheap) are dropped: they would pull
  // the sweep p90 down.
  const cue = () => {
    render(session);
    const start = into.length;
    const n = frames(session, into);
    if (session.won()) {
      into.length = start;
      return -1;
    }
    if (n >= SWEEP_FRAMES) swept += 1;
    else into.length = start;
    return n;
  };
  for (let k = 0; k < 24 && swept < SWEEP_TURNS; k++) {
    let at = traySlot(k);
    if (!lifts(at)) continue;
    session.pointer_cancel();
    render(session);
    let cells = 0;
    let turns = true;
    for (const i of litCells()) {
      if (swept >= SWEEP_TURNS || cells === 6) break;
      const cell = boardCell(i);
      if (!lifts(at)) break;
      session.pointer_move(cell[0], cell[1]);
      render(session);
      session.pointer_up(cell[0], cell[1], 0);
      // The move's own cue: the beams it unblocked sweep on (or a target it lit bursts).
      const moved = cue();
      if (!lifts(cell)) continue;
      session.pointer_cancel();
      render(session);
      at = cell;
      cells += 1;
      // A move that won the level: no turns on this cell.
      if (moved < 0) continue;
      // A piece that doesn't turn animates nothing: it only moves. A turn that won: the next cell.
      for (let turn = 0; turn < 4 && swept < SWEEP_TURNS && turns; turn++) {
        session.pointer_down(cell[0], cell[1], 0, false, false, false);
        session.pointer_up(cell[0], cell[1], 0);
        const n = cue();
        if (n < 0) break;
        turns = n > 0;
      }
    }
  }
  return swept;
}

const THEMES = [["dark", true], ["light", false]];
const rows = [];
for (const [theme, dark] of THEMES) for (const game of [1, 2, 3, 4]) {
  for (const slot of [0, 20, 41]) {
    const session = new Session();
    session.set_theme(dark);
    session.select(game, undefined, undefined);
    const [cheat, ctrl] = CHEATS[game];
    while (session.slot() < slot) {
      session.key(cheat, ctrl, false, false);
      session.key("=", false, false, false);
    }
    assert.equal(session.slot(), slot, `game ${game}: slot ${slot} opens`);
    session.set_look(Look.Modern);
    session.resize(WIDTH, HEIGHT, SCALE, [0, 0, 0, 0], false);
    render(session);

    // Cold, as after a resize: re-arranged, backdrop re-baked, sprites drawn again. `render_at`
    // is gated; with `Session::resize` (which builds the lightbox) it is reported.
    // Noise only ever adds time, so each row is judged by its best of REPEATS medians.
    const colds = [];
    const coldsWithResize = [];
    for (let r = 0; r < REPEATS; r++) {
      const times = [];
      const withResize = [];
      for (let k = 0; k < 7; k++) {
        const start = performance.now();
        session.resize(WIDTH + ((k + 1) % 2), HEIGHT, SCALE, [0, 0, 0, 0], false);
        const resized = performance.now();
        render(session);
        const end = performance.now();
        times.push(end - resized);
        withResize.push(end - start);
      }
      colds.push(median(times));
      coldsWithResize.push(median(withResize));
    }
    session.resize(WIDTH, HEIGHT, SCALE, [0, 0, 0, 0], false);
    render(session);

    // A tray piece to play with.
    const press = ([x, y]) => {
      session.pointer_down(x, y, 0, false, false, false);
      session.pointer_move(x + 30, y + 30);
    };
    let from = null;
    for (let k = 0; k < 24 && !from; k++) {
      press(traySlot(k));
      if (session.dragging()) from = traySlot(k);
      session.pointer_cancel();
      render(session);
    }
    assert.ok(from, `game ${game} slot ${slot}: a tray piece`);

    // Carry it from cell to cell over the board, timing the drag frames, the drop and then a
    // rotation; a drop that doesn't land (the cell holds a piece that can't be moved) is not counted;
    // one onto a movable occupant is.
    const edits = [];
    const drags = [];
    const editRuns = [];
    const trayDrops = [];
    const anims = [];
    const sweeps = [];
    for (let r = 0; r < REPEATS; r++) {
    const runStart = edits.length;
    for (let i = 0, landed = 0; i < 225 && landed < 9; i++) {
      const target = boardCell((i * 97 + 31) % 225);
      press(from);
      if (!session.dragging()) break;
      render(session);
      for (let m = 0; m < 4; m++) {
        session.pointer_move(target[0] + 3 * m, target[1] - 2 * m);
        drags.push(timed(() => render(session)));
      }
      session.pointer_up(target[0], target[1], 0);
      const drop = timed(() => lightRepainted(session));
      press(target);
      const there = session.dragging();
      session.pointer_cancel();
      render(session);
      if (!there) continue;
      edits.push(drop);
      session.pointer_down(target[0], target[1], 0, false, false, false);
      session.pointer_up(target[0], target[1], 0);
      let rotated = false;
      edits.push(timed(() => (rotated = lightRepainted(session))));
      assert.ok(rotated, `game ${game} slot ${slot}: a rotation repaints the board's light`);
      // The rotation's cue (its sweep), frame by frame, for the run's first 3 landings; the
      // frames of those that swept are also kept apart.
      if (landed < 3) {
        const start = anims.length;
        if (frames(session, anims) >= SWEEP_FRAMES) sweeps.push(...anims.slice(start));
      }
      from = target;
      landed += 1;
    }
    // A tray drop, timed as an edit: the piece from its last board cell back to an empty tray slot
    // (the first whose press starts no drag).
    let home = null;
    for (let k = 0; k < 24 && !home; k++) {
      press(traySlot(k));
      if (!session.dragging()) home = traySlot(k);
      session.pointer_cancel();
      render(session);
    }
    assert.ok(home, `game ${game} slot ${slot}: an empty tray slot`);
    press(from);
    assert.ok(session.dragging(), `game ${game} slot ${slot}: the piece lifts for the tray drop`);
    render(session);
    session.pointer_move(home[0], home[1]);
    render(session);
    session.pointer_up(home[0], home[1], 0);
    const trayDrop = timed(() => render(session));
    edits.push(trayDrop);
    trayDrops.push(trayDrop);
    from = home;
    editRuns.push(median(edits.slice(runStart)));
    }
    assert.ok(edits.length >= 10 * REPEATS, `game ${game} slot ${slot}: only ${edits.length} board edits landed`);
    // Sweeps, driven (as the bench's `sweep_frames`): the carried pieces' turns above seldom
    // change the beams, so tray pieces are dropped onto the board's lit cells and turned until
    // SWEEP_TURNS turns have swept; every frame of those turns' cues is a sweep frame.
    const drivenSweeps = driveSweeps(session, sweeps, dark);
    assert.ok(drivenSweeps >= SWEEP_TURNS, `${theme} game ${game} slot ${slot}: only ${drivenSweeps} turns swept`);
    // The pieces back in the tray, the restart's cue run out: the wins below start from rest.
    session.key("r", false, false, false);
    render(session);
    frames(session, []);
    // A win, timed as an edit: the level restarted, then won with the cheat key, which wins
    // without moving a piece, so the card (its header and Next ▶) changes and, the first time, the
    // picker.
    // Each win's animation frames follow (its packets, pulses and header written on).
    const wins = [];
    const winAnims = [];
    for (let r = 0; r < REPEATS; r++) {
      session.key("r", false, false, false);
      render(session);
      assert.ok(!session.won(), `game ${game} slot ${slot}: the restart unwins the level`);
      session.key(cheat, ctrl, false, false);
      assert.ok(session.won(), `game ${game} slot ${slot}: the cheat key wins`);
      wins.push(timed(() => render(session)));
      frames(session, winAnims);
    }
    assert.ok(winAnims.length >= REPEATS * 60, `game ${game} slot ${slot}: a win animates for 1.4 s`);
    // A carried piece whose turns change no beam and show no turn animates nothing: no rotations'
    // row (as the bench's `sweep_frames`, ruling F12). Every row has sweeps (driveSweeps).
    const animated = anims.length > 0;
    rows.push({ name: `${theme} C${game} slot ${slot}`, cold: Math.min(...colds), coldResize: Math.min(...coldsWithResize), edit: Math.min(...editRuns), drag: median(drags), tray: Math.min(...trayDrops), win: Math.min(...wins), firstWin: wins[0], anim: animated ? p90(anims) : null, animMax: animated ? Math.max(...anims) : null, animFrames: anims.length, sweep: p90(sweeps), sweepMax: Math.max(...sweeps), sweepFrames: sweeps.length, winAnim: p90(winAnims), winAnimMax: Math.max(...winAnims), winAnimFrames: winAnims.length });
    session.free();
  }
}

// A winning drop, per game on its solution fixture's level: the solution pasted, one of its
// pieces lifted to an empty tray slot (the level unwon), then dropped back onto its cell, which
// wins it: the board, its light and the card (YOU WIN! and Next ▶) repaint in that one frame,
// timed REPEATS times and judged by the best, against the edit budget.
const winningDrops = [];
for (const [theme, dark] of THEMES) for (const game of [1, 2, 3, 4]) {
  const dir = join(solutionsDir, `game${game}`);
  for (const file of readdirSync(dir).filter((f) => /^level-\d\d\.txt$/.test(f)).sort()) {
    const slot = Number(file.slice(6, 8)) - 1;
    const solution = readFileSync(join(dir, file), "utf8").trim();
    const session = new Session();
    session.set_theme(dark);
    session.select(game, undefined, undefined);
    const [cheat, ctrl] = CHEATS[game];
    while (session.slot() < slot) {
      session.key(cheat, ctrl, false, false);
      session.key("=", false, false, false);
    }
    assert.equal(session.slot(), slot, `game ${game}: slot ${slot} opens`);
    session.set_look(Look.Modern);
    session.resize(WIDTH, HEIGHT, SCALE, [0, 0, 0, 0], false);
    render(session);
    session.paste(solution);
    assert.ok(session.won(), `game ${game} slot ${slot}: the solution wins`);
    render(session);
    // Carries the piece at `from` to `to` (a drag frame rendered on the way); returns whether it
    // was lifted.
    const carry = (from, to) => {
      session.pointer_down(from[0], from[1], 0, false, false, false);
      session.pointer_move(from[0] + 30, from[1] + 30);
      if (!session.dragging()) {
        session.pointer_cancel();
        return false;
      }
      session.pointer_move(to[0], to[1]);
      render(session);
      session.pointer_up(to[0], to[1], 0);
      return true;
    };
    let home = null;
    for (let k = 0; k < 24 && !home; k++) {
      session.pointer_down(...traySlot(k), 0, false, false, false);
      session.pointer_move(traySlot(k)[0] + 30, traySlot(k)[1] + 30);
      if (!session.dragging()) home = traySlot(k);
      session.pointer_cancel();
      render(session);
    }
    assert.ok(home, `game ${game} slot ${slot}: an empty tray slot`);
    // The first board piece whose lifting unwins the level.
    let cell = null;
    for (let i = 0; i < 225 && !cell; i++) {
      if (!carry(boardCell(i), home)) continue;
      render(session);
      const unwon = !session.won();
      assert.ok(carry(home, boardCell(i)), `game ${game} slot ${slot}: the piece comes back`);
      render(session);
      if (unwon && session.won()) cell = boardCell(i);
    }
    assert.ok(cell, `game ${game} slot ${slot}: a piece whose drop wins`);
    const drops = [];
    for (let r = 0; r < REPEATS; r++) {
      carry(cell, home);
      render(session);
      assert.ok(!session.won(), `game ${game} slot ${slot}: lifting the piece unwins`);
      session.pointer_down(home[0], home[1], 0, false, false, false);
      session.pointer_move(home[0] + 30, home[1] + 30);
      session.pointer_move(cell[0], cell[1]);
      render(session);
      session.pointer_up(cell[0], cell[1], 0);
      assert.ok(session.won(), `game ${game} slot ${slot}: the drop wins`);
      drops.push(timed(() => render(session)));
    }
    winningDrops.push({ name: `${theme} C${game} slot ${slot}`, drop: Math.min(...drops), median: median(drops) });
    session.free();
  }
}
// Per game at slot 0: the level sheet opening, timed as an edit (the whole canvas is composed
// again under the scrim), in a desktop sheet layout (1600 × 1200 @2) and a phone's (780 × 1480
// @2, touch); then a resize and its first paint at the spec size, which builds a new lightbox
// (construction is measured, not gated).
const SHEET_SIZES = [
  [1600, 1200, 2, false],
  [780, 1480, 2, true],
];
const sheetRows = [];
const resizeRows = [];
for (const [theme, dark] of THEMES) for (const game of [1, 2, 3, 4]) {
  const session = new Session();
  session.set_theme(dark);
  session.select(game, undefined, undefined);
  session.set_look(Look.Modern);
  for (const [width, height, scale, touch] of SHEET_SIZES) {
    const name = `${theme} C${game} at ${width} × ${height} @${scale}${touch ? " touch" : ""}`;
    session.resize(width, height, scale, [0, 0, 0, 0], touch);
    render(session);
    assert.ok(!session.has_inline_picker(), `${name}: a sheet picker`);
    const opens = [];
    const sheetAnims = [];
    for (let r = 0; r < REPEATS; r++) {
      assert.ok(session.command("levels") & Flag.SheetOpened, `${name}: the sheet opens`);
      opens.push(timed(() => render(session)));
      frames(session, sheetAnims);
      session.command("levels");
      assert.ok(!session.sheet_open(), `${name}: the sheet closes`);
      render(session);
      frames(session, sheetAnims);
    }
    assert.ok(sheetAnims.length >= REPEATS * 2 * 11, `${name}: the sheet fades in and out (${sheetAnims.length} frames)`);
    sheetRows.push({ name, open: Math.min(...opens), sheetAnim: p90(sheetAnims), sheetAnimMax: Math.max(...sheetAnims), sheetAnimFrames: sheetAnims.length });
  }
  const resizes = [];
  for (let r = 0; r < REPEATS; r++) {
    resizes.push(
      timed(() => {
        session.resize(WIDTH + (r % 2), HEIGHT, SCALE, [0, 0, 0, 0], false);
        render(session);
      }),
    );
  }
  resizeRows.push({ name: `${theme} C${game} slot 0`, best: Math.min(...resizes), typical: median(resizes) });
  session.free();
}

// Per game at slot 0, at the spec size, REPEATS times: the cheat key, then `=` (the next level),
// its first frame (the old board kept, the new one painted whole; held to the cold cap), then its
// cross-fade's and sweep's frames (the animation cap), then `-` back to slot 0, past the clock.
const levelRows = [];
for (const [theme, dark] of THEMES) for (const game of [1, 2, 3, 4]) {
  const session = new Session();
  session.set_theme(dark);
  session.select(game, undefined, undefined);
  session.set_look(Look.Modern);
  session.resize(WIDTH, HEIGHT, SCALE, [0, 0, 0, 0], false);
  render(session);
  const [cheat, ctrl] = CHEATS[game];
  const firsts = [];
  const levelAnims = [];
  for (let r = 0; r < REPEATS; r++) {
    session.key(cheat, ctrl, false, false);
    session.key("=", false, false, false);
    assert.equal(session.slot(), 1, `game ${game}: on to slot 1`);
    firsts.push(timed(() => render(session)));
    frames(session, levelAnims);
    session.key("-", false, false, false);
    assert.equal(session.slot(), 0, `game ${game}: back to slot 0`);
    render(session);
  }
  assert.ok(levelAnims.length > 0, `game ${game}: the level changes animate`);
  levelRows.push({ name: `${theme} C${game} slot 0 → 1`, levelFirst: Math.min(...firsts), levelAnim: p90(levelAnims), levelAnimMax: Math.max(...levelAnims), levelAnimFrames: levelAnims.length });
  session.free();
}

for (const { name, cold, coldResize, edit, drag, tray, win, firstWin, anim, animMax, animFrames, sweep, sweepMax, sweepFrames, winAnim, winAnimMax, winAnimFrames } of rows) {
  const turned = anim === null ? "no rotation animates" : `rotations' anim p90 ${anim.toFixed(1)} ms (worst ${animMax.toFixed(1)} ms, ${animFrames} frames)`;
  const motion = `sweep anim p90 ${sweep.toFixed(1)} ms (worst ${sweepMax.toFixed(1)} ms, ${sweepFrames} frames; ${turned})`;
  const winMotion = `win anim p90 ${winAnim.toFixed(1)} ms (worst ${winAnimMax.toFixed(1)} ms, ${winAnimFrames} frames)`;
  console.log(`${name}: cold ${cold.toFixed(1)} ms (with Session::resize ${coldResize.toFixed(1)} ms, reported), drop or rotate ${edit.toFixed(1)} ms (tray drop ${tray.toFixed(1)} ms, cheat win ${win.toFixed(1)} ms, first cheat win ${firstWin.toFixed(1)} ms), drag frame ${drag.toFixed(2)} ms, ${motion}, ${winMotion}`);
}
for (const { name, drop, median: typical } of winningDrops) {
  console.log(`${name}: winning drop ${drop.toFixed(1)} ms (median ${typical.toFixed(1)} ms)`);
}
for (const { name, open, sheetAnim, sheetAnimMax, sheetAnimFrames } of sheetRows) {
  console.log(`${name}: sheet open ${open.toFixed(1)} ms, sheet anim p90 ${sheetAnim.toFixed(1)} ms (worst ${sheetAnimMax.toFixed(1)} ms, ${sheetAnimFrames} frames)`);
}
for (const { name, best, typical } of resizeRows) {
  console.log(`${name} at ${WIDTH} × ${HEIGHT} @${SCALE}: resize + first paint ${best.toFixed(1)} ms (median ${typical.toFixed(1)} ms; reported, not gated)`);
}
for (const { name, levelFirst, levelAnim, levelAnimMax, levelAnimFrames } of levelRows) {
  console.log(`${name} at ${WIDTH} × ${HEIGHT} @${SCALE}: level change first frame ${levelFirst.toFixed(1)} ms, level anim p90 ${levelAnim.toFixed(1)} ms (worst ${levelAnimMax.toFixed(1)} ms, ${levelAnimFrames} frames)`);
}
// Ruling L11: each light row against its dark twin; > 1.15 is recorded for phase 6, not failed.
const twin = (name) => name.replace(/^light /, "dark ");
for (const list of [rows, winningDrops, sheetRows, levelRows]) {
  for (const row of list.filter((r) => r.name.startsWith("light "))) {
    const dark = list.find((r) => r.name === twin(row.name));
    for (const key of ["cold", "edit", "drag", "anim", "sweep", "winAnim", "drop", "open", "sheetAnim", "levelFirst", "levelAnim"]) {
      if (typeof row[key] !== "number" || typeof dark?.[key] !== "number" || dark[key] <= 0) continue;
      const ratio = row[key] / dark[key];
      console.log(`light/dark ${row.name.slice(6)} ${key}: ${ratio.toFixed(2)}${ratio > 1.15 ? " (over 1.15: recorded for phase 6)" : ""}`);
    }
  }
}
if (!TIMING) {
  console.log("timing skipped on CI (budgets assert on local runs only)");
} else {
  for (const row of rows) {
    // The tray drop is an edit too: in the edits' median, and on its own at the same cap; so is
    // a win, its first (which also repaints the picker) included. The sweeps' and the wins' own
    // frames are held to the animation cap too. A row with none (null) is not gated.
    const asEdits = ["tray", "win", "firstWin"].map((what) => [what, BUDGETS.edit]);
    asEdits.push(["sweep", BUDGETS.anim], ["winAnim", BUDGETS.anim]);
    for (const [what, cap] of [...Object.entries(BUDGETS), ...asEdits]) {
      if (row[what] === null) continue;
      assert.ok(row[what] <= cap, `${row.name} at ${WIDTH} × ${HEIGHT} @${SCALE}: ${what} ${row[what].toFixed(1)} ms, over ${cap}`);
    }
  }
  for (const { name, drop } of winningDrops) {
    assert.ok(drop <= BUDGETS.edit, `${name} at ${WIDTH} × ${HEIGHT} @${SCALE}: winning drop ${drop.toFixed(1)} ms, over ${BUDGETS.edit}`);
  }
  for (const { name, open, sheetAnim } of sheetRows) {
    assert.ok(open <= BUDGETS.edit, `${name}: sheet open ${open.toFixed(1)} ms, over ${BUDGETS.edit}`);
    assert.ok(sheetAnim <= BUDGETS.anim, `${name}: sheet anim p90 ${sheetAnim.toFixed(1)} ms, over ${BUDGETS.anim}`);
  }
  for (const { name, levelFirst, levelAnim } of levelRows) {
    assert.ok(levelFirst <= BUDGETS.cold, `${name}: level change first frame ${levelFirst.toFixed(1)} ms, over ${BUDGETS.cold}`);
    assert.ok(levelAnim <= BUDGETS.anim, `${name}: level anim p90 ${levelAnim.toFixed(1)} ms, over ${BUDGETS.anim}`);
  }
  console.log(`ok: the modern look's paint budgets hold on every row (cold and level change first frame ${BUDGETS.cold} ms, drop or rotate ${BUDGETS.edit} ms, drag frame ${BUDGETS.drag} ms, sweep, win, level and sheet anim p90 ${BUDGETS.anim} ms)`);
}
