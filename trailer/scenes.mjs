// The trailer's picture: renderFrame(ctx, t, { width, height, shape }) draws frame time t (seconds)
// of the storyboard in timeline.mjs, landscape (1920 × 1080) or portrait (1080 × 1920), directed
// like a film trailer: a virtual camera sees each board as a plane in space (lens.mjs), with depth
// of field, haze and dust lit by the beams, speed ramps, title cards drawn on by a beam between
// shots, and a film finish (grade, edge aberration, vignette, grain, a 2.39:1 letterbox in
// landscape until the facts).
//
// A frame is a pure function of t: board changes, beam fronts, bursts, the cursor, camera moves
// and every entrance are computed from t and the timeline, and particles and grain come from seeded
// PRNGs, so any frame rendered alone has the same pixels as in sequence. The memoised plays and the
// per-size canvases (draw.mjs's lightFor, lens.mjs's scratch and haze layers) are fixed, not state.
//
// Portrait keeps text and key action out of the platforms' UI: the top 8 %, the bottom 20 % and the
// right 12 % of the frame.

import { DIRS, playback, stateAt, targetAt } from "./board.mjs";
import { CHANNEL_RGB, FONTS, clamp, drawBeams, drawBurst, drawCursor, drawFlare, drawGlints, drawMark, drawMarkCore, drawPiece, drawPieceLight, drawTile, ease, lightFor, rgba } from "./draw.mjs";
import { cameraAt, camera, depthOfField, drawAtmosphere, drawDust, finish, letterbox, onPlane } from "./lens.mjs";
import { drawBeamPath, drawLettering, drawSparkle, drawTitleCard, drawWordmark, drawZhLine, layoutLine, wordmarkMetrics } from "./lettering.mjs";
import { BEAM_SPEED, END, END_TEXT, FACTS, FPS, INTRO, LETTERBOX, PLATFORMS, PLAYS, captionWrite, sceneAt, shotAt, storyTime } from "./timeline.mjs";

// 0 before a, 1 after b, linear between (a step at a when the two meet).
const ramp = (t, a, b) => (b > a ? clamp((t - a) / (b - a)) : t >= a ? 1 : 0);
const outCubic = (k) => 1 - (1 - k) ** 3;
const rad = (deg) => (deg * Math.PI) / 180;
// How every scene composites its light layer onto the frame: a tight glow and a wide bloom.
const COMPOSITE = { glowRadius: 9, bloomRadius: 70, bloom: 0.8 };
// The flat drawing scale for things laid on a plane: px per cell before the camera maps them.
const FLAT = 100;
// Heights above the board plane, in cells: pieces and beams float a little over their tiles, so
// they slide against the grid as the camera moves.
const RAISE = 0.12;

// --- Frame geometry -------------------------------------------------------------------------------------

// Where things go in each shape. Portrait is framed tighter and centred, its board a little left of
// the middle to keep clear of the right-hand UI.
function stageFor(width, height, shape) {
  if (shape === "portrait") {
    return { portrait: true, width, height, center: [512, 1000], cell: 104, titleY: 820, title: { cap: 96, zh: 52 }, word: { cap: 128, zh: 62 }, caption: { x: 540, y: 300 } };
  }
  return { portrait: false, width, height, center: [960, 556], cell: 86, titleY: 470, title: { cap: 96, zh: 50 }, word: { cap: 150, zh: 62 }, caption: { x: 960, y: 820 } };
}

// --- Boards on the camera ---------------------------------------------------------------------------------

// A point's distance from its emitter along a segment that passes through it, or -1 if it does not.
function distanceOn(segment, x, y) {
  const [dx, dy] = DIRS[segment.dir];
  const s = dx ? (x - segment.x0) / dx : (y - segment.y0) / dy;
  const steps = dx ? (segment.x1 - segment.x0) / dx : (segment.y1 - segment.y0) / dy;
  if (s < 0 || s > steps || Math.abs(segment.x0 + s * dx - x) > 1e-9 || Math.abs(segment.y0 + s * dy - y) > 1e-9) return -1;
  return segment.d0 + s * (dx && dy ? Math.SQRT2 : 1);
}

// The channels whose drawn beams have reached point (x, y).
function reached(segments, limits, x, y) {
  let mask = 0;
  for (let i = 0; i < segments.length; i += 1) {
    const d = distanceOn(segments[i], x, y);
    if (d >= 0 && limits[i] >= d - 1e-9) mask |= segments[i].mask;
  }
  return mask;
}
const bitCount = (mask) => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1);

// The visible stretch of each segment in picture space, for the dust and the haze: [x0, y0, x1, y1, rgb].
function visibleRuns(layout, segments, from, to, out) {
  for (let i = 0; i < segments.length; i += 1) {
    const seg = segments[i];
    const a = typeof from === "number" ? from : from[i];
    const b = typeof to === "number" ? to : to[i];
    const s0 = Math.max(seg.d0, a);
    const s1 = Math.min(seg.d1, b);
    if (s1 <= s0) continue;
    const k0 = (s0 - seg.d0) / (seg.d1 - seg.d0);
    const k1 = (s1 - seg.d0) / (seg.d1 - seg.d0);
    const [x0, y0] = layout.at(seg.x0 + (seg.x1 - seg.x0) * k0, seg.y0 + (seg.y1 - seg.y0) * k0);
    const [x1, y1] = layout.at(seg.x0 + (seg.x1 - seg.x0) * k1, seg.y0 + (seg.y1 - seg.y0) * k1);
    out.push([x0, y0, x1, y1, CHANNEL_RGB[seg.mask]]);
  }
  return out;
}

// Volumetric light: each visible beam lights the haze around it, drawn into a Light's soft layer
// (`haze`), which blurs it into a wide glow.
function drawHaze(lit, runs, scale, intensity = 1) {
  lit.save();
  lit.lineCap = "round";
  for (const [x0, y0, x1, y1, rgb] of runs) {
    for (const [width, alpha] of [
      [1.6, 0.1],
      [0.6, 0.16],
    ]) {
      lit.strokeStyle = rgba(rgb, alpha * intensity);
      lit.lineWidth = scale * width;
      lit.beginPath();
      lit.moveTo(x0, y0);
      lit.lineTo(x1, y1);
      lit.stroke();
    }
  }
  lit.restore();
}

