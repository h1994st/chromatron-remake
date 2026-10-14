// The brand's lettering: Latin capitals, digits and a little punctuation of our own, drawn as laser
// beams. Every glyph is a set of centre lines on a grid whose cap height is 1, running in the
// board's eight directions (0°, 45°, 90°…) with 45° corner cuts; a few diagonals (N, X, Z, /, 2)
// are free. Each line is drawn as a beam: a faint halo, red, green and blue strands side by side
// that add up to white where they overlap, and a white-hot core.
//
// Chinese is never lettered: drawZhLine sets it in PingFang SC with a soft glow, and
// drawTitleCard pairs a lettered English line with it.
//
// The geometry (GLYPHS, layoutLine) is pure and runs in Node; the drawing takes a Light layer's
// context (draw.mjs) and, for typeset text, the frame's context.

import { CHANNELS } from "./board.mjs";
import { CHANNEL_RGB, FONTS, drawFlare, rgba } from "./draw.mjs";

// --- Glyphs -----------------------------------------------------------------------------------------

// The beam's nominal width in cap units; letters are spaced as if their strokes were this wide.
export const WEIGHT = 0.1;
// Every character the lettering has.
export const CHARSET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789·.+–×=:/&!? ";

const C = 0.3; // the 45° corner cut
const S = 0.2; // a smaller cut, for small bowls

