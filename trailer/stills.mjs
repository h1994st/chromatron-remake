// The brand stills, drawn with the trailer's own pieces (trailer/draw.mjs) and lettering
// (trailer/lettering.mjs): the social preview, the touch icon, the banner, the two portrait covers and three
// video thumbnails. `just social-images` renders them through stage.html.

import { BLUE, BOARDS, GREEN, RED, WHITE, trace } from "./board.mjs";
import { boardLayout, drawAppTile, drawBackground, drawBoard, drawBurst, drawCaption, drawFlare, drawMark, drawMarkCore, drawMotes, lightFor, prng, rgba } from "./draw.mjs";
import { drawBeamPath, drawLettering, drawSparkle, drawWordmark, drawZhLine, layoutLine } from "./lettering.mjs";
import { encodeQr } from "./qr.mjs";

const hero = trace(BOARDS.hero);

// The lit white target's burst, frozen mid-flight, and a lens streak through it.
function heroAccents(lit, layout, { burst = 0.16, flare = 1 } = {}) {
  const white = hero.targets.find((target) => target.color === 7);
  const [x, y] = layout.at(white.x, white.y);
  drawBurst(lit, x, y, { age: burst, mask: 7, size: layout.cell * 1.15, seed: 21, count: 16 });
  if (flare > 0) drawFlare(lit, x, y, { length: layout.cell * 6, thickness: layout.cell * 0.045, intensity: flare });
}

// The cover's "+": a red beam that comes in from the left edge and a blue one from above cross in
// a white star, drawn as the + glyph's two strokes. (cx, cy) is the crossing; `arm` its half-size.
function crossing(lit, cx, cy, arm, t, fromX) {
  const beam = (x0, y0, x1, y1, rgb, fadeFrom) => {
    const path = new Path2D();
    path.moveTo(x0, y0);
    path.lineTo(x1, y1);
    lit.save();
    const fade = lit.createLinearGradient(fadeFrom[0], fadeFrom[1], x1, y1);
    fade.addColorStop(0, `rgba(${rgb},0)`);
    fade.addColorStop(0.55, `rgba(${rgb},0.9)`);
    fade.addColorStop(1, `rgba(${rgb},0.95)`);
    lit.lineCap = "round";
    lit.strokeStyle = fade;
    lit.lineWidth = 1.5 * t;
    lit.stroke(path);
    lit.restore();
  };
  beam(fromX, cy, cx - arm, cy, "255,34,52", [fromX, cy]);
  const plus = new Path2D();
  plus.moveTo(cx - arm, cy);
  plus.lineTo(cx + arm, cy);
  plus.moveTo(cx, cy - arm);
  plus.lineTo(cx, cy + arm);
  drawBeamPath(lit, plus, t);
  drawSparkle(lit, cx, cy, arm * 1.5, 1);
}

// The covers' headline, set as one block: CHROMATRON / + AI / REVERSE ENGINEER, each line as wide
// as the first (`blockWidth`), with the same beam width throughout; `k` scales the gaps, the + AI
// line and the beam from the 1080-wide cover's. Returns the bottom of the block.
function coverHeadline(lit, cx, top, blockWidth, k = 1) {
  const t = 4.2 * k;
  const gap = 64 * k;
  const capFor = (text, track) => blockWidth / layoutLine(text, { track }).width;
  const capTop = capFor("CHROMATRON", 0.36);
  drawLettering(lit, "CHROMATRON", cx, top, { cap: capTop, track: 0.36, align: "center", stroke: t, target: 3 });
  const capMid = 170 * k;
  const midTop = top + capTop + gap;
  const mid = drawLettering(lit, "+AI", cx, midTop, { cap: capMid, track: 0.42, align: "center", stroke: t * 1.25, skip: [0] });
  const plus = mid.glyphs[0];
  crossing(lit, plus.left + plus.width / 2, midTop + capMid / 2, plus.width / 2, t * 1.25, 0);
  const capLow = capFor("REVERSE ENGINEER", 0.3);
  const lowTop = midTop + capMid + gap;
  drawLettering(lit, "REVERSE ENGINEER", cx, lowTop, { cap: capLow, track: 0.3, align: "center", stroke: t });
  return lowTop + capLow;
}

// The page's address, which the English cover's QR code opens.
const PAGE_URL = "https://shengtuo.me/chromatron-remake/";

