// A tiny beam tracer for the trailer's invented boards (no level from the original games), and the
// boards themselves. Pure and deterministic: the same board always gives the same beams.
//
// A board is { w, h, pieces: [{ x, y, kind, ... }] } with cells (0, 0) at the top left. Kinds:
//   emitter  { color, dir }    fires a beam of `color` (bitmask R = 1, G = 2, B = 4) towards `dir`
//   mirror   { angle }         a two-sided mirror whose line runs along direction `angle` (0–7, so
//                              angle and angle + 4 are the same mirror); a beam along it is blocked
//   splitter { dir, spread }   takes a beam travelling `dir` and sends it on at dir ± spread (default 2,
//                              90° each way); beams from any other side are blocked
//   filter   { color }         passes only the beam's bits that are in `color`
//   wall                       blocks
//   target   { color }         absorbs beams; lit when the incoming masks OR to exactly `color`
// Any piece may carry `movable: true`, which only changes how it is drawn.
//
// Directions run clockwise from east with y pointing down: 0 E, 1 SE, 2 S, 3 SW, 4 W, 5 NW, 6 N, 7 NE.

export const DIRS = [
  [1, 0],
  [1, 1],
  [0, 1],
  [-1, 1],
  [-1, 0],
  [-1, -1],
  [0, -1],
  [1, -1],
];
export const RED = 1;
export const GREEN = 2;
export const BLUE = 4;
export const WHITE = 7;
// The single-colour channels of a mask, in drawing order.
export const CHANNELS = [RED, GREEN, BLUE];

const turn = (dir, by) => (((dir + by) % 8) + 8) % 8;