// Draws a playback (board.mjs) through camera `cam` at story time `tau`: the grid and glass tiles
// on the plane, pieces and beams floating just above it, beam fronts that grow and drain as the
// board changes, glints where they have reached, emitters with lens streaks, targets that light
// with a burst and a flare, a piece turning, and a dragged piece lifted in the hand. `ambient`
// (0–1) is how lit the bodies are (they come up as the first beam ignites). Pushes picture-space
// beam runs onto `runs` and the cursor (if any) onto `over`.
function drawPlay(ctx, lit, cam, tau, { play, board, moves = [], ambient = 1, runs, over, haze }) {
  const state = stateAt(play.states, tau);
  const pieces = state ? state.board.pieces : board.pieces;
  // Beams are lasers: dollying in widens them only as the square root of the zoom, so close-ups
  // keep thin, hot lines instead of blooming into slabs.
  const scale = cam.base ? cam.base * Math.sqrt(cam.scale / cam.base) : cam.scale;
  const beamLayout = { cell: scale, at: (x, y) => cam.project(x, y, RAISE) };
  drawPlaneGrid(ctx, cam, board, ambient);

  // The latest turn or drop of the piece at (x, y), for its spring.
  const recent = (piece, kind) => {
    let found = null;
    for (const move of moves) {
      if (kind === "rotate" && move.kind === "rotate" && move.at[0] === piece.x && move.at[1] === piece.y && move.t <= tau) found = move;
      if (kind === "drop" && move.kind === "drag" && move.to[0] === piece.x && move.to[1] === piece.y && move.t + move.dur <= tau) found = move;
    }
    return found;
  };
  const shown = (piece) => {
    const turn = recent(piece, "rotate");
    const drop = recent(piece, "drop");
    let out = piece;
    if (turn && tau - turn.t < 1) out = { ...out, rotation: (-(turn.by ?? 1) * Math.PI) / 4 * (1 - ease.spring(tau - turn.t, 9, 1.6)) };
    if (drop && tau - (drop.t + drop.dur) < 0.5) out = { ...out, lift: 1 - ease.outExpo(ramp(tau, drop.t + drop.dur, drop.t + drop.dur + 0.3)) };
    return out;
  };
  const flat = { cell: FLAT };
  for (const piece of pieces) {
    onPlane(ctx, cam, piece.x, piece.y, 0, FLAT, (at) => drawTile(ctx, { cell: FLAT, at: () => at }, 0, 0, { highlight: piece.movable ? 1 : 0, alpha: 0.35 + 0.65 * ambient }));
  }
  for (const piece of pieces) {
    const lift = shown(piece).lift ?? 0;
    onPlane(ctx, cam, piece.x, piece.y, RAISE + lift * 0.5, FLAT, (at) => drawPiece(ctx, flat, shown(piece), { at, alpha: 0.45 + 0.55 * ambient }));
  }

  // Beams: the new trace up to its fronts, the old one's draining tails, and the haze they light.
  const emitters = pieces.filter((piece) => piece.kind === "emitter");
  let limits = [];
  if (state) {
    const dt = tau - state.t;
    const { fresh, stale, grow, drain } = state.change;
    const after = state.traced.segments;
    limits = after.map((segment, i) => (fresh[i] === Infinity ? Infinity : Math.max(fresh[i], grow[segment.mask] + BEAM_SPEED * dt)));
    const old = state.before.segments;
    const tails = old.map((segment, i) => (stale[i] === Infinity ? Infinity : Math.max(stale[i], drain[segment.mask] + BEAM_SPEED * dt)));
    const kept = old.map((_, i) => (stale[i] === Infinity ? -Infinity : Infinity));
    visibleRuns(beamLayout, after, 0, limits, runs);
    visibleRuns(beamLayout, old, tails, kept, runs);
    if (haze) drawHaze(haze, runs, scale);
    drawBeams(lit, beamLayout, after, { progress: limits, emitters });
    drawBeams(lit, beamLayout, old, { from: tails, progress: kept });
    const points = [];
    for (const bounce of state.traced.bounces) {
      const mask = reached(after, limits, bounce.x, bounce.y) & bounce.mask;
      if (mask) points.push({ x: bounce.x, y: bounce.y, mask });
    }
    drawGlints(lit, beamLayout, points);
    points.length = 0;
    for (const crossing of state.traced.crossings) {
      const mask = reached(after, limits, crossing.x, crossing.y);
      if (bitCount(mask) > 1) points.push({ x: crossing.x, y: crossing.y, mask });
    }
    drawGlints(lit, beamLayout, points, { radius: 0.3 });
  }

  // Piece lights: emitters with a lens streak (a big one as they ignite), mirrors, and targets as
  // their colours arrive, with a burst and an anamorphic flare when they light.
  const start = play.states[0].t;
  const targets = new Map(play.targets.map((target) => [`${target.x},${target.y}`, target]));
  for (const piece of pieces) {
    if (piece.kind === "emitter") {
      const on = ramp(tau, start - 0.04, start + 0.02);
      onPlane(lit, cam, piece.x, piece.y, RAISE, FLAT, (at) => drawPieceLight(lit, flat, piece, { at, on: 0.25 + 0.75 * on }));
      if (on > 0) {
        const [dx, dy] = DIRS[piece.dir];
        const [ax, ay] = cam.project(piece.x + dx * 0.25, piece.y + dy * 0.25, RAISE);
        const ignite = Math.exp(-Math.max(0, tau - start) * 2.6);
        drawFlare(lit, ax, ay, { length: scale * (1.6 + 7 * ignite), thickness: scale * (0.03 + 0.05 * ignite), intensity: 0.45 + 0.55 * ignite });
        if (ignite > 0.05) drawSparkle(lit, ax, ay, scale * 1.4 * ignite, ignite);
      }
      continue;
    }
    if (piece.kind !== "target") {
      onPlane(lit, cam, piece.x, piece.y, RAISE, FLAT, (at) => drawPieceLight(lit, flat, shown(piece), { at }));
      continue;
    }
    const { mask, litAt } = targetAt(targets.get(`${piece.x},${piece.y}`), tau);
    const fill = litAt === null ? 0 : ramp(tau, litAt, litAt + 0.35);
    onPlane(lit, cam, piece.x, piece.y, RAISE, FLAT, (at) => drawPieceLight(lit, flat, piece, { at, state: { mask, lit: litAt !== null }, fill }));
    if (litAt !== null) {
      const age = tau - litAt;
      const [px, py] = cam.project(piece.x, piece.y, RAISE);
      drawBurst(lit, px, py, { age, mask: piece.color, size: scale * 1.3, seed: 101 + piece.x * 13 + piece.y * 7, count: 24 });
      const flash = Math.min(1, Math.exp(-age * 3.5) + (piece.color === 7 ? 0.45 : 0.18));
      drawFlare(lit, px, py, { length: scale * (2.4 + 6 * Math.exp(-age * 2.5)), thickness: scale * 0.045, intensity: flash, color: [200, 215, 255] });
    }
  }

  // The hand: a dragged piece rises off the board and follows the cursor.
  const hand = cursorAt(moves, beamLayout, tau);
  for (const move of moves) {
    if (move.kind !== "drag" || tau < move.t || tau >= move.t + move.dur || !hand) continue;
    const k = ease.inOutCubic(ramp(tau, move.t, move.t + move.dur));
    const x = move.from[0] + (move.to[0] - move.from[0]) * k;
    const y = move.from[1] + (move.to[1] - move.from[1]) * k;
    const lift = ease.outExpo(ramp(tau, move.t, move.t + 0.15)) * (1 - ease.outExpo(ramp(tau, move.t + move.dur - 0.1, move.t + move.dur)));
    const piece = { ...board.pieces.find((p) => p.x === move.from[0] && p.y === move.from[1]), lift };
    // Its shadow stays on the plane; the piece floats over it.
    onPlane(ctx, cam, x + 0.05, y + 0.1, 0, FLAT, (at) => {
      const shadow = ctx.createRadialGradient(at[0], at[1], 0, at[0], at[1], FLAT * 0.5);
      shadow.addColorStop(0, `rgba(0, 0, 0, ${0.5 * lift})`);
      shadow.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx.fillStyle = shadow;
      ctx.fillRect(at[0] - FLAT, at[1] - FLAT, FLAT * 2, FLAT * 2);
    });
    onPlane(ctx, cam, x, y, RAISE + 0.55 * lift, FLAT, (at) => {
      drawTile(ctx, { cell: FLAT, at: () => at }, 0, 0, { highlight: 1, alpha: 0.75 });
      drawPiece(ctx, flat, piece, { at });
    });
    onPlane(lit, cam, x, y, RAISE + 0.55 * lift, FLAT, (at) => drawPieceLight(lit, flat, piece, { at }));
  }
  if (hand) over.push(() => drawCursor(ctx, hand.x, hand.y, { radius: scale * 0.26, press: hand.press, ripple: hand.ripple, alpha: hand.alpha }));
}

