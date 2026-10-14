// The trailer's storyboard as data, directed like a film trailer: scenes and their shots, title
// cards between them, captions (English and Chinese, always shown together), the invented puzzles
// with their scripted moves, camera moves, speed ramps, and the sound cues. scenes.mjs draws from
// it and score.mjs plays to it; the cues are derived from the same scenes, moves and cameras as the
// picture, so sound and picture cannot drift. Pure data, runs in Node and in the browser.
//
// 120 BPM: a beat is 0.5 s and a bar 2 s. Cuts and title hits land on beats.

import { BOARDS, playback, targetAt } from "./board.mjs";

export const DURATION = 30;
export const FPS = 60;
export const BPM = 120;
export const BEAT = 60 / BPM;
export const BAR = 4 * BEAT;
// How fast a beam front travels, in cells per second of story time.
export const BEAM_SPEED = 18;

// The intro: near darkness with a single drifting spark; at `ignite` it fires the red beam (with a
// lens flare), which bounces off two mirrors and draws the wordmark's underline. A held breath,
// then the title hits at `impact` and the Chinese title follows at `zh`.
export const INTRO = { spark: 0.12, ignite: 0.8, speed: 1700, impact: 2, zh: 2.4 };

// The facts: the letterbox opens, four game cards land on beats, then the level counter rolls from
// `count` to `counted`.
export const FACTS = { open: 19, cards: [19.5, 20, 20.5, 21], count: 21.5, counted: 23 };

// The platforms: the device outlines draw on from `outline`, their screens light at `screens`, and
// the end riser builds from `riser` into the final hit.
export const PLATFORMS = { outline: 24.05, screens: 24.6, riser: 26.4 };

// The end card: the mark's three beams converge from `converge`, the white beam writes the wordmark
// from `write` and the final flash lands at `hit`; then the rest settles in.
export const END = { converge: 28, write: 28.25, hit: 28.85, zh: 29, url: 29.1, credit: 29.3 };

// How a caption's lettering is written on: from `delay` s after its `at`, over `length` s. A caption
// may set its own `write`.
const TITLE_WRITE = { delay: 0.04, length: 0.58 };

// Camera keys: { t, yaw, tilt, dist, look: [x, y], roll } with angles in degrees, `dist` in cells
// and `look` a board point (lens.mjs). Each shot's keys run from its start to its end.
const key = (t, yaw, tilt, dist, look, roll = 0) => ({ t, yaw, tilt, dist, look, roll });

