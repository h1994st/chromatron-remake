// The trailer's camera and film look: a virtual camera that sees a board as a plane in space
// (perspective, dolly, orbit, tilt, roll), depth of field, atmospheric haze and dust, and the
// finishing pass (grade, edge chromatic aberration, vignette, grain, letterbox).
//
// Everything is a pure function of its inputs. The expensive static layers (the haze) are drawn
// once per frame size and the scratch canvases are made once per size and reused, never per frame.

import { clamp, drawGrain, ease, prng, surface } from "./draw.mjs";

// --- Scratch canvases ----------------------------------------------------------------------------------

const scratches = new Map();
// A canvas for `name` at width × height, made on first use and reused.
function scratch(name, width, height) {
  const key = `${name}:${width}x${height}`;
  let found = scratches.get(key);
  if (!found) {
    found = surface(width, height);
    scratches.set(key, found);
  }
  return found;
}

// --- The camera ----------------------------------------------------------------------------------------

// A camera looking at a board plane. Board coordinates are cells (cell centres on integers); `h` is
// a height above the plane in cells, towards the camera. The camera orbits `look` (a board point):
//   yaw   turns the board in the picture (radians);
//   tilt  leans the camera from straight overhead (0) towards the board's lower edge, so the far
//         rows recede in perspective;
//   dist  is the camera's distance from `look` in cells (a dolly);
//   roll  turns the picture;
// and `cell` is the size in px of a cell at `look` seen from `dist` = `ref` (so dollying in grows
// it), with `look` landing on `center` in the frame.
export function camera({ look, yaw = 0, tilt = 0, dist = 9, roll = 0, cell, ref = 9, center }) {
  const focal = cell * ref;
  const cy = Math.cos(yaw);
  const sy = Math.sin(yaw);
  const ct = Math.cos(tilt);
  const st = Math.sin(tilt);
  const cr = Math.cos(roll);
  const sr = Math.sin(roll);
  // Board point → camera space → picture.
  const project = (x, y, h = 0) => {
    const dx = x - look[0];
    const dy = y - look[1];
    const bx = dx * cy - dy * sy;
    const by = dx * sy + dy * cy;
    const yc = by * ct - h * st;
    const zc = Math.max(0.6, dist - by * st - h * ct);
    const px = (focal * bx) / zc;
    const py = (focal * yc) / zc;
    return [center[0] + px * cr - py * sr, center[1] + px * sr + py * cr];
  };
  return {
    project,
    // The depth of a board point (for depth of field).
    depth: (x, y, h = 0) => {
      const dx = x - look[0];
      const dy = y - look[1];
      return dist - (dx * sy + dy * cy) * st - h * ct;
    },
    // The picture's px per cell at a board point, along the board's x and y: the local linear map
    // [a, b, c, d] (x axis to (a, b), y axis to (c, d)).
    jacobian(x, y, h = 0) {
      const e = 0.01;
      const [x0, y0] = project(x, y, h);
      const [x1, y1] = project(x + e, y, h);
      const [x2, y2] = project(x, y + e, h);
      return [(x1 - x0) / e, (y1 - y0) / e, (x2 - x0) / e, (y2 - y0) / e];
    },
    // The px per cell at `look`.
    scale: focal / dist,
    look,
    yaw,
    tilt,
    dist,
  };
}

// Runs `draw(at)` with `c` mapped so that a flat drawing `ref` px per cell, centred on `at`, lies on
// the board plane at (x, y, h) in perspective (the plane's local linear map there).
export function onPlane(c, cam, x, y, h, ref, draw) {
  const at = cam.project(x, y, h);
  const [a, b, cc, d] = cam.jacobian(x, y, h);
  c.save();
  c.translate(at[0], at[1]);
  c.transform(a / ref, b / ref, cc / ref, d / ref, 0, 0);
  c.translate(-at[0], -at[1]);
  draw(at);
  c.restore();
}