// Each glyph: { w, paths: [{ pts: [[x, y]…], closed?, dot? }] }, x from 0 to w, y from 0 (cap
// line) to 1 (baseline). A dot is a single point, drawn as a round spot the beam's width.
function buildGlyphs() {
  const line = (...pts) => ({ pts });
  const loop = (...pts) => ({ pts, closed: true });
  const dot = (x, y) => ({ pts: [[x, y]], dot: true });
  const g = (w, ...paths) => ({ w, paths });
  // Turns a glyph 180° in its box (6 into 9).
  const turn = ({ w, paths }) => g(w, ...paths.map((p) => ({ ...p, pts: p.pts.map(([x, y]) => [w - x, 1 - y]) })));
  const bowl = 0.54;
  const six = (w) =>
    g(w, line([w - 0.06, 0], [C, 0], [0, C], [0, 1 - C], [C, 1], [w - C, 1], [w, 1 - C], [w, 0.46 + S], [w - S, 0.46], [0, 0.46]));
  return {
    A: g(0.8, line([0, 1], [0, C], [C, 0], [0.8 - C, 0], [0.8, C], [0.8, 1]), line([0, 0.58], [0.8, 0.58])),
    B: g(0.76, line([0, 0.47], [0.68 - S, 0.47], [0.68, 0.47 - S], [0.68, S], [0.68 - S, 0], [0, 0], [0, 1], [0.76 - S, 1], [0.76, 1 - S], [0.76, 0.47 + S], [0.76 - S, 0.47], [0, 0.47])),
    C: g(0.74, line([0.74, 0], [C, 0], [0, C], [0, 1 - C], [C, 1], [0.74, 1])),
    D: g(0.8, loop([0, 0], [0.8 - C, 0], [0.8, C], [0.8, 1 - C], [0.8 - C, 1], [0, 1])),
    E: g(0.64, line([0.64, 0], [0, 0], [0, 1], [0.64, 1]), line([0, 0.5], [0.54, 0.5])),
    F: g(0.62, line([0.62, 0], [0, 0], [0, 1]), line([0, 0.5], [0.52, 0.5])),
    G: g(0.78, line([0.78, 0], [C, 0], [0, C], [0, 1 - C], [C, 1], [0.78, 1], [0.78, 0.54], [0.42, 0.54])),
    H: g(0.78, line([0, 0], [0, 1]), line([0.78, 0], [0.78, 1]), line([0, 0.5], [0.78, 0.5])),
    I: g(0, line([0, 0], [0, 1])),
    J: g(0.64, line([0.64, 0], [0.64, 1 - C], [0.64 - C, 1], [C, 1], [0, 1 - C])),
    K: g(0.7, line([0, 0], [0, 1]), line([0.7, 0], [0.2, 0.5], [0.7, 1]), line([0, 0.5], [0.2, 0.5])),
    L: g(0.62, line([0, 0], [0, 1], [0.62, 1])),
    M: g(1, line([0, 1], [0, 0], [0.5, 0.5], [1, 0], [1, 1])),
    N: g(0.8, line([0, 1], [0, 0], [0.8, 1], [0.8, 0])),
    O: g(0.86, loop([C, 0], [0.86 - C, 0], [0.86, C], [0.86, 1 - C], [0.86 - C, 1], [C, 1], [0, 1 - C], [0, C])),
    P: g(0.74, line([0, 1], [0, 0], [0.74 - S, 0], [0.74, S], [0.74, bowl - S], [0.74 - S, bowl], [0, bowl])),
    Q: g(0.86, loop([C, 0], [0.86 - C, 0], [0.86, C], [0.86, 1 - C], [0.86 - C, 1], [C, 1], [0, 1 - C], [0, C]), line([0.56, 0.7], [0.92, 1.06])),
    R: g(0.74, line([0, 1], [0, 0], [0.74 - S, 0], [0.74, S], [0.74, bowl - S], [0.74 - S, bowl], [0, bowl]), line([0.28 + 0.03, bowl + 0.03], [0.74, 1])),
    S: g(0.72, line([0.72, 0], [C, 0], [0, C], [0, 0.36], [0.14, 0.5], [0.58, 0.5], [0.72, 0.64], [0.72, 1 - C], [0.72 - C, 1], [0, 1])),
    T: g(0.78, line([0, 0], [0.78, 0]), line([0.39, 0], [0.39, 1])),
    U: g(0.78, line([0, 0], [0, 1 - C], [C, 1], [0.78 - C, 1], [0.78, 1 - C], [0.78, 0])),
    V: g(0.76, line([0, 0], [0, 0.62], [0.38, 1], [0.76, 0.62], [0.76, 0])),
    W: g(1, line([0, 0], [0, 1], [0.5, 0.5], [1, 1], [1, 0])),
    X: g(0.76, line([0, 0], [0.76, 1]), line([0.76, 0], [0, 1])),
    Y: g(0.76, line([0, 0], [0, 0.17], [0.38, 0.55], [0.76, 0.17], [0.76, 0]), line([0.38, 0.55], [0.38, 1])),
    Z: g(0.74, line([0, 0], [0.74, 0], [0, 1], [0.74, 1])),
    0: g(0.7, loop([0.24, 0], [0.46, 0], [0.7, 0.24], [0.7, 0.76], [0.46, 1], [0.24, 1], [0, 0.76], [0, 0.24])),
    1: g(0.24, line([0, 0.24], [0.24, 0], [0.24, 1])),
    2: g(0.7, line([0, C], [C, 0], [0.7 - C, 0], [0.7, C], [0.7, 0.36], [0, 1], [0.7, 1])),
    3: g(0.7, line([0, 0], [0.7 - S, 0], [0.7, S], [0.7, 0.46 - S], [0.7 - S, 0.46], [0.2, 0.46]), line([0.7 - S, 0.46], [0.7, 0.46 + S], [0.7, 1 - C], [0.7 - C, 1], [0, 1])),
    4: g(0.7, line([0, 0], [0, 0.64], [0.7, 0.64]), line([0.5, 0.3], [0.5, 1])),
    5: g(0.7, line([0.7, 0], [0, 0], [0, 0.46], [0.7 - C, 0.46], [0.7, 0.46 + C], [0.7, 1 - C], [0.7 - C, 1], [0, 1])),
    6: six(0.7),
    7: g(0.7, line([0, 0], [0.7, 0], [0.7, C], [0, 1])),
    8: g(0.7, loop([S, 0], [0.7 - S, 0], [0.7, S], [0.7, 0.46 - S], [0.7 - S, 0.46], [S, 0.46], [0, 0.46 - S], [0, S]), loop([S, 0.46], [0.7 - S, 0.46], [0.7, 0.46 + S], [0.7, 1 - C], [0.7 - C, 1], [C, 1], [0, 1 - C], [0, 0.46 + S])),
    9: turn(six(0.7)),
    "·": g(0, dot(0, 0.5)),
    ".": g(0, dot(0, 1)),
    "+": g(0.6, line([0, 0.5], [0.6, 0.5]), line([0.3, 0.2], [0.3, 0.8])),
    "=": g(0.6, line([0, 0.36], [0.6, 0.36]), line([0, 0.68], [0.6, 0.68])),
    "–": g(0.56, line([0, 0.52], [0.56, 0.52])),
    "×": g(0.5, line([0, 0.25], [0.5, 0.75]), line([0.5, 0.25], [0, 0.75])),
    ":": g(0, dot(0, 0.38), dot(0, 1)),
    "/": g(0.5, line([0, 1], [0.5, 0])),
    "&": g(0.86, line([0.86, 1], [0.22, 0.36], [0.22, 0.16], [0.38, 0], [0.54, 0], [0.7, 0.16], [0.7, 0.26], [0.12, 0.84], [0.28, 1], [0.56, 1], [0.86, 0.7])),
    "!": g(0, line([0, 0], [0, 0.68]), dot(0, 1)),
    "?": g(0.66, line([0, 0.22], [0.22, 0], [0.44, 0], [0.66, 0.22], [0.66, 0.32], [0.33, 0.65], [0.33, 0.72]), dot(0.33, 1)),
    " ": g(0.34),
  };
}
export const GLYPHS = buildGlyphs();