// Traces every emitter's beam. Coordinates are in cells, with cell centres on integers.
// Returns:
//   edges:    each straight step a beam takes, merged where beams share it in the same direction, as
//             { x0, y0, x1, y1, dir, mask, d0, d1 }; d0 and d1 are the distances from the emitter
//             (in cells) at each end, so a front at distance `progress` has drawn the part below it
//   segments: one per colour channel and maximal straight run, { x0, y0, x1, y1, dir, mask, d0, d1 }
//             with a single-bit mask: draw each additively and colours add up where beams cross
//   targets:  { x, y, color, mask, lit, reach } in board order; reach[channel] is the distance at
//             which that channel's beam arrives
//   bounces:  { x, y, mask } where a beam turned or split (mirrors, splitters) or was filtered
//   crossings: { x, y, mask } at empty cells where beams of different colours cross
//   solved:   every target is lit
export function trace(board) {
  const cells = new Map();
  for (const piece of board.pieces) cells.set(piece.y * board.w + piece.x, piece);
  const inside = (x, y) => x >= 0 && y >= 0 && x < board.w && y < board.h;
  const incoming = new Map();
  const reaches = new Map();
  const edgeMap = new Map();
  const bounceMap = new Map();
  const seen = new Set();

  // Edges are kept per direction: two beams running opposite ways along one step are two edges,
  // each with its own direction and distances, so both of their fronts animate the right way.
  const addEdge = (x0, y0, x1, y1, dir, mask, d0, d1) => {
    const key = `${x0},${y0},${x1},${y1}`;
    const found = edgeMap.get(key);
    if (found) {
      found.mask |= mask;
      found.d0 = Math.min(found.d0, d0);
      found.d1 = Math.min(found.d1, d1);
    } else {
      edgeMap.set(key, { x0, y0, x1, y1, dir, mask, d0, d1 });
    }
  };
  const addBounce = (x, y, mask) => {
    const key = y * board.w + x;
    const found = bounceMap.get(key);
    if (found) found.mask |= mask;
    else bounceMap.set(key, { x, y, mask });
  };

  // Rays leave the centre of (x, y) towards dir, `dist` cells from their emitter.
  const queue = [];
  for (const piece of board.pieces) {
    if (piece.kind === "emitter") queue.push({ x: piece.x, y: piece.y, dir: piece.dir, mask: piece.color, dist: 0 });
  }
  while (queue.length) {
    const ray = queue.shift();
    const state = `${ray.x},${ray.y},${ray.dir},${ray.mask}`;
    if (seen.has(state)) continue;
    seen.add(state);
    const [dx, dy] = DIRS[ray.dir];
    const step = dx && dy ? Math.SQRT2 : 1;
    const nx = ray.x + dx;
    const ny = ray.y + dy;
    const piece = cells.get(ny * board.w + nx);
    // Stops at the boundary of the next cell: the board's edge, or a piece the beam cannot enter.
    const stop = () => addEdge(ray.x, ray.y, ray.x + dx / 2, ray.y + dy / 2, ray.dir, ray.mask, ray.dist, ray.dist + step / 2);
    const enter = () => addEdge(ray.x, ray.y, nx, ny, ray.dir, ray.mask, ray.dist, ray.dist + step);
    const go = (dir, mask = ray.mask) => queue.push({ x: nx, y: ny, dir, mask, dist: ray.dist + step });
    if (!inside(nx, ny)) {
      stop();
      continue;
    }
    switch (piece?.kind) {
      case undefined:
        enter();
        go(ray.dir);
        break;
      case "mirror":
        if (ray.dir % 4 === piece.angle % 4) {
          stop();
        } else {
          enter();
          addBounce(nx, ny, ray.mask);
          go(turn(2 * piece.angle, -ray.dir));
        }
        break;
      case "splitter":
        if (ray.dir !== piece.dir) {
          stop();
        } else {
          enter();
          addBounce(nx, ny, ray.mask);
          go(turn(piece.dir, piece.spread ?? 2));
          go(turn(piece.dir, -(piece.spread ?? 2)));
        }
        break;
      case "filter": {
        enter();
        const mask = ray.mask & piece.color;
        if (mask !== ray.mask) addBounce(nx, ny, ray.mask);
        if (mask) go(ray.dir, mask);
        break;
      }
      case "target": {
        enter();
        const key = ny * board.w + nx;
        incoming.set(key, (incoming.get(key) ?? 0) | ray.mask);
        const reach = reaches.get(key) ?? {};
        for (const channel of CHANNELS) if (ray.mask & channel) reach[channel] = Math.min(reach[channel] ?? Infinity, ray.dist + step);
        reaches.set(key, reach);
        break;
      }
      default:
        stop();
    }
  }

  const edges = [...edgeMap.values()];
  // Empty cells where beams of different colours cross: the light adds up there.
  const through = new Map();
  for (const edge of edges) {
    for (const [x, y] of [
      [edge.x0, edge.y0],
      [edge.x1, edge.y1],
    ]) {
      if (!Number.isInteger(x) || !Number.isInteger(y) || cells.has(y * board.w + x)) continue;
      const key = y * board.w + x;
      const point = through.get(key) ?? { x, y, axes: new Map() };
      point.axes.set(edge.dir % 4, (point.axes.get(edge.dir % 4) ?? 0) | edge.mask);
      through.set(key, point);
    }
  }
  const crossings = [];
  for (const { x, y, axes } of through.values()) {
    const masks = [...axes.values()];
    const mask = masks.reduce((all, m) => all | m, 0);
    if (masks.length > 1 && masks.some((m) => m !== mask)) crossings.push({ x, y, mask });
  }
  const targets = board.pieces
    .filter((piece) => piece.kind === "target")
    .map((piece) => {
      const key = piece.y * board.w + piece.x;
      const mask = incoming.get(key) ?? 0;
      return { x: piece.x, y: piece.y, color: piece.color, mask, lit: mask === piece.color, reach: reaches.get(key) ?? {} };
    });
  return {
    edges,
    segments: runs(edges),
    targets,
    bounces: [...bounceMap.values()],
    crossings,
    solved: targets.length > 0 && targets.every((target) => target.lit),
  };
}

// Joins each channel's collinear, touching edges into straight runs, oriented as the beam travels.
function runs(edges) {
  const segments = [];
  for (const channel of CHANNELS) {
    // Edges of this channel by the point they start from (in travel order) and their direction.
    const from = new Map();
    const pending = [];
    for (const edge of edges) {
      if (!(edge.mask & channel)) continue;
      const own = { ...edge, mask: channel };
      from.set(`${own.x0},${own.y0},${own.dir}`, own);
      pending.push(own);
    }
    const into = new Set(pending.map((edge) => `${edge.x1},${edge.y1},${edge.dir}`));
    for (const edge of pending) {
      // Start a run only at an edge no same-direction edge leads into.
      if (into.has(`${edge.x0},${edge.y0},${edge.dir}`)) continue;
      const run = { ...edge };
      for (let next = from.get(`${run.x1},${run.y1},${run.dir}`); next; next = from.get(`${run.x1},${run.y1},${run.dir}`)) {
        run.x1 = next.x1;
        run.y1 = next.y1;
        run.d1 = next.d1;
      }
      segments.push(run);
    }
  }
  return segments;
}