// Camera keys → the camera's settings at time t: { t, yaw, tilt, dist, look: [x, y], roll } keys are
// joined by cubic Hermite curves whose tangents run through the neighbouring keys, so the camera
// never stops at a key (every shot keeps moving) and eases only where the keys say so.
export function cameraAt(keys, t) {
  if (t <= keys[0].t) return extrapolate(keys, 0, t);
  const last = keys.length - 1;
  if (t >= keys[last].t) return extrapolate(keys, last, t);
  let i = 0;
  while (keys[i + 1].t < t) i += 1;
  const k0 = keys[i];
  const k1 = keys[i + 1];
  const span = k1.t - k0.t;
  const u = (t - k0.t) / span;
  const h00 = 2 * u ** 3 - 3 * u ** 2 + 1;
  const h10 = u ** 3 - 2 * u ** 2 + u;
  const h01 = -2 * u ** 3 + 3 * u ** 2;
  const h11 = u ** 3 - u ** 2;
  const value = (get) => {
    const m0 = tangent(keys, i, get);
    const m1 = tangent(keys, i + 1, get);
    return h00 * get(k0) + h10 * span * m0 + h01 * get(k1) + h11 * span * m1;
  };
  return {
    yaw: value((k) => k.yaw),
    tilt: value((k) => k.tilt),
    dist: value((k) => k.dist),
    roll: value((k) => k.roll ?? 0),
    look: [value((k) => k.look[0]), value((k) => k.look[1])],
  };
}
function tangent(keys, i, get) {
  const a = keys[Math.max(0, i - 1)];
  const b = keys[Math.min(keys.length - 1, i + 1)];
  return b.t === a.t ? 0 : (get(b) - get(a)) / (b.t - a.t);
}
function extrapolate(keys, i, t) {
  const k = keys[i];
  const drift = (get) => get(k) + tangent(keys, i, get) * (t - k.t);
  return { yaw: drift((c) => c.yaw), tilt: drift((c) => c.tilt), dist: drift((c) => c.dist), roll: drift((c) => c.roll ?? 0), look: [drift((c) => c.look[0]), drift((c) => c.look[1])] };
}

// --- Atmosphere ----------------------------------------------------------------------------------------

// The haze: soft cool clouds of seeded noise, drawn once per frame size at a quarter of it and
// stretched (it has no detail to lose).
function hazeLayer(width, height) {
  const w = Math.ceil(width / 4);
  const h = Math.ceil(height / 4);
  const layer = scratch("haze", w, h);
  if (layer.done) return layer;
  const c = layer.ctx;
  const random = prng(1234);
  c.clearRect(0, 0, w, h);
  for (let i = 0; i < 26; i += 1) {
    const x = random() * w;
    const y = random() * h;
    const r = (0.15 + random() * 0.35) * Math.max(w, h);
    const g = c.createRadialGradient(x, y, 0, x, y, r);
    const a = 0.025 + random() * 0.035;
    const tint = random() < 0.5 ? "70, 90, 150" : "90, 80, 150";
    g.addColorStop(0, `rgba(${tint}, ${a})`);
    g.addColorStop(1, `rgba(${tint}, 0)`);
    c.fillStyle = g;
    c.fillRect(0, 0, w, h);
  }
  layer.done = true;
  return layer;
}

// The world's backdrop: deep blue-black with a lifted cool floor, and the haze drifting slowly and
// shifted by `pan` (px) for parallax against the board; `glow` lifts it around (gx, gy).
export function drawAtmosphere(ctx, { width, height, t, pan = [0, 0], haze = 1, glow = 0.6, gx = width / 2, gy = height / 2 }) {
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalCompositeOperation = "source-over";
  ctx.globalAlpha = 1;
  ctx.fillStyle = "#040509";
  ctx.fillRect(0, 0, width, height);
  if (glow > 0) {
    const span = Math.hypot(width, height);
    const lift = ctx.createRadialGradient(gx, gy, 0, gx, gy, span * 0.5);
    lift.addColorStop(0, `rgba(24, 30, 60, ${0.38 * glow})`);
    lift.addColorStop(0.5, `rgba(12, 15, 30, ${0.22 * glow})`);
    lift.addColorStop(1, "rgba(5, 6, 10, 0)");
    ctx.fillStyle = lift;
    ctx.fillRect(0, 0, width, height);
  }
  if (haze > 0) {
    const layer = hazeLayer(width, height);
    const over = 1.25;
    const x = -width * 0.125 + pan[0] + Math.sin(t * 0.13) * width * 0.03;
    const y = -height * 0.125 + pan[1] + Math.cos(t * 0.11) * height * 0.03;
    ctx.globalAlpha = haze;
    ctx.globalCompositeOperation = "lighter";
    ctx.drawImage(layer.canvas, x, y, width * over, height * over);
  }
  ctx.restore();
}