// A QR code of `text` on a lit panel: dark modules `module` px square (whole pixels, so a scanner
// sees clean edges) on a pale glass tile with the four-module quiet zone, its finders' eyes in the
// lasers' red, green and blue, ringed by a white beam. The beam and the panel's glow go into
// `lit`; call drawQrPanel after the light's composite with the same box for the crisp code.
function qrBox(text, x, y, module) {
  const code = encodeQr(text, { level: "Q" });
  const side = (code.size + 8) * module;
  return { code, x, y, side, module };
}
function drawQrLight(lit, { x, y, side }, t) {
  drawGlow(lit, x + side / 2, y + side / 2, side * 0.95, [150, 175, 255], 0.35);
  const ring = new Path2D();
  const pad = t * 4;
  ring.roundRect(x - pad, y - pad, side + 2 * pad, side + 2 * pad, side * 0.09 + pad);
  drawBeamPath(lit, ring, t);
}
function drawQrPanel(ctx, { code, x, y, side, module }) {
  ctx.save();
  ctx.fillStyle = "#eef2ff";
  ctx.beginPath();
  ctx.roundRect(x, y, side, side, side * 0.09);
  ctx.fill();
  const eyes = [[3, 3, "#d8102a"], [code.size - 4, 3, "#008f48"], [3, code.size - 4, "#1d4cf0"]];
  for (let my = 0; my < code.size; my += 1) {
    for (let mx = 0; mx < code.size; mx += 1) {
      if (!code.dark(mx, my)) continue;
      const eye = eyes.find(([ex, ey]) => Math.abs(mx - ex) <= 1 && Math.abs(my - ey) <= 1);
      ctx.fillStyle = eye ? eye[2] : "#0b0e19";
      ctx.fillRect(x + (mx + 4) * module, y + (my + 4) * module, module, module);
    }
  }
  ctx.restore();
}

// The thumbnails' boards, invented for them. A's: red from the top left and green from the top
// right each come round two mirrors, blue runs in along the bottom through its filter and turns
// up, and all three meet as white in the middle.
const CONVERGE = {
  w: 13,
  h: 5,
  pieces: [
    { x: 0, y: 0, kind: "emitter", color: RED, dir: 0 },
    { x: 3, y: 0, kind: "mirror", angle: 1, movable: true },
    { x: 3, y: 2, kind: "mirror", angle: 1, movable: true },
    { x: 12, y: 0, kind: "emitter", color: GREEN, dir: 4 },
    { x: 9, y: 0, kind: "mirror", angle: 3, movable: true },
    { x: 9, y: 2, kind: "mirror", angle: 3, movable: true },
    { x: 1, y: 4, kind: "emitter", color: BLUE, dir: 0 },
    { x: 3, y: 4, kind: "filter", color: BLUE },
    { x: 6, y: 4, kind: "mirror", angle: 3, movable: true },
    { x: 6, y: 2, kind: "target", color: WHITE },
    { x: 1, y: 2, kind: "wall" },
    { x: 11, y: 3, kind: "wall" },
  ],
};
// B's close-up: red up from below and green in from the right meet in a yellow target.
const MEET = {
  w: 5,
  h: 5,
  pieces: [
    { x: 2, y: 4, kind: "emitter", color: RED, dir: 6 },
    { x: 4, y: 1, kind: "emitter", color: GREEN, dir: 4 },
    { x: 2, y: 1, kind: "target", color: RED | GREEN },
  ],
};

// Light rays fanning out of (x, y): `count` thin wedges of `rgb` up to `length` px long, seeded.
function drawRays(lit, x, y, { length, count, rgb, alpha = 0.22, seed = 5, spread = 0.05 }) {
  const random = prng(seed);
  for (let i = 0; i < count; i += 1) {
    const a = (i / count) * Math.PI * 2 + random() * 0.3;
    const reach = length * (0.45 + random() * 0.55);
    const half = spread * (0.5 + random());
    const g = lit.createRadialGradient(x, y, 0, x, y, reach);
    g.addColorStop(0, rgba(rgb, alpha));
    g.addColorStop(1, rgba(rgb, 0));
    lit.fillStyle = g;
    lit.beginPath();
    lit.moveTo(x, y);
    lit.arc(x, y, reach, a - half, a + half);
    lit.closePath();
    lit.fill();
  }
}

// A soft disc of light: `rgb` at `alpha` in the middle, fading out by `r`.
function drawGlow(lit, x, y, r, rgb, alpha = 1) {
  const g = lit.createRadialGradient(x, y, 0, x, y, r);
  g.addColorStop(0, rgba(rgb, alpha));
  g.addColorStop(0.3, rgba(rgb, alpha * 0.35));
  g.addColorStop(1, rgba(rgb, 0));
  lit.fillStyle = g;
  lit.beginPath();
  lit.arc(x, y, r, 0, Math.PI * 2);
  lit.fill();
}