// --- Playback: a board changing over time -------------------------------------------------------------

// The trace of a board with no light at all, to light a board up from.
export const DARK = Object.freeze({ edges: [], segments: [], targets: [], bounces: [], crossings: [], solved: false });

// Every channel's steps of a trace, as keys.
function stepKeys(traced) {
  const keys = new Set();
  for (const edge of traced.edges) {
    for (const channel of CHANNELS) if (edge.mask & channel) keys.add(`${channel}:${edge.x0},${edge.y0}>${edge.x1},${edge.y1}`);
  }
  return keys;
}

// The distance from its emitter at which a segment's first step that is not in `keys` starts, or
// Infinity when every step is.
function firstMissing(segment, keys) {
  const [dx, dy] = DIRS[segment.dir];
  const steps = dx ? (segment.x1 - segment.x0) / dx : (segment.y1 - segment.y0) / dy;
  const step = dx && dy ? Math.SQRT2 : 1;
  for (let k = 0; k < steps; k += 1) {
    const end = Math.min(k + 1, steps);
    const key = `${segment.mask}:${segment.x0 + k * dx},${segment.y0 + k * dy}>${segment.x0 + end * dx},${segment.y0 + end * dy}`;
    if (!keys.has(key)) return segment.d0 + k * step;
  }
  return Infinity;
}

// How the beams change from `before` to `after` (two traces). Light travels at a finite speed, so
// the new beams grow from where they part from the old ones and the old ones drain away from there:
//   fresh[i]: where after.segments[i] starts to be new (Infinity if it is all old);
//   stale[i]: where before.segments[i] starts to be gone (Infinity if it all stays);
//   grow[channel], drain[channel]: the nearest such point of each channel, where its front starts.
// `t` seconds after the change, at `speed` cells per second, a new segment shows up to
// max(fresh, grow + speed·t) and an old one from max(stale, drain + speed·t) on (scenes.mjs
// drawPlay).
export function beamChange(before, after) {
  const old = stepKeys(before);
  const kept = stepKeys(after);
  const fresh = after.segments.map((segment) => firstMissing(segment, old));
  const stale = before.segments.map((segment) => firstMissing(segment, kept));
  const nearest = (segments, marks) => {
    const out = { [RED]: Infinity, [GREEN]: Infinity, [BLUE]: Infinity };
    segments.forEach((segment, i) => (out[segment.mask] = Math.min(out[segment.mask], marks[i])));
    return out;
  };
  return { fresh, stale, grow: nearest(after.segments, fresh), drain: nearest(before.segments, stale) };
}

// How long a change takes to settle at `speed`: until the last new front and the last old tail
// have left the board.
function settleTime(before, after, change, speed) {
  let far = 0;
  after.segments.forEach((segment, i) => {
    if (change.fresh[i] < Infinity) far = Math.max(far, segment.d1 - change.grow[segment.mask]);
  });
  before.segments.forEach((segment, i) => {
    if (change.stale[i] < Infinity) far = Math.max(far, segment.d1 - change.drain[segment.mask]);
  });
  return far / speed;
}

const cellOf = (pieces, [x, y]) => {
  const index = pieces.findIndex((piece) => piece.x === x && piece.y === y);
  if (index < 0) throw new Error(`no piece at ${x}, ${y}`);
  return index;
};

// A turned copy of a piece: `by` 45° steps clockwise.
function turned(piece, by) {
  if (piece.kind === "mirror") return { ...piece, angle: (((piece.angle + by) % 8) + 8) % 8 };
  if (piece.kind === "splitter" || piece.kind === "emitter") return { ...piece, dir: turn(piece.dir, by) };
  return piece;
}