// Dust in the air: motes at different depths that drift and slide with `pan` by their depth
// (parallax), faint until a beam passes near them, then lit in its colour. `beams` are picture-space
// segments [x0, y0, x1, y1, [r, g, b]].
export function drawDust(lit, { width, height, t, count = 70, seed = 5, pan = [0, 0], beams = [], reach = 70 }) {
  const random = prng(seed);
  for (let i = 0; i < count; i += 1) {
    const depth = 0.45 + random() * 1.6;
    const bx = random() * width;
    const by = random() * height;
    const vx = (random() - 0.5) * 14;
    const vy = -4 - random() * 10;
    const phase = random() * 6.28;
    const x = (((bx + vx * t + pan[0] / depth + Math.sin(t * 0.7 + phase) * 6) % width) + width) % width;
    const y = (((by + vy * t + pan[1] / depth) % height) + height) % height;
    let best = 0;
    let rgb = null;
    for (const [x0, y0, x1, y1, color] of beams) {
      const dx = x1 - x0;
      const dy = y1 - y0;
      const len2 = dx * dx + dy * dy || 1;
      const k = clamp(((x - x0) * dx + (y - y0) * dy) / len2);
      const d = Math.hypot(x - x0 - k * dx, y - y0 - k * dy);
      const near = 1 - d / reach;
      if (near > best) {
        best = near;
        rgb = color;
      }
    }
    const r = (0.7 + 1.6 / depth) * (1 + best);
    const a = 0.1 + 0.08 / depth + best * best * 0.85;
    const [cr, cg, cb] = rgb && best > 0 ? rgb.map((v) => v * best + 200 * (1 - best)) : [190, 205, 255];
    lit.fillStyle = `rgba(${cr | 0},${cg | 0},${cb | 0},${a})`;
    lit.beginPath();
    lit.arc(x, y, r, 0, Math.PI * 2);
    lit.fill();
  }
}

// --- Depth of field ------------------------------------------------------------------------------------

// Blurs the frame by `radius` px where `mask` says (0 sharp, 1 fully blurred): `mask` is
// { from: [x, y], to: [x, y], stops: [[offset, amount]…] } (a linear ramp across the picture) or a
// number for the whole frame. The blur is made at half size and laid over the sharp frame.
export function depthOfField(ctx, { width, height, radius, mask }) {
  if (radius <= 0.3) return;
  const w = Math.ceil(width / 2);
  const h = Math.ceil(height / 2);
  const half = scratch("dof", w, h);
  const c = half.ctx;
  c.save();
  c.setTransform(1, 0, 0, 1, 0, 0);
  c.globalCompositeOperation = "copy";
  c.globalAlpha = 1;
  c.filter = `blur(${(radius / 2).toFixed(2)}px)`;
  c.drawImage(ctx.canvas, 0, 0, w, h);
  c.filter = "none";
  c.globalCompositeOperation = "destination-in";
  if (typeof mask === "number") {
    c.fillStyle = `rgba(0,0,0,${clamp(mask)})`;
  } else {
    const g = c.createLinearGradient(mask.from[0] / 2, mask.from[1] / 2, mask.to[0] / 2, mask.to[1] / 2);
    for (const [offset, amount] of mask.stops) g.addColorStop(clamp(offset), `rgba(0,0,0,${clamp(amount)})`);
    c.fillStyle = g;
  }
  c.fillRect(0, 0, w, h);
  c.restore();
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalCompositeOperation = "source-over";
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(half.canvas, 0, 0, width, height);
  ctx.restore();
}

// --- The finish ----------------------------------------------------------------------------------------

// The grade's tone curve, per channel: a toe that keeps blacks deep, a floor lifted a little and
// cool, and a shoulder over the top fifth that rolls highlights off below pure white instead of
// clipping hard.
const LIFT = [0.004, 0.007, 0.016];
function curve(x, channel) {
  const knee = 0.8;
  const toe = x ** 1.14;
  // Above the knee the slope falls steadily from the toe's to 0.6, so white lands at about 0.95.
  const kneeY = knee ** 1.14;
  const slope = 1.14 * knee ** 0.14;
  const u = Math.max(0, x - knee) / (1 - knee);
  const shoulder = kneeY + (1 - knee) * (slope * u + ((0.6 - slope) * u * u) / 2);
  const y = x <= knee ? toe : shoulder;
  return LIFT[channel] + (1 - LIFT[channel]) * y;
}