// Each scene: { name, start, end, kind, caption?, puzzle?, shots?, ramps? }.
//   kind     "intro", "title" (a title card on near-black), "board", "facts", "platforms", "end";
//   caption  { en, zh, at, lines? }: `en` is lettered by a beam (upper case) and `zh` typeset under
//            it once the English is written; `lines` breaks `en` per shape; `sub` is a typeset
//            English line under the lettering; `word` marks a montage word laid over its shot;
//            `write` overrides TITLE_WRITE (captionWrite);
//   puzzle   { board, start, moves } in story time (board.mjs's playback);
//   shots    [{ start, end, keys }]: the camera, cutting between shots on beats;
//   ramps    speed ramps [storyTime, frameLength, slowest]: from the frame where story time reaches
//            `storyTime`, time slows to `slowest` and back over `frameLength` s of frames.
export const SCENES = [
  {
    name: "intro",
    kind: "intro",
    start: 0,
    end: 3,
    caption: { en: "CHROMATRON · REMAKE", zh: "CHROMATRON 重制版", at: INTRO.impact },
  },
  {
    name: "bend-title",
    kind: "title",
    start: 3,
    end: 4,
    caption: { en: "Bend the light.", zh: "折射光线", at: 3, lines: { landscape: ["Bend the light."], portrait: ["Bend the", "light."] } },
  },
  {
    name: "bend",
    kind: "board",
    start: 4,
    end: 8,
    puzzle: { board: BOARDS.bend, start: 4.05, moves: [{ t: 5.2, kind: "drag", from: [1, 2], to: [4, 4], dur: 0.7 }] },
    ramps: [[5.92, 0.95, 0.18]],
    shots: [
      {
        start: 4,
        end: 8,
        keys: [
          key(4, -34, 64, 5, [0.4, 1.1], -5),
          key(4.9, -15, 36, 7.4, [1.8, 1.9], -1.5),
          key(5.6, -5, 14, 8.4, [2.5, 2.6]),
          key(6.4, -1, 9, 7.2, [2.3, 3.4]),
          key(7.2, 9, 26, 7.8, [2.5, 2.9], 1),
          key(8, 22, 44, 7.1, [2.5, 2.5], 2.5),
        ],
      },
    ],
  },
  {
    name: "mix-title",
    kind: "title",
    start: 8,
    end: 9,
    caption: { en: "Mix the colours.", zh: "混合色彩", at: 8, lines: { landscape: ["Mix the colours."], portrait: ["Mix the", "colours."] } },
  },
  {
    name: "mix",
    kind: "board",
    start: 9,
    end: 13,
    puzzle: {
      board: BOARDS.mix,
      start: 9.02,
      moves: [
        { t: 10.1, kind: "rotate", at: [1, 0] },
        { t: 11.4, kind: "rotate", at: [6, 6] },
      ],
    },
    ramps: [
      [10.12, 0.5, 0.45],
      [11.42, 0.95, 0.18],
    ],
    shots: [
      {
        start: 9,
        end: 13,
        keys: [
          key(9, 40, 52, 6.6, [1.4, 1.2], 4),
          key(9.9, 16, 28, 8.2, [2.2, 2], 1),
          key(10.8, 5, 13, 9.4, [3, 3]),
          key(11.6, -2, 9, 9, [3.6, 3.8]),
          key(12.3, -9, 16, 8, [3.3, 4.4], -1),
          key(13, -22, 40, 7.6, [3, 3.6], -3),
        ],
      },
    ],
  },
  // The montage, "Split. Filter. Solve." / 分光 · 滤色 · 通关: each word hits over its board out of
  // focus, focus racks to the board, and a cut on the next beat moves in close for the move.
  {
    name: "split",
    kind: "board",
    start: 13,
    end: 15,
    caption: { en: "Split.", zh: "分光", at: 13, word: true },
    puzzle: { board: BOARDS.split, start: 13.02, moves: [{ t: 14.1, kind: "rotate", at: [3, 2] }] },
    shots: [
      { start: 13, end: 14, keys: [key(13, 0, 52, 9.6, [2.5, 2.5]), key(14, 9, 40, 8.6, [2.6, 2.3], 1)] },
      { start: 14, end: 15, keys: [key(14, -32, 50, 5, [3, 2.1], -2), key(15, -44, 40, 4.3, [3, 2], -3)] },
    ],
  },
  {
    name: "filter",
    kind: "board",
    start: 15,
    end: 17,
    caption: { en: "Filter.", zh: "滤色", at: 15, word: true },
    puzzle: { board: BOARDS.filter, start: 15.02, moves: [{ t: 15.6, kind: "drag", from: [1, 4], to: [4, 3], dur: 0.5 }] },
    shots: [
      { start: 15, end: 16, keys: [key(15, -10, 48, 9.6, [2.5, 2.5]), key(16, -2, 30, 8.4, [3, 3])] },
      { start: 16, end: 17, keys: [key(16, 26, 56, 4.9, [4, 3.8], 2), key(17, 38, 44, 4.2, [4, 4.3], 3)] },
    ],
  },
  {
    name: "solve",
    kind: "board",
    start: 17,
    end: 19,
    caption: { en: "Solve.", zh: "通关", at: 17, word: true },
    puzzle: { board: BOARDS.solve, start: 17.02, moves: [{ t: 17.6, kind: "drag", from: [5, 4], to: [3, 3], dur: 0.5 }] },
    ramps: [[18.12, 0.8, 0.2]],
    shots: [
      { start: 17, end: 18, keys: [key(17, 20, 50, 9.2, [3, 3]), key(18, 8, 24, 8.4, [3, 3])] },
      { start: 18, end: 19, keys: [key(18, 0, 4, 7, [3, 3]), key(18.5, -2, 6, 8.2, [3, 3]), key(19, -8, 12, 10.8, [3, 3], -1)] },
    ],
  },
  {
    name: "facts",
    kind: "facts",
    start: 19,
    end: 24,
    // The spec's "4 classic games · 200 levels", lettered short; the counter rolls to 200 in it.
    // "· 200 LEVELS" writes from FACTS.count, over the same length.
    caption: {
      en: "4 games · 200 levels",
      zh: "4 部经典 · 200 个关卡",
      at: 19,
      write: { delay: 0.1, length: 0.55 },
      lines: { landscape: ["4 games · 200 levels"], portrait: ["4 games", "200 levels"] },
    },
    shots: [{ start: 19, end: 24, keys: [key(19, -20, 60, 10.5, [0, 0.9], -2), key(21, -7, 27, 12, [0, 0.3]), key(24, 7, 13, 12.8, [0, 0], 1)] }],
  },
  {
    name: "platforms",
    kind: "platforms",
    start: 24,
    end: 28,
    caption: {
      en: "Play free in your browser",
      sub: "Native on Apple Silicon",
      zh: "浏览器免费畅玩 · Apple Silicon 原生运行",
      at: 24.5,
      write: { delay: 0, length: 0.6 },
      lines: { landscape: ["Play free in your browser"], portrait: ["Play free in", "your browser"] },
    },
  },
  {
    name: "end",
    kind: "end",
    start: 28,
    end: 30,
    caption: { en: "CHROMATRON · REMAKE", zh: "CHROMATRON 重制版", at: END.write },
  },
];

