// The trailer's look, in Canvas 2D: background, board, pieces, additive beams with glow and bloom,
// particles, cursor, mark and captions (the lettering is in lettering.mjs). Every function takes
// its size in pixels (or a board layout's cell size), so the same code draws a 16 px favicon idea
// and a 1920 px frame.
// Nothing here calls Math.random: grain and particles come from a seeded PRNG, so a frame is a pure
// function of its inputs.
//
// Naming: a `Light` is the light layer (its canvases); the functions that draw light take `lit`,
// the additive 2D context that `light.begin()` returns, and `ctx` is the frame's own context.

import { CHANNELS, DIRS } from "./board.mjs";

// --- Colour ---------------------------------------------------------------------------------------

const INK = "#06070b";
// Each channel's beam colour. They are close to the pure primaries, so beams add up as light should:
// red + green is yellow, all three white, with no channel clipping where different colours meet.
export const CHANNEL_RGB = { 1: [255, 34, 52], 2: [24, 255, 118], 4: [40, 96, 255] };
// A hot, nearly white core for each channel.
const CORE_RGB = { 1: [255, 200, 196], 2: [210, 255, 226], 4: [214, 226, 255] };

export const clamp = (v, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
export const rgba = ([r, g, b], a = 1) => `rgba(${r | 0},${g | 0},${b | 0},${a})`;

// The additive mix of a mask's channels, clamped: [255, 255, 255] for white.
function mixRGB(mask) {
  const out = [0, 0, 0];
  for (const channel of CHANNELS) {
    if (!(mask & channel)) continue;
    for (let k = 0; k < 3; k += 1) out[k] = Math.min(255, out[k] + CHANNEL_RGB[channel][k]);
  }
  return out;
}
// A mixed colour as a display colour: secondaries are lifted so yellow, cyan and magenta read clean.
function maskColor(mask, a = 1) {
  const [r, g, b] = mixRGB(mask);
  const top = Math.max(r, g, b, 1);
  return rgba([(r / top) * 255, (g / top) * 255, (b / top) * 255], a);
}

// --- Seeded randomness ----------------------------------------------------------------------------

// mulberry32: a small, fast PRNG; the same seed always gives the same sequence in [0, 1).
export function prng(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// --- Easing ---------------------------------------------------------------------------------------

export const ease = {
  outExpo: (t) => (t >= 1 ? 1 : 1 - 2 ** (-10 * clamp(t))),
  inOutCubic: (t) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2),
  // A damped spring from 0 to 1 that overshoots a little and settles.
  spring: (t, damping = 7, freq = 2.2) => (t <= 0 ? 0 : 1 - Math.exp(-damping * t) * Math.cos(2 * Math.PI * freq * t)),
};

// --- Offscreen canvases ---------------------------------------------------------------------------

// A canvas of width × height and its 2D context, offscreen where the browser has OffscreenCanvas.
export function surface(width, height) {
  const canvas = typeof OffscreenCanvas === "function" ? new OffscreenCanvas(width, height) : Object.assign(document.createElement("canvas"), { width, height });
  return { canvas, ctx: canvas.getContext("2d") };
}

// The light layer: beams, glows and sparks go here (additively), then `composite` adds them onto the
// frame together with two blurred copies of themselves, a tight glow and a wide bloom. Make one per
// frame size and reuse it: the canvases are allocated once.
export class Light {
  constructor(width, height) {
    this.width = width;
    this.height = height;
    this.sharp = surface(width, height);
    this.glow = surface(Math.ceil(width / 2), Math.ceil(height / 2));
    this.bloom = surface(Math.ceil(width / 8), Math.ceil(height / 8));
  }
  // A soft layer for volumetric light (haze around beams): a quarter-size canvas, drawn to in frame
  // pixels, that `composite` blurs widely and adds; made on first use and cleared by `begin`.
  haze() {
    if (!this.soft) this.soft = surface(Math.ceil(this.width / 4), Math.ceil(this.height / 4));
    const { ctx } = this.soft;
    if (!this.softUsed) {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.globalCompositeOperation = "source-over";
      ctx.clearRect(0, 0, this.soft.canvas.width, this.soft.canvas.height);
      ctx.setTransform(0.25, 0, 0, 0.25, 0, 0);
      ctx.globalCompositeOperation = "lighter";
      this.softUsed = true;
    }
    return ctx;
  }
  // Clears the layer and returns its context, set to add light.
  begin() {
    this.softUsed = false;
    const { ctx } = this.sharp;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalCompositeOperation = "source-over";
    ctx.globalAlpha = 1;
    ctx.clearRect(0, 0, this.width, this.height);
    ctx.globalCompositeOperation = "lighter";
    return ctx;
  }
  // Adds the layer onto `ctx`: glow and bloom radii are in pixels of the full frame.
  composite(ctx, { glowRadius = 8, glow = 0.9, bloomRadius = 48, bloom = 0.55, hazeRadius = 36 } = {}) {
    if (this.softUsed) {
      // The haze, blurred at quarter size and added under the beams.
      const soft = this.soft;
      if (!this.softBlur) this.softBlur = surface(soft.canvas.width, soft.canvas.height);
      const b = this.softBlur.ctx;
      b.setTransform(1, 0, 0, 1, 0, 0);
      b.globalCompositeOperation = "copy";
      b.filter = `blur(${(hazeRadius / 4).toFixed(2)}px)`;
      b.drawImage(soft.canvas, 0, 0);
      b.filter = "none";
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.globalCompositeOperation = "lighter";
      ctx.imageSmoothingEnabled = true;
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(this.softBlur.canvas, 0, 0, this.width, this.height);
      ctx.restore();
    }
    const pass = (target, radius) => {
      const t = target.ctx;
      t.setTransform(1, 0, 0, 1, 0, 0);
      t.globalCompositeOperation = "source-over";
      t.filter = "none";
      t.clearRect(0, 0, target.canvas.width, target.canvas.height);
      t.filter = `blur(${Math.max(0.5, (radius * target.canvas.width) / this.width).toFixed(2)}px)`;
      t.drawImage(this.sharp.canvas, 0, 0, target.canvas.width, target.canvas.height);
      t.filter = "none";
    };
    pass(this.glow, glowRadius);
    pass(this.bloom, bloomRadius);
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalCompositeOperation = "lighter";
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.globalAlpha = bloom;
    ctx.drawImage(this.bloom.canvas, 0, 0, this.width, this.height);
    ctx.globalAlpha = glow;
    ctx.drawImage(this.glow.canvas, 0, 0, this.width, this.height);
    ctx.globalAlpha = 1;
    ctx.drawImage(this.sharp.canvas, 0, 0);
    ctx.restore();
  }
}

const lights = new Map();
// The one Light for a frame size (and `name`, for a frame that composites more than one layer),
// made on first use: per-frame code takes it from here rather than allocating canvases each frame.
export function lightFor(width, height, name = "main") {
  const key = `${name}:${width}x${height}`;
  let light = lights.get(key);
  if (!light) {
    light = new Light(width, height);
    lights.set(key, light);
  }
  return light;
}

// --- Background -----------------------------------------------------------------------------------

const grainTiles = new Map();
// A 256 × 256 tile of seeded monochrome noise, centred on mid grey.
function grainTile(seed) {
  let tile = grainTiles.get(seed);
  if (tile) return tile;
  tile = surface(256, 256);
  const image = tile.ctx.createImageData(256, 256);
  const random = prng(seed);
  for (let i = 0; i < image.data.length; i += 4) {
    const v = 128 + (random() + random() - 1) * 127;
    image.data[i] = image.data[i + 1] = image.data[i + 2] = v;
    image.data[i + 3] = 255;
  }
  tile.ctx.putImageData(image, 0, 0);
  grainTiles.set(seed, tile);
  return tile;
}

// The grain tile as a repeating pattern, made once per context and seed, and the one transform
// that moves it (a plain DOMMatrix2DInit, reused), so drawing a frame allocates neither.
const grainPatterns = new WeakMap();
const grainShift = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 };
function grainPattern(ctx, seed) {
  let bySeed = grainPatterns.get(ctx);
  if (!bySeed) {
    bySeed = new Map();
    grainPatterns.set(ctx, bySeed);
  }
  let pattern = bySeed.get(seed);
  if (!pattern) {
    pattern = ctx.createPattern(grainTile(seed).canvas, "repeat");
    bySeed.set(seed, pattern);
  }
  return pattern;
}