// The board's grid as lines on the plane, fading with `alpha`, and small dots where they cross.
function drawPlaneGrid(ctx, cam, board, alpha) {
  const reach = 0.35;
  ctx.save();
  ctx.strokeStyle = `rgba(150, 170, 255, ${0.1 * (0.4 + 0.6 * alpha)})`;
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  for (let i = 0; i <= board.w; i += 1) {
    const [x0, y0] = cam.project(i - 0.5, -0.5 - reach);
    const [x1, y1] = cam.project(i - 0.5, board.h - 0.5 + reach);
    ctx.moveTo(x0, y0);
    ctx.lineTo(x1, y1);
  }
  for (let j = 0; j <= board.h; j += 1) {
    const [x0, y0] = cam.project(-0.5 - reach, j - 0.5);
    const [x1, y1] = cam.project(board.w - 0.5 + reach, j - 0.5);
    ctx.moveTo(x0, y0);
    ctx.lineTo(x1, y1);
  }
  ctx.stroke();
  ctx.fillStyle = `rgba(190, 205, 255, ${0.24 * (0.4 + 0.6 * alpha)})`;
  for (let i = 0; i <= board.w; i += 1) {
    for (let j = 0; j <= board.h; j += 1) {
      const [x, y] = cam.project(i - 0.5, j - 0.5);
      ctx.beginPath();
      ctx.arc(x, y, Math.max(0.8, cam.scale * 0.018), 0, Math.PI * 2);
      ctx.fill();
    }
  }
  ctx.restore();
}

// Where the cursor is at story time tau for a scene's moves, or null when it is off: it comes in
// from the lower right, presses on each piece (a ripple), drags along a gentle arc or taps to turn,
// glides between moves and drifts away after the last.
function cursorAt(moves, layout, tau) {
  if (!moves.length) return null;
  const spans = moves.map((move) =>
    move.kind === "drag"
      ? { t0: move.t, t1: move.t + move.dur, a: layout.at(...move.from), b: layout.at(...move.to), drag: true }
      : { t0: move.t, t1: move.t, a: layout.at(...move.at), b: layout.at(...move.at), drag: false },
  );
  const off = [layout.cell * 1.4, layout.cell * 1.9];
  const first = spans[0];
  const last = spans[spans.length - 1];
  const enter = first.t0 - 0.55;
  const leave = last.t1 + 0.9;
  if (tau < enter || tau > leave) return null;
  let x;
  let y;
  let alpha = 1;
  const lerp = (p, q, k) => [p[0] + (q[0] - p[0]) * k, p[1] + (q[1] - p[1]) * k];
  if (tau < first.t0) {
    [x, y] = lerp([first.a[0] + off[0], first.a[1] + off[1]], first.a, ease.inOutCubic(ramp(tau, enter, first.t0 - 0.05)));
    alpha = ease.outExpo(ramp(tau, enter, enter + 0.3));
  } else if (tau > last.t1) {
    [x, y] = lerp(last.b, [last.b[0] + off[0], last.b[1] + off[1]], ease.inOutCubic(ramp(tau, last.t1 + 0.25, leave)));
    alpha = 1 - ramp(tau, leave - 0.35, leave);
  } else {
    for (let i = 0; i < spans.length; i += 1) {
      const span = spans[i];
      if (tau >= span.t0 && tau <= span.t1) {
        [x, y] = lerp(span.a, span.b, ease.inOutCubic(ramp(tau, span.t0, span.t1)));
        break;
      }
      const next = spans[i + 1];
      if (next && tau > span.t1 && tau < next.t0) {
        [x, y] = lerp(span.b, next.a, ease.inOutCubic(ramp(tau, span.t1 + 0.15, next.t0 - 0.05)));
        break;
      }
    }
  }
  let press = 0;
  let ripple = -1;
  for (const span of spans) {
    press = Math.max(press, span.drag ? ramp(tau, span.t0 - 0.06, span.t0) * (1 - ramp(tau, span.t1, span.t1 + 0.1)) : ramp(tau, span.t0 - 0.06, span.t0) * (1 - ramp(tau, span.t0 + 0.08, span.t0 + 0.2)));
    if (tau >= span.t0 && tau < span.t0 + 0.55) ripple = (tau - span.t0) / 0.55;
  }
  return { x, y, alpha, press, ripple };
}

// The camera for a shot at frame time t in a shape: the shot's keys (degrees) as a lens.mjs camera.
function shotCamera(shot, t, stage, board) {
  const c = cameraAt(shot.keys, t);
  const cam = camera({ look: c.look, yaw: rad(c.yaw), tilt: rad(c.tilt), dist: c.dist, roll: rad(c.roll), cell: stage.cell * (board?.scale ?? 1), center: stage.center });
  // The cell size at the reference distance, against which beam widths are scaled.
  cam.base = stage.cell;
  return cam;
}

// Depth of field for a camera on a plane: sharp at depth `focus` (cells from the camera), blurring
// with the distance from it along the picture's depth axis, and everywhere by `defocus` (0–1).
function planeFocus(ctx, stage, cam, { focus = cam.dist, defocus = 0, radius }) {
  const [lx, ly] = cam.look;
  const sy = Math.sin(cam.yaw);
  const cy = Math.cos(cam.yaw);
  // Board points along the axis that recedes from the camera: board y turned by the yaw. The ramp
  // runs between their pictures, from far to near (stopping short of the camera), and each stop
  // sits where its point lands on it, since perspective spaces them unevenly.
  const along = (s) => [lx + s * sy, ly + s * cy];
  const st = Math.sin(cam.tilt);
  const far = -8;
  const near = st > 0.02 ? Math.min(8, (cam.dist - 1.6) / st) : 8;
  const from = cam.project(...along(far));
  const to = cam.project(...along(near));
  const dx = to[0] - from[0];
  const dy = to[1] - from[1];
  const length2 = dx * dx + dy * dy;
  if (length2 < 4) {
    depthOfField(ctx, { width: stage.width, height: stage.height, radius, mask: defocus });
    return;
  }
  const stops = [];
  for (let k = 0; k <= 10; k += 1) {
    const s = far + ((near - far) * k) / 10;
    const [px, py] = cam.project(...along(s));
    const offset = ((px - from[0]) * dx + (py - from[1]) * dy) / length2;
    const blur = Math.min(0.85, Math.abs(cam.depth(...along(s)) - focus) / (cam.dist * 0.5));
    stops.push([offset, Math.max(defocus, blur)]);
  }
  depthOfField(ctx, { width: stage.width, height: stage.height, radius, mask: { from, to, stops } });
}

// --- Titles ---------------------------------------------------------------------------------------------

// A scene's caption lines for a shape.
const linesFor = (caption, stage) => (caption.lines ? caption.lines[stage.portrait ? "portrait" : "landscape"] : [caption.en]);

// --- Scenes ---------------------------------------------------------------------------------------------

// A board scene: its shots' cameras, the puzzle in story time, depth of field, dust and haze, and
// in the montage the word that hits over the defocused board before focus racks to it.
function boardScene(ctx, t, scene, stage) {
  const { width, height } = stage;
  const tau = storyTime(scene, t);
  const shot = shotAt(scene, t);
  const board = scene.puzzle.board;
  const cam = shotCamera(shot, t, stage);
  const center = cam.project((board.w - 1) / 2, (board.h - 1) / 2);
  const pan = [(center[0] - stage.center[0]) * 0.25 - cam.yaw * 140, (center[1] - stage.center[1]) * 0.25 - cam.tilt * 60];
  drawAtmosphere(ctx, { width, height, t, pan, haze: 0.9, glow: 0.55, gx: center[0], gy: center[1] });
  const light = lightFor(width, height, "world");
  const lit = light.begin();
  const ambient = ramp(tau, scene.puzzle.start, scene.puzzle.start + 0.6);
  const runs = [];
  const over = [];
  drawPlay(ctx, lit, cam, tau, { play: PLAYS[scene.name], board, moves: scene.puzzle.moves, ambient, runs, over, haze: light.haze() });
  drawDust(lit, { width, height, t, count: stage.portrait ? 60 : 80, seed: 5, pan: [pan[0] * 2, pan[1] * 2], beams: runs, reach: cam.scale * 0.9 });
  light.composite(ctx, COMPOSITE);
  // Focus: on the look point; a montage word throws the board out of focus until it racks back.
  const word = scene.caption?.word ? scene.caption : null;
  const defocus = word ? 1 - ease.inOutCubic(ramp(t, word.at + 0.45, word.at + 0.8)) : 0;
  planeFocus(ctx, stage, cam, { defocus, radius: stage.portrait ? 16 : 14 });
  // The cursor sits on the board's plane of focus: it fades while the board is thrown out.
  ctx.save();
  ctx.globalAlpha = 1 - defocus;
  for (const draw of over) draw();
  ctx.restore();
  if (word && t < word.at + 0.9) drawWord(ctx, t, scene, stage);
}