// Plays scripted moves on a board. The board lights up from dark at `start`; then each move is
//   { t, kind: "drag", from: [x, y], to: [x, y], dur }  picked up at t and dropped at t + dur
//   { t, kind: "rotate", at: [x, y], by = 1 }            turned `by` 45° steps clockwise at t
// Beam fronts travel at `speed` cells per second, and each change must settle before the next.
// Returns
//   states:  [{ t, cause, board, traced, before, change, changed, move }] in time order: cause is
//            "start", "grab", "drop" or "rotate"; changed says whether any beam moved;
//   targets: [{ x, y, color, changes: [{ t, mask }] }]: the colours each target receives, and when.
export function playback(board, moves, { start, speed }) {
  const states = [];
  let pieces = board.pieces;
  let before = DARK;
  const push = (t, cause, move) => {
    const next = { ...board, pieces };
    const traced = trace(next);
    const change = beamChange(before, traced);
    const changed = change.fresh.some((d) => d < Infinity) || change.stale.some((d) => d < Infinity);
    const last = states[states.length - 1];
    if (last && t < last.t + last.settle) throw new Error(`a ${cause} at ${t} s comes before the change at ${last.t} s has settled`);
    states.push({ t, cause, board: next, traced, before, change, changed, move, settle: settleTime(before, traced, change, speed) });
    before = traced;
  };
  push(start, "start", null);
  for (const move of [...moves].sort((a, b) => a.t - b.t)) {
    if (move.kind === "drag") {
      const index = cellOf(pieces, move.from);
      const piece = pieces[index];
      pieces = pieces.filter((_, i) => i !== index);
      push(move.t, "grab", { ...move, piece });
      pieces = [...pieces, { ...piece, x: move.to[0], y: move.to[1] }];
      push(move.t + move.dur, "drop", { ...move, piece });
    } else if (move.kind === "rotate") {
      const index = cellOf(pieces, move.at);
      pieces = pieces.map((piece, i) => (i === index ? turned(piece, move.by ?? 1) : piece));
      push(move.t, "rotate", move);
    } else {
      throw new Error(`no move kind ${move.kind}`);
    }
  }

  // Each target's colours over time: a channel arrives when its new front reaches the target and
  // leaves when its old tail has passed.
  const targets = board.pieces
    .filter((piece) => piece.kind === "target")
    .map((piece) => ({ x: piece.x, y: piece.y, color: piece.color, changes: [] }));
  for (const target of targets) {
    let mask = 0;
    for (const state of states) {
      const find = (traced) => traced.targets.find((other) => other.x === target.x && other.y === target.y)?.reach ?? {};
      const was = find(state.before);
      const now = find(state.traced);
      const times = [];
      for (const channel of CHANNELS) {
        if (was[channel] !== undefined && was[channel] === now[channel]) continue;
        if (was[channel] !== undefined) times.push([state.t + Math.max(0, was[channel] - state.change.drain[channel]) / speed, channel, false]);
        if (now[channel] !== undefined) times.push([state.t + Math.max(0, now[channel] - state.change.grow[channel]) / speed, channel, true]);
      }
      times.sort((a, b) => a[0] - b[0]);
      for (const [t, channel, on] of times) {
        mask = on ? mask | channel : mask & ~channel;
        target.changes.push({ t, mask });
      }
    }
  }
  return { states, targets };
}

// The state of a playback at time t (the last that has begun), or null before the first.
export function stateAt(states, t) {
  let found = null;
  for (const state of states) if (state.t <= t) found = state;
  return found;
}

// A target's received colours at time t, and since when it has been lit (or null).
export function targetAt(target, t) {
  let mask = 0;
  let litAt = null;
  for (const change of target.changes) {
    if (change.t > t) break;
    if (change.mask === target.color && mask !== target.color) litAt = change.t;
    if (change.mask !== target.color) litAt = null;
    mask = change.mask;
  }
  return { mask, litAt };
}