// Pairs whose open corners leave too much air, in cap units added to the advance.
const KERN = { LT: -0.2, LY: -0.14, LV: -0.1, "T.": -0.18, "P.": -0.14, "F.": -0.14, "Y.": -0.12, "V.": -0.1, "7.": -0.12, "1.": -0.04 };

// Lays out `text` (upper-cased; every character must be in CHARSET) with `track` extra cap units
// between letters. Returns each glyph's left x and the line's width, both in cap units.
export function layoutLine(text, { track = 0.3 } = {}) {
  const chars = [...text.toUpperCase()];
  const glyphs = [];
  let x = 0;
  chars.forEach((ch, i) => {
    const g = GLYPHS[ch];
    if (!g) throw new Error(`no glyph for ${JSON.stringify(ch)} in ${JSON.stringify(text)}`);
    glyphs.push({ ch, x, g });
    if (i < chars.length - 1) x += g.w + WEIGHT + track + (KERN[ch + chars[i + 1]] ?? 0);
  });
  const last = glyphs[glyphs.length - 1];
  return { glyphs, width: last ? last.x + last.g.w : 0 };
}

// --- Beams ------------------------------------------------------------------------------------------

// Adds a glyph's centre lines (not its dots) to `path`, its origin at (ox, oy) px and `cap` px per
// unit.
function glyphPath(path, g, ox, oy, cap) {
  for (const { pts, closed, dot } of g.paths) {
    if (dot) continue;
    pts.forEach(([x, y], i) => (i ? path.lineTo(ox + x * cap, oy + y * cap) : path.moveTo(ox + x * cap, oy + y * cap)));
    if (closed) path.closePath();
  }
  return path;
}

// A dot as a spot of beam light: a halo, the three strands as discs, a white core; with `tint`,
// one strand of that colour and a core lifted towards white.
function drawDot(lit, x, y, t, intensity, strand, tint) {
  const disc = (dx, dy, r, style) => {
    lit.fillStyle = style;
    lit.beginPath();
    lit.arc(x + dx, y + dy, r, 0, Math.PI * 2);
    lit.fill();
  };
  if (tint) {
    disc(0, 0, 2.6 * t, rgba(tint, 0.1 * intensity));
    disc(0, 0, 1.25 * t, rgba(tint, strand * intensity));
    disc(0, 0, 0.75 * t, rgba(hot(tint), 0.95 * intensity));
    return;
  }
  disc(0, 0, 2.6 * t, `rgba(150, 175, 255, ${0.06 * intensity})`);
  for (const [channel, dx, dy] of strands(t)) disc(dx, dy, 1.25 * t, rgba(CHANNEL_RGB[channel], strand * intensity));
  disc(0, 0, 0.75 * t, `rgba(255, 255, 255, ${0.95 * intensity})`);
}

// A tint's hot core: the colour lifted most of the way to white.
const hot = (tint) => tint.map((c) => c + (255 - c) * 0.6);