// Near-black blue, a faint lift towards (cx, cy), a vignette, and grain. `grain` is the noise's
// opacity (0 for none); `shift` moves the grain tile for parallax or per-frame variation.
export function drawBackground(ctx, { width, height, cx = width / 2, cy = height / 2, lift = 1, vignette = 1, grain = 0.05, seed = 7, shift = [0, 0] }) {
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalCompositeOperation = "source-over";
  ctx.fillStyle = INK;
  ctx.fillRect(0, 0, width, height);
  const span = Math.hypot(width, height);
  if (lift > 0) {
    const glow = ctx.createRadialGradient(cx, cy, 0, cx, cy, span * 0.55);
    glow.addColorStop(0, `rgba(30, 38, 72, ${0.55 * lift})`);
    glow.addColorStop(0.45, `rgba(16, 20, 40, ${0.35 * lift})`);
    glow.addColorStop(1, "rgba(6, 7, 11, 0)");
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, width, height);
  }
  if (vignette > 0) {
    const shade = ctx.createRadialGradient(width / 2, height / 2, span * 0.25, width / 2, height / 2, span * 0.62);
    shade.addColorStop(0, "rgba(0, 0, 0, 0)");
    shade.addColorStop(1, `rgba(0, 0, 2, ${0.75 * vignette})`);
    ctx.fillStyle = shade;
    ctx.fillRect(0, 0, width, height);
  }
  if (grain > 0) drawGrain(ctx, { width, height, amount: grain, seed, shift });
  ctx.restore();
}

// Overlays seeded film grain on the whole frame at opacity `amount`, the tile moved by `shift`.
export function drawGrain(ctx, { width, height, amount, seed = 7, shift = [0, 0] }) {
  const pattern = grainPattern(ctx, seed);
  grainShift.e = shift[0];
  grainShift.f = shift[1];
  pattern.setTransform(grainShift);
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalCompositeOperation = "overlay";
  ctx.globalAlpha = amount;
  ctx.fillStyle = pattern;
  ctx.fillRect(0, 0, width, height);
  ctx.restore();
}

// --- Board layout ---------------------------------------------------------------------------------

// Where a board sits: centred on (cx, cy) with each cell `cell` px. `at(x, y)` maps board
// coordinates (cell centres on integers) to pixels.
export function boardLayout(board, { cx, cy, cell }) {
  const left = cx - (board.w * cell) / 2;
  const top = cy - (board.h * cell) / 2;
  return {
    board,
    cell,
    left,
    top,
    width: board.w * cell,
    height: board.h * cell,
    at: (x, y) => [left + (x + 0.5) * cell, top + (y + 0.5) * cell],
  };
}