// Each scene's start, by name: the sections the score is arranged in.
export const START = Object.fromEntries(SCENES.map((scene) => [scene.name, scene.start]));

// A caption's write window: it writes from `from` (frame time), `delay` s after its `at`, over
// `length` s.
export function captionWrite(caption) {
  const { delay, length } = caption.write ?? TITLE_WRITE;
  return { from: caption.at + delay, delay, length };
}

// The end card's small print, in both languages.
export const END_TEXT = {
  url: "shengtuo.me/chromatron-remake",
  credit: { en: "Original games by Sean Barrett", zh: "原作 Sean Barrett" },
  notice: { en: "Fan-made remake, not affiliated with Silver Spaceship Software", zh: "粉丝重制，与 Silver Spaceship Software 无关" },
};

// The letterbox: 2.39:1 for the dramatic shots, opening to the full frame from the facts on.
export const LETTERBOX = { open: FACTS.open, length: 0.6 };

// --- Speed ramps ---------------------------------------------------------------------------------------

// A speed ramp's lag: how far story time has fallen behind frame time, u (0–1) of the way through
// a ramp of `length` s whose speed dips to `slowest` as 1 − (1 − slowest)·sin²(πu).
const rampLag = (u, length, slowest) => (1 - slowest) * length * (Math.min(1, Math.max(0, u)) / 2 - Math.sin(2 * Math.PI * Math.min(1, Math.max(0, u))) / (4 * Math.PI));

// A scene's ramps in frame time: [{ from, length, slowest }], each placed where story time reaches
// its mark given the ramps before it.
function placeRamps(scene) {
  const placed = [];
  for (const [storyMark, length, slowest] of scene.ramps ?? []) {
    const lagBefore = placed.reduce((sum, ramp) => sum + rampLag(1, ramp.length, ramp.slowest), 0);
    placed.push({ from: storyMark + lagBefore, length, slowest });
  }
  return placed;
}
const RAMPS = new Map(SCENES.map((scene) => [scene.name, placeRamps(scene)]));

// Story time at frame time t in a scene: the time its puzzle, bursts and cursor run on.
export function storyTime(scene, t) {
  let lag = 0;
  for (const ramp of RAMPS.get(scene.name)) lag += rampLag((t - ramp.from) / ramp.length, ramp.length, ramp.slowest);
  return t - lag;
}

// The frame time at which a scene's story time reaches τ (story time never runs backwards).
export function frameTime(scene, tau) {
  let lo = scene.start - 1;
  let hi = scene.end + 2;
  for (let i = 0; i < 60; i += 1) {
    const mid = (lo + hi) / 2;
    if (storyTime(scene, mid) < tau) lo = mid;
    else hi = mid;
  }
  return hi;
}

// Each board scene's playback (board.mjs), by scene name, in story time.
export const PLAYS = Object.fromEntries(SCENES.filter((scene) => scene.puzzle).map((scene) => [scene.name, playback(scene.puzzle.board, scene.puzzle.moves, { start: scene.puzzle.start, speed: BEAM_SPEED })]));