// A montage word: it slams in over the defocused board (a flash, a little scale-down) and leaves
// as the focus racks to the board.
function drawWord(ctx, t, scene, stage) {
  const { width, height } = stage;
  const { caption } = scene;
  const light = lightFor(width, height, "title");
  const lit = light.begin();
  const hit = t - caption.at;
  const out = 1 - ease.inOutCubic(ramp(t, caption.at + 0.55, caption.at + 0.85));
  const s = 1 + 0.12 * Math.exp(-hit * 9);
  const cx = stage.portrait ? 540 : width / 2;
  const cy = stage.portrait ? 820 : height / 2 - 40;
  const { cap, zh } = stage.word;
  withScale([ctx, lit], cx, cy, s, () => {
    drawTitleCard(ctx, lit, cx, cy - cap * 0.75, { en: [caption.en], zh: caption.zh, cap, zhSize: zh, align: "center", track: 0.3, intensity: out, zhAlpha: out });
  });
  if (hit < 0.08) {
    lit.fillStyle = `rgba(200, 215, 255, ${0.05 * (1 - hit / 0.08)})`;
    lit.fillRect(0, 0, width, height);
  }
  drawFlare(lit, cx, cy, { length: width * 1.4 * Math.exp(-hit * 4), thickness: 4, intensity: Math.exp(-hit * 5) });
  light.composite(ctx, COMPOSITE);
}

// Scales the contexts about (cx, cy) while `draw` runs.
function withScale(contexts, cx, cy, s, draw) {
  for (const c of contexts) {
    c.save();
    c.translate(cx, cy);
    c.scale(s, s);
    c.translate(-cx, -cy);
  }
  draw();
  for (const c of contexts) c.restore();
}

// A title card between shots: near-black and haze, the English written on by a beam whose tip
// flares, the Chinese rising in once the English is done, and a slow push-in throughout.
function titleScene(ctx, t, scene, stage) {
  const { width, height } = stage;
  const { caption } = scene;
  const k = ramp(t, scene.start, scene.end);
  drawAtmosphere(ctx, { width, height, t, pan: [-k * 40, 0], haze: 0.5, glow: 0.35 });
  const light = lightFor(width, height, "title");
  const lit = light.begin();
  const lines = linesFor(caption, stage);
  const { cap, zh } = stage.title;
  const { from, length } = captionWrite(caption);
  const write = ramp(t, from, from + length);
  const rise = ease.outExpo(ramp(t, caption.at + 0.6, caption.at + 0.95));
  const fade = 1 - ramp(t, scene.end - 0.08, scene.end);
  const cx = stage.portrait ? 540 : width / 2;
  const blockHeight = (lines.length - 1) * cap * 1.42 + cap + cap * 0.42 + zh;
  const top = stage.titleY - blockHeight / 2;
  drawDust(lit, { width, height, t, count: 40, seed: 8, pan: [-k * 80, 0] });
  withScale([ctx, lit], cx, stage.titleY, 1 + 0.05 * ease.inOutCubic(k), () => {
    drawTitleCard(ctx, lit, cx, top, { en: lines, zh: caption.zh, cap, zhSize: zh, align: "center", track: 0.28, reveal: write, flare: 1, rise: 20 * (1 - rise), zhAlpha: rise * fade, intensity: fade });
  });
  light.composite(ctx, COMPOSITE);
}

// The intro: near darkness and one drifting spark; it ignites into the red beam with a lens flare,
// the beam bounces off two mirrors and draws the underline; a held breath with dust drifting
// through the beam, then the wordmark hits (a glitch, a flash, a camera punch) and the Chinese title
// follows.
function introScene(ctx, t, scene, stage) {
  const { width, height } = stage;
  const markWidth = stage.portrait ? 820 : 1080;
  const m = wordmarkMetrics(markWidth);
  const cx = stage.portrait ? 540 : width / 2;
  const cy = stage.portrait ? 860 : 505;
  const top = cy - m.height / 2 - m.cap * 0.3;
  const left = cx - markWidth / 2;
  // A slow push-in all through, with a punch on the hit.
  const hit = t - INTRO.impact;
  const punch = hit >= 0 ? 0.035 * Math.exp(-hit * 5) * Math.cos(hit * 18) : 0;
  const s = 1 + 0.06 * ease.inOutCubic(ramp(t, 0, 3)) + punch;
  const drift = [Math.sin(t * 0.6) * 8, (1 - ease.inOutCubic(ramp(t, 0, 3))) * (stage.portrait ? 40 : 18)];
  drawAtmosphere(ctx, { width, height, t, pan: [drift[0] * 2, drift[1]], haze: 0.25 + 0.5 * ramp(t, INTRO.ignite, INTRO.ignite + 0.6), glow: 0.3 * ramp(t, INTRO.ignite, INTRO.impact) });
  const light = lightFor(width, height, "world");
  const lit = light.begin();
  const u = m.cap * 0.62;
  const yA = top - m.cap * 0.95;
  const xA = left - m.cap * 0.85;
  const yU = top + m.underlineY;
  const xEnd = left + m.underlineEnd;
  const start = [xA - (stage.portrait ? 0 : m.cap * 3.2), stage.portrait ? yA - m.cap * 2.6 : yA];
  const points = [start, [xA, yA], [xA, yU], [xEnd, yU]];
  const runs = [];
  lit.save();
  ctx.save();
  for (const c of [ctx, lit]) {
    c.translate(cx + drift[0], cy + drift[1]);
    c.scale(s, s);
    c.translate(-cx, -cy);
  }
  const ignited = t >= INTRO.ignite;
  // The spark: a point of light drifting in, breathing, before it fires.
  const sparkIn = ease.outExpo(ramp(t, INTRO.spark, INTRO.spark + 0.5));
  if (!ignited) {
    const wobble = Math.sin(t * 9) * 0.15 + 0.85;
    const [sx, sy] = [start[0] - (1 - sparkIn) * 60 + Math.sin(t * 3) * 4, start[1] + Math.cos(t * 2.3) * 3];
    drawSparkle(lit, sx, sy, u * 0.9 * wobble, sparkIn * wobble);
    drawFlare(lit, sx, sy, { length: u * 5 * sparkIn, thickness: 2, intensity: 0.5 * sparkIn, color: [255, 120, 130] });
  } else {
    // The beam, as segments in units of u for drawBeams.
    const segments = [];
    let d = 0;
    for (let k = 1; k < points.length; k += 1) {
      const [x0, y0] = points[k - 1];
      const [x1, y1] = points[k];
      const length = Math.hypot(x1 - x0, y1 - y0) / u;
      segments.push({ x0: x0 / u, y0: y0 / u, x1: x1 / u, y1: y1 / u, dir: x1 > x0 ? 0 : 2, mask: 1, d0: d, d1: d + length });
      d += length;
    }
    const layout = { cell: u, at: (x, y) => [x * u, y * u] };
    const front = ((t - INTRO.ignite) * INTRO.speed) / u;
    const gone = ramp(t, INTRO.impact + 0.05, INTRO.impact + 0.5);
    // The beam hums while the frame holds its breath.
    const hum = 0.9 + 0.1 * Math.sin(t * 41) * Math.sin(t * 17);
    if (gone < 1) {
      visibleRuns(layout, segments, 0, front, runs);
      drawHaze(light.haze(), runs, u, 1 - gone);
      drawBeams(lit, layout, segments, { progress: front, intensity: (1 - gone) * hum });
    }
    const age = t - INTRO.ignite;
    drawFlare(lit, start[0], start[1], { length: width * (0.25 + 1.1 * Math.exp(-age * 2.2)), thickness: 3 + 6 * Math.exp(-age * 3), intensity: (1 - gone) * (0.5 + 0.5 * Math.exp(-age * 2)), color: [255, 150, 160] });
    drawSparkle(lit, start[0], start[1], u * (0.8 + 2.4 * Math.exp(-age * 4)), 1 - gone);
    for (const [k, [x, y]] of [points[1], points[2]].entries()) {
      const mirror = { kind: "mirror", angle: 1 };
      const mirrorLayout = { cell: u * 1.5 };
      const shown = (1 - gone) * ease.outExpo(ramp(t, 0.3, 1.2));
      if (shown > 0) {
        drawPiece(ctx, mirrorLayout, mirror, { at: [x, y], alpha: shown });
        drawPieceLight(lit, mirrorLayout, mirror, { at: [x, y] });
      }
      const at = INTRO.ignite + (segments[k].d1 * u) / INTRO.speed;
      if (t >= at) drawSparkle(lit, x, y, u * 1.6 * (0.35 + 0.65 * Math.exp(-(t - at) * 5)), (1 - gone) * (0.4 + 0.6 * Math.exp(-(t - at) * 4)));
    }
  }
  // The wordmark: its bands jump sideways and flicker for a moment, then hold.
  if (hit >= 0) {
    const glitch = 1 - ramp(t, INTRO.impact, INTRO.impact + 0.38);
    drawGlitched(lit, t, glitch, left, top, markWidth, m, (intensity) => drawWordmark(lit, left, top, { width: markWidth, intensity }));
    const [ox, oy] = [left + m.cap * 3.4, top + m.cap / 2];
    drawSparkle(lit, ox, oy, m.cap * 2.6 * Math.exp(-hit * 3), Math.exp(-hit * 2.5));
  }
  const zhIn = ease.outExpo(ramp(t, INTRO.zh, INTRO.zh + 0.6));
  if (zhIn > 0) drawZhLine(ctx, lit, scene.caption.zh, cx, top + m.height + m.cap * 0.95 + 16 * (1 - zhIn), { size: stage.portrait ? 48 : 44, align: "center", alpha: zhIn, spacing: 0.12 });
  ctx.restore();
  lit.restore();
  drawDust(lit, { width, height, t, count: 70, seed: 3, pan: [drift[0] * 3, drift[1] * 2], beams: runs, reach: u * 1.4 });
  if (hit >= 0 && hit < 0.12) {
    lit.fillStyle = `rgba(210, 220, 255, ${0.08 * (1 - hit / 0.12)})`;
    lit.fillRect(0, 0, width, height);
  }
  if (hit >= 0) drawFlare(lit, cx, top + m.cap / 2, { length: width * 1.5 * Math.exp(-hit * 2.5), thickness: 5, intensity: Math.exp(-hit * 3) });
  light.composite(ctx, COMPOSITE);
}