// Three strands side by side: where they overlap they add to white, and their edges keep a
// spectral fringe (red, yellow | cyan, blue).
const strands = (t) => [
  [1, -0.5 * t, -0.5 * t],
  [2, 0, 0],
  [4, 0.5 * t, 0.5 * t],
];

// Strokes `path` into `lit` as a beam `t` px wide; `tint` ([r, g, b]) makes it a beam of that one
// colour rather than three strands adding to white.
export function drawBeamPath(lit, path, t, { intensity = 1, strand = 0.85, tint = null } = {}) {
  lit.save();
  lit.lineJoin = "round";
  lit.lineCap = "round";
  if (tint) {
    lit.strokeStyle = rgba(tint, 0.1 * intensity);
    lit.lineWidth = 5 * t;
    lit.stroke(path);
    lit.strokeStyle = rgba(tint, strand * intensity);
    lit.lineWidth = 2.5 * t;
    lit.stroke(path);
    lit.strokeStyle = rgba(hot(tint), 0.95 * intensity);
    lit.lineWidth = 0.7 * t;
    lit.stroke(path);
    lit.restore();
    return;
  }
  lit.strokeStyle = `rgba(150, 175, 255, ${0.06 * intensity})`;
  lit.lineWidth = 5 * t;
  lit.stroke(path);
  for (const [channel, dx, dy] of strands(t)) {
    lit.save();
    lit.translate(dx, dy);
    lit.strokeStyle = rgba(CHANNEL_RGB[channel], strand * intensity);
    lit.lineWidth = 1.5 * t;
    lit.stroke(path);
    lit.restore();
  }
  lit.strokeStyle = `rgba(255, 255, 255, ${0.95 * intensity})`;
  lit.lineWidth = 0.6 * t;
  lit.stroke(path);
  lit.restore();
}

function glint(lit, x, y, r, a) {
  const g = lit.createRadialGradient(x, y, 0, x, y, r);
  g.addColorStop(0, `rgba(255,255,255,${a})`);
  g.addColorStop(0.25, `rgba(200,220,255,${0.4 * a})`);
  g.addColorStop(1, "rgba(160,190,255,0)");
  lit.fillStyle = g;
  lit.beginPath();
  lit.arc(x, y, r, 0, Math.PI * 2);
  lit.fill();
}

// A four-point star of light at (x, y), `r` px across its long ray.
export function drawSparkle(lit, x, y, r, a = 1) {
  glint(lit, x, y, r * 0.35, a);
  for (const [w, h] of [
    [r, r * 0.05],
    [r * 0.05, r * 0.6],
  ]) {
    const g = lit.createRadialGradient(x, y, 0, x, y, Math.max(w, h));
    g.addColorStop(0, `rgba(255,255,255,${0.9 * a})`);
    g.addColorStop(1, "rgba(200,220,255,0)");
    lit.fillStyle = g;
    lit.beginPath();
    lit.ellipse(x, y, w, h, 0, 0, Math.PI * 2);
    lit.fill();
  }
}

// A lit target in place of an O: one arc per channel round a white core.
function drawTarget(lit, cx, cy, r, t, intensity) {
  lit.save();
  lit.lineCap = "round";
  lit.strokeStyle = `rgba(150,175,255,${0.05 * intensity})`;
  lit.lineWidth = 5 * t;
  lit.beginPath();
  lit.arc(cx, cy, r, 0, Math.PI * 2);
  lit.stroke();
  CHANNELS.forEach((channel, k) => {
    const a0 = -Math.PI / 2 + (k * 2 * Math.PI) / 3 + 0.16;
    const a1 = a0 + (2 * Math.PI) / 3 - 0.32;
    lit.strokeStyle = rgba(CHANNEL_RGB[channel], intensity);
    lit.lineWidth = 1.7 * t;
    lit.beginPath();
    lit.arc(cx, cy, r, a0, a1);
    lit.stroke();
    lit.strokeStyle = `rgba(255,255,255,${0.7 * intensity})`;
    lit.lineWidth = 0.5 * t;
    lit.stroke();
  });
  const core = lit.createRadialGradient(cx, cy, 0, cx, cy, r * 0.5);
  core.addColorStop(0, `rgba(255,255,255,${intensity})`);
  core.addColorStop(0.45, `rgba(255,255,255,${0.9 * intensity})`);
  core.addColorStop(1, "rgba(190,210,255,0)");
  lit.fillStyle = core;
  lit.beginPath();
  lit.arc(cx, cy, r * 0.5, 0, Math.PI * 2);
  lit.fill();
  lit.restore();
}