// The scene at frame time t, and its shot.
export function sceneAt(t) {
  for (const scene of SCENES) if (t < scene.end) return scene;
  return SCENES[SCENES.length - 1];
}
export function shotAt(scene, t) {
  if (!scene.shots) return null;
  for (const shot of scene.shots) if (t < shot.end) return shot;
  return scene.shots[scene.shots.length - 1];
}

// --- Cues ----------------------------------------------------------------------------------------------

const COLOUR_NAMES = { 1: "red", 2: "green", 3: "yellow", 4: "blue", 5: "magenta", 6: "cyan", 7: "white" };

// The sound cues, { t, kind, colour?, dur?, scene }, in time order (frame time). Kinds:
//   cut      a hard cut between scenes or shots (a whoosh);
//   whoosh   a fast camera move or the letterbox opening, over `dur` s;
//   riser    a build of `dur` s into the hit at its end;
//   impact   a big hit: the intro title, each montage word, the counter reaching 200;
//   title    a title card or caption landing, its lettering written from `delay` s later over
//            `dur` s (captionWrite);
//   final    the end card's flash;
//   ignite   a beam lighting from nothing: the intro's spark firing, a board lighting up;
//   beam     beams rerouting after a move;
//   grab, drop, rotate  a piece picked up, put down or turned (UI ticks; the facts cards landing
//            are drops too);
//   lit      a target lighting, with its `colour` (red, green, blue, yellow, magenta, cyan, white).
function deriveCues() {
  const cues = [];
  for (const scene of SCENES) {
    const add = (t, kind, extra) => cues.push({ t: Math.round(t * 1e6) / 1e6, kind, ...extra, scene: scene.name });
    const onFrame = (tau) => frameTime(scene, tau);
    const title = () => {
      const { delay, length } = captionWrite(scene.caption);
      add(scene.caption.at, "title", { delay, dur: length });
    };
    if (scene.start > 0) add(scene.start, "cut");
    for (const shot of (scene.shots ?? []).slice(1)) add(shot.start, "cut");
    // The opening swing of each shot's camera.
    for (const shot of scene.shots ?? []) {
      const [k0, k1] = shot.keys;
      if (Math.abs(k1.tilt - k0.tilt) + Math.abs(k1.yaw - k0.yaw) > 20 && k1.t - k0.t <= 1.2) add(shot.start, "whoosh", { dur: Math.round((k1.t - k0.t) * 1e6) / 1e6 });
    }
    if (scene.kind === "intro") {
      add(INTRO.ignite, "ignite");
      add(INTRO.ignite, "riser", { dur: INTRO.impact - INTRO.ignite });
      add(INTRO.impact, "impact");
    } else if (scene.kind === "end") {
      add(END.converge, "ignite");
      add(END.hit, "final");
    } else if (scene.caption?.word) {
      add(scene.caption.at, "impact");
    } else if (scene.caption) {
      title();
    }
    if (scene.kind === "facts") {
      add(FACTS.open, "whoosh", { dur: LETTERBOX.length });
      for (const t of FACTS.cards) add(t, "drop");
      add(FACTS.count, "riser", { dur: FACTS.counted - FACTS.count });
      add(FACTS.counted, "impact");
    }
    if (scene.kind === "platforms") add(PLATFORMS.riser, "riser", { dur: Math.round((END.hit - PLATFORMS.riser) * 1e6) / 1e6 });
    const play = PLAYS[scene.name];
    if (!play) continue;
    for (const state of play.states) {
      if (state.cause === "start") add(onFrame(state.t), "ignite");
      else {
        add(onFrame(state.t), state.cause);
        if (state.changed) add(onFrame(state.t), "beam");
      }
    }
    for (const target of play.targets) {
      for (const change of target.changes) {
        // A target lights when it starts receiving exactly its colour.
        const before = targetAt(target, change.t - 1e-9);
        if (change.mask === target.color && before.mask !== target.color) add(onFrame(change.t), "lit", { colour: COLOUR_NAMES[target.color] });
      }
    }
  }
  return cues.sort((a, b) => a.t - b.t || a.kind.localeCompare(b.kind));
}
export const cues = deriveCues();