// Draws `draw` (which draws the wordmark at an intensity) in horizontal bands that jump sideways
// and flicker by `glitch` (0–1), seeded by the frame time, so it is the same on every render.
function drawGlitched(lit, t, glitch, left, top, width, m, draw) {
  if (glitch <= 0) {
    draw(1);
    return;
  }
  const bands = 7;
  const tall = m.height + m.cap * 0.4;
  const step = Math.floor(t * 30);
  for (let b = 0; b < bands; b += 1) {
    const jump = hash(step * 31 + b * 7 + 3) < 0.55 ? (hash(step * 17 + b) - 0.5) * m.cap * 1.4 * glitch : 0;
    const flicker = hash(step * 5 + b * 11) < 0.3 * glitch ? 0.25 : 1;
    lit.save();
    lit.beginPath();
    lit.rect(left - m.cap * 2, top - m.cap * 0.2 + (b * tall) / bands, width + m.cap * 4, tall / bands + 0.5);
    lit.clip();
    lit.translate(jump, 0);
    draw(flicker);
    lit.restore();
  }
}

// A number in [0, 1) from an integer: a pure stand-in for a PRNG draw.
function hash(n) {
  let h = (n | 0) ^ 0x9e3779b9;
  h = Math.imul(h ^ (h >>> 16), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

// Facts: four game cards drop onto a plane the camera orbits down to, each writing its numeral,
// then the counter rolls to 200 in the caption.
function factsScene(ctx, t, scene, stage) {
  const { width, height, portrait } = stage;
  const shot = scene.shots[0];
  const c = cameraAt(shot.keys, t);
  const cam = camera({ look: c.look, yaw: rad(c.yaw), tilt: rad(c.tilt), dist: c.dist, roll: rad(c.roll), cell: portrait ? 150 : 152, center: portrait ? [520, 1010] : [960, 440] });
  drawAtmosphere(ctx, { width, height, t, pan: [-cam.yaw * 160, -cam.tilt * 60], haze: 0.8, glow: 0.45, gy: portrait ? 1000 : 440 });
  const light = lightFor(width, height, "world");
  const lit = light.begin();
  const numerals = ["I", "II", "III", "IV"];
  const accents = [1, 2, 4, 7];
  const card = portrait ? { w: 3.3, h: 2.4, gx: 3.65, gy: 2.75 } : { w: 2.4, h: 3, gx: 2.85, gy: 0 };
  numerals.forEach((numeral, i) => {
    const land = FACTS.cards[i];
    if (t < land - 0.35) return;
    const k = ease.spring(Math.max(0, t - land + 0.35), 7, 1.3);
    const x = portrait ? (i % 2 ? 0.5 : -0.5) * card.gx : (i - 1.5) * card.gx;
    const y = portrait ? (i < 2 ? -0.5 : 0.5) * card.gy : 0;
    const h = 2.2 * (1 - clamp(k, 0, 1.2));
    const alpha = ramp(t, land - 0.35, land - 0.12);
    drawPlaneCard(ctx, lit, cam, x, y, h, card, { numeral, accent: accents[i], alpha, write: ramp(t, land - 0.1, land + 0.4) });
  });
  drawDust(lit, { width, height, t, count: 60, seed: 6, pan: [-cam.yaw * 300, 0] });
  light.composite(ctx, COMPOSITE);
  planeFocus(ctx, stage, cam, { radius: 12 });
  drawFactsCaption(ctx, t, scene, stage);
}

// A game card lying on the plane at (x, y), `h` cells above it: glass, an accent beam along its top
// edge, its numeral written by a beam and its level count in both languages.
function drawPlaneCard(ctx, lit, cam, x, y, h, card, { numeral, accent, alpha, write }) {
  const w = card.w * FLAT;
  const tall = card.h * FLAT;
  onPlane(ctx, cam, x, y, h, FLAT, ([px, py]) => {
    ctx.save();
    ctx.globalAlpha = alpha;
    const left = px - w / 2;
    const top = py - tall / 2;
    const fill = ctx.createLinearGradient(left, top, left + w * 0.4, top + tall);
    fill.addColorStop(0, "rgba(160, 182, 255, 0.13)");
    fill.addColorStop(1, "rgba(110, 130, 220, 0.04)");
    ctx.beginPath();
    ctx.roundRect(left, top, w, tall, 18);
    ctx.fillStyle = fill;
    ctx.fill();
    ctx.strokeStyle = "rgba(220, 230, 255, 0.32)";
    ctx.lineWidth = 2;
    ctx.stroke();
    ctx.textAlign = "center";
    ctx.globalAlpha = alpha * ease.outExpo(ramp(write, 0.5, 1));
    ctx.fillStyle = "rgba(214, 224, 255, 0.95)";
    ctx.font = `600 22px ${FONTS.en}`;
    ctx.letterSpacing = "2px";
    ctx.fillText("50 LEVELS", px, top + tall - 52);
    ctx.font = `500 22px ${FONTS.zh}`;
    ctx.fillStyle = "rgba(190, 202, 240, 0.9)";
    ctx.fillText("50 个关卡", px, top + tall - 20);
    ctx.restore();
  });
  onPlane(lit, cam, x, y, h, FLAT, ([px, py]) => {
    const top = py - tall / 2;
    const reach = ease.outExpo(write);
    lit.save();
    lit.lineCap = "round";
    lit.beginPath();
    lit.moveTo(px - w / 2 + 22, top + 1);
    lit.lineTo(px - w / 2 + 22 + (w - 44) * reach, top + 1);
    for (const channel of [1, 2, 4]) {
      if (!(accent & channel)) continue;
      lit.strokeStyle = rgba(CHANNEL_RGB[channel], 0.9 * alpha);
      lit.lineWidth = 5;
      lit.stroke();
    }
    lit.strokeStyle = `rgba(255,255,255,${0.85 * alpha})`;
    lit.lineWidth = 1.6;
    lit.stroke();
    lit.restore();
    const cap = Math.min(tall * 0.42, 118);
    drawLettering(lit, numeral, px, top + tall * 0.2, { cap, align: "center", track: 0.34, reveal: write, intensity: alpha, flare: 0.6 });
  });
}

// The facts caption: "4 GAMES" first, then "· 200 LEVELS" written as the counter rolls; the Chinese
// line counts with it.
function drawFactsCaption(ctx, t, scene, stage) {
  const { width, height, portrait } = stage;
  const light = lightFor(width, height, "title");
  const lit = light.begin();
  const lines = linesFor(scene.caption, stage);
  const cap = portrait ? 78 : 66;
  const x = stage.caption.x;
  const y = portrait ? stage.caption.y : 822;
  const fade = 1 - ramp(t, scene.end - 0.08, scene.end);
  const value = 200 * outCubic(ramp(t, FACTS.count, FACTS.counted));
  const track = 0.28;
  const full = lines.join(" ");
  const placements = portrait
    ? [
        { text: lines[0], dx: 0, dy: 0, width: layoutLine(lines[0], { track }).width },
        { text: lines[1], dx: 0, dy: cap * 1.42, width: layoutLine(lines[1], { track }).width },
      ]
    : (() => {
        const whole = layoutLine(full, { track });
        const split = [...full].indexOf("·");
        return [
          { text: "4 GAMES", dx: 0, dy: 0, width: whole.width },
          { text: "· 200 LEVELS", dx: whole.glyphs[split].x, dy: 0, width: whole.width },
        ];
      })();
  const { from, length } = captionWrite(scene.caption);
  const starts = [from, FACTS.count];
  // Alone, "4 GAMES" sits centred; it slides left to make room as the rest writes in.
  const settle = ease.inOutCubic(ramp(t, FACTS.count - 0.15, FACTS.count + 0.45));
  const alone = portrait ? 0 : ((1 - settle) * (placements[0].width - layoutLine("4 GAMES", { track }).width) * cap) / 2;
  placements.forEach((part, k) => {
    const write = ramp(t, starts[k], starts[k] + length);
    if (write <= 0) return;
    const left = x - (part.width * cap) / 2 + part.dx * cap + alone;
    // Only the second part's number is the counter; the 4 of "4 GAMES" stays put.
    const digits = [];
    if (k === 1) [...part.text].forEach((ch, i) => /[0-9]/.test(ch) && digits.push(i));
    const box = drawLettering(lit, part.text, left, y + part.dy, { cap, track, reveal: write, skip: digits, intensity: fade, flare: 0.8 });
    // The counter: odometer wheels, the ones spinning fastest.
    digits.forEach((i, place) => drawWheel(lit, box.glyphs[i].left, y + part.dy, cap, value / 10 ** (digits.length - 1 - place), write * fade));
  });
  // The counter lands with a flash.
  const landed = t - FACTS.counted;
  if (landed >= 0) drawFlare(lit, x, y + (portrait ? cap * 1.92 : cap / 2), { length: width * 1.2 * Math.exp(-landed * 3), thickness: 4, intensity: Math.exp(-landed * 3) });
  const zhSize = portrait ? 46 : 40;
  const zhY = y + (lines.length - 1) * cap * 1.42 + cap + cap * 0.5 + zhSize * 0.82;
  const zhIn = ease.outExpo(ramp(t, scene.caption.at + 0.6, scene.caption.at + 1.1));
  const zhMore = ease.outExpo(ramp(t, FACTS.count + 0.5, FACTS.count + 1));
  drawCountedZh(ctx, lit, x, zhY, { size: zhSize, value: Math.round(value), settle, alpha: [zhIn * fade, zhMore * fade], rise: [16 * (1 - zhIn), 16 * (1 - zhMore)] });
  light.composite(ctx, COMPOSITE);
}

// One odometer wheel of the counter: the digit at `wheel` (its integer part, mod 10) rolling up to
// the next, clipped to its slot.
function drawWheel(lit, left, top, cap, wheel, intensity) {
  if (intensity <= 0) return;
  const digit = Math.floor(wheel) % 10;
  const frac = wheel - Math.floor(wheel);
  const pitch = cap * 1.35;
  lit.save();
  lit.beginPath();
  lit.rect(left - cap * 0.3, top - cap * 0.18, cap * 1.3, cap * 1.36);
  lit.clip();
  for (const [d, dy] of [
    [digit, -frac * pitch],
    [(digit + 1) % 10, (1 - frac) * pitch],
  ]) {
    if (Math.abs(dy) >= pitch) continue;
    const g = layoutLine(String(d)).width;
    drawLettering(lit, String(d), left + ((0.7 - g) * cap) / 2, top + dy, { cap, intensity, glints: false });
  }
  lit.restore();
}

// "4 部经典 · 200 个关卡", typeset and centred on x: "4 部经典" alone (centred until `settle` slides
// it into place), then "· ### 个关卡" with its number right-aligned in a box as wide as "200", so
// the line does not shift as it counts.
function drawCountedZh(ctx, lit, x, baseline, { size, value, settle, alpha, rise }) {
  const spacing = 0.08 * size;
  ctx.save();
  ctx.font = `600 ${size}px ${FONTS.zh}`;
  ctx.letterSpacing = `${spacing}px`;
  const measure = (s) => ctx.measureText(s).width;
  const head = "4 部经典";
  const middle = " · ";
  const tail = " 个关卡";
  const full = measure(head) + measure(middle) + measure("200") + measure(tail) - spacing;
  const headWidth = measure(head) - spacing;
  const dotLeft = measure(head);
  const numberRight = measure(head) + measure(middle) + measure("200") - spacing;
  const space = measure(" ");
  ctx.restore();
  const left = x - full / 2 + ((1 - settle) * (full - headWidth)) / 2;
  if (alpha[0] > 0) drawZhLine(ctx, lit, head, left, baseline + rise[0], { size, alpha: alpha[0] });
  if (alpha[1] > 0) {
    drawZhLine(ctx, lit, "·", left + dotLeft + space, baseline + rise[1], { size, alpha: alpha[1] });
    drawZhLine(ctx, lit, String(value), left + numberRight, baseline + rise[1], { size, alpha: alpha[1], align: "right" });
    drawZhLine(ctx, lit, tail.trim(), left + numberRight + spacing + space, baseline + rise[1], { size, alpha: alpha[1] });
  }
}

// A rounded rectangle as a closed polyline (corner arcs in short steps), for drawing on.
function roundedPoints(x, y, w, h, r) {
  const pts = [];
  const corner = (cx, cy, a0) => {
    for (let k = 0; k <= 6; k += 1) {
      const a = a0 + (k / 6) * (Math.PI / 2);
      pts.push([cx + Math.cos(a) * r, cy + Math.sin(a) * r]);
    }
  };
  corner(x + w - r, y + r, -Math.PI / 2);
  corner(x + w - r, y + h - r, 0);
  corner(x + r, y + h - r, Math.PI / 2);
  corner(x + r, y + r, Math.PI);
  pts.push(pts[0]);
  return pts;
}

// Strokes the first `frac` of a polyline as a beam (the lettering's look), with a hot, flaring tip
// while it draws.
function drawOutline(lit, pts, frac, t, intensity) {
  if (frac <= 0) return;
  let total = 0;
  for (let k = 1; k < pts.length; k += 1) total += Math.hypot(pts[k][0] - pts[k - 1][0], pts[k][1] - pts[k - 1][1]);
  let left = total * clamp(frac);
  const path = new Path2D();
  path.moveTo(pts[0][0], pts[0][1]);
  let tip = pts[0];
  for (let k = 1; k < pts.length && left > 0; k += 1) {
    const [ax, ay] = pts[k - 1];
    const [bx, by] = pts[k];
    const length = Math.hypot(bx - ax, by - ay);
    const f = Math.min(1, left / length);
    tip = [ax + (bx - ax) * f, ay + (by - ay) * f];
    path.lineTo(tip[0], tip[1]);
    left -= length;
  }
  drawBeamPath(lit, path, t, { intensity, strand: 0.7 });
  if (frac < 1) {
    drawSparkle(lit, tip[0], tip[1], t * 10, intensity);
    drawFlare(lit, tip[0], tip[1], { length: t * 90, thickness: t, intensity: 0.7 * intensity });
  }
}

// The plays shown on the platform scene's screens: the solved "mix" and "solve" boards, lighting up
// as the screens come on.
const solvedBoard = (name) => PLAYS[name].states[PLAYS[name].states.length - 1].board;
const SCREENS = {
  laptop: { board: solvedBoard("mix"), play: playback(solvedBoard("mix"), [], { start: PLATFORMS.screens + 0.1, speed: BEAM_SPEED }) },
  phone: { board: solvedBoard("solve"), play: playback(solvedBoard("solve"), [], { start: PLATFORMS.screens + 0.25, speed: BEAM_SPEED }) },
};

// Platforms: a laptop running the Mac app and a phone in the browser, outlined in beams, each with a
// lit board, at two depths so they slide against each other as the camera trucks and dollies;
// focus racks from the laptop to the phone. The caption sits below (landscape) or above (portrait).
function platformsScene(ctx, t, scene, stage) {
  const { width, height, portrait } = stage;
  const k = ease.inOutCubic(ramp(t, scene.start, scene.end));
  const truck = (0.5 - k) * (portrait ? 36 : 120);
  const dolly = 0.05 * k;
  drawAtmosphere(ctx, { width, height, t, pan: [truck * 0.4, 0], haze: 0.8, glow: 0.45, gy: portrait ? 1050 : 430 });
  const light = lightFor(width, height, "world");
  const lit = light.begin();
  // Side by side, never overlapping: the outlines are light, so nothing in front can hide them.
  const laptop = portrait ? { x: 96, y: 800, w: 520, h: 325 } : { x: 330, y: 136, w: 820, h: 512 };
  const phone = portrait ? { x: 690, y: 700, w: 232, h: 480 } : { x: 1320, y: 196, w: 254, h: 528 };
  const draw = ease.inOutCubic(ramp(t, PLATFORMS.outline, PLATFORMS.outline + 0.7));
  const screens = ramp(t, PLATFORMS.screens, PLATFORMS.screens + 0.4);
  const focus = portrait ? [540, 1060] : [width / 2, 430];
  // The laptop is further away than the phone: it moves and grows less.
  const layer = (depth, draw) => withScale([ctx, lit], focus[0], focus[1], 1 + dolly / depth, () => {
    ctx.translate(truck / depth, 0);
    lit.translate(truck / depth, 0);
    draw();
  });
  layer(1.3, () => {
    drawScreen(ctx, lit, t, laptop, { radius: 18, inset: 14, bar: 34, screens, which: "laptop" });
    drawOutline(lit, roundedPoints(laptop.x, laptop.y, laptop.w, laptop.h, 18), draw, 2.4, 0.7);
    const base = { x: laptop.x - laptop.w * 0.09, y: laptop.y + laptop.h + 4, w: laptop.w * 1.18, h: 20 };
    drawOutline(lit, roundedPoints(base.x, base.y, base.w, base.h, 9), ramp(draw, 0.3, 1), 2.4, 0.7);
  });
  layer(0.8, () => {
    ctx.save();
    ctx.fillStyle = "#05060a";
    ctx.beginPath();
    ctx.roundRect(phone.x - 6, phone.y - 6, phone.w + 12, phone.h + 12, 46);
    ctx.globalAlpha = clamp(draw * 3);
    ctx.fill();
    ctx.restore();
    drawScreen(ctx, lit, t, phone, { radius: 38, inset: 12, bar: 0, screens, which: "phone" });
    drawOutline(lit, roundedPoints(phone.x, phone.y, phone.w, phone.h, 40), ramp(draw, 0.15, 1), 2.4, 0.7);
  });
  drawDust(lit, { width, height, t, count: 60, seed: 4, pan: [truck * 1.6, 0] });
  light.composite(ctx, COMPOSITE);
  // Rack focus: the laptop first, then the phone.
  const rack = ease.inOutCubic(ramp(t, 25.8, 26.6));
  const a = [laptop.x + laptop.w / 2, laptop.y + laptop.h / 2];
  const b = [phone.x + phone.w / 2, phone.y + phone.h / 2];
  depthOfField(ctx, {
    width,
    height,
    radius: 10,
    mask: {
      from: a,
      to: b,
      stops: [
        [0, 0.7 * rack],
        [0.45, 0.35 * rack + 0.35 * (1 - rack)],
        [1, 0.75 * (1 - rack)],
      ],
    },
  });
  // The caption, sharp over it all.
  const title = lightFor(width, height, "title");
  const tl = title.begin();
  const lines = linesFor(scene.caption, stage);
  const { from, length } = captionWrite(scene.caption);
  const write = ramp(t, from, from + length);
  const rise = ease.outExpo(ramp(t, scene.caption.at + 0.6, scene.caption.at + 1.1));
  const fade = 1 - ramp(t, scene.end - 0.08, scene.end);
  const common = { en: lines, zh: scene.caption.zh, sub: scene.caption.sub, align: "center", track: 0.28, reveal: write, flare: 0.8, rise: 18 * (1 - rise), zhAlpha: rise * fade, subAlpha: rise * fade, intensity: fade };
  if (portrait) drawTitleCard(ctx, tl, 540, 250, { ...common, cap: 60, zhSize: 40 });
  else drawTitleCard(ctx, tl, width / 2, 790, { ...common, cap: 48, zhSize: 34 });
  title.composite(ctx, COMPOSITE);
}

// A device's screen: dark glass, a window bar (laptop) or a URL bar (phone), and its board.
function drawScreen(ctx, lit, t, box, { radius, inset, bar, screens, which }) {
  const { x, y, w, h } = box;
  ctx.save();
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, radius);
  const glass = ctx.createLinearGradient(x, y, x + w * 0.3, y + h);
  glass.addColorStop(0, `rgba(24, 30, 54, ${0.55 * screens + 0.2})`);
  glass.addColorStop(1, `rgba(8, 10, 18, ${0.6 * screens + 0.2})`);
  ctx.fillStyle = glass;
  ctx.fill();
  ctx.restore();
  if (screens <= 0) return;
  ctx.save();
  ctx.globalAlpha = screens;
  if (bar) {
    [0, 1, 2].forEach((k) => {
      ctx.fillStyle = ["rgba(255, 95, 87, 0.9)", "rgba(254, 188, 46, 0.9)", "rgba(40, 200, 64, 0.9)"][k];
      ctx.beginPath();
      ctx.arc(x + inset + 10 + k * 22, y + bar / 2 + 4, 6, 0, Math.PI * 2);
      ctx.fill();
    });
    ctx.fillStyle = "rgba(210, 220, 250, 0.7)";
    ctx.font = `600 15px ${FONTS.en}`;
    ctx.textAlign = "center";
    ctx.fillText("Chromatron", x + w / 2, y + bar / 2 + 9);
  } else {
    ctx.fillStyle = "#020306";
    ctx.beginPath();
    ctx.roundRect(x + w / 2 - 38, y + 14, 76, 22, 11);
    ctx.fill();
    ctx.fillStyle = "rgba(160, 180, 240, 0.14)";
    ctx.beginPath();
    ctx.roundRect(x + 16, y + h - 54, w - 32, 32, 16);
    ctx.fill();
    ctx.fillStyle = "rgba(220, 228, 255, 0.85)";
    ctx.font = `500 13px ${FONTS.en}`;
    ctx.textAlign = "center";
    ctx.fillText("shengtuo.me/chromatron-remake", x + w / 2, y + h - 33);
  }
  ctx.restore();
  const screen = SCREENS[which];
  const top = y + (bar ? bar + 10 : 46);
  const bottom = y + h - (bar ? inset : 64);
  const cell = Math.min((w - 2 * inset) / screen.board.w, (bottom - top) / screen.board.h);
  // A flat, straight-on camera onto the screen's board.
  const cam = camera({ look: [(screen.board.w - 1) / 2, (screen.board.h - 1) / 2], cell, ref: 9, dist: 9, center: [x + w / 2, (top + bottom) / 2] });
  drawPlay(ctx, lit, cam, t, { play: screen.play, board: screen.board, ambient: screens, runs: [], over: [] });
}

// The end card: the mark's three beams converge, the white beam writes the wordmark (its tip
// flaring), a final flash and flare, then the Chinese title, the URL and the credits settle in, all
// on a slow pull-back.
function endScene(ctx, t, scene, stage) {
  const { width, height, portrait } = stage;
  const cx = width / 2;
  const markWidth = portrait ? 800 : 1040;
  const m = wordmarkMetrics(markWidth);
  const markY = portrait ? 520 : 236;
  const markSize = portrait ? 190 : 170;
  const top = portrait ? 690 : 372;
  drawAtmosphere(ctx, { width, height, t, pan: [0, 0], haze: 0.5, glow: 0.4 * ramp(t, END.hit - 0.2, END.hit + 0.5), gy: top });
  const light = lightFor(width, height, "world");
  const lit = light.begin();
  const s = 1.06 - 0.06 * ease.outExpo(ramp(t, scene.start, scene.end + 0.4));
  const hit = t - END.hit;
  const punch = hit >= 0 ? 0.02 * Math.exp(-hit * 6) * Math.cos(hit * 20) : 0;
  const cores = [];
  withScale([ctx, lit], cx, top, s + punch, () => {
    const converge = ease.outExpo(ramp(t, END.converge, END.converge + 0.4));
    drawMark(lit, cx, markY, { size: markSize, progress: converge });
    const write = ramp(t, END.write, END.hit);
    drawWordmark(lit, cx - markWidth / 2, top, { width: markWidth, reveal: write, underline: ease.outExpo(ramp(t, END.hit - 0.2, END.hit + 0.3)) });
    // The writing beam: from the mark's core to the letter being written.
    if (write > 0 && write < 1) {
      const x = cx - markWidth / 2 + markWidth * write;
      drawFlare(lit, x, top + m.cap / 2, { length: m.cap * 3, thickness: 3, intensity: 0.8 });
    }
    if (hit >= 0) {
      drawBurst(lit, cx + markSize * 0.08, markY, { age: hit, mask: 7, size: markSize * 1.1, seed: 77, count: 30 });
    }
    const zhIn = ease.outExpo(ramp(t, END.zh, END.zh + 0.5));
    drawZhLine(ctx, lit, scene.caption.zh, cx, top + m.height + m.cap * 0.9 + 14 * (1 - zhIn), { size: portrait ? 44 : 40, align: "center", alpha: zhIn, spacing: 0.12 });
    cores.push([ctx.getTransform(), converge]);
  });
  if (hit >= 0 && hit < 0.15) {
    lit.fillStyle = `rgba(215, 225, 255, ${0.1 * (1 - hit / 0.15)})`;
    lit.fillRect(0, 0, width, height);
  }
  if (hit >= 0) drawFlare(lit, cx, top + m.cap / 2, { length: width * 1.6 * Math.exp(-hit * 2) + 200, thickness: 5, intensity: 0.35 + 0.65 * Math.exp(-hit * 3) });
  drawDust(lit, { width, height, t, count: 50, seed: 2 });
  light.composite(ctx, COMPOSITE);
  const [transform, converge] = cores[0];
  ctx.save();
  ctx.setTransform(transform);
  drawMarkCore(ctx, cx, markY, { size: markSize, alpha: converge });
  ctx.restore();
  // The URL and the credits, sharp and small, in both languages.
  const zhY = top + m.height + m.cap * 0.9;
  const urlIn = ease.outExpo(ramp(t, END.url, END.url + 0.5));
  ctx.save();
  ctx.globalAlpha = urlIn;
  ctx.font = `600 ${portrait ? 40 : 36}px ${FONTS.en}`;
  ctx.letterSpacing = "1px";
  ctx.textAlign = "center";
  ctx.fillStyle = "#ffffff";
  ctx.fillText(END_TEXT.url, cx, zhY + (portrait ? 96 : 82) + 12 * (1 - urlIn));
  ctx.restore();
  const creditIn = ease.outExpo(ramp(t, END.credit, END.credit + 0.6));
  const rows = portrait
    ? [
        [END_TEXT.credit.en, 26, 0.9],
        [END_TEXT.credit.zh, 26, 0.9],
        [END_TEXT.notice.en, 21, 0.62],
        [END_TEXT.notice.zh, 21, 0.62],
      ]
    : [
        [`${END_TEXT.credit.en}  ·  ${END_TEXT.credit.zh}`, 24, 0.9],
        [`${END_TEXT.notice.en}  ·  ${END_TEXT.notice.zh}`, 18, 0.62],
      ];
  let y = portrait ? 1250 : 905;
  ctx.save();
  ctx.textAlign = "center";
  for (const [text, size, a] of rows) {
    ctx.globalAlpha = creditIn * a;
    ctx.font = `500 ${size}px ${FONTS.zh}`;
    ctx.letterSpacing = "0.3px";
    ctx.fillStyle = "rgb(214, 222, 250)";
    ctx.fillText(text, cx, y + 10 * (1 - creditIn));
    y += size * (portrait ? 1.75 : 1.9);
  }
  ctx.restore();
}

// --- The frame ------------------------------------------------------------------------------------------

// Draws frame time t (seconds) into ctx, whose canvas is width × height; `shape` is "landscape" or
// "portrait".
export function renderFrame(ctx, t, { width, height, shape }) {
  const stage = stageFor(width, height, shape);
  const scene = sceneAt(t);
  ctx.save();
  if (scene.kind === "intro") introScene(ctx, t, scene, stage);
  else if (scene.kind === "title") titleScene(ctx, t, scene, stage);
  else if (scene.kind === "board") boardScene(ctx, t, scene, stage);
  else if (scene.kind === "facts") factsScene(ctx, t, scene, stage);
  else if (scene.kind === "platforms") platformsScene(ctx, t, scene, stage);
  else endScene(ctx, t, scene, stage);
  ctx.restore();
  const bars = stage.portrait ? 0 : letterbox(width, height, ramp(t, LETTERBOX.open, LETTERBOX.open + LETTERBOX.length));
  finish(ctx, { width, height, frame: Math.round(t * FPS), bars, aberration: stage.portrait ? 0.0036 : 0.003, grain: 0.055 });
}

// The keyframes for a contact sheet: each scene's midpoint and the key moments, in time order.
export const KEYFRAMES = [0.5, 1.2, 1.8, 2.3, 3.4, 4.3, 5.5, 6.3, 7.6, 8.6, 10.4, 11.9, 13.2, 14.4, 16.3, 18.5, 19.8, 22.4, 23.4, 25, 27, 28.4, 29.1, 29.9];

// The shapes' sizes.
export const SHAPES = { landscape: [1920, 1080], portrait: [1080, 1920] };