// While a line is revealed, how many glyphs' worth of the reveal each glyph takes to draw.
const REVEAL_SPAN = 2.5;

// Part of a glyph, as a beam writing it has got to: `drawn` (0–1) of each of its strokes from their
// starts, a hot white tip where each is being drawn, and its dots once it is mostly done.
function drawGlyphPart(lit, g, ox, oy, cap, t, drawn, intensity, strand, flare, tint) {
  const path = new Path2D();
  const tips = [];
  for (const { pts, closed, dot } of g.paths) {
    if (dot) continue;
    const corners = closed ? pts.length + 1 : pts.length;
    let total = 0;
    for (let k = 1; k < corners; k += 1) total += Math.hypot(pts[k % pts.length][0] - pts[k - 1][0], pts[k % pts.length][1] - pts[k - 1][1]);
    let left = total * drawn;
    let [tx, ty] = pts[0];
    path.moveTo(ox + tx * cap, oy + ty * cap);
    for (let k = 1; k < corners && left > 0; k += 1) {
      const [ax, ay] = pts[k - 1];
      const [bx, by] = pts[k % pts.length];
      const length = Math.hypot(bx - ax, by - ay);
      const f = Math.min(1, left / length);
      tx = ax + (bx - ax) * f;
      ty = ay + (by - ay) * f;
      path.lineTo(ox + tx * cap, oy + ty * cap);
      left -= length;
    }
    tips.push([ox + tx * cap, oy + ty * cap]);
  }
  drawBeamPath(lit, path, t, { intensity, strand, tint });
  for (const [tx, ty] of tips) {
    drawSparkle(lit, tx, ty, t * 7, intensity);
    // An anamorphic streak through the drawing tip.
    if (flare > 0) drawFlare(lit, tx, ty, { length: cap * 2.4 * flare, thickness: t * 1.1, intensity: 0.7 * intensity });
  }
  const dots = Math.min(1, Math.max(0, (drawn - 0.6) / 0.4));
  if (dots > 0) for (const { pts, dot } of g.paths) if (dot) drawDot(lit, ox + pts[0][0] * cap, oy + pts[0][1] * cap, t, intensity * dots, strand, tint);
}

// The beam width for a cap height, unless given.
const strokeFor = (cap, stroke, thickness = 1) => stroke ?? Math.max(1, cap * 0.028 * thickness);

// Draws `text` in beams into `lit`, its cap line at y and `cap` px tall. `align` places x at the
// line's left, centre or right; `stroke` sets the beam width in px (default from the cap height);
// `skip` lists glyph indices to leave out (to draw them some other way); `target` turns that
// glyph (an O) into a lit target. `reveal` (0–1) draws the line on as a beam would write it: each
// glyph's strokes grow from their starts with a hot tip (and an anamorphic streak `flare` × 2.4
// caps long), the glyphs one after another and overlapping. Returns the box { left, top, width,
// height } and each glyph's pixel position { ch, left, width }. `tint` ([r, g, b]) letters it in
// beams of that one colour (drawBeamPath).
export function drawLettering(lit, text, x, y, { cap, track = 0.3, align = "left", stroke, intensity = 1, strand = 0.85, glints = true, target = -1, skip = [], reveal = 1, flare = 0, tint = null }) {
  const line = layoutLine(text, { track });
  const width = line.width * cap;
  const left = align === "center" ? x - width / 2 : align === "right" ? x - width : x;
  const t = strokeFor(cap, stroke);
  const placed = [];
  const count = line.glyphs.length;
  line.glyphs.forEach(({ ch, x: gx, g }, i) => {
    const ox = left + gx * cap;
    placed.push({ ch, left: ox, width: g.w * cap });
    if (skip.includes(i) || !g.paths.length) return;
    const drawn = reveal >= 1 ? 1 : Math.min(1, Math.max(0, (reveal * (count + REVEAL_SPAN) - i) / REVEAL_SPAN));
    if (drawn <= 0) return;
    if (i === target) {
      drawTarget(lit, ox + (g.w * cap) / 2, y + cap / 2, cap / 2, t, intensity * drawn);
      return;
    }
    if (drawn < 1) {
      drawGlyphPart(lit, g, ox, y, cap, t, drawn, intensity, strand, flare, tint);
      return;
    }
    drawBeamPath(lit, glyphPath(new Path2D(), g, ox, y, cap), t, { intensity, strand, tint });
    for (const { pts, dot } of g.paths) if (dot) drawDot(lit, ox + pts[0][0] * cap, y + pts[0][1] * cap, t, intensity, strand, tint);
    // Faint hot spots where the beam turns.
    if (glints) {
      for (const { pts } of g.paths) {
        for (let k = 1; k < pts.length - 1; k += 1) glint(lit, ox + pts[k][0] * cap, y + pts[k][1] * cap, 2.6 * t, 0.35 * intensity);
      }
    }
  });
  return { left, top: y, width, height: cap, glyphs: placed };
}