// Each still: { width, height, draw(ctx, size) }.
export const STILLS = {
  og: {
    width: 1200,
    height: 630,
    draw(ctx, { width, height }) {
      const light = lightFor(width, height);
      const cell = 74;
      const layout = boardLayout(BOARDS.hero, { cx: width - 70 - 3.5 * cell, cy: height / 2, cell });
      drawBackground(ctx, { width, height, cx: layout.left + layout.width / 2, cy: height / 2, grain: 0.035 });
      const lit = light.begin();
      drawBoard(ctx, lit, layout, hero);
      heroAccents(lit, layout);
      const left = 72;
      drawWordmark(lit, left, 206, { width: 500, stroke: 2.6 });
      light.composite(ctx, { glowRadius: 6, bloomRadius: 40, bloom: 0.75 });
      drawCaption(ctx, "Laser puzzles · now in your browser", left, 370, { size: 28, weight: 500, color: "rgba(236, 240, 255, 0.96)" });
      drawCaption(ctx, "and on Apple Silicon", left, 410, { size: 28, weight: 500, color: "rgba(236, 240, 255, 0.96)" });
      drawCaption(ctx, "shengtuo.me/chromatron-remake", left, height - 66, { size: 19, weight: 500, color: "rgba(160, 174, 214, 0.9)", letterSpacing: 0.4 });
    },
  },
  icon: {
    width: 180,
    height: 180,
    draw(ctx, { width }) {
      const light = lightFor(width, width);
      drawAppTile(ctx, width);
      const lit = light.begin();
      const size = width * 0.8;
      drawMark(lit, width / 2 - size * 0.02, width / 2, { size });
      light.composite(ctx, { glowRadius: 3, bloomRadius: 20, bloom: 0.75 });
      drawMarkCore(ctx, width / 2 - size * 0.02, width / 2, { size });
    },
  },
  banner: {
    width: 1920,
    height: 1080,
    draw(ctx, { width, height }) {
      const light = lightFor(width, height);
      const cell = 118;
      const layout = boardLayout(BOARDS.hero, { cx: width - 150 - 3.5 * cell, cy: height / 2, cell });
      drawBackground(ctx, { width, height, cx: layout.left + layout.width / 2, cy: height / 2, grain: 0.05 });
      const lit = light.begin();
      drawMotes(lit, { width, height, size: 2.2, count: 70, seed: 9, alpha: 0.55 });
      drawBoard(ctx, lit, layout, hero);
      heroAccents(lit, layout);
      const left = 140;
      drawWordmark(lit, left, 396, { width: 820, stroke: 3.6 });
      light.composite(ctx, { glowRadius: 9, bloomRadius: 70, bloom: 0.75 });
      drawCaption(ctx, "Laser puzzles · Chromatron 1–4", left, 630, { size: 40, weight: 600 });
      drawCaption(ctx, "Free in your browser · native on Apple Silicon", left, 686, { size: 32, color: "rgba(214, 222, 250, 0.9)" });
      drawCaption(ctx, "shengtuo.me/chromatron-remake", left, height - 110, { size: 26, weight: 500, color: "rgba(160, 174, 214, 0.9)", letterSpacing: 0.6 });
    },
  },
  "cover-portrait": {
    width: 1080,
    height: 1440,
    draw(ctx, { width, height }) {
      const light = lightFor(width, height);
      const cell = 86;
      const boardY = 1052;
      const layout = boardLayout(BOARDS.hero, { cx: width / 2, cy: boardY, cell });
      drawBackground(ctx, { width, height, cx: width / 2, cy: 420, grain: 0.05 });
      const lit = light.begin();
      drawMotes(lit, { width, height, size: 2, count: 50, seed: 4, alpha: 0.5 });
      drawBoard(ctx, lit, layout, hero);
      heroAccents(lit, layout);
      const headlineBottom = coverHeadline(lit, width / 2, 150, 900);
      const cx = width / 2;
      const zhTop = headlineBottom + 86;
      drawZhLine(ctx, lit, "用 AI 逆向工程，重制 20 多年前的经典激光解谜", cx, zhTop, { size: 40, weight: 600, align: "center", color: "#ffffff", glow: 0.18 });
      drawZhLine(ctx, lit, "Chromatron 1–4 · 浏览器免费玩", cx, zhTop + 58, { size: 30, weight: 500, align: "center", color: "rgba(200, 212, 245, 0.92)", glow: 0 });
      light.composite(ctx, { glowRadius: 9, bloomRadius: 70, bloom: 0.75 });
      drawCaption(ctx, "shengtuo.me/chromatron-remake", cx, height - 46, { size: 22, weight: 500, align: "center", color: "rgba(160, 174, 214, 0.9)", letterSpacing: 0.6 });
    },
  },
  // The English cover, 1440 × 2400: the headline, the board, and a QR code to the page with the
  // address beside it. `scan` is what reel.mjs reads back from the rendered code.
  "cover-portrait-en": {
    width: 1440,
    height: 2400,
    scan: PAGE_URL,
    draw(ctx, { width, height }) {
      const light = lightFor(width, height);
      const cx = width / 2;
      const cell = 116;
      const layout = boardLayout(BOARDS.hero, { cx, cy: 1500, cell });
      drawBackground(ctx, { width, height, cx, cy: 560, grain: 0.05 });
      const lit = light.begin();
      drawMotes(lit, { width, height, size: 2.4, count: 80, seed: 4, alpha: 0.5 });
      drawBoard(ctx, lit, layout, hero);
      heroAccents(lit, layout);
      const headlineBottom = coverHeadline(lit, cx, 230, 1200, 4 / 3);
      const subTop = headlineBottom + 116;
      drawZhLine(ctx, lit, "Remaking a 20-year-old laser puzzle classic", cx, subTop, { size: 52, weight: 600, spacing: 0.01, align: "center", color: "#ffffff", glow: 0.18 });
      drawZhLine(ctx, lit, "Chromatron 1–4 · Free in your browser", cx, subTop + 76, { size: 40, weight: 500, spacing: 0.01, align: "center", color: "rgba(200, 212, 245, 0.92)", glow: 0 });
      // The footer: SCAN TO PLAY and the address on the left, a beam arrow into the code on the right.
      const qr = qrBox(PAGE_URL, width - 120 - 41 * 8, 2010, 8);
      drawQrLight(lit, qr, 3.2);
      const left = 120;
      const scan = drawLettering(lit, "SCAN TO PLAY", left, qr.y + 80, { cap: 50, track: 0.3, stroke: 4.6 });
      const arrowY = qr.y + 80 + 25;
      const arrowFrom = scan.left + scan.width + 40;
      const arrowTo = qr.x - 36;
      const arrow = new Path2D();
      arrow.moveTo(arrowFrom, arrowY);
      arrow.lineTo(arrowTo, arrowY);
      arrow.moveTo(arrowTo - 20, arrowY - 20);
      arrow.lineTo(arrowTo, arrowY);
      arrow.lineTo(arrowTo - 20, arrowY + 20);
      drawBeamPath(lit, arrow, 4.6);
      light.composite(ctx, { glowRadius: 12, bloomRadius: 90, bloom: 0.75 });
      drawQrPanel(ctx, qr);
      drawCaption(ctx, "shengtuo.me/chromatron-remake", left, qr.y + 222, { size: 34, weight: 500, color: "rgba(200, 212, 245, 0.92)", letterSpacing: 0.6 });
    },
  },
  // Three 1280 × 720 video thumbnails, each testing its own idea. The player covers the
  // bottom-right corner (right of x 1050 and below y 612) with the video's length, so nothing that
  // matters goes there, and every line is lettered heavy enough to read at 160 × 90.
  // A, the brand: the wordmark, big, over a wide board whose three beams meet in a white burst.
  "thumbnail-a": {
    width: 1280,
    height: 720,
    draw(ctx, { width, height }) {
      const light = lightFor(width, height);
      const cell = 96;
      const layout = boardLayout(CONVERGE, { cx: width / 2, cy: 470, cell });
      const [tx, ty] = layout.at(6, 2);
      drawBackground(ctx, { width, height, cx: tx, cy: ty, grain: 0.04 });
      const lit = light.begin();
      drawMotes(lit, { width, height, size: 2, count: 50, seed: 11, alpha: 0.5 });
      drawBoard(ctx, lit, layout, trace(CONVERGE), { beamWidth: 1.5 });
      drawRays(lit, tx, ty, { length: 460, count: 30, rgb: [220, 230, 255], alpha: 0.13, seed: 3 });
      drawGlow(lit, tx, ty, cell * 1.5, [255, 255, 255], 0.7);
      drawBurst(lit, tx, ty, { age: 0.22, mask: WHITE, size: cell * 2.2, seed: 21, count: 34 });
      drawFlare(lit, tx, ty, { length: width * 1.1, thickness: cell * 0.07, intensity: 1.1 });
      drawSparkle(lit, tx, ty, cell * 1.4, 1);
      drawWordmark(lit, width / 2, 58, { width: 1120, align: "center", stroke: 7 });
      light.composite(ctx, { glowRadius: 8, bloomRadius: 60, bloom: 0.8 });
    },
  },
  // B, the hook: a close-up of red and green meeting in a yellow blast, and the sum it poses.
  "thumbnail-b": {
    width: 1280,
    height: 720,
    draw(ctx, { width, height }) {
      const light = lightFor(width, height);
      const cell = 250;
      const [tx, ty] = [985, 290];
      const layout = boardLayout(MEET, { cx: tx, cy: ty + cell, cell });
      drawBackground(ctx, { width, height, cx: tx, cy: ty, grain: 0.04 });
      const lit = light.begin();
      drawBoard(ctx, lit, layout, trace(MEET), { grid: 0.6 });
      const yellow = [255, 220, 40];
      drawRays(lit, tx, ty, { length: 620, count: 36, rgb: yellow, alpha: 0.2, seed: 8, spread: 0.06 });
      drawGlow(lit, tx, ty, 300, yellow, 0.8);
      drawGlow(lit, tx, ty, 110, [255, 236, 90], 0.9);
      drawBurst(lit, tx, ty, { age: 0.2, mask: RED | GREEN, size: 320, seed: 5, count: 44 });
      const left = 64;
      const cap = 132;
      const stroke = 9;
      const track = 0.32;
      const white = [255, 255, 255];
      const advance = (text) => layoutLine(text, { track }).width * cap + (0.1 + track) * cap;
      drawLettering(lit, "RED", left, 64, { cap, track, stroke, tint: [255, 40, 56] });
      drawLettering(lit, "+", left + advance("RED") + 0.34 * cap, 64, { cap, track, stroke, tint: white });
      drawLettering(lit, "GREEN", left, 64 + cap * 1.36, { cap, track, stroke, tint: [30, 255, 110] });
      // The = and a bigger ?, on one baseline.
      const sumTop = 64 + cap * 3.02;
      const big = cap * 1.35;
      drawLettering(lit, "=", left, sumTop, { cap, track, stroke, tint: white });
      drawLettering(lit, "?", left + advance("=") + 0.3 * cap, sumTop + cap - big, { cap: big, track, stroke: stroke * 1.15, tint: yellow });
      light.composite(ctx, { glowRadius: 8, bloomRadius: 60, bloom: 0.85 });
    },
  },
  // C, the story: a 2003 game, reverse-engineered with AI in 2026, by a small board.
  "thumbnail-c": {
    width: 1280,
    height: 720,
    draw(ctx, { width, height }) {
      const light = lightFor(width, height);
      const cell = 40;
      const layout = boardLayout(BOARDS.hero, { cx: 1100, cy: 354, cell });
      drawBackground(ctx, { width, height, cx: 640, cy: 300, grain: 0.04 });
      const lit = light.begin();
      drawMotes(lit, { width, height, size: 2, count: 40, seed: 6, alpha: 0.5 });
      drawBoard(ctx, lit, layout, hero);
      heroAccents(lit, layout, { flare: 0.6 });
      const left = 64;
      const cap = 88;
      const stroke = 7;
      const track = 0.26;
      drawWordmark(lit, left, 86, { width: 420, sub: "", stroke: 3.4 });
      const yearsTop = 204;
      const old = drawLettering(lit, "2003", left, yearsTop, { cap, track, stroke, intensity: 0.45, strand: 0.5, glints: false });
      const arrowFrom = old.left + old.width + cap * 0.35;
      const arrowTo = arrowFrom + cap * 1.5;
      const mid = yearsTop + cap / 2;
      const arrow = new Path2D();
      arrow.moveTo(arrowFrom, mid);
      arrow.lineTo(arrowTo, mid);
      arrow.moveTo(arrowTo - cap * 0.3, mid - cap * 0.3);
      arrow.lineTo(arrowTo, mid);
      arrow.lineTo(arrowTo - cap * 0.3, mid + cap * 0.3);
      drawBeamPath(lit, arrow, stroke);
      drawLettering(lit, "2026", arrowTo + cap * 0.35, yearsTop, { cap, track, stroke });
      drawLettering(lit, "AI REVERSE", left, yearsTop + cap * 1.55, { cap, track, stroke });
      drawLettering(lit, "ENGINEERED", left, yearsTop + cap * 3.1, { cap, track, stroke });
      light.composite(ctx, { glowRadius: 8, bloomRadius: 60, bloom: 0.8 });
    },
  },
};

// Draws still `name` onto ctx (whose canvas is { width, height }).
export function drawStill(ctx, name, size) {
  const still = STILLS[name];
  if (!still) throw new Error(`no still named ${name}`);
  still.draw(ctx, size);
}