// Thin grid lines that fade out towards the board's edge, and small dots where they cross.
function drawGrid(ctx, layout, { alpha = 1, fade = 0.35 } = {}) {
  const { board, cell, left, top, width, height } = layout;
  const cx = left + width / 2;
  const cy = top + height / 2;
  ctx.save();
  const mask = ctx.createRadialGradient(cx, cy, 0, cx, cy, Math.max(width, height) * 0.75);
  mask.addColorStop(0, `rgba(150, 170, 255, ${0.11 * alpha})`);
  mask.addColorStop(1 - fade, `rgba(150, 170, 255, ${0.08 * alpha})`);
  mask.addColorStop(1, `rgba(150, 170, 255, ${0.02 * alpha})`);
  ctx.strokeStyle = mask;
  ctx.lineWidth = Math.max(1, cell * 0.012);
  ctx.beginPath();
  for (let i = 0; i <= board.w; i += 1) {
    const x = Math.round(left + i * cell) + 0.5;
    ctx.moveTo(x, top - cell * 0.35);
    ctx.lineTo(x, top + height + cell * 0.35);
  }
  for (let j = 0; j <= board.h; j += 1) {
    const y = Math.round(top + j * cell) + 0.5;
    ctx.moveTo(left - cell * 0.35, y);
    ctx.lineTo(left + width + cell * 0.35, y);
  }
  ctx.stroke();
  ctx.fillStyle = `rgba(190, 205, 255, ${0.22 * alpha})`;
  const r = Math.max(0.8, cell * 0.018);
  for (let i = 0; i <= board.w; i += 1) {
    for (let j = 0; j <= board.h; j += 1) {
      ctx.beginPath();
      ctx.arc(left + i * cell + 0.5, top + j * cell + 0.5, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  ctx.restore();
}

function roundRect(ctx, x, y, w, h, r) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
}

// A soft glass tile under cell (x, y); `highlight` (0–1) brightens it for movable pieces.
export function drawTile(ctx, layout, x, y, { highlight = 0, alpha = 1 } = {}) {
  const { cell } = layout;
  const [px, py] = layout.at(x, y);
  const inset = cell * 0.07;
  const size = cell - 2 * inset;
  const left = px - size / 2;
  const top = py - size / 2;
  ctx.save();
  ctx.globalAlpha = alpha;
  const fill = ctx.createLinearGradient(left, top, left + size * 0.4, top + size);
  fill.addColorStop(0, `rgba(170, 190, 255, ${0.085 + 0.05 * highlight})`);
  fill.addColorStop(1, `rgba(120, 140, 220, ${0.025 + 0.02 * highlight})`);
  roundRect(ctx, left, top, size, size, cell * 0.16);
  ctx.fillStyle = fill;
  ctx.fill();
  const rim = ctx.createLinearGradient(0, top, 0, top + size);
  rim.addColorStop(0, `rgba(220, 230, 255, ${0.26 + 0.3 * highlight})`);
  rim.addColorStop(0.5, `rgba(200, 215, 255, ${0.06 + 0.1 * highlight})`);
  rim.addColorStop(1, `rgba(200, 215, 255, ${0.1 + 0.16 * highlight})`);
  ctx.strokeStyle = rim;
  ctx.lineWidth = Math.max(1, cell * 0.014);
  ctx.stroke();
  ctx.restore();
}

// --- Pieces ---------------------------------------------------------------------------------------

const angleOf = (dir) => (dir * Math.PI) / 4;

// A piece's body (drawn normally, under the light). `piece` may carry an animated `rotation`
// (radians added to its own angle) and `lift` (0–1, a grabbed piece rises and grows a little).
export function drawPiece(ctx, layout, piece, { alpha = 1, at } = {}) {
  const { cell } = layout;
  const [px, py] = at ?? layout.at(piece.x, piece.y);
  const lift = piece.lift ?? 0;
  ctx.save();
  ctx.globalAlpha = alpha;
  ctx.translate(px, py);
  ctx.scale(1 + 0.08 * lift, 1 + 0.08 * lift);
  const u = cell;
  switch (piece.kind) {
    case "emitter": {
      ctx.rotate(angleOf(piece.dir) + (piece.rotation ?? 0));
      const len = u * 0.66;
      const wid = u * 0.36;
      const body = ctx.createLinearGradient(0, -wid / 2, 0, wid / 2);
      body.addColorStop(0, "#3a4152");
      body.addColorStop(0.45, "#1a1e29");
      body.addColorStop(1, "#0b0d13");
      roundRect(ctx, -len / 2 - u * 0.04, -wid / 2, len, wid, wid / 2);
      ctx.fillStyle = body;
      ctx.fill();
      ctx.strokeStyle = "rgba(230, 236, 255, 0.28)";
      ctx.lineWidth = Math.max(1, u * 0.014);
      ctx.stroke();
      // A colour stripe along the body, so an emitter shows its colour even when off.
      ctx.strokeStyle = maskColor(piece.color, 0.85);
      ctx.lineWidth = Math.max(1, u * 0.03);
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(-len * 0.38, 0);
      ctx.lineTo(len * 0.05, 0);
      ctx.stroke();
      // The aperture: a dark ring at the front.
      ctx.fillStyle = "#05060a";
      ctx.beginPath();
      ctx.arc(len * 0.29 - u * 0.04, 0, wid * 0.3, 0, Math.PI * 2);
      ctx.fill();
      break;
    }
    case "mirror": {
      ctx.rotate(angleOf(piece.angle) + (piece.rotation ?? 0));
      const len = u * 0.86;
      const thick = u * 0.15;
      const glass = ctx.createLinearGradient(0, -thick / 2, 0, thick / 2);
      glass.addColorStop(0, "rgba(190, 210, 255, 0.30)");
      glass.addColorStop(1, "rgba(120, 140, 220, 0.08)");
      roundRect(ctx, -len / 2, -thick / 2, len, thick, thick * 0.35);
      ctx.fillStyle = glass;
      ctx.fill();
      ctx.strokeStyle = "rgba(210, 225, 255, 0.35)";
      ctx.lineWidth = Math.max(1, u * 0.012);
      ctx.stroke();
      const bar = ctx.createLinearGradient(-len / 2, 0, len / 2, 0);
      bar.addColorStop(0, "rgba(255, 255, 255, 0.35)");
      bar.addColorStop(0.5, "rgba(255, 255, 255, 1)");
      bar.addColorStop(1, "rgba(255, 255, 255, 0.35)");
      ctx.strokeStyle = bar;
      ctx.lineWidth = Math.max(1, u * 0.04);
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(-len * 0.44, 0);
      ctx.lineTo(len * 0.44, 0);
      ctx.stroke();
      break;
    }
    case "splitter": {
      // A glass triangle, its tip towards the beam it takes.
      ctx.rotate(angleOf(piece.dir) + (piece.rotation ?? 0));
      // Rounded corners, a glass body and two bright edges along the faces the split beams leave by.
      const r = u * 0.37;
      const corners = [0, 1, 2].map((k) => {
        const a = Math.PI + (k * 2 * Math.PI) / 3;
        return [Math.cos(a) * r, Math.sin(a) * r];
      });
      ctx.beginPath();
      corners.forEach((corner, k) => {
        const next = corners[(k + 1) % 3];
        const mid = [(corner[0] + next[0]) / 2, (corner[1] + next[1]) / 2];
        if (k === 0) ctx.moveTo(mid[0], mid[1]);
        ctx.arcTo(next[0], next[1], (next[0] + corners[(k + 2) % 3][0]) / 2, (next[1] + corners[(k + 2) % 3][1]) / 2, u * 0.05);
      });
      ctx.closePath();
      const glass = ctx.createLinearGradient(-r, -r, r * 0.6, r);
      glass.addColorStop(0, "rgba(215, 228, 255, 0.40)");
      glass.addColorStop(0.5, "rgba(170, 160, 255, 0.16)");
      glass.addColorStop(1, "rgba(120, 220, 255, 0.12)");
      ctx.fillStyle = glass;
      ctx.fill();
      ctx.strokeStyle = "rgba(235, 242, 255, 0.5)";
      ctx.lineWidth = Math.max(1, u * 0.016);
      ctx.stroke();
      ctx.lineCap = "round";
      ctx.lineWidth = Math.max(1, u * 0.026);
      for (const [a, b] of [
        [0, 1],
        [0, 2],
      ]) {
        const edge = ctx.createLinearGradient(corners[a][0], corners[a][1], corners[b][0], corners[b][1]);
        edge.addColorStop(0, "rgba(255, 255, 255, 0.95)");
        edge.addColorStop(1, "rgba(255, 255, 255, 0.1)");
        ctx.strokeStyle = edge;
        ctx.beginPath();
        ctx.moveTo(corners[a][0] * 0.8, corners[a][1] * 0.8);
        ctx.lineTo(corners[b][0] * 0.8, corners[b][1] * 0.8);
        ctx.stroke();
      }
      break;
    }
    case "filter": {
      ctx.rotate(Math.PI / 4 + (piece.rotation ?? 0));
      const s = u * 0.44;
      roundRect(ctx, -s / 2, -s / 2, s, s, u * 0.06);
      ctx.fillStyle = maskColor(piece.color, 0.3);
      ctx.fill();
      ctx.strokeStyle = maskColor(piece.color, 0.95);
      ctx.lineWidth = Math.max(1, u * 0.024);
      ctx.stroke();
      const sheen = ctx.createLinearGradient(-s / 2, -s / 2, s / 2, s / 2);
      sheen.addColorStop(0, "rgba(255, 255, 255, 0.32)");
      sheen.addColorStop(0.45, "rgba(255, 255, 255, 0.04)");
      sheen.addColorStop(1, "rgba(255, 255, 255, 0)");
      ctx.fillStyle = sheen;
      ctx.fill();
      break;
    }
    case "wall": {
      const s = u * 0.8;
      const fill = ctx.createLinearGradient(0, -s / 2, 0, s / 2);
      fill.addColorStop(0, "#1b1f2b");
      fill.addColorStop(1, "#0b0d13");
      roundRect(ctx, -s / 2, -s / 2, s, s, u * 0.12);
      ctx.fillStyle = fill;
      ctx.fill();
      ctx.strokeStyle = "rgba(220, 230, 255, 0.16)";
      ctx.lineWidth = Math.max(1, u * 0.012);
      ctx.stroke();
      // Fine diagonal hatching.
      ctx.save();
      ctx.clip();
      ctx.strokeStyle = "rgba(220, 230, 255, 0.05)";
      ctx.beginPath();
      for (let k = -4; k <= 4; k += 1) {
        ctx.moveTo(k * s * 0.18 - s, s);
        ctx.lineTo(k * s * 0.18 + s, -s);
      }
      ctx.stroke();
      ctx.restore();
      break;
    }
    case "target": {
      // A ring split into one arc per required channel; each arc is dim until its colour arrives.
      const r = u * 0.3;
      ctx.fillStyle = "rgba(4, 5, 9, 0.85)";
      ctx.beginPath();
      ctx.arc(0, 0, r + u * 0.05, 0, Math.PI * 2);
      ctx.fill();
      const bits = CHANNELS.filter((channel) => piece.color & channel);
      const gap = bits.length > 1 ? 0.22 : 0;
      ctx.lineWidth = Math.max(1, u * 0.045);
      ctx.lineCap = "round";
      bits.forEach((channel, k) => {
        const a0 = -Math.PI / 2 + (k * 2 * Math.PI) / bits.length + gap / 2;
        const a1 = -Math.PI / 2 + ((k + 1) * 2 * Math.PI) / bits.length - gap / 2;
        ctx.strokeStyle = rgba(CHANNEL_RGB[channel], 0.42);
        ctx.beginPath();
        ctx.arc(0, 0, r, a0, a1);
        ctx.stroke();
      });
      ctx.strokeStyle = "rgba(220, 230, 255, 0.18)";
      ctx.lineWidth = Math.max(1, u * 0.01);
      ctx.beginPath();
      ctx.arc(0, 0, r * 0.62, 0, Math.PI * 2);
      ctx.stroke();
      break;
    }
  }
  ctx.restore();
}

// A piece's light, drawn into the light layer: an emitter's aperture, a target's lit arcs and fill,
// a mirror's faint glint. `state` is the target's { mask, lit } from the tracer and `fill` (0–1)
// how far its lit core has grown; `on` (0–1) dims an emitter.
export function drawPieceLight(lit, layout, piece, { state, fill = state?.lit ? 1 : 0, on = 1, at } = {}) {
  const { cell: u } = layout;
  const [px, py] = at ?? layout.at(piece.x, piece.y);
  lit.save();
  lit.translate(px, py);
  switch (piece.kind) {
    case "emitter": {
      lit.rotate(angleOf(piece.dir) + (piece.rotation ?? 0));
      const x = u * 0.29 - u * 0.04;
      lit.globalAlpha = on;
      lit.fillStyle = maskColor(piece.color, 1);
      lit.beginPath();
      lit.arc(x, 0, u * 0.075, 0, Math.PI * 2);
      lit.fill();
      lit.fillStyle = rgba(CORE_RGB[piece.color] ?? [255, 255, 255], 1);
      lit.beginPath();
      lit.arc(x, 0, u * 0.04, 0, Math.PI * 2);
      lit.fill();
      break;
    }
    case "mirror": {
      lit.rotate(angleOf(piece.angle) + (piece.rotation ?? 0));
      lit.strokeStyle = "rgba(170, 190, 255, 0.22)";
      lit.lineWidth = u * 0.03;
      lit.lineCap = "round";
      lit.beginPath();
      lit.moveTo(-u * 0.4, 0);
      lit.lineTo(u * 0.4, 0);
      lit.stroke();
      break;
    }
    case "target": {
      const r = u * 0.3;
      const mask = state?.mask ?? 0;
      const bits = CHANNELS.filter((channel) => piece.color & channel);
      const gap = bits.length > 1 ? 0.22 : 0;
      // A colour the target does not want spoils it: its arcs only glow faintly.
      const spoilt = mask & ~piece.color ? 0.4 : 1;
      lit.lineWidth = u * 0.045;
      lit.lineCap = "round";
      bits.forEach((channel, k) => {
        if (!(mask & channel)) return;
        const a0 = -Math.PI / 2 + (k * 2 * Math.PI) / bits.length + gap / 2;
        const a1 = -Math.PI / 2 + ((k + 1) * 2 * Math.PI) / bits.length - gap / 2;
        lit.strokeStyle = rgba(CHANNEL_RGB[channel], spoilt);
        lit.beginPath();
        lit.arc(0, 0, r, a0, a1);
        lit.stroke();
      });
      if (fill > 0) {
        const f = ease.outExpo(fill);
        const core = lit.createRadialGradient(0, 0, 0, 0, 0, r * 0.75 * f);
        core.addColorStop(0, "rgba(255, 255, 255, 1)");
        core.addColorStop(0.35, maskColor(piece.color, 0.95));
        core.addColorStop(1, maskColor(piece.color, 0));
        lit.fillStyle = core;
        lit.beginPath();
        lit.arc(0, 0, r * 0.75 * f, 0, Math.PI * 2);
        lit.fill();
        lit.strokeStyle = maskColor(piece.color, 0.9 * f);
        lit.lineWidth = u * 0.02;
        lit.beginPath();
        lit.arc(0, 0, r * 0.62, 0, Math.PI * 2);
        lit.stroke();
      }
      break;
    }
  }
  lit.restore();
}

// --- Beams ----------------------------------------------------------------------------------------

// Draws traced beam segments into the light layer: a soft halo, a coloured line and a hot core per
// channel. Only the part of each segment between `from` and `progress` cells from its emitter is
// drawn: `progress` is how far a front has come (it gets a bright head while it moves) and `from`
// where a draining beam's tail has got to. Each is one number for every segment or an array with
// one per segment. Beams that start at an emitter in `emitters` leave from its aperture, not its
// centre. `intensity` scales all.
export function drawBeams(lit, layout, segments, { progress = Infinity, from = 0, intensity = 1, width = 1, emitters = [] } = {}) {
  const { cell: u } = layout;
  lit.save();
  lit.lineCap = "round";
  for (let i = 0; i < segments.length; i += 1) {
    const seg = segments[i];
    const to = typeof progress === "number" ? progress : progress[i];
    const start = typeof from === "number" ? from : from[i];
    if (to <= seg.d0 || start >= seg.d1 || to <= start) continue;
    const length = seg.d1 - seg.d0;
    const a = Math.max(0, (start - seg.d0) / length);
    const shown = Math.min(1, (to - seg.d0) / length);
    const [sx, sy] = layout.at(seg.x0, seg.y0);
    const [ex, ey] = layout.at(seg.x1, seg.y1);
    let x0 = sx + (ex - sx) * a;
    let y0 = sy + (ey - sy) * a;
    const x1 = sx + (ex - sx) * shown;
    const y1 = sy + (ey - sy) * shown;
    if (a === 0 && seg.d0 === 0 && emitters.some((piece) => piece.x === seg.x0 && piece.y === seg.y0)) {
      const [dx, dy] = DIRS[seg.dir];
      const k = (u * 0.25) / Math.hypot(dx, dy);
      x0 += dx * k;
      y0 += dy * k;
    }
    const color = CHANNEL_RGB[seg.mask];
    const core = CORE_RGB[seg.mask];
    const stroke = (style, lineWidth) => {
      lit.strokeStyle = style;
      lit.lineWidth = lineWidth;
      lit.beginPath();
      lit.moveTo(x0, y0);
      lit.lineTo(x1, y1);
      lit.stroke();
    };
    stroke(rgba(color, 0.16 * intensity), u * 0.2 * width);
    stroke(rgba(color, 0.95 * intensity), u * 0.072 * width);
    stroke(rgba(core, 0.85 * intensity), u * 0.02 * width);
    // A bright head on a front that is still moving.
    if (shown < 1) {
      const head = lit.createRadialGradient(x1, y1, 0, x1, y1, u * 0.22);
      head.addColorStop(0, rgba(core, intensity));
      head.addColorStop(1, rgba(color, 0));
      lit.fillStyle = head;
      lit.beginPath();
      lit.arc(x1, y1, u * 0.22, 0, Math.PI * 2);
      lit.fill();
    }
  }
  lit.restore();
}

// A small hot spot in the mixed colour of `mask` at each point: where a beam turns or splits
// (traced.bounces) or where colours cross (traced.crossings, with a larger `radius`, in cells).
export function drawGlints(lit, layout, points, { intensity = 1, radius = 0.18 } = {}) {
  const r = layout.cell * radius;
  for (const point of points) {
    const [x, y] = layout.at(point.x, point.y);
    const g = lit.createRadialGradient(x, y, 0, x, y, r);
    g.addColorStop(0, `rgba(255, 255, 255, ${0.7 * intensity})`);
    g.addColorStop(0.3, maskColor(point.mask, 0.45 * intensity));
    g.addColorStop(1, maskColor(point.mask, 0));
    lit.fillStyle = g;
    lit.beginPath();
    lit.arc(x, y, r, 0, Math.PI * 2);
    lit.fill();
  }
}

// An anamorphic lens streak through (x, y): a thin horizontal line of light that fades to the sides.
export function drawFlare(lit, x, y, { length, thickness, color = [180, 200, 255], intensity = 1 }) {
  const g = lit.createLinearGradient(x - length / 2, 0, x + length / 2, 0);
  g.addColorStop(0, rgba(color, 0));
  g.addColorStop(0.5, rgba(color, 0.55 * intensity));
  g.addColorStop(1, rgba(color, 0));
  lit.fillStyle = g;
  lit.beginPath();
  lit.ellipse(x, y, length / 2, thickness / 2, 0, 0, Math.PI * 2);
  lit.fill();
  const core = lit.createRadialGradient(x, y, 0, x, y, thickness * 3);
  core.addColorStop(0, `rgba(255, 255, 255, ${0.8 * intensity})`);
  core.addColorStop(1, "rgba(255, 255, 255, 0)");
  lit.fillStyle = core;
  lit.beginPath();
  lit.arc(x, y, thickness * 3, 0, Math.PI * 2);
  lit.fill();
}

// A whole board: grid, glass tiles, piece bodies into `ctx`, and beams, glints and piece lights
// into `lit`. `traced` is trace(board); `progress` limits the beam fronts (cells from emitters);
// `beamWidth` scales the beams' width.
export function drawBoard(ctx, lit, layout, traced, { progress = Infinity, intensity = 1, glints = true, grid = 1, beamWidth = 1 } = {}) {
  const { board } = layout;
  if (grid > 0) drawGrid(ctx, layout, { alpha: grid });
  for (const piece of board.pieces) drawTile(ctx, layout, piece.x, piece.y, { highlight: piece.movable ? 1 : 0 });
  for (const piece of board.pieces) drawPiece(ctx, layout, piece);
  const emitters = board.pieces.filter((piece) => piece.kind === "emitter");
  drawBeams(lit, layout, traced.segments, { progress, intensity, emitters, width: beamWidth });
  if (glints) {
    drawGlints(lit, layout, traced.bounces, { intensity });
    drawGlints(lit, layout, traced.crossings, { intensity, radius: 0.3 });
  }
  const states = new Map(traced.targets.map((target) => [`${target.x},${target.y}`, target]));
  for (const piece of board.pieces) drawPieceLight(lit, layout, piece, { state: states.get(`${piece.x},${piece.y}`) });
}

// --- Particles ------------------------------------------------------------------------------------

// A target's burst `age` seconds after it lit: sparks fly out, slow down and fade. Pure in its inputs.
export function drawBurst(lit, x, y, { age, mask, size, seed = 1, count = 28 }) {
  if (age < 0 || age > 1.6) return;
  const random = prng(seed);
  // A ring wave first.
  const wave = ease.outExpo(clamp(age / 0.7));
  if (age < 0.7) {
    lit.strokeStyle = maskColor(mask, 0.45 * (1 - wave) ** 2);
    lit.lineWidth = size * 0.05 * (1 - wave) + 0.5;
    lit.beginPath();
    lit.arc(x, y, size * (0.3 + 1.1 * wave), 0, Math.PI * 2);
    lit.stroke();
  }
  // Sparks in the target's channels and white: each flies out, slows down, shrinks and fades, and
  // the faster ones trail a short streak.
  const tints = [...CHANNELS.filter((channel) => mask & channel).map((channel) => CHANNEL_RGB[channel]), [255, 255, 255]];
  lit.lineCap = "round";
  for (let i = 0; i < count; i += 1) {
    const angle = random() * Math.PI * 2;
    const speed = 0.35 + random() ** 0.7 * 1.3;
    const life = 0.5 + random() * 0.9;
    const tint = tints[Math.floor(random() * tints.length)];
    const thick = size * (0.018 + random() * 0.03);
    const t = age / life;
    if (t >= 1) continue;
    const at = (s) => size * speed * (1 - Math.exp(-4.5 * s));
    const head = at(age);
    const tail = at(Math.max(0, age - 0.035 * speed));
    const cos = Math.cos(angle);
    const sin = Math.sin(angle);
    const fade = (1 - t) ** 1.4;
    lit.strokeStyle = rgba(tint, fade);
    lit.lineWidth = thick * (1 - 0.5 * t);
    lit.beginPath();
    lit.moveTo(x + cos * tail, y + sin * tail);
    lit.lineTo(x + cos * head + 0.01, y + sin * head);
    lit.stroke();
  }
}

// Slow drifting motes of light over a region, seeded; `time` in seconds moves them. `size` is the
// largest mote's radius in px.
export function drawMotes(lit, { width, height, count = 40, size, seed = 3, time = 0, alpha = 0.5 }) {
  const random = prng(seed);
  for (let i = 0; i < count; i += 1) {
    const x = (random() * width + time * size * 4 * (random() - 0.5) + width * 4) % width;
    const y = (random() * height - time * size * (2 + random() * 4) + height * 4) % height;
    const r = size * (0.3 + 0.7 * random() ** 3);
    const tint = CHANNEL_RGB[CHANNELS[i % 3]];
    lit.fillStyle = rgba(tint, alpha * (0.3 + random() * 0.7));
    lit.beginPath();
    lit.arc(x, y, r, 0, Math.PI * 2);
    lit.fill();
  }
}

// --- Cursor ---------------------------------------------------------------------------------------

// A soft fingertip: a translucent disc with a bright rim. `press` (0–1) shrinks it; `ripple` (0–1)
// is the progress of the ring that spreads out on a press.
export function drawCursor(ctx, x, y, { radius, press = 0, ripple = -1, alpha = 1 }) {
  ctx.save();
  ctx.globalAlpha *= alpha;
  const r = radius * (1 - 0.18 * press);
  const fill = ctx.createRadialGradient(x, y - r * 0.3, 0, x, y, r);
  fill.addColorStop(0, "rgba(255, 255, 255, 0.34)");
  fill.addColorStop(1, "rgba(255, 255, 255, 0.10)");
  ctx.fillStyle = fill;
  ctx.beginPath();
  ctx.arc(x, y, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "rgba(255, 255, 255, 0.75)";
  ctx.lineWidth = Math.max(1, radius * 0.07);
  ctx.stroke();
  if (ripple >= 0 && ripple < 1) {
    const k = ease.outExpo(ripple);
    ctx.strokeStyle = `rgba(255, 255, 255, ${0.6 * (1 - ripple)})`;
    ctx.lineWidth = Math.max(1, radius * 0.06 * (1 - ripple));
    ctx.beginPath();
    ctx.arc(x, y, radius * (1 + 1.4 * k), 0, Math.PI * 2);
    ctx.stroke();
  }
  ctx.restore();
}

// --- Type -----------------------------------------------------------------------------------------

// system-ui is SF Pro in Chrome on macOS (it does not know -apple-system). Chinese text sets its
// Latin letters and digits in SF Pro too, and its Han characters in PingFang SC.
export const FONTS = {
  en: 'system-ui, "Helvetica Neue", sans-serif',
  zh: 'system-ui, "PingFang SC", sans-serif',
};

// A caption in the language's typeface. `weight` defaults to regular for EN captions and medium for
// ZH. Returns its width.
export function drawCaption(ctx, text, x, y, { size, lang = "en", weight, align = "left", baseline = "alphabetic", color = "#ffffff", alpha = 1, letterSpacing = 0 }) {
  ctx.save();
  ctx.globalAlpha = alpha;
  ctx.font = `${weight ?? (lang === "zh" ? 500 : 400)} ${size}px ${FONTS[lang]}`;
  ctx.letterSpacing = `${letterSpacing}px`;
  ctx.textAlign = align;
  ctx.textBaseline = baseline;
  ctx.fillStyle = color;
  ctx.fillText(text, align === "center" ? x + letterSpacing / 2 : x, y);
  const width = ctx.measureText(text).width - letterSpacing;
  ctx.restore();
  return width;
}

// --- The mark -------------------------------------------------------------------------------------

// The mark's geometry, in units of its size with (0, 0) its centre: the three beams start at the
// left and meet in the core; the white beam leaves to the right. web/index.html's favicon is a
// simplified SVG of it for 16 px (solid lines, no ring).
const MARK = {
  core: [0.08, 0],
  starts: [
    [1, -0.44, -0.34],
    [2, -0.48, 0],
    [4, -0.44, 0.34],
  ],
  end: 0.46,
  ring: 0.135,
};

// The mark: red, green and blue beams converge from the left into a white core inside a target
// ring, and one white beam leaves it to the right. `size` is the mark's width in px; (x, y) its
// centre. Lines go into `lit` (a Light's layer), the crisp core and ring into ctx afterwards
// with drawMarkCore. `progress` (0–1) draws the beams in from their starts.
export function drawMark(lit, x, y, { size, intensity = 1, progress = 1 }) {
  const [cx, cy] = [x + MARK.core[0] * size, y + MARK.core[1] * size];
  lit.save();
  lit.lineCap = "round";
  const p = clamp(progress);
  for (const [channel, sx, sy] of MARK.starts) {
    const x0 = x + sx * size;
    const y0 = y + sy * size;
    const x1 = x0 + (cx - x0) * p;
    const y1 = y0 + (cy - y0) * p;
    const fade = lit.createLinearGradient(x0, y0, cx, cy);
    fade.addColorStop(0, rgba(CHANNEL_RGB[channel], 0));
    fade.addColorStop(0.3, rgba(CHANNEL_RGB[channel], intensity));
    fade.addColorStop(1, rgba(CHANNEL_RGB[channel], intensity));
    lit.strokeStyle = fade;
    lit.lineWidth = size * 0.052;
    lit.beginPath();
    lit.moveTo(x0, y0);
    lit.lineTo(x1, y1);
    lit.stroke();
    const hot = lit.createLinearGradient(x0, y0, cx, cy);
    hot.addColorStop(0.2, rgba(CORE_RGB[channel], 0));
    hot.addColorStop(0.55, rgba(CORE_RGB[channel], 0.9 * intensity));
    lit.strokeStyle = hot;
    lit.lineWidth = size * 0.016;
    lit.beginPath();
    lit.moveTo(x0, y0);
    lit.lineTo(x1, y1);
    lit.stroke();
  }
  if (p >= 1) {
    const endX = x + MARK.end * size;
    const out = lit.createLinearGradient(cx, 0, endX, 0);
    out.addColorStop(0, `rgba(255, 255, 255, ${intensity})`);
    out.addColorStop(0.6, `rgba(240, 244, 255, ${intensity})`);
    out.addColorStop(1, "rgba(220, 230, 255, 0)");
    lit.strokeStyle = out;
    lit.lineWidth = size * 0.056;
    lit.beginPath();
    lit.moveTo(cx, cy);
    lit.lineTo(endX, cy);
    lit.stroke();
    const glow = lit.createRadialGradient(cx, cy, 0, cx, cy, size * 0.17);
    glow.addColorStop(0, `rgba(255, 255, 255, ${0.55 * intensity})`);
    glow.addColorStop(0.5, `rgba(200, 212, 255, ${0.18 * intensity})`);
    glow.addColorStop(1, "rgba(190, 205, 255, 0)");
    lit.fillStyle = glow;
    lit.beginPath();
    lit.arc(cx, cy, size * 0.17, 0, Math.PI * 2);
    lit.fill();
  }
  lit.restore();
}

// The mark's crisp core and ring, drawn over the composited light.
export function drawMarkCore(ctx, x, y, { size, alpha = 1 }) {
  const [cx, cy] = [x + MARK.core[0] * size, y + MARK.core[1] * size];
  ctx.save();
  ctx.globalAlpha = alpha;
  ctx.fillStyle = "rgba(6, 7, 11, 0.5)";
  ctx.beginPath();
  ctx.arc(cx, cy, size * MARK.ring, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "rgba(255, 255, 255, 0.92)";
  ctx.lineWidth = Math.max(1, size * 0.022);
  ctx.beginPath();
  ctx.arc(cx, cy, size * MARK.ring, 0, Math.PI * 2);
  ctx.stroke();
  ctx.fillStyle = "#ffffff";
  ctx.beginPath();
  ctx.arc(cx, cy, size * 0.062, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

// An iOS-style app tile filling a size × size canvas: a continuous-corner rounded square of deep
// blue-black glass with a faint top rim, on near-black corners (iOS masks them off anyway).
export function drawAppTile(ctx, size) {
  ctx.save();
  ctx.fillStyle = "#020306";
  ctx.fillRect(0, 0, size, size);
  // A superellipse (n = 5) close to iOS's icon shape.
  const path = new Path2D();
  const n = 5;
  const half = size / 2;
  for (let k = 0; k <= 180; k += 1) {
    const a = (k / 180) * Math.PI * 2;
    const c = Math.cos(a);
    const s = Math.sin(a);
    const px = half + half * Math.sign(c) * Math.abs(c) ** (2 / n);
    const py = half + half * Math.sign(s) * Math.abs(s) ** (2 / n);
    if (k) path.lineTo(px, py);
    else path.moveTo(px, py);
  }
  path.closePath();
  const fill = ctx.createRadialGradient(half * 0.9, half * 0.7, 0, half, half, size * 0.75);
  fill.addColorStop(0, "#161b30");
  fill.addColorStop(0.55, "#0b0e19");
  fill.addColorStop(1, "#06070b");
  ctx.fillStyle = fill;
  ctx.fill(path);
  ctx.clip(path);
  const rim = ctx.createLinearGradient(0, 0, 0, size);
  rim.addColorStop(0, "rgba(200, 215, 255, 0.35)");
  rim.addColorStop(0.3, "rgba(200, 215, 255, 0.06)");
  rim.addColorStop(1, "rgba(200, 215, 255, 0.10)");
  ctx.strokeStyle = rim;
  ctx.lineWidth = Math.max(1.5, size * 0.016);
  ctx.stroke(path);
  ctx.restore();
}