// --- The wordmark -----------------------------------------------------------------------------------

const WORDMARK_TRACK = 0.36;

// The wordmark's measures at `width` px, relative to its left edge and cap line: the cap height, the
// `sub` line's cap, tracking and width, the underline's y and where the underline beam ends.
export function wordmarkMetrics(width, sub = "REMAKE") {
  const cap = width / layoutLine("CHROMATRON", { track: WORDMARK_TRACK }).width;
  const subCap = cap * 0.22;
  const subTrack = 0.95;
  const subWidth = sub ? layoutLine(sub, { track: subTrack }).width * subCap : 0;
  return { cap, subCap, subTrack, subWidth, underlineY: cap * 1.4, underlineEnd: width - subWidth - cap * 0.3, height: cap * 1.4 + subCap / 2 };
}

// The wordmark, `width` px wide with its cap line at y: CHROMATRON in beams, its first O a lit
// target, and under it red, green and blue beams that bend in at 45°, merge into one white beam
// and run into a small lettered `sub` (REMAKE) at the right. `underline` (0–1) draws that beam in
// from the left, and `reveal` (0–1) draws the letters on (drawLettering). Returns
// { left, top, width, height, cap }.
export function drawWordmark(lit, x, y, { width, align = "left", sub = "REMAKE", stroke, intensity = 1, underline = 1, reveal = 1 }) {
  const { cap, subCap, subTrack, subWidth, underlineY } = wordmarkMetrics(width, sub);
  const left = align === "center" ? x - width / 2 : x;
  const t = strokeFor(cap, stroke);
  drawLettering(lit, "CHROMATRON", left, y, { cap, track: WORDMARK_TRACK, target: 3, stroke: t, intensity, reveal });
  if (!sub) return { left, top: y, width, height: cap, cap };
  const lineY = y + underlineY;
  const subLeft = left + width - subWidth;
  const end = subLeft - cap * 0.3;
  const d = cap * 0.09;
  const run = cap * 0.5;
  if (underline > 0) {
    const reach = left + (end - left) * Math.min(1, underline);
    lit.save();
    lit.lineCap = "round";
    lit.lineJoin = "round";
    lit.beginPath();
    lit.rect(left - t * 4, lineY - d - t * 4, reach - left + t * 8, 2 * d + t * 8);
    lit.clip();
    for (const [channel, dy] of [
      [1, -d],
      [2, 0],
      [4, d],
    ]) {
      const p = new Path2D();
      p.moveTo(left, lineY + dy);
      p.lineTo(left + run, lineY + dy);
      p.lineTo(left + run + Math.abs(dy), lineY);
      p.lineTo(end, lineY);
      const fade = lit.createLinearGradient(left, 0, left + run, 0);
      fade.addColorStop(0, rgba(CHANNEL_RGB[channel], 0));
      fade.addColorStop(1, rgba(CHANNEL_RGB[channel], 0.95 * intensity));
      lit.strokeStyle = fade;
      lit.lineWidth = 1.3 * t;
      lit.stroke(p);
    }
    lit.strokeStyle = `rgba(255,255,255,${0.9 * intensity})`;
    lit.lineWidth = 0.5 * t;
    lit.beginPath();
    lit.moveTo(left + run + d, lineY);
    lit.lineTo(end, lineY);
    lit.stroke();
    lit.restore();
    if (underline >= 1) drawSparkle(lit, end, lineY, cap * 0.22, intensity);
  }
  drawLettering(lit, sub, subLeft, lineY - subCap / 2, { cap: subCap, track: subTrack, stroke: Math.max(1, t * 0.6), strand: 0.6, intensity, glints: false, reveal });
  return { left, top: y, width, height: lineY + subCap / 2 - y, cap };
}