let gradeReady = false;
// Puts the grade into the page as an SVG filter (a lookup table per channel), once.
function gradeFilter() {
  if (gradeReady || typeof document === "undefined") return gradeReady;
  const table = (channel) =>
    Array.from({ length: 33 }, (_, i) => curve(i / 32, channel).toFixed(4)).join(" ");
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("width", "0");
  svg.setAttribute("height", "0");
  svg.style.position = "absolute";
  svg.innerHTML = `<filter id="trailer-grade" color-interpolation-filters="sRGB"><feComponentTransfer><feFuncR type="table" tableValues="${table(0)}"/><feFuncG type="table" tableValues="${table(1)}"/><feFuncB type="table" tableValues="${table(2)}"/></feComponentTransfer></filter>`;
  document.body.appendChild(svg);
  gradeReady = true;
  return true;
}

// The finishing pass over the whole frame: the grade, chromatic aberration growing towards the
// edges (`aberration`: the red and blue images' scale difference), a vignette, grain (seeded by
// `frame`) and letterbox bars `bars` px tall at the top and bottom.
export function finish(ctx, { width, height, frame, aberration = 0.0032, vignette = 1, grain = 0.05, bars = 0 }) {
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalAlpha = 1;
  if (gradeFilter()) {
    const copy = scratch("grade", width, height);
    copy.ctx.globalCompositeOperation = "copy";
    copy.ctx.drawImage(ctx.canvas, 0, 0);
    ctx.globalCompositeOperation = "copy";
    ctx.filter = "url(#trailer-grade)";
    ctx.drawImage(copy.canvas, 0, 0);
    ctx.filter = "none";
  }
  if (aberration > 0) {
    // Red and blue copies of the frame, scaled about its centre by ± aberration / 2, added back
    // onto its green: the fringes grow with the distance from the centre.
    for (const [name, color] of [
      ["ca-red", "#ff0000"],
      ["ca-blue", "#0000ff"],
    ]) {
      const layer = scratch(name, width, height);
      layer.ctx.globalCompositeOperation = "copy";
      layer.ctx.drawImage(ctx.canvas, 0, 0);
      layer.ctx.globalCompositeOperation = "multiply";
      layer.ctx.fillStyle = color;
      layer.ctx.fillRect(0, 0, width, height);
    }
    ctx.globalCompositeOperation = "multiply";
    ctx.fillStyle = "#00ff00";
    ctx.fillRect(0, 0, width, height);
    ctx.globalCompositeOperation = "lighter";
    const grow = aberration / 2;
    ctx.drawImage(scratch("ca-red", width, height).canvas, (-width * grow) / 2, (-height * grow) / 2, width * (1 + grow), height * (1 + grow));
    ctx.drawImage(scratch("ca-blue", width, height).canvas, (width * grow) / 2, (height * grow) / 2, width * (1 - grow), height * (1 - grow));
  }
  ctx.globalCompositeOperation = "source-over";
  if (vignette > 0) {
    const span = Math.hypot(width, height);
    const shade = ctx.createRadialGradient(width / 2, height / 2, span * 0.22, width / 2, height / 2, span * 0.6);
    shade.addColorStop(0, "rgba(0, 0, 3, 0)");
    shade.addColorStop(1, `rgba(0, 0, 3, ${0.72 * vignette})`);
    ctx.fillStyle = shade;
    ctx.fillRect(0, 0, width, height);
  }
  ctx.restore();
  if (grain > 0) drawGrain(ctx, { width, height, amount: grain, seed: 7, shift: [(frame * 73) % 256, (frame * 151) % 256] });
  if (bars > 0.5) {
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = "#000000";
    ctx.fillRect(0, 0, width, bars);
    ctx.fillRect(0, height - bars, width, bars);
    ctx.restore();
  }
}

// The letterbox bars' height for a 2.39:1 picture inside width × height, opened by `open` (0–1).
export function letterbox(width, height, open = 0) {
  const full = Math.max(0, (height - width / 2.39) / 2);
  return full * (1 - ease.inOutCubic(clamp(open)));
}