// The trailer's boards: all invented for it.
export const BOARDS = {
  // The hero board of the stills: red, green and blue meet as white in the middle. Red crosses blue
  // (magenta where they cross), splits and comes back through a mirror; green splits too, so a red
  // and a green target light as well.
  hero: {
    w: 7,
    h: 7,
    pieces: [
      { x: 0, y: 4, kind: "emitter", color: RED, dir: 0 },
      { x: 5, y: 4, kind: "splitter", dir: 0, movable: true },
      { x: 5, y: 3, kind: "mirror", angle: 1, movable: true },
      { x: 5, y: 6, kind: "target", color: RED },
      { x: 6, y: 1, kind: "emitter", color: GREEN, dir: 4 },
      { x: 3, y: 1, kind: "splitter", dir: 4, movable: true },
      { x: 3, y: 0, kind: "target", color: GREEN },
      { x: 0, y: 6, kind: "emitter", color: BLUE, dir: 7 },
      { x: 1, y: 5, kind: "filter", color: BLUE },
      { x: 3, y: 3, kind: "target", color: WHITE },
      { x: 1, y: 1, kind: "wall" },
      { x: 6, y: 5, kind: "wall" },
    ],
  },
  // "Bend the light": red runs east, turns south and leaves the board; a mirror dropped on its
  // path at (4, 4) sends it west into the red target.
  bend: {
    w: 6,
    h: 6,
    pieces: [
      { x: 0, y: 1, kind: "emitter", color: RED, dir: 0 },
      { x: 4, y: 1, kind: "mirror", angle: 1 },
      { x: 1, y: 4, kind: "target", color: RED },
      { x: 1, y: 2, kind: "mirror", angle: 3, movable: true },
      { x: 0, y: 4, kind: "wall" },
      { x: 5, y: 5, kind: "wall" },
      { x: 2, y: 0, kind: "wall" },
    ],
  },
  // "Mix the colours": red splits to a yellow target at the top and a white one at the bottom;
  // green splits to both too, but its upper half is blocked until the mirror at (1, 0) turns; blue
  // reaches the white target once the mirror at (6, 6) turns.
  mix: {
    w: 7,
    h: 7,
    pieces: [
      { x: 0, y: 3, kind: "emitter", color: RED, dir: 0 },
      { x: 3, y: 3, kind: "splitter", dir: 0 },
      { x: 3, y: 0, kind: "target", color: RED | GREEN },
      { x: 3, y: 6, kind: "target", color: WHITE },
      { x: 6, y: 1, kind: "emitter", color: GREEN, dir: 4 },
      { x: 1, y: 1, kind: "splitter", dir: 4 },
      { x: 1, y: 0, kind: "mirror", angle: 2, movable: true },
      { x: 1, y: 6, kind: "mirror", angle: 1 },
      { x: 6, y: 3, kind: "emitter", color: BLUE, dir: 2 },
      { x: 6, y: 6, kind: "mirror", angle: 2, movable: true },
      { x: 5, y: 4, kind: "wall" },
    ],
  },
  // "Split": a cyan beam meets the side of a prism; turned to face it, the prism splits it into
  // two cyan targets at once.
  split: {
    w: 6,
    h: 6,
    pieces: [
      { x: 0, y: 2, kind: "emitter", color: GREEN | BLUE, dir: 0 },
      { x: 3, y: 2, kind: "splitter", dir: 7, movable: true },
      { x: 3, y: 0, kind: "target", color: GREEN | BLUE },
      { x: 3, y: 4, kind: "target", color: GREEN | BLUE },
      { x: 5, y: 5, kind: "wall" },
      { x: 0, y: 5, kind: "wall" },
    ],
  },
  // "Filter": white light overloads a magenta target until a magenta filter takes its green out.
  filter: {
    w: 6,
    h: 6,
    pieces: [
      { x: 0, y: 1, kind: "emitter", color: WHITE, dir: 0 },
      { x: 4, y: 1, kind: "mirror", angle: 1 },
      { x: 4, y: 5, kind: "target", color: RED | BLUE },
      { x: 1, y: 4, kind: "filter", color: RED | BLUE, movable: true },
      { x: 0, y: 3, kind: "wall" },
      { x: 5, y: 0, kind: "wall" },
    ],
  },
  // "Solve": one white beam; a prism dropped in its path feeds two more, and filters turn the four
  // branches red, green, blue and white. Every target is 8 cells from the emitter, so all four
  // light at once.
  solve: {
    w: 7,
    h: 7,
    pieces: [
      { x: 0, y: 3, kind: "emitter", color: WHITE, dir: 0 },
      { x: 3, y: 1, kind: "splitter", dir: 6 },
      { x: 3, y: 5, kind: "splitter", dir: 2 },
      { x: 5, y: 1, kind: "filter", color: RED },
      { x: 1, y: 1, kind: "filter", color: GREEN },
      { x: 1, y: 5, kind: "filter", color: BLUE },
      { x: 6, y: 1, kind: "target", color: RED },
      { x: 0, y: 1, kind: "target", color: GREEN },
      { x: 0, y: 5, kind: "target", color: BLUE },
      { x: 6, y: 5, kind: "target", color: WHITE },
      { x: 5, y: 4, kind: "splitter", dir: 0, movable: true },
    ],
  },
};