// --- Typeset Chinese --------------------------------------------------------------------------------

// A line of Chinese in PingFang SC (its Latin letters and digits in SF Pro), crisp in ctx with a
// soft glow added into `lit` (pass null for none). (x, baseline) as fillText with `align`.
// Returns its width.
export function drawZhLine(ctx, lit, text, x, baseline, { size, weight = 600, color = "#e1e8ff", spacing = 0.08, align = "left", glow = 0.22, alpha = 1 }) {
  const font = `${weight} ${size}px ${FONTS.zh}`;
  const set = (c) => {
    c.font = font;
    c.letterSpacing = `${spacing * size}px`;
    c.textAlign = "left";
    c.textBaseline = "alphabetic";
  };
  ctx.save();
  set(ctx);
  const width = ctx.measureText(text).width - spacing * size;
  const left = align === "center" ? x - width / 2 : align === "right" ? x - width : x;
  ctx.globalAlpha = alpha;
  ctx.fillStyle = color;
  ctx.fillText(text, left, baseline);
  ctx.restore();
  if (lit && glow > 0) {
    lit.save();
    set(lit);
    lit.fillStyle = `rgba(170, 190, 255, ${glow * alpha})`;
    lit.fillText(text, left, baseline);
    lit.restore();
  }
  return width;
}

// --- Bilingual title cards --------------------------------------------------------------------------

// A title card: the English lines `en` (an array) lettered in beams, `leading` caps apart (cap
// height `cap`, the first cap line at y), with the Chinese line typeset under them; `align` places
// them about x. `sub` adds a typeset English line between the lettering and the Chinese, `reveal`
// (0–1) draws the lettering on line after line (drawLettering) and `rise` sets the typeset lines
// that many px low, for them to rise into place. Returns the card's box
// { left, top, width, height }.
export function drawTitleCard(ctx, lit, x, y, { en: lines, zh, cap, align = "left", track = 0.3, stroke, zhSize = cap * 0.5, zhWeight = 600, gap, intensity = 1, zhAlpha = 1, sub, subAlpha = zhAlpha, leading = 1.42, reveal = 1, rise = 0, flare = 0 }) {
  let width = 0;
  const stagger = 0.5;
  const span = 1 + stagger * (lines.length - 1);
  lines.forEach((line, k) => {
    const shown = Math.min(1, Math.max(0, reveal * span - stagger * k));
    const box = drawLettering(lit, line, x, y + k * cap * leading, { cap, track, align, stroke, intensity, reveal: shown, flare });
    width = Math.max(width, box.width);
  });
  const space = gap ?? cap * 0.42;
  let baseline = y + (lines.length - 1) * cap * leading + cap + space + zhSize * 0.82 + rise;
  if (sub) {
    ctx.save();
    ctx.globalAlpha = subAlpha;
    ctx.font = `600 ${zhSize}px ${FONTS.en}`;
    ctx.letterSpacing = `${0.01 * zhSize}px`;
    ctx.textAlign = align;
    ctx.textBaseline = "alphabetic";
    ctx.fillStyle = "#e1e8ff";
    ctx.fillText(sub, x, baseline);
    width = Math.max(width, ctx.measureText(sub).width);
    ctx.restore();
    baseline += zhSize * 1.45;
  }
  if (zh) width = Math.max(width, drawZhLine(ctx, lit, zh, x, baseline, { size: zhSize, weight: zhWeight, align, alpha: zhAlpha }));
  const left = align === "center" ? x - width / 2 : align === "right" ? x - width : x;
  return { left, top: y, width, height: baseline + zhSize * 0.12 - y };
}
