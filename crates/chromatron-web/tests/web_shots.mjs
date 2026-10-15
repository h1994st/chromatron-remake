// `just web-shots [dir]`: serves dist/web/ and photographs it in headless Google Chrome.
// The classic look (`?look=classic`), seven views: a desktop (1400 × 1000 at 1×); a phone in
// landscape (844 × 390) and in portrait, both full height (390 × 844), with Safari's bars shown
// (390 × 664) and with WeChat's slim top bar (390 × 780), at devicePixelRatio 3 with touch
// emulation, so `(pointer: coarse)` matches; a mouse at 390 × 844 with `?layout=touch`; and the
// desktop with `?analytics=debug`, whose game events it reads from `window.dataLayer`.
// The modern look (the default): the desktop at 1400 × 1000 @1 and @2 and at 928 × 667 @1 (where
// the toolbar may wrap: a sweep of widths must settle); the phone portrait (390 × 844 @3), with
// Safari's bars (390 × 664 @3, and the longest C1 hint there), landscape (844 × 390 @3), the level
// sheet open, the won card (its Next ▶ tapped); a reload that keeps the Classic look; and the
// short phones 360 × 740 @3, 375 × 667 @2, and with their bars 375 × 548 @2 and 360 × 560 @3,
// where the page scrolls by hand from the card. It prints (does not assert) the page's cold paint
// time, `render_at` plus `putImageData`, at 1400 × 1000 @2 and in portrait.
// Motion, on the desktop at 1400 × 1000 @2 with `?perf`: modern-motion (a pasted win on C2's first
// level animates on its own, settles within SETTLE_MS and then paints nothing; its animation
// frames' p90 is held to ANIM_CAP), modern-reduced (prefers-reduced-motion emulated from the load:
// a win's frames end within 150 ms and a move paints once) and modern-reduced-live (reduced motion
// switched on mid-animation stops the frames at once). The `?perf` animation rows are printed at
// the end.
// For each view it writes <name>.png and <name>.json (the page's layout facts) into <dir> and
// checks them; some views take more pictures. No view sends anything to Google: the page never
// asks for googletagmanager.com off the site.
// A local development aid, not run in CI: it needs Google Chrome ($CHROME overrides the path). The
// pictures show the games' sprites: inside the repo they may only go under dist/ or target/, which
// git ignores (`just web-shots` writes dist/web-shots/). Every way out (the end, an error, Ctrl-C,
// SIGTERM, the 240 s limit) stops Chrome and removes its temporary profile.
// The rig (server, Chrome, CDP, hashing) is shot_rig.mjs.
// Usage: node web_shots.mjs <dist/web> <dir>

import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { isAbsolute, join, relative, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  CANVAS,
  FNV,
  FRAMES,
  IDLE,
  SETTLE,
  backingOf,
  centreOf,
  clientOf,
  configure,
  connect,
  evaluate,
  finish,
  fnvRect,
  hook,
  lightboxHash,
  load,
  pageErrors,
  perfFrames,
  requested,
  resizeViewport,
  rightClick,
  send,
  serve,
  setMedia,
  shoot,
  touch,
  typeKey,
} from "./shot_rig.mjs";

const [webDir, outDir] = process.argv.slice(2);
assert.ok(webDir && outDir, "usage: node web_shots.mjs <dist/web> <dir>");
const repo = fileURLToPath(new URL("../../..", import.meta.url));
const inRepo = relative(repo, resolve(outDir));
assert.ok(
  inRepo.startsWith("..") || isAbsolute(inRepo) || /^(dist|target)(\/|$)/.test(inRepo),
  `${outDir} is in the repo but not git-ignored; use dist/web-shots/… or a directory outside the repo`,
);
const CHROME = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
assert.ok(existsSync(CHROME), `no Google Chrome at ${CHROME}; set CHROME to its executable`);
mkdirSync(outDir, { recursive: true });
configure({ webDir, outDir, chrome: CHROME });

// The engine's own first frame of Chromatron 1, and lumen's regions, from the built wasm.
const glue = await import(pathToFileURL(join(webDir, "chromatron_web.js")).href);
const wasm = glue.initSync({ module: readFileSync(join(webDir, "chromatron_web_bg.wasm")) });
const reference = new glue.Session();
reference.select(1, undefined, undefined);
reference.render_at(0);
const BOARD = [...reference.board_rect()];
const TOOLBOX = [...reference.toolbox_rect()];
const CLICK_NEXT = reference.click_next();
assert.ok(CLICK_NEXT.length > 0, "the game has a click_next text");
// What the touch layouts say instead (web/main.js TOUCH_NEXT).
const TOUCH_PORTRAIT = "Tap Next level or pick a level from Levels.";
const TOUCH_LANDSCAPE = "Tap a level number or ▶ for the next level.";
const rgba = new Uint8Array(wasm.memory.buffer, reference.rgba_ptr(), 640 * 480 * 4).slice();
const { LIFT, TOOLBOX_MAX, TOOLBOX_MIN, fillScale, portraitScales, rectOf } = await import(pathToFileURL(join(webDir, "touch_layout.js")).href);
const expected = { frame: fnvRect(rgba, [0, 0, 640, 480]), board: fnvRect(rgba, BOARD), toolbox: fnvRect(rgba, TOOLBOX) };
// Where today's page (Task 4's run, Chrome 154 on macOS) shows the desktop game at 1400 × 1000.
const DESKTOP_GAME = { left: 380, top: 255, width: 640, height: 480 };
// The modern card's hint (lumen's TextRole::Hint caption) on each of C1's 50 levels, as a phone
// portrait (390 × 664 @2) lays it out, stepping with the cheat key and "=": level 1's is checked on
// the desktop, and the longest is R8's check (spec §16) in modern-portrait-bars.
const ROLE_HINT = glue.CaptionRole.Hint;
const HINTS = [];
reference.set_look(glue.Look.Modern);
reference.resize(780, 1328, 2, [0, 0, 0, 0], true);
for (let slot = 0; slot < 50; slot += 1) {
  assert.equal(reference.slot(), slot, `C1 level ${slot + 1} opens with the cheat key`);
  reference.render_at(slot);
  let hint = "";
  for (let i = 0; i < reference.caption_count(); i += 1) {
    if (reference.caption_role(i) === ROLE_HINT) hint = reference.caption_text(i).replace(/\r\n?/g, "\n");
  }
  HINTS.push(hint);
  reference.key("L", false, false, false);
  reference.key("=", false, false, false);
}
const LONGEST_SLOT = HINTS.reduce((best, hint, slot) => (hint.length > HINTS[best].length ? slot : best), 0);
assert.ok(HINTS[0].length > 0 && HINTS[LONGEST_SLOT].length >= 200, `C1's hints: level 1 "${HINTS[0]}", longest ${HINTS[LONGEST_SLOT].length}`);

// Pastes a copied solution into the page.
const paste = (text) => `(() => {
  const clipboardData = new DataTransfer();
  clipboardData.setData("text/plain", ${JSON.stringify(text)});
  document.dispatchEvent(new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true }));
})()`;
// Level 1 of Chromatron 1 solved by pasting its stored solution (tests/solutions/game1/level-01.txt).
const PASTE_WIN = paste("1-01-hI");
// Level 1 of Chromatron 2 solved by pasting the clipboard text of its stored placement, as
// `lumen::stored_solution` encodes it from the original (no tests/solutions fixture holds it):
// from one lit strand to ten lit targets along long strands, so the paste sweeps for the full
// 0.6 s and bursts eight times.
const PASTE_C2_WIN = paste("2-01-NXrmCVilip");

// Two frames into a sheet cue: whether the session still animates, whether #captions' opacity
// transition is running (its CSS fade, however far it has got: review M1), and its span count.
const CAPTIONS_MID_FADE = `(async () => {
  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
  await frame();
  await frame();
  const box = document.getElementById("captions");
  const fading = box.getAnimations().some((animation) => animation.transitionProperty === "opacity" && animation.playState === "running");
  return { fading, spans: box.children.length, moving: window.__session.next_wake(performance.now()) >= 0 };
})()`;

// How many times the page has painted the hooked Session (render_at).
const RENDERS = "window.__calls.render_at";

// Twelve animation frames.
const TWELVE_FRAMES = `(async () => {
  for (let k = 0; k < 12; k += 1) await new Promise((resolve) => requestAnimationFrame(resolve));
})()`;

// The user's web animation cap for `render_at` + `putImageData` in Chrome (phase-4 Task 13): the
// tightened `lightbox_budgets.mjs` animation cap (BUDGETS.anim, `render_at` alone in Node) plus
// the measured `putImageData` p90 of the motion view's frames, rounded up to 5 ms: 35 + 0.4 → 40.
// It follows BUDGETS.anim in lightbox_budgets.mjs.
const ANIM_CAP = 40;
// The 90th percentile of `xs` (null for none).
const p90 = (xs) => (xs.length === 0 ? null : [...xs].sort((a, b) => a - b)[Math.floor(xs.length * 0.9)]);
const REDUCE = [{ name: "prefers-reduced-motion", value: "reduce" }];
const LIGHT = [{ name: "prefers-color-scheme", value: "light" }];
// Every view runs under a dark system unless it names a scheme (phase 5: the modern look follows
// prefers-color-scheme), so the dark views stay dark whatever the machine's default is.
const withScheme = (media = []) => (media.some((f) => f.name === "prefers-color-scheme") ? media : [...media, { name: "prefers-color-scheme", value: "dark" }]);

// Ends a view that won level 1 without leaving the win in the browser's storage for the next view:
// the page stores its progress when it goes away, so every later write is dropped first.
async function forgetWin() {
  await evaluate(`(() => {
    Storage.prototype.setItem = () => {};
    localStorage.removeItem("chromatron1.save");
  })()`);
}

// What the page says in the hint box after a win: the strings drawn on the text canvas (landscape,
// desktop) or listed as HTML (portrait), and whether the Next level button shows.
const WIN_FACTS = `(() => {
  const button = document.getElementById("next-level");
  const rect = button?.getBoundingClientRect();
  return {
    drawn: JSON.parse(document.getElementById("text").dataset.drawn ?? "[]"),
    listed: [...document.querySelectorAll("#text-list p")].map((p) => p.textContent),
    button: button && rect.width > 0 && rect.height > 0 ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2, width: rect.width, height: rect.height, text: button.textContent } : null,
    inList: button?.parentElement?.id ?? null,
  };
})()`;

// The computed user-select of the first element matching each selector.
function userSelect(...selectors) {
  return evaluate(`${JSON.stringify(selectors)}.map((selector) => {
    const style = getComputedStyle(document.querySelector(selector));
    return [selector, style.userSelect];
  })`);
}

async function assertNoSelection(...selectors) {
  // -webkit-touch-callout is iOS Safari's: Chrome does not compute it, so only user-select is read.
  for (const [selector, select] of await userSelect(...selectors)) assert.equal(select, "none", `${selector} has user-select: none`);
}

// What the page shows: its layout, the toolbar, where the game's views are, and their pixels.
const FACTS = `(() => {
  const root = document.documentElement;
  const box = (selector) => {
    const rect = document.querySelector(selector)?.getBoundingClientRect();
    return rect && rect.width > 0 && rect.height > 0
      ? { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, width: rect.width, height: rect.height }
      : null;
  };
  const pixelsOf = (selector) => {
    const canvas = document.querySelector(selector);
    if (!canvas || canvas.width === 0 || canvas.height === 0) return null;
    const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    return ${FNV}(data).toString(16).padStart(8, "0");
  };
  // What shows: the desktop modern Levels keeps its slot unseen (visibility: hidden).
  const shown = (element) => element !== null && element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden";
  const tools = [...document.querySelectorAll(".toolbar > *")].filter((element) => shown(element) && !element.classList.contains("spacer"));
  const levelsOpen = document.getElementById("levels-open");
  const help = document.getElementById("help");
  const stage = document.getElementById("stage");
  const style = getComputedStyle(stage);
  const px = (value) => parseFloat(value) || 0;
  const next = document.querySelector('.toolbar [data-command="next"]');
  return {
    layout: root.dataset.layout ?? "",
    width: innerWidth,
    height: innerHeight,
    clientWidth: root.clientWidth,
    ratio: devicePixelRatio,
    overflowX: root.scrollWidth - root.clientWidth,
    overflowY: root.scrollHeight - root.clientHeight,
    scrollY,
    // Whether the page may scroll at all, and whether its overscroll (pull-to-refresh) is off.
    scrollable: [root, document.body].every((element) => !["hidden", "clip"].includes(getComputedStyle(element).overflowY)),
    overscroll: getComputedStyle(root).overscrollBehaviorY,
    partTouch: ["#board-part", "#toolbox-part"].map((selector) => getComputedStyle(document.querySelector(selector)).touchAction),
    // The room for the board and the toolbox: the viewport (no browser bars here) below the
    // toolbar, less the stage's top padding and two gaps (web/main.js).
    portraitRoom: innerHeight - (document.querySelector("header")?.getBoundingClientRect().height ?? 0) - px(style.paddingTop) - 2 * px(style.rowGap),
    nextWon: { highlighted: next.classList.contains("won"), background: getComputedStyle(next).backgroundColor, label: next.getAttribute("aria-label") },
    toolbarRows: new Set(tools.map((element) => Math.round(element.getBoundingClientRect().top))).size,
    toolbarRight: Math.max(0, ...tools.map((element) => element.getBoundingClientRect().right)),
    toolbarColumns: new Set(tools.map((element) => Math.round(element.getBoundingClientRect().left))).size,
    toolbarBottom: Math.max(0, ...tools.map((element) => element.getBoundingClientRect().bottom)),
    tools: tools.map((element) => element.id || element.dataset.command || element.localName),
    // Each shown control's box, and the toolbar's (whose overflow would clip one).
    toolRects: tools.map((element) => { const r = element.getBoundingClientRect(); return { id: element.id || element.dataset.command || element.localName, left: r.left, top: r.top, right: r.right, bottom: r.bottom }; }),
    toolbarBox: (() => { const r = document.querySelector(".toolbar").getBoundingClientRect(); return { left: r.left, top: r.top, right: r.right, bottom: r.bottom }; })(),
    header: box("header"),
    game: box("#game"),
    look: root.dataset.look ?? "",
    lightbox: box("#lightbox"),
    lightboxBacking: (() => { const c = document.getElementById("lightbox"); return c ? [c.width, c.height] : null; })(),
    lightboxCorner: (() => { const c = document.getElementById("lightbox"); return c && c.width > 0 ? [...c.getContext("2d").getImageData(2, 2, 1, 1).data] : null; })(),
    board: box("#board-part"),
    toolbox: box("#toolbox-part"),
    texts: box("#texts"),
    textCount: document.querySelectorAll("#texts p").length,
    framePixels: pixelsOf("#pixels"),
    boardPixels: pixelsOf("#board-part"),
    toolboxPixels: pixelsOf("#toolbox-part"),
    dialogs: [...document.querySelectorAll("dialog[open]")].map((dialog) => dialog.id),
    message: document.getElementById("message").textContent,
    fatal: !document.getElementById("fatal").hidden,
    // The modern card's typeset text (#captions spans), for people and screen readers alike.
    captions: [...document.querySelectorAll("#captions > span")].map((span) => {
      const style = getComputedStyle(span);
      const r = span.getBoundingClientRect();
      return {
        className: span.className,
        text: span.textContent,
        rect: { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height },
        overflows: span.scrollWidth > span.clientWidth,
        clips: span.scrollHeight > span.clientHeight + 1,
        fontSize: parseFloat(style.fontSize),
        ariaHidden: span.getAttribute("aria-hidden"),
      };
    }),
    captionsDisplay: getComputedStyle(document.getElementById("captions")).display,
    // The sheet fades the captions out and hides them (style.css), and fades them back in.
    captionsVisibility: getComputedStyle(document.getElementById("captions")).visibility,
    captionsOpacity: getComputedStyle(document.getElementById("captions")).opacity,
    captionsAriaHidden: document.getElementById("captions").getAttribute("aria-hidden"),
    winLive: document.getElementById("win-live").textContent,
    levelsOpen: shown(levelsOpen),
    // Whether Levels takes its place in the toolbar, seen or not (never display: none).
    levelsSlot: levelsOpen.getClientRects().length > 0 ? { visibility: getComputedStyle(levelsOpen).visibility, left: levelsOpen.getBoundingClientRect().left } : null,
    gameShort: shown(document.getElementById("game-short")) ? document.getElementById("game-short").textContent : null,
    footerInHelp: help.contains(document.querySelector("footer")),
    creditsHelp: document.getElementById("credits-help").textContent,
    themeColor: document.querySelector('meta[name="theme-color"]').content,
    stageHeight: stage.getBoundingClientRect().height,
    // The room portrait's stage is sized from: the small viewport below the toolbar (web/main.js).
    stageRoom: document.getElementById("small-viewport").getBoundingClientRect().height - (document.querySelector("header")?.getBoundingClientRect().height ?? 0),
    canvasTouch: getComputedStyle(document.getElementById("lightbox")).touchAction,
    storedLook: localStorage.getItem("chromatron.look"),
  };
})()`;

// Where frame point (x, y) of region `frame` ([left, top, right, bottom]) is, in the view `box` (FACTS).
function clientPoint(box, frame, [x, y]) {
  return [box.left + ((x - frame[0]) * box.width) / (frame[2] - frame[0]), box.top + ((y - frame[1]) * box.height) / (frame[3] - frame[1])];
}

// Chromatron 1's first level (tests/session.rs): the reflector in toolbox slot 0 (top-left (448, 8),
// centre (460, 20)), the empty board cell (8, 7) (top-left (240, 186), centre (252, 198)) and the
// empty cell (9, 7) beside it (top-left (264, 186), centre (276, 198)), and the empty cells (8, 0)
// on the board's top row (top-left (240, 18), centre (252, 30)) and (5, 10) (top-left (168, 258),
// centre (180, 270)), away from the beam. CELLS hashes these 24 × 24 squares in the portrait
// parts' canvases, and slot 0 and cells (8, 7) and (5, 10) in the full frame.
const CELLS = `(() => {
  const hash = (id, x, y) => {
    const data = document.getElementById(id).getContext("2d").getImageData(x, y, 24, 24).data;
    return ${FNV}(data);
  };
  return {
    slot: hash("toolbox-part", ${448 - TOOLBOX[0]}, ${8 - TOOLBOX[1]}),
    cell: hash("board-part", ${240 - BOARD[0]}, ${186 - BOARD[1]}),
    beside: hash("board-part", ${264 - BOARD[0]}, ${186 - BOARD[1]}),
    top: hash("board-part", ${240 - BOARD[0]}, ${18 - BOARD[1]}),
    frameSlot: hash("pixels", 448, 8),
    frameCell: hash("pixels", 240, 186),
    frameAside: hash("pixels", 168, 258),
  };
})()`;

const SCENARIOS = [
  // The classic look, pinned with ?look=classic (the page's default is modern).
  { name: "desktop", width: 1400, height: 1000, ratio: 1, touch: false, query: "&look=classic" },
  { name: "landscape", width: 844, height: 390, ratio: 3, touch: true, query: "&look=classic" },
  { name: "forced-touch", width: 390, height: 844, ratio: 1, touch: false, query: "&layout=touch&look=classic" },
  // iOS Safari's usable portrait height on a 390 × 844 phone, with its bars shown.
  { name: "portrait-bars", width: 390, height: 664, ratio: 3, touch: true, query: "&look=classic" },
  // An iPhone in WeChat's in-app browser: a slim top bar, so more height than Safari's bars leave.
  { name: "portrait-wechat", width: 390, height: 780, ratio: 3, touch: true, query: "&look=classic" },
  // The full-height phone portrait.
  { name: "portrait", width: 390, height: 844, ratio: 3, touch: true, query: "&look=classic" },
  // The desktop with analytics' debug mode: game events go to window.dataLayer, and no script loads.
  { name: "analytics", width: 1400, height: 1000, ratio: 1, touch: false, query: "&analytics=debug&look=classic" },
  // The modern look, with no ?look= (the default).
  { name: "modern-desktop", width: 1400, height: 1000, ratio: 1, touch: false },
  { name: "modern-desktop-2x", width: 1400, height: 1000, ratio: 2, touch: false, query: "&perf" },
  // A desktop window where the toolbar's Levels may wrap it: sizes must settle (Task 13).
  { name: "modern-desktop-narrow", width: 928, height: 667, ratio: 1, touch: false },
  { name: "modern-portrait", width: 390, height: 844, ratio: 3, touch: true, query: "&perf" },
  { name: "modern-portrait-bars", width: 390, height: 664, ratio: 3, touch: true },
  { name: "modern-landscape", width: 844, height: 390, ratio: 3, touch: true },
  // Landscape phones with their browser's bars (review I1, spec §6 "Revision: short landscape"):
  // iPhone 12–14 and SE in Safari, and 360-pt Androids in Chrome. Each stays Beside, unscrolled.
  { name: "modern-landscape-bars-844", width: 844, height: 342, ratio: 3, touch: true },
  { name: "modern-landscape-bars-667", width: 667, height: 327, ratio: 2, touch: true },
  { name: "modern-landscape-bars-740", width: 740, height: 304, ratio: 3, touch: true },
  { name: "modern-landscape-bars-800", width: 800, height: 304, ratio: 3, touch: true },
  // A first-generation iPhone SE (its canvas under 568 pt wide): too small for Beside, so the stage
  // grows to stage_height and the page scrolls.
  { name: "modern-landscape-tiny", width: 568, height: 300, ratio: 2, touch: true },
  // The level sheet, opened with the toolbar's Levels; analytics' debug mode counts open_levels.
  { name: "modern-sheet", width: 390, height: 844, ratio: 3, touch: true, query: "&analytics=debug" },
  { name: "modern-won", width: 390, height: 844, ratio: 3, touch: true },
  { name: "modern-reload", width: 1400, height: 1000, ratio: 1, touch: false },
  // Short phones (review I6): with no bars the stage fits; with them the page scrolls by hand.
  { name: "modern-360", width: 360, height: 740, ratio: 3, touch: true },
  { name: "modern-375", width: 375, height: 667, ratio: 2, touch: true },
  { name: "modern-375-bars", width: 375, height: 548, ratio: 2, touch: true },
  { name: "modern-360-bars", width: 360, height: 560, ratio: 3, touch: true },
  // Motion (spec §9, §15): a pasted solution's frames on the desktop @2, timed (`?perf`) against
  // ANIM_CAP; the same with prefers-reduced-motion emulated from the load (`media`); and reduced
  // motion switched on in the middle of the animation.
  { name: "modern-motion", width: 1400, height: 1000, ratio: 2, touch: false, query: "&perf" },
  { name: "modern-reduced", width: 1400, height: 1000, ratio: 2, touch: false, query: "&perf", media: REDUCE },
  { name: "modern-reduced-live", width: 1400, height: 1000, ratio: 2, touch: false, query: "&perf" },
  // Phase 5: the light theme (prefers-color-scheme: light) at desktop and portrait; classic
  // under a light system (its page stays dark; and the first paint before main.js runs); a live
  // switch; a switch while in classic, then the look toggled to modern; the motion row in light.
  { name: "modern-light-desktop", width: 1400, height: 1000, ratio: 2, touch: false, media: LIGHT },
  { name: "modern-light-portrait", width: 390, height: 844, ratio: 3, touch: true, media: LIGHT },
  { name: "classic-light", width: 1400, height: 1000, ratio: 1, touch: false, query: "&look=classic", media: LIGHT },
  { name: "modern-theme-live", width: 1400, height: 1000, ratio: 1, touch: false },
  { name: "classic-theme-live", width: 1400, height: 1000, ratio: 1, touch: false, query: "&look=classic" },
  { name: "modern-light-motion", width: 1400, height: 1000, ratio: 2, touch: false, query: "&perf", media: LIGHT },
];

// The page's theme facts: data-theme, theme-color, the body's background, the hint's colour
// (C1 level 1 has a hint and no level name, and the inline picker means no Levels pill).
const THEME_FACTS = `(() => JSON.stringify({
  theme: document.documentElement.dataset.theme,
  color: document.querySelector('meta[name="theme-color"]').content,
  page: getComputedStyle(document.body).backgroundColor,
  body: (() => { const t = document.querySelector("#captions .cap-body"); return t ? getComputedStyle(t).color : null; })(),
  origin: performance.timeOrigin,
}))()`;
const theme = async () => JSON.parse(await evaluate(THEME_FACTS));

// The page as index.html's head script leaves it before main.js runs: its look and theme, every
// theme-color, the page and toolbar colours, and whether main.js has started (it sets `?perf`'s log).
const FIRST_PAINT = `(() => JSON.stringify({
  look: document.documentElement.dataset.look,
  theme: document.documentElement.dataset.theme,
  colors: [...document.querySelectorAll('meta[name="theme-color"]')].map((meta) => meta.content),
  page: getComputedStyle(document.body).backgroundColor,
  toolbar: getComputedStyle(document.querySelector("header")).backgroundColor,
  started: window.__perf !== undefined,
}))()`;

// Loads the page with main.js held back (it fails, as blocked), so the picture is the one a visitor
// sees while the wasm loads (final review I1), with the stored look `stored` (null: none) and
// `query`; the browser's scheme is the view's.
let firstPaints = 0;
async function firstPaint(stored, query = "") {
  const origin = await evaluate("location.origin + location.pathname");
  await evaluate(stored === null ? `localStorage.removeItem("chromatron.look")` : `localStorage.setItem("chromatron.look", ${JSON.stringify(stored)})`);
  await send("Fetch.enable", { patterns: [{ urlPattern: "*/main.js*" }] });
  try {
    await evaluate("window.shotStale = true");
    // A query of its own each time: the same URL again (with its #1) would only scroll.
    firstPaints += 1;
    await send("Page.navigate", { url: `${origin}?shot=first-paint-${firstPaints}&perf${query}#1` });
    for (let tries = 0; !(await evaluate(`!window.shotStale && document.readyState === "complete"`).catch(() => false)); tries += 1) {
      assert.ok(tries < 100, "the held-back page did not load");
      await delay(100);
    }
    await evaluate(FRAMES);
    return JSON.parse(await evaluate(FIRST_PAINT));
  } finally {
    await send("Fetch.disable");
    await evaluate(`localStorage.removeItem("chromatron.look")`);
  }
}

// The analytics commands the page has pushed (web/analytics.js), as [command, target, params].
const DATA_LAYER = `(window.dataLayer ?? []).map((entry) => [entry[0], entry[0] === "js" ? null : entry[1], entry[2] ?? null])`;

// Waits until analytics, loaded after the first paint, has pushed its config; returns the commands.
async function analyticsStarted() {
  for (let tries = 0; ; tries += 1) {
    const layer = await evaluate(DATA_LAYER);
    if (layer.some(([command]) => command === "config")) return layer;
    assert.ok(tries < 50, "analytics did not start with ?analytics=debug");
    await delay(100);
  }
}

// The params of each `name` event pushed so far.
async function eventsNamed(name) {
  return (await evaluate(DATA_LAYER)).filter(([command, target]) => command === "event" && target === name).map(([, , params]) => params);
}

// Clears what earlier views stored (the help stays seen unless `firstVisit`), so the page starts on
// a fresh level 1, and stops this page storing anything when it goes away.
async function forgetAll(firstVisit = false) {
  await evaluate(`(() => {
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = () => {};
    localStorage.clear();
    if (!${firstVisit}) set.call(localStorage, "chromatron.helpSeen", "1");
  })()`);
}

// Whether `actual` is `wanted` to within `tolerance` CSS px.
function assertNear(actual, wanted, what, tolerance = 0.5) {
  assert.ok(Math.abs(actual - wanted) <= tolerance, `${what}: ${actual}, expected ${wanted}`);
}

// A portrait page at scroll-top: a page that scrolls under a sticky toolbar, with the board (full
// width), the toolbox and the texts in that order, the board and the toolbox sized as
// touch_layout.js's portraitScales says for the room the viewport leaves and both on screen.
function checkPortrait(facts) {
  assert.equal(facts.layout, "portrait");
  assert.ok(facts.overflowX <= 0, `no horizontal scroll: ${facts.overflowX}`);
  assert.ok(facts.scrollable, "the page may scroll vertically");
  assert.ok(facts.overflowY > 0, `the texts run below the screen, so the page scrolls: ${facts.overflowY}`);
  assert.equal(facts.overscroll, "none", "no pull-to-refresh mid-drag");
  assert.deepEqual(facts.partTouch, ["none", "none"], "a touch on the board or the toolbox never scrolls the page");
  assert.equal(facts.scrollY, 0, "at scroll-top");
  assert.equal(facts.toolbarRows, 1, "one toolbar row");
  assert.ok(facts.toolbarRight <= facts.clientWidth, "the toolbar fits the width");
  assert.deepEqual(facts.tools, ["game-select", "previous", "next", "actions", "levels-open", "clockwise", "fullscreen", "help-open"]);
  assert.equal(facts.game, null, "the whole frame is hidden");
  const scales = portraitScales({
    width: facts.clientWidth,
    height: facts.portraitRoom,
    ratio: facts.ratio,
    board: rectOf(BOARD),
    toolbox: rectOf(TOOLBOX),
  });
  assertNear(facts.board.width, (BOARD[2] - BOARD[0]) * scales.board, "the board's scale");
  assertNear(facts.toolbox.width, (TOOLBOX[2] - TOOLBOX[0]) * scales.toolbox, "the toolbox's scale");
  assertNear(facts.board.width, facts.clientWidth, "the board fills the width", 1);
  assert.ok(facts.toolbox.width / (TOOLBOX[2] - TOOLBOX[0]) >= TOOLBOX_MIN - 0.01, "the toolbox slots stay finger-sized");
  assert.ok(facts.toolbox.width / ((TOOLBOX[2] - TOOLBOX[0])) <= TOOLBOX_MAX + 0.01, "the toolbox is at most TOOLBOX_MAX");
  const gutter = (facts.clientWidth - facts.toolbox.width) / 2;
  assertNear(facts.toolbox.left, gutter, "the toolbox is centred", 1);
  if (facts.clientWidth <= 400) assert.ok(gutter >= 30, `gutters of 30 px or more beside the toolbox to swipe on: ${gutter}`);
  assert.ok(facts.header.bottom <= facts.board.top, "the board below the toolbar");
  assert.ok(facts.board.bottom <= facts.toolbox.top && facts.toolbox.bottom <= facts.texts.top, "board, toolbox, then texts");
  assert.ok(facts.toolbox.bottom <= facts.height + 0.5, `the board and the toolbox on screen at scroll-top: ${facts.toolbox.bottom}`);
  assert.ok(facts.textCount >= 1, "the texts are HTML");
  assert.equal(facts.nextWon.highlighted, false, "the toolbar's ▶ is plain on an unsolved level");
  assert.equal(facts.boardPixels, expected.board, "the board part shows the engine's board");
  assert.equal(facts.toolboxPixels, expected.toolbox, "the toolbox part shows the engine's toolbox");
}

// Scrolled to the end, the toolbar stays at the top and the texts (with the credits) come into
// view; the layout is the one at scroll-top. Takes <name>-scrolled.png, then scrolls back.
async function checkPortraitScroll(name, facts) {
  await evaluate("window.scrollTo(0, 9999)");
  await evaluate(SETTLE);
  const scrolled = await evaluate(FACTS);
  await shoot(`${name}-scrolled`);
  assertNear(scrolled.scrollY, facts.overflowY, "scrolled to the end", 1);
  assert.ok(scrolled.scrollY > 0, "the page scrolled");
  assertNear(scrolled.header.top, 0, "the toolbar stays at the top");
  assert.ok(scrolled.texts.top >= scrolled.header.bottom && scrolled.texts.bottom <= scrolled.height + 0.5, `the texts in view: ${JSON.stringify(scrolled.texts)}`);
  assert.equal(await evaluate(`document.elementFromPoint(innerWidth / 2, 2)?.closest("header") !== null`), true, "the toolbar is on top of the page");
  assert.deepEqual([scrolled.board.width, scrolled.toolbox.width], [facts.board.width, facts.toolbox.width], "scrolling changed no sizes");
  await evaluate("window.scrollTo(0, 0)");
  await evaluate(SETTLE);
}

// Scrolls the Next level button into view and returns where it is, checking that it is on screen
// below the toolbar and not covered.
async function revealNextButton() {
  return evaluate(`new Promise((resolve) => {
    const button = document.getElementById("next-level");
    button.scrollIntoView({ block: "nearest" });
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const rect = button.getBoundingClientRect();
      const x = rect.left + rect.width / 2;
      const y = rect.top + rect.height / 2;
      resolve({
        x,
        y,
        top: rect.top,
        bottom: rect.bottom,
        header: document.querySelector("header").getBoundingClientRect().bottom,
        hit: document.elementFromPoint(x, y) === button,
        scrollY,
      });
    }));
  })`);
}

// The toolbar's ▶: whether it is highlighted (FACTS's nextWon), and where it is.
const TOOLBAR_NEXT = `(() => {
  const next = document.querySelector('.toolbar [data-command="next"]');
  const rect = next.getBoundingClientRect();
  return { highlighted: next.classList.contains("won"), background: getComputedStyle(next).backgroundColor, label: next.getAttribute("aria-label"), x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
})()`;
// --accent in web/style.css.
const ACCENT = "rgb(138, 180, 255)";

// The page's Session: a board cell's or tray slot's backing rect (lumen cell index).
const cellRect = (index) => evaluate(`[...window.__session.cell_rect(${index})]`);
const slotNow = () => evaluate("window.__session.slot()");

// A touch drag from client point `from` to LIFT px below `to`, so a lifted piece drops on `to`.
async function touchDrag(from, to, steps = 8) {
  await touch("touchStart", ...from);
  for (let step = 1; step <= steps; step += 1) {
    await touch("touchMove", from[0] + (to[0] - from[0]) * (step / steps), from[1] + (to[1] + LIFT - from[1]) * (step / steps));
  }
  await touch("touchEnd");
  await evaluate(SETTLE);
}

// A tap at client point (x, y).
async function tap(x, y) {
  await touch("touchStart", x, y);
  await touch("touchEnd");
  await evaluate(SETTLE);
}

// Spec §14's web rows (`?perf`, reported, not gated): the page's cold paint, `render_at` and the
// `putImageData`s of the paint that follows a session.resize, at this view's size, from the
// page's own `?perf` log (window.__perf, so these views load with &perf). The view is
// made 1 CSS px wider and back (each settles after the page's resize debounce), three times; the
// paints at the view's own size are reported, the best (noise only adds time) and every one.
async function coldPaintTime(scenario) {
  const { width, height, ratio, touch: mobile } = scenario;
  const runs = [];
  for (const w of [width + 1, width, width + 1, width, width + 1, width]) {
    await resizeViewport({ width: w, height, deviceScaleFactor: ratio, mobile });
    const run = await evaluate(`(() => {
      const resize = window.__calls.resizes.at(-1);
      const frames = window.__perf.frames;
      const paint = frames.find((f) => f.wall >= resize.at && f.rects > 0);
      return { backing: [resize.width, resize.height], render: paint.render, put: paint.put };
    })()`);
    if (w === width) runs.push(run);
  }
  const best = runs.reduce((a, b) => (b.render + b.put < a.render + a.put ? b : a));
  const all = runs.map(({ render, put }) => (render + put).toFixed(1)).join(", ");
  console.log(`perf ${scenario.name}: cold paint at ${best.backing.join(" × ")} backing px: render_at ${best.render.toFixed(1)} ms + putImageData ${best.put.toFixed(1)} ms = ${(best.render + best.put).toFixed(1)} ms, best of ${all} ms (reported, not gated)`);
  return { ...best, runs: runs.map(({ render, put }) => render + put) };
}

// The page's ?perf frames since `since` that painted something.
const litSince = async (since) => (await perfFrames(since)).filter((frame) => frame.rects > 0);

// A pasted solution (C2 level 1: a full 0.6 s sweep along long strands with eight bursts, then the
// win, so the p90 lands on sweep frames and is a real tripwire; C1 level 1 sweeps five cells):
// frames run on their own, then stop within SETTLE_MS, and none follow for 1 s: idle (spec §9.1,
// ruling M9). Its animation frames (all but the paste's own, an edit) are held to ANIM_CAP at
// their p90 (`render_at` + `putImageData`; one frame is noise, ruling: judge on p90). Then the
// same paste again, photographed 200 ms in (a mid-animation frame) and settled.
async function motionFrames(name) {
  await hook();
  try {
    await evaluate(`(() => {
      const select = document.getElementById("game-select");
      select.value = "2";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    })()`);
    await evaluate(SETTLE);
    assert.ok(await evaluate("window.__session.game() === 2 && window.__session.slot() === 0"), `${name}: C2's first level`);
    const t0 = await evaluate("performance.now()");
    await evaluate(PASTE_C2_WIN);
    await delay(3_000);
    const lit = await litSince(t0);
    assert.ok(lit.length >= 20, `${name}: the win animates (${lit.length} frames)`);
    assert.ok(lit.at(-1).wall - lit[0].wall <= 2_500, `${name}: it settles within SETTLE_MS (${(lit.at(-1).wall - lit[0].wall).toFixed(0)} ms)`);
    const quiet = await evaluate("performance.now()");
    await delay(1_000);
    assert.equal((await perfFrames(quiet)).length, 0, `${name}: idle means no paints`);
    const frames = lit.slice(1);
    const anim = frames.map((f) => f.render + f.put);
    const row = { name: `${name} animation frames`, p90: p90(anim), worst: Math.max(...anim), count: anim.length, renderP90: p90(frames.map((f) => f.render)), putP90: p90(frames.map((f) => f.put)) };
    perfRows.push(row);
    console.log(`perf ${row.name}: render_at + putImageData p90 ${row.p90.toFixed(1)} ms (render_at ${row.renderP90.toFixed(1)}, putImageData ${row.putP90.toFixed(1)}; worst ${row.worst.toFixed(1)} ms, ${row.count} frames), cap ${ANIM_CAP} ms`);
    assert.ok(row.p90 <= ANIM_CAP, `${name}: animation p90 ${row.p90.toFixed(1)} ms over ${ANIM_CAP}`);
    // The pictures: restarted (settled), pasted again, 200 ms in, then settled.
    await typeKey("r", "KeyR", "r");
    await evaluate(SETTLE);
    const restarted = await lightboxHash();
    await evaluate(PASTE_C2_WIN);
    await delay(200);
    const mid = await lightboxHash();
    await shoot(`${name}-mid`);
    assert.ok(await evaluate(IDLE), `${name}: the second paste settles`);
    const won = await lightboxHash();
    await shoot(`${name}-settled`);
    assert.ok(mid !== won && mid !== restarted, `${name}: 200 ms in is a frame between the restart and the win`);
  } finally {
    await forgetAll();
  }
}

// Reduced motion from the load (spec §9.3, emulated before it): a pasted win repaints without a
// sweep or bursts, and its frames (the header's 150 ms fade) end within 150 ms; then the restart,
// a real board change (the pieces leave the board), paints once and nothing after (ruling F9c).
// Photographed two frames after each, the paste's picture already the settled board.
async function reducedFrames(name) {
  await hook();
  try {
    assert.ok(await evaluate(`matchMedia("(prefers-reduced-motion: reduce)").matches`), `${name}: reduced motion is emulated`);
    let t0 = await evaluate("performance.now()");
    await evaluate(PASTE_WIN);
    await delay(800);
    const lit = await litSince(t0);
    assert.ok(lit.length >= 1, `${name}: the paste paints`);
    assert.ok(lit.at(-1).wall - lit[0].wall <= 150 + 34, `${name}: frames end within 150 ms (${lit.length} frames over ${(lit.at(-1).wall - lit[0].wall).toFixed(0)} ms)`);
    const won = await lightboxHash();
    t0 = await evaluate("performance.now()");
    await typeKey("r", "KeyR", "r");
    await delay(500);
    const restart = await litSince(t0);
    assert.equal(restart.length, 1, `${name}: a move paints once (${restart.length} frames)`);
    assert.notEqual(await lightboxHash(), won, `${name}: the restart changed the board`);
    // The pictures, after the timings: four frames after a paste, whose board (its light whole
    // at once, no sweep) is already the settled one (only the header may still fade), and four
    // frames after the restart.
    const [first, last] = [await cellRect(0), await cellRect(224)];
    const board = [first[0], first[1], last[2], last[3]];
    await evaluate(PASTE_WIN);
    await evaluate(FRAMES);
    const pasted = await lightboxHash(board);
    await shoot(`${name}-pasted`);
    assert.ok(await evaluate(IDLE), `${name}: idle after the paste`);
    assert.equal(await lightboxHash(board), pasted, `${name}: the board four frames after a paste is the settled one`);
    await typeKey("r", "KeyR", "r");
    await evaluate(FRAMES);
    await shoot(`${name}-restarted`);
  } finally {
    await forgetAll();
  }
}

// Reduced motion switched on mid-animation (the media changed live): the frames stop at once,
// the settling frame and at most a header fade's first after it; then motion again.
async function reducedLive(name) {
  await hook();
  try {
    await evaluate(PASTE_WIN);
    await delay(120);
    assert.ok(await evaluate("window.__session.next_wake(performance.now()) >= 0"), `${name}: the win is animating`);
    await setMedia(withScheme(REDUCE));
    const t0 = await evaluate("performance.now()");
    await delay(400);
    const lit = await litSince(t0);
    assert.ok(lit.length >= 1 && lit.length <= 2, `${name}: at most the settling frame and a header fade's first after the switch (${lit.length})`);
    assert.ok(await evaluate(IDLE), `${name}: idle`);
    await shoot(`${name}-switched`);
  } finally {
    await setMedia(withScheme());
    await forgetAll();
  }
}

// Facts every modern view shares: one canvas, its backing at the render scale, ink in its corner,
// the footer in the Help, the dark theme colour, and captions screen readers can read (only a
// win text that #win-live already reads is aria-hidden). Captions never overflow their box
// across, but for the ellipsised credits and compact name.
function checkModern(facts, touchLayout, light = false) {
  assert.equal(facts.look, "modern");
  assert.equal(facts.game, null, "the classic frame is hidden");
  const { width, height } = facts.lightbox;
  const scale = glue.render_scale(width, height, facts.ratio, touchLayout);
  assert.deepEqual(facts.lightboxBacking, [Math.round(width * scale), Math.round(height * scale)], "backing = CSS size × render scale");
  assert.ok(facts.lightboxCorner.slice(0, 3).every((v) => (light ? v >= 230 : v < 40)) && facts.lightboxCorner[3] === 255, `${light ? "paper" : "ink"} in the corner: ${facts.lightboxCorner}`);
  assert.ok(facts.footerInHelp, "the footer is in the Help sheet");
  assert.equal(facts.themeColor, light ? "#eef1f6" : "#06070b");
  assert.equal(facts.storedLook, null, "no look stored unasked");
  assert.equal(facts.captionsAriaHidden, null, "screen readers read #captions");
  assert.ok(facts.captions.length >= 1, "the card's captions are typeset");
  for (const caption of facts.captions) {
    if (caption.ariaHidden !== null) assert.ok(caption.ariaHidden === "true" && caption.text === facts.winLive, `only a win text #win-live reads is aria-hidden: ${JSON.stringify(caption)}`);
    if (!["cap-credit", "cap-title-compact"].includes(caption.className)) assert.ok(!caption.overflows, `a caption overflows its box: ${JSON.stringify(caption)}`);
  }
}

// The modern desktop at 1400 × 1000: the inline picker (Levels keeps its slot unseen), the hint
// in the card's body and the credits line.
function checkModernDesktop(facts, light = false) {
  checkModern(facts, false, light);
  assert.equal(facts.layout, "desktop");
  assert.equal(facts.toolbarRows, 1, "one toolbar row");
  assert.ok(!facts.tools.includes("levels-open"), `an inline picker, no Levels: ${facts.tools}`);
  assert.equal(facts.levelsSlot?.visibility, "hidden", "Levels keeps its slot, unseen");
  assert.equal(facts.captions.find((caption) => caption.className === "cap-body")?.text, HINTS[0], "the hint in the card's body");
  assert.ok(facts.captions.some((caption) => caption.className === "cap-credit" && caption.text.length > 0), "a credits line");
}

// The modern phone portrait: Levels and the compact "C1" in the toolbar, the stage sized by
// stage_height for the room below the toolbar; no page scroll when the room holds Stacked's
// minimum (`fits`), and a page taller than the screen otherwise.
function checkModernPortrait(facts, { fits = true, light = false } = {}) {
  checkModern(facts, true, light);
  assert.equal(facts.layout, "portrait");
  assert.ok(facts.levelsOpen, "Levels shows");
  assert.equal(facts.gameShort, "C1", "the compact game control");
  const wanted = glue.stage_height(facts.clientWidth, facts.stageRoom, true);
  assertNear(facts.stageHeight, Math.max(facts.stageRoom, wanted), "the stage height");
  assert.equal(wanted <= facts.stageRoom + 0.5, fits, `the room ${facts.stageRoom} ${fits ? "holds" : "is short of"} the stage ${wanted}`);
  assert.equal(facts.canvasTouch, "none", "the canvas never pans the page itself");
  if (fits) assert.ok(facts.overflowY <= 0, `no page scroll: ${facts.overflowY}`);
  else assert.ok(facts.overflowY > 0, `the page is taller than the screen: ${facts.overflowY}`);
}

// The landscape rail: every control shown whole, inside the viewport and the toolbar's box (whose
// overflow scrolls), Help among them; below 380 px Turn clockwise and Full screen leave it for the
// Actions menu (modern only).
function checkRail(facts) {
  const { toolbarBox: bar } = facts;
  assert.ok(facts.tools.includes("help-open") && facts.tools.includes("actions"), `the rail: ${facts.tools}`);
  for (const r of facts.toolRects) {
    const whole = r.left >= -0.5 && r.right <= facts.width + 0.5 && r.top >= Math.max(0, bar.top) - 0.5 && r.bottom <= Math.min(facts.height, bar.bottom) + 0.5;
    assert.ok(whole, `the rail's ${r.id} ${JSON.stringify(r)} is cut off (toolbar ${JSON.stringify(bar)}, viewport ${facts.width} × ${facts.height})`);
  }
  const short = facts.look === "modern" && facts.height < 380;
  assert.equal(facts.tools.includes("clockwise"), !short, `Turn clockwise ${short ? "leaves" : "stays on"} the rail at ${facts.height} px`);
}

// A landscape phone with its browser's bars: the room (the viewport beside the rail) holds the
// stage (stage_height needs no more), so the page never scrolls, and the whole card, its captions
// included, is on the canvas, on screen. The compact card sends the credits to the Help.
function checkModernLandscapeBars(facts) {
  checkModern(facts, true);
  assert.equal(facts.layout, "landscape");
  checkRail(facts);
  assert.equal(facts.toolbarColumns, 1, "the toolbar is a rail down the side");
  const { width } = facts.lightbox;
  const wanted = glue.stage_height(width, facts.height, true);
  assert.ok(wanted <= facts.height + 0.5, `the room ${facts.height} holds the stage ${wanted}`);
  assertNear(facts.stageHeight, facts.height, "the stage is the room");
  assert.ok(facts.overflowY <= 0 && facts.overflowX <= 0, `no page scroll: ${facts.overflowX}, ${facts.overflowY}`);
  assert.ok(facts.lightbox.bottom <= facts.height + 0.5, `the canvas on screen: ${JSON.stringify(facts.lightbox)}`);
  assert.ok(facts.levelsOpen, "Levels shows: the picker is a sheet");
  assert.ok(facts.creditsHelp.length > 0, "a compact card: the credits in the Help");
  const box = facts.lightbox;
  for (const { className, text, rect } of facts.captions) {
    const on = rect.left >= box.left - 0.5 && rect.right <= box.right + 0.5 && rect.top >= box.top - 0.5 && rect.bottom <= box.bottom + 0.5;
    assert.ok(on, `the caption ${className} "${text}" ${JSON.stringify(rect)} lies on the canvas ${JSON.stringify(box)}`);
  }
}

// What each view must show.
const CHECKS = {
  "modern-desktop": checkModernDesktop,
  "modern-desktop-2x": checkModernDesktop,
  "modern-reload": checkModernDesktop,
  "modern-motion": checkModernDesktop,
  "modern-reduced": checkModernDesktop,
  "modern-reduced-live": checkModernDesktop,
  "modern-light-desktop": (facts) => checkModernDesktop(facts, true),
  "modern-light-portrait": (facts) => checkModernPortrait(facts, { light: true }),
  "classic-light": (facts) => CHECKS.desktop(facts),
  "modern-theme-live": (facts) => checkModernDesktop(facts),
  "classic-theme-live": (facts) => CHECKS.desktop(facts),
  "modern-light-motion": (facts) => checkModernDesktop(facts, true),
  "modern-desktop-narrow"(facts) {
    checkModern(facts, false);
    assert.equal(facts.layout, "desktop");
    assert.ok(facts.levelsSlot !== null, "Levels keeps its slot in the toolbar");
  },
  "modern-portrait": (facts) => checkModernPortrait(facts),
  "modern-portrait-bars": (facts) => checkModernPortrait(facts),
  "modern-360": (facts) => checkModernPortrait(facts),
  "modern-375": (facts) => checkModernPortrait(facts),
  "modern-sheet": (facts) => checkModernPortrait(facts),
  "modern-won": (facts) => checkModernPortrait(facts),
  "modern-375-bars": (facts) => checkModernPortrait(facts, { fits: false }),
  "modern-360-bars": (facts) => checkModernPortrait(facts, { fits: false }),
  "modern-landscape-bars-844": checkModernLandscapeBars,
  "modern-landscape-bars-667": checkModernLandscapeBars,
  "modern-landscape-bars-740": checkModernLandscapeBars,
  "modern-landscape-bars-800": checkModernLandscapeBars,
  "modern-landscape-tiny"(facts) {
    checkModern(facts, true);
    assert.equal(facts.layout, "landscape");
    checkRail(facts);
    const wanted = glue.stage_height(facts.lightbox.width, facts.height, true);
    assert.ok(wanted > facts.height + 0.5, `a stage ${wanted} taller than the room ${facts.height}`);
    assertNear(facts.stageHeight, wanted, "the stage grew to stage_height");
    assert.ok(facts.scrollable && facts.overflowY > 0, `the page scrolls: ${facts.overflowY}`);
  },
  "modern-landscape"(facts) {
    checkModern(facts, true);
    assert.equal(facts.layout, "landscape");
    checkRail(facts);
    assert.equal(facts.toolbarColumns, 1, "the toolbar is a rail down the side");
    assert.ok(facts.toolbarBottom <= facts.height, "the rail fits the height");
    assert.ok(facts.gameShort !== null && facts.gameShort.startsWith("C1"), `the compact game control: ${facts.gameShort}`);
    // A compact card: the credits go to the Help, after the footer.
    assert.ok(facts.creditsHelp.length > 0, "the credits in the Help");
    assert.ok(!facts.captions.some((caption) => caption.className === "cap-credit"), "no credits line on the canvas");
  },
  desktop(facts) {
    assert.equal(facts.layout, "desktop");
    assert.equal(facts.look, "classic");
    assert.equal(facts.toolbarRows, 1, "one toolbar row");
    assert.deepEqual(facts.tools, ["game-select", "previous", "next", "actions", "wheel", "fullscreen", "help-open"]);
    const { left, top, width, height } = facts.game;
    assert.deepEqual({ left, width, height }, { left: DESKTOP_GAME.left, width: DESKTOP_GAME.width, height: DESKTOP_GAME.height }, "the desktop game where it was");
    // `top` follows the toolbar's font metrics, so it may differ a little between machines.
    assert.ok(Math.abs(top - DESKTOP_GAME.top) <= 6, `the desktop game's top ${top} is not within 6 px of ${DESKTOP_GAME.top}`);
    assert.equal(facts.framePixels, expected.frame, "the engine's frame, pixel for pixel");
  },
  landscape(facts) {
    assert.equal(facts.layout, "landscape");
    checkRail(facts);
    assert.ok(facts.overflowX <= 0 && facts.overflowY <= 0, `the page does not scroll: ${facts.overflowX}, ${facts.overflowY}`);
    assert.equal(facts.toolbarColumns, 1, "the toolbar is one column down the side");
    assert.ok(facts.toolbarBottom <= facts.height, "the toolbar fits the height");
    assert.deepEqual(facts.tools, ["game-select", "previous", "next", "actions", "clockwise", "fullscreen", "help-open"]);
    assert.equal(facts.board, null, "no portrait parts");
    assert.ok(facts.game.left >= facts.header.right, "the frame beside the toolbar");
    const fit = Math.min((facts.clientWidth - facts.header.width) / 640, facts.height / 480);
    assertNear(facts.game.width, 640 * fillScale(fit, facts.ratio), "the frame fills the height");
    assert.ok(facts.game.bottom <= facts.height + 0.5, "the whole frame on screen");
    assert.equal(facts.framePixels, expected.frame, "the engine's frame, pixel for pixel");
  },
  "forced-touch"(facts) {
    assert.equal(facts.layout, "portrait", "?layout=touch with a mouse");
  },
  "portrait-bars"(facts) {
    checkPortrait(facts);
    assert.ok(facts.texts.top >= facts.height - 1, `the texts start below the screen, for the page to scroll to: ${facts.texts.top}`);
  },
  portrait(facts) {
    checkPortrait(facts);
  },
  "portrait-wechat"(facts) {
    checkPortrait(facts);
    // The tall WeChat view would have filled the width with the toolbox: 2x (316 px) it stays.
    assertNear(facts.toolbox.width, 316, "the toolbox capped at 2x");
    assertNear(facts.toolbox.left, 37, "and centred", 1);
    assertNear(facts.clientWidth - facts.toolbox.right, 37, "with equal gutters", 1);
  },
  analytics(facts) {
    assert.equal(facts.layout, "desktop");
  },
};

// Steps after a view's first picture, which may take more pictures.
const ACTIONS = {
  async "portrait-bars"(facts) {
    await checkPortraitScroll("portrait-bars", facts);
    // Scrolled part way, a touch drag from toolbox slot 0 still lands on board cell (8, 7): the
    // views are measured at each event. The drag itself does not scroll the page.
    const by = Math.min(120, facts.overflowY);
    await evaluate(`window.scrollTo(0, ${by})`);
    await evaluate(SETTLE);
    const moved = await evaluate(FACTS);
    let results;
    try {
      const slot = clientPoint(moved.toolbox, TOOLBOX, [460, 20]);
      const cell = clientPoint(moved.board, BOARD, [252, 198]);
      const start = await evaluate(CELLS);
      await touch("touchStart", ...slot);
      for (let step = 1; step <= 12; step += 1) {
        const t = step / 12;
        await touch("touchMove", slot[0] + (cell[0] - slot[0]) * t, slot[1] + (cell[1] + LIFT - slot[1]) * t);
      }
      await touch("touchEnd");
      await evaluate(SETTLE);
      const dropped = await evaluate(CELLS);
      const afterDrag = (await evaluate(FACTS)).scrollY;
      await shoot("portrait-bars-scrolled-drop");
      // A scroll while a drag is live (another finger on the texts) ends the drag: the piece stays
      // on its cell, and the finger's later moves and release reach nothing.
      await touch("touchStart", ...cell);
      for (let step = 1; step <= 4; step += 1) await touch("touchMove", cell[0], cell[1] - 15 * step);
      await evaluate(SETTLE);
      const lifted = await evaluate(CELLS);
      // Past the press's grace for a settling fling (touch_layout.js SCROLL_GRACE).
      await delay(200);
      await evaluate("window.scrollBy(0, -60)");
      await evaluate(SETTLE);
      const ended = await evaluate(CELLS);
      const back = await evaluate(FACTS);
      const beside = clientPoint(back.board, BOARD, [276, 198]);
      for (let step = 1; step <= 4; step += 1) {
        await touch("touchMove", cell[0] + (beside[0] - cell[0]) * (step / 4), cell[1] - 60 + (beside[1] + LIFT - cell[1] + 60) * (step / 4));
      }
      await touch("touchEnd");
      await evaluate(SETTLE);
      const released = await evaluate(CELLS);
      // A scroll event right after the press (a fling settling, the page unmoved) and a 3 px
      // scroll later leave the drag alone: it carries the piece from cell (8, 7) to (9, 7).
      const near = await evaluate(FACTS);
      const from = clientPoint(near.board, BOARD, [252, 198]);
      await touch("touchStart", ...from);
      await evaluate(`window.dispatchEvent(new Event("scroll"))`);
      for (let step = 1; step <= 3; step += 1) await touch("touchMove", from[0], from[1] - 15 * step);
      await delay(200);
      await evaluate("window.scrollBy(0, 3)");
      await evaluate(SETTLE);
      const nudged = await evaluate(FACTS);
      const to = clientPoint(nudged.board, BOARD, [276, 198]);
      const finger = [from[0], from[1] - 45];
      for (let step = 1; step <= 4; step += 1) {
        await touch("touchMove", finger[0] + (to[0] - finger[0]) * (step / 4), finger[1] + (to[1] + LIFT - finger[1]) * (step / 4));
      }
      await touch("touchEnd");
      await evaluate(SETTLE);
      const kept = await evaluate(CELLS);
      results = { cell, start, dropped, afterDrag, lifted, ended, back, released, nudge: nudged.scrollY - near.scrollY, kept };
    } finally {
      // Reset, so the later views start from the fresh level whatever happened.
      await evaluate(`document.querySelector("#actions-menu [data-command=reset]").click()`);
      await evaluate("window.scrollTo(0, 0)");
      await evaluate(SETTLE);
    }
    const { cell, start, dropped, afterDrag, lifted, ended, back, released, nudge, kept } = results;
    assertNear(moved.scrollY, by, "scrolled part way", 1);
    assertNear(moved.board.top, facts.board.top - by, "the board moved up with the page", 1);
    assert.ok(cell[1] > moved.header.bottom, "the drop cell is on screen below the toolbar");
    assert.ok(dropped.slot !== start.slot && dropped.cell !== start.cell, "the scrolled drag moved the piece from toolbox slot 0 to board cell (8, 7)");
    assertNear(afterDrag, by, "a drag on the parts did not scroll the page", 0);
    assert.notEqual(lifted.cell, dropped.cell, "the piece is lifted off its cell before the scroll");
    assertNear(back.scrollY, by - 60, "the page scrolled mid-drag", 1);
    assert.equal(ended.cell, dropped.cell, "the scroll ended the drag: the piece is back on its cell");
    assert.deepEqual({ cell: released.cell, beside: released.beside }, { cell: dropped.cell, beside: dropped.beside }, "and the release after it moved nothing");
    assertNear(nudge, 3, "the page moved 3 px mid-drag", 0.5);
    assert.ok(kept.cell !== released.cell && kept.beside !== released.beside, "a settling fling and a tiny scroll left the drag alone: the piece moved to cell (9, 7)");
    const reset = await evaluate(FACTS);
    assert.deepEqual([reset.boardPixels, reset.toolboxPixels], [expected.board, expected.toolbox], "Reset put the piece back");
    // Win, scroll to the end, and tap Next level there: level 2 shows from the top of the page.
    await evaluate(PASTE_WIN);
    await evaluate(SETTLE);
    await evaluate("window.scrollTo(0, 9999)");
    await evaluate(SETTLE);
    const atEnd = await evaluate(`(() => {
      const rect = document.getElementById("next-level").getBoundingClientRect();
      const x = rect.left + rect.width / 2;
      const y = rect.top + rect.height / 2;
      return { x, y, scrollY, hit: document.elementFromPoint(x, y)?.id === "next-level" };
    })()`);
    let level2;
    try {
      await touch("touchStart", atEnd.x, atEnd.y);
      await touch("touchEnd");
      await evaluate(SETTLE);
      level2 = { facts: await evaluate(FACTS), win: await evaluate(WIN_FACTS) };
    } finally {
      // Level 1's win and the move to level 2 stay out of the next view's storage.
      await evaluate(`(() => {
        Storage.prototype.setItem = () => {};
        localStorage.removeItem("chromatron1.save");
        localStorage.removeItem("chromatron1.lastLevelSlot");
      })()`);
    }
    assert.ok(atEnd.scrollY > 0 && atEnd.hit, `the Next level button on screen at the end of the page: ${JSON.stringify(atEnd)}`);
    assert.equal(level2.facts.scrollY, 0, "the next level scrolled the page back to the top");
    assert.ok(level2.facts.boardPixels !== expected.board && level2.win.button === null, "level 2's board shows");
    assert.ok(level2.win.listed.length > 0 && !level2.win.listed.some((line) => /win/i.test(line)), `level 2 is not won: ${JSON.stringify(level2.win.listed)}`);
  },
  async portrait(facts) {
    await checkPortraitScroll("portrait", facts);
    const slot = clientPoint(facts.toolbox, TOOLBOX, [460, 20]);
    const cell = clientPoint(facts.board, BOARD, [252, 198]);
    const start = await evaluate(CELLS);
    // A touch drag: the piece rides LIFT px above the finger and lands on the cell under it.
    await touch("touchStart", ...slot);
    for (let step = 1; step <= 12; step += 1) {
      const t = step / 12;
      await touch("touchMove", slot[0] + (cell[0] - slot[0]) * t, slot[1] + (cell[1] + LIFT - slot[1]) * t);
    }
    await evaluate(SETTLE);
    await shoot("portrait-dragging");
    await touch("touchEnd");
    await evaluate(SETTLE);
    await shoot("portrait-dropped");
    const dropped = await evaluate(CELLS);
    assert.notEqual(dropped.slot, start.slot, "the piece left toolbox slot 0");
    assert.notEqual(dropped.cell, start.cell, "and landed on board cell (8, 7), under the lifted point");
    // A 5 px wobble is still a tap: it turns the piece.
    await touch("touchStart", ...cell);
    await touch("touchMove", cell[0] + 5, cell[1] + 2);
    await touch("touchEnd");
    await evaluate(SETTLE);
    const turned = await evaluate(CELLS);
    assert.ok(turned.cell !== dropped.cell && turned.cell !== start.cell, "a wobbly tap turned the piece where it lies");
    // A swipe that carries no piece turns nothing, though its lifted point ends on the piece.
    await touch("touchStart", cell[0], cell[1] + 2 * LIFT);
    await touch("touchMove", cell[0], cell[1] + 2 * LIFT - 10);
    await touch("touchMove", cell[0], cell[1] + LIFT);
    await touch("touchEnd");
    await evaluate(SETTLE);
    assert.equal((await evaluate(CELLS)).cell, turned.cell, "a swipe without a piece turned nothing");
    // A drop in the gap between the board and the toolbox is off the frame: the piece goes back
    // to its cell, unturned, and the toolbox slot stays empty.
    const gap = (facts.board.bottom + facts.toolbox.top) / 2;
    assert.ok(facts.toolbox.top - facts.board.bottom >= 2, "a gap between the board and the toolbox");
    await touch("touchStart", ...cell);
    for (let step = 1; step <= 6; step += 1) {
      await touch("touchMove", cell[0], cell[1] + (gap + LIFT - cell[1]) * (step / 6));
    }
    await evaluate(SETTLE);
    // A live drag: the piece is no longer drawn on its cell (the beams stay as they were until the drop).
    assert.notEqual((await evaluate(CELLS)).cell, turned.cell, "the piece is lifted off its cell, over the gap");
    await touch("touchEnd");
    await evaluate(SETTLE);
    const gapped = await evaluate(CELLS);
    assert.deepEqual({ slot: gapped.slot, cell: gapped.cell }, { slot: dropped.slot, cell: turned.cell }, "a drop in the gap left the piece on its cell, unturned");
    // A touch drag: the finger goes from `from` to LIFT px below `to`, so the drop lands on `to`.
    const drag = async (from, to) => {
      await touch("touchStart", ...from);
      for (let step = 1; step <= 6; step += 1) {
        await touch("touchMove", from[0] + (to[0] - from[0]) * (step / 6), from[1] + (to[1] + LIFT - from[1]) * (step / 6));
      }
      await touch("touchEnd");
      await evaluate(SETTLE);
    };
    // A piece on the board's top row, whose lifted point is above the board: a sideways drag still
    // picks it up (the drag starts at the finger), and the drop lands under the lifted point.
    const top = clientPoint(facts.board, BOARD, [252, 30]);
    await drag(cell, top);
    const onTop = await evaluate(CELLS);
    assert.ok(onTop.top !== gapped.top && onTop.cell === start.cell, "the piece moved to board cell (8, 0)");
    await touch("touchStart", ...top);
    for (let step = 1; step <= 6; step += 1) await touch("touchMove", top[0] - 10 * step, top[1]);
    await evaluate(SETTLE);
    assert.equal((await evaluate(CELLS)).top, gapped.top, "a sideways drag picked the piece up off board row 0");
    const aside = [top[0] - 60, top[1]];
    for (let step = 1; step <= 6; step += 1) {
      await touch("touchMove", aside[0] + (cell[0] - aside[0]) * (step / 6), aside[1] + (cell[1] + LIFT - aside[1]) * (step / 6));
    }
    await touch("touchEnd");
    await evaluate(SETTLE);
    const returned = await evaluate(CELLS);
    assert.deepEqual({ cell: returned.cell, top: returned.top }, { cell: turned.cell, top: gapped.top }, "and dropped it back on cell (8, 7), unturned");
    // A right-click on the board turns the piece clockwise and keeps the browser's menu away;
    // on the texts the menu stays.
    assert.deepEqual(await rightClick(...cell), [true], "no browser menu on the portrait board");
    assert.notEqual((await evaluate(CELLS)).cell, turned.cell, "the right-click turned the piece");
    const scrollY = await evaluate("scrollY");
    const texts = await evaluate(`(() => { document.getElementById("text-list").scrollIntoView(); const r = document.getElementById("text-list").getBoundingClientRect(); return [r.left + r.width / 2, r.top + Math.min(r.height / 2, 10)]; })()`);
    assert.deepEqual(await rightClick(...texts), [false], "the browser menu stays on the portrait texts");
    await evaluate(`scrollTo(0, ${scrollY})`);
    await evaluate(SETTLE);
    // Turn the piece round to where it was, for the steps below.
    for (let turns = 0; turns < 8 && (await evaluate(CELLS)).cell !== turned.cell; turns += 1) await rightClick(...cell);
    assert.equal((await evaluate(CELLS)).cell, turned.cell, "right-clicks brought the piece round");
    // Turning the phone mid-drag ends the drag: the piece stays where it was.
    await touch("touchStart", ...cell);
    await touch("touchMove", cell[0], cell[1] - 30);
    await touch("touchMove", cell[0], cell[1] - 60);
    await evaluate(SETTLE);
    assert.notEqual((await evaluate(CELLS)).cell, turned.cell, "the piece is lifted off its cell before the turn");
    await send("Emulation.setDeviceMetricsOverride", { width: 844, height: 390, deviceScaleFactor: 3, mobile: true });
    await evaluate(SETTLE);
    assert.equal((await evaluate(CELLS)).frameCell, turned.cell, "landscape shows the piece back on its cell");
    // The finger lifts over the piece, where even a release still taken for a lifted drag would
    // land: it reaches nothing.
    const landscape = await evaluate(FACTS);
    const onCell = clientPoint(landscape.game, [0, 0, 640, 480], [252, 198]);
    await touch("touchMove", onCell[0], onCell[1] + LIFT);
    await touch("touchEnd");
    await evaluate(SETTLE);
    assert.equal((await evaluate(CELLS)).frameCell, turned.cell, "a release over the piece after the turn turned nothing");
    await send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 3, mobile: true });
    await evaluate(SETTLE);
    assert.equal((await evaluate(CELLS)).cell, turned.cell, "the piece stayed on its cell");
    // The Levels picker: 50 numbers in the strip's colours, the current one marked.
    await evaluate(`document.getElementById("levels-open").click()`);
    await evaluate(SETTLE);
    await shoot("portrait-levels");
    const picker = await evaluate(`(() => {
      const buttons = [...document.querySelectorAll("#level-grid button")];
      return {
        open: document.getElementById("levels").open,
        count: buttons.length,
        current: buttons.findIndex((button) => button.getAttribute("aria-current") === "true"),
        locked: buttons.filter((button) => button.disabled).length,
        label: buttons[0]?.getAttribute("aria-label"),
        focused: document.activeElement === buttons[0],
      };
    })()`);
    assert.ok(picker.open && picker.count === 50 && picker.current === 0 && picker.focused, JSON.stringify(picker));
    assert.ok(picker.locked >= 48 && picker.locked <= 49, `a fresh Chromatron 1 opens one or two levels: ${picker.locked}`);
    assert.equal(picker.label, "Level 1, current");
    const beforeChoice = await evaluate(CELLS);
    await evaluate(`document.querySelector("#level-grid button").click()`);
    await evaluate(SETTLE);
    assert.equal(await evaluate(`document.getElementById("levels").open`), false, "choosing a level closes the picker");
    assert.deepEqual(await evaluate(CELLS), beforeChoice, "choosing the current level only closed the picker: the board kept its pieces");
    // The help, with its Touch section.
    await evaluate(`document.getElementById("help-open").click()`);
    await evaluate(SETTLE);
    await shoot("portrait-help");
    await evaluate(`document.getElementById("help-close").click()`);
    // With "Turn clockwise" on, a lifted drag still drops the piece unturned, and a tap turns it
    // clockwise (a plain tap turns counter-clockwise, so this one undoes the wobbly tap).
    const back = await evaluate(FACTS);
    const here = clientPoint(back.board, BOARD, [252, 198]);
    const beside = clientPoint(back.board, BOARD, [276, 198]);
    await evaluate(`document.getElementById("clockwise").click()`);
    assert.equal(await evaluate(`document.getElementById("clockwise").getAttribute("aria-pressed")`), "true", "Turn clockwise is on");
    const emptyBeside = (await evaluate(CELLS)).beside;
    await drag(here, beside);
    const moved = await evaluate(CELLS);
    assert.ok(moved.cell === start.cell && moved.beside !== emptyBeside, "with Turn clockwise on, a drag moved the piece to cell (9, 7)");
    await drag(beside, here);
    const home = await evaluate(CELLS);
    assert.deepEqual({ cell: home.cell, beside: home.beside }, { cell: turned.cell, beside: emptyBeside }, "and a drag back dropped it unturned");
    await touch("touchStart", ...here);
    await touch("touchEnd");
    await evaluate(SETTLE);
    assert.equal((await evaluate(CELLS)).cell, dropped.cell, "a tap with Turn clockwise on turned it clockwise");
    // The choice is remembered: a reload keeps it on, the desktop layout leaves the saved "1" alone,
    // and turning it off is remembered too. It ends on, as the steps below found it.
    const clockwisePressed = () => evaluate(`document.getElementById("clockwise").getAttribute("aria-pressed")`);
    const scenario = { name: "portrait", width: 390, height: 844, ratio: 3, touch: true };
    const pageUrl = await evaluate("location.origin + location.pathname");
    let reloads = 0;
    await load({ ...scenario, query: `&reload=${++reloads}&look=classic` }, pageUrl);
    assert.equal(await clockwisePressed(), "true", "Turn clockwise is still on after a reload");
    await load({ name: "desktop", width: 1400, height: 1000, ratio: 1, touch: false, query: "&look=classic" }, pageUrl);
    assert.equal(await evaluate(`localStorage.getItem("chromatron.clockwise")`), "1", "the desktop layout kept the saved choice");
    await load({ ...scenario, query: `&reload=${++reloads}&look=classic` }, pageUrl);
    assert.equal(await clockwisePressed(), "true", "and the phone layout still has it on");
    await evaluate(`document.getElementById("clockwise").click()`);
    await load({ ...scenario, query: `&reload=${++reloads}&look=classic` }, pageUrl);
    assert.equal(await clockwisePressed(), "false", "Turn clockwise stays off after a reload");
    await evaluate(`document.getElementById("clockwise").click()`);
    await load({ ...scenario, query: `&reload=${++reloads}&look=classic` }, pageUrl);
    assert.equal(await clockwisePressed(), "true", "and on again");
    // Solve level 1 by pasting its solution, move on, and the picker shows it as solved, with a tick.
    const levelFacts = `(() => {
      const buttons = [...document.querySelectorAll("#level-grid button")];
      return {
        open: document.getElementById("levels").open,
        labels: buttons.map((button) => button.getAttribute("aria-label")),
        ticks: buttons.map((button) => getComputedStyle(button, "::after").content),
      };
    })()`;
    await evaluate(`(() => {
      const clipboardData = new DataTransfer();
      clipboardData.setData("text/plain", "1-01-hI");
      document.dispatchEvent(new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true }));
    })()`);
    await evaluate(SETTLE);
    // Won: the Next level button sits under "You win!", and the hint is worded for touch.
    const won = await evaluate(WIN_FACTS);
    await shoot("portrait-next-level");
    assert.ok(won.button !== null, "the Next level button shows after a win");
    assert.equal(won.button.text, "Next level ▶");
    assert.equal(won.inList, "text-list");
    assert.ok(won.button.height >= 44 && won.button.width >= 0.8 * facts.clientWidth, `a comfortable target: ${JSON.stringify(won.button)}`);
    assert.ok(won.listed.includes(TOUCH_PORTRAIT), `the touch wording shows: ${JSON.stringify(won.listed)}`);
    assert.ok(!won.listed.includes(CLICK_NEXT), "the original click_next text does not");
    assert.ok(/win/i.test(won.listed[0]), "the button follows the win text");
    const buttonIndex = await evaluate(`[...document.getElementById("text-list").children].indexOf(document.getElementById("next-level"))`);
    assert.equal(buttonIndex, 1, "the button is right under the win text");
    await assertNoSelection("#texts", "#board-part", "#toolbox-part", "header", "#stage");
    assert.notEqual((await userSelect("#help p"))[0][1], "none", "the help keeps text selection");
    // The win shows in the toolbar too: its ▶ is highlighted like the Next level button.
    const wonNext = await evaluate(TOOLBAR_NEXT);
    assert.ok(wonNext.highlighted && wonNext.background === ACCENT, `the toolbar's ▶ is highlighted after a win: ${JSON.stringify(wonNext)}`);
    assert.equal(wonNext.label, "Next level: this one is solved");
    // Tapping the button, scrolled into view, is the toolbar's Next: level 2, and the button and
    // the highlight are gone.
    const shown = await revealNextButton();
    await shoot("portrait-next-level-scrolled");
    assert.ok(shown.hit && shown.top >= shown.header - 0.5 && shown.bottom <= facts.height + 0.5, `the Next level button is reachable by scrolling: ${JSON.stringify(shown)}`);
    await touch("touchStart", shown.x, shown.y);
    await touch("touchEnd");
    await evaluate(SETTLE);
    const onLevel2 = await evaluate(WIN_FACTS);
    assert.equal(onLevel2.button, null, "the button is hidden on the next level");
    assert.ok(onLevel2.listed.length > 0 && !onLevel2.listed.some((line) => /win/i.test(line)), `level 2 is not won: ${JSON.stringify(onLevel2.listed)}`);
    const plainNext = await evaluate(TOOLBAR_NEXT);
    assert.ok(!plainNext.highlighted && plainNext.background !== ACCENT && plainNext.label === null, `the toolbar's ▶ is plain on level 2: ${JSON.stringify(plainNext)}`);
    // Back on the solved level 1 the ▶ is highlighted again, and a tap on it is level 2, plain.
    await evaluate(`document.querySelector('.toolbar [data-command="previous"]').click()`);
    await evaluate(SETTLE);
    const again = await evaluate(TOOLBAR_NEXT);
    assert.ok(again.highlighted, "the solved level 1 highlights the ▶ again");
    await touch("touchStart", again.x, again.y);
    await touch("touchEnd");
    await evaluate(SETTLE);
    const pressed = await evaluate(TOOLBAR_NEXT);
    assert.ok(!pressed.highlighted && pressed.background !== ACCENT, `a tap on the ▶ went to level 2, plain: ${JSON.stringify(pressed)}`);
    assert.ok(!(await evaluate(WIN_FACTS)).listed.some((line) => /win/i.test(line)), "level 2 is not won");
    await evaluate(`document.getElementById("levels-open").click()`);
    await evaluate(SETTLE);
    await shoot("portrait-levels-solved");
    const solved = await evaluate(levelFacts);
    assert.ok(solved.open, "the picker opened");
    assert.deepEqual(solved.labels.slice(0, 2), ["Level 1, solved", "Level 2, current"], "level 1 solved, level 2 current");
    assert.equal(solved.ticks[0], '"\u2713"', "level 1 carries a tick");
    assert.ok(solved.ticks.slice(1).every((content) => content === "none"), "no other level does");
    await evaluate(`document.getElementById("levels-close").click()`);
    // Another game's picker shows that game's levels.
    await evaluate(`(() => {
      const select = document.getElementById("game-select");
      select.value = "2";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    })()`);
    await evaluate(SETTLE);
    await evaluate(`document.getElementById("levels-open").click()`);
    await evaluate(SETTLE);
    const other = await evaluate(levelFacts);
    assert.equal(other.labels[0], "Level 1, current", "game 2 starts on its level 1");
    assert.ok(other.ticks.every((content) => content === "none"), "and has solved nothing");
    assert.ok(other.labels.some((label) => label.endsWith("locked")) && other.labels[1] !== "Level 2, current", "and its later levels are not game 1's");
    await evaluate(`document.getElementById("levels-close").click()`);
  },
  async desktop() {
    // A right-click on the board keeps the browser's menu away (the event targets the stage, as the press captured the pointer).
    const game = await evaluate(`(() => { const r = document.getElementById("pixels").getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
    assert.deepEqual(await rightClick(...game), [true], "no browser menu on the desktop board");
    // The Actions menu opens under its button, runs Reset, closes, and leaves the keys to the game.
    await evaluate(`document.getElementById("actions").click()`);
    await evaluate(SETTLE);
    await shoot("desktop-actions");
    const open = await evaluate(`(() => {
      const menu = document.getElementById("actions-menu").getBoundingClientRect();
      const button = document.getElementById("actions").getBoundingClientRect();
      return { open: document.getElementById("actions-menu").matches(":popover-open"), below: menu.top >= button.bottom, left: menu.left };
    })()`);
    assert.ok(open.open && open.below && open.left >= 8, `the menu opens under its button: ${JSON.stringify(open)}`);
    await evaluate(`document.querySelector("#actions-menu [data-command=reset]").click()`);
    const after = await evaluate(`({ open: document.getElementById("actions-menu").matches(":popover-open"), focus: document.activeElement?.localName })`);
    assert.deepEqual(after, { open: false, focus: "body" }, "Reset closed the menu and gave the keys back");
    assert.equal(await evaluate(`document.getElementById("game-select").value`), "1", "the game picker shows the game");
    // An outside click and Escape both close the menu; opening the help closes it too.
    const menuOpen = `document.getElementById("actions-menu").matches(":popover-open")`;
    await evaluate(`document.getElementById("actions").click()`);
    await evaluate(SETTLE);
    assert.equal(await evaluate(menuOpen), true, "the menu opened again");
    await send("Input.dispatchMouseEvent", { type: "mousePressed", x: 20, y: 600, button: "left", clickCount: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", x: 20, y: 600, button: "left", clickCount: 1 });
    await evaluate(SETTLE);
    assert.equal(await evaluate(menuOpen), false, "an outside click closed the menu");
    // A real click: the browser closes a popover on Escape only after a user activation.
    const at = await evaluate(`(() => { const r = document.getElementById("actions").getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    await send("Input.dispatchMouseEvent", { type: "mousePressed", ...at, button: "left", clickCount: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", ...at, button: "left", clickCount: 1 });
    await evaluate(SETTLE);
    assert.equal(await evaluate(menuOpen), true, "a click on Actions opened the menu");
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    await evaluate(SETTLE);
    assert.equal(await evaluate(menuOpen), false, "Escape closed the menu");
    await evaluate(`document.getElementById("actions").click()`);
    await evaluate(SETTLE);
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "?", code: "Slash", text: "?" });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "?", code: "Slash" });
    await evaluate(SETTLE);
    assert.deepEqual(await evaluate(`({ menu: ${"${menuOpen}"}, help: document.getElementById("help").open })`.replace("${menuOpen}", menuOpen)), { menu: false, help: true }, "? closed the menu and opened the help");
    await evaluate(`document.getElementById("help-close").click()`);
    // A click that closes the menu does not reach the game: the reflector in toolbox slot 0 stays put.
    await evaluate(`document.getElementById("actions").click()`);
    const frame = await evaluate(`(() => { const r = document.getElementById("game").getBoundingClientRect(); return { left: r.left, top: r.top, scale: r.width / 640 }; })()`);
    for (const type of ["mousePressed", "mouseReleased"]) {
      await send("Input.dispatchMouseEvent", { type, x: frame.left + 460 * frame.scale, y: frame.top + 20 * frame.scale, button: "left", clickCount: 1 });
    }
    await evaluate(SETTLE);
    assert.equal(await evaluate(`document.getElementById("actions-menu").matches(":popover-open")`), false, "the click closed the menu");
    assert.equal((await evaluate(FACTS)).framePixels, expected.frame, "and turned nothing");
    // The game picker gives the keys back: Escape blurs it, "?" reaches the page, and neither a
    // typed letter nor an arrow switches the game.
    await evaluate(`document.getElementById("game-select").focus()`);
    await typeKey("Escape", "Escape");
    await evaluate(SETTLE);
    assert.equal(await evaluate(`document.activeElement.localName`), "body", "Escape gave the keys back");
    await evaluate(`document.getElementById("game-select").focus()`);
    await typeKey("c", "KeyC", "c");
    await typeKey("?", "Slash", "?");
    await evaluate(SETTLE);
    const picker = await evaluate(`({ game: document.getElementById("game-select").value, help: document.getElementById("help").open })`);
    assert.deepEqual(picker, { game: "1", help: true }, "a letter left the game alone, and ? opened the help");
    await evaluate(`document.getElementById("help-close").click()`);
    await evaluate(`document.getElementById("game-select").focus()`);
    await typeKey("ArrowDown", "ArrowDown");
    assert.equal(await evaluate(`document.getElementById("game-select").value`), "1", "an arrow opens the picker, not the next game");
    // After a win the game's own click_next text stays, and there is no button. The classic look
    // requests no frames after it (Task 12 review M5): its paints stop with the paste's.
    await evaluate(`document.getElementById("game-select").blur()`);
    await hook();
    await evaluate(PASTE_WIN);
    await evaluate(SETTLE);
    const painted = await evaluate(RENDERS);
    assert.ok(painted > 0, "the paste painted the classic frame");
    await evaluate(TWELVE_FRAMES);
    assert.equal(await evaluate(RENDERS), painted, "classic: no paints in the 12 frames after the win's");
    assert.equal(await evaluate("window.__session.next_wake(performance.now())"), -1, "classic: idle");
    const won = await evaluate(WIN_FACTS);
    await forgetWin();
    assert.ok(won.drawn.includes(CLICK_NEXT), `the original text stays on the desktop: ${JSON.stringify(won.drawn)}`);
    assert.ok(!won.drawn.includes(TOUCH_PORTRAIT) && !won.drawn.includes(TOUCH_LANDSCAPE), "and no touch wording");
    assert.equal(won.button, null, "no Next level button on the desktop");
    assert.equal(await evaluate(`getComputedStyle(document.getElementById("game")).userSelect`), "none");
  },
  async landscape(facts) {
    // With the toolbar down the side, the Actions menu opens beside its button.
    await evaluate(`document.getElementById("actions").click()`);
    await evaluate(SETTLE);
    await shoot("landscape-actions");
    const menu = await evaluate(`(() => {
      const menu = document.getElementById("actions-menu").getBoundingClientRect();
      const button = document.getElementById("actions").getBoundingClientRect();
      return { beside: menu.left >= button.right, inside: menu.bottom <= innerHeight };
    })()`);
    assert.deepEqual(menu, { beside: true, inside: true }, "the menu beside its button, on screen");
    await evaluate(`document.getElementById("actions-menu").hidePopover()`);
    // A touch drag straight left from toolbox slot 0, on the top row, whose lifted point is above
    // the frame: the piece still picks up (the drag starts at the finger), then drops under the
    // lifted point on board cell (5, 10).
    const frame = [0, 0, 640, 480];
    const slot = clientPoint(facts.game, frame, [460, 20]);
    const aside = clientPoint(facts.game, frame, [180, 270]);
    const start = await evaluate(CELLS);
    await touch("touchStart", ...slot);
    for (let step = 1; step <= 10; step += 1) await touch("touchMove", slot[0] - 30 * step, slot[1]);
    await evaluate(SETTLE);
    const picked = (await evaluate(CELLS)).frameSlot !== start.frameSlot;
    const left = [slot[0] - 300, slot[1]];
    for (let step = 1; step <= 6; step += 1) {
      await touch("touchMove", left[0] + (aside[0] - left[0]) * (step / 6), left[1] + (aside[1] + LIFT - left[1]) * (step / 6));
    }
    await touch("touchEnd");
    await evaluate(SETTLE);
    const dropped = await evaluate(CELLS);
    // Reset before any check, so the later views start from the fresh level whatever happened.
    await evaluate(`document.querySelector("#actions-menu [data-command=reset]").click()`);
    await evaluate(SETTLE);
    assert.ok(picked, "a drag straight left picked the piece up from toolbox slot 0");
    assert.ok(dropped.frameSlot !== start.frameSlot && dropped.frameAside !== start.frameAside, "and dropped it on board cell (5, 10)");
    assert.equal((await evaluate(FACTS)).framePixels, expected.frame, "Reset put the piece back");
    // After a win the text canvas draws the landscape wording, not the game's click_next.
    await evaluate(PASTE_WIN);
    await evaluate(SETTLE);
    const won = await evaluate(WIN_FACTS);
    await forgetWin();
    assert.ok(won.drawn.includes(TOUCH_LANDSCAPE), `the landscape wording is drawn: ${JSON.stringify(won.drawn)}`);
    assert.ok(!won.drawn.includes(CLICK_NEXT), "the original click_next text is not");
    assert.equal(won.button, null, "no Next level button in landscape");
    await assertNoSelection("#game", "header");
  },
  async analytics() {
    const scenario = SCENARIOS.find(({ name }) => name === "analytics");
    const pageUrl = await evaluate("location.origin + location.pathname");
    const desktop = { game: 1, layout: "desktop", look: "classic" };
    const nextLevel = `document.querySelector('.toolbar [data-command="next"]').click()`;
    // A first visit: the help opens before analytics loads, and its event waits for it.
    await forgetAll(true);
    await load({ ...scenario, query: `${scenario.query}&reload=1` }, pageUrl);
    const layer = await analyticsStarted();
    assert.deepEqual(
      layer.map(([command, target]) => [command, target]),
      [["js", null], ["config", "G-ZMH6NLL7C7"], ["event", "tutorial_begin"]],
      "the load sent the config and the first visit's help, and no level or game event",
    );
    assert.deepEqual(layer[1][2], { content_group: "chromatron" });
    assert.deepEqual(layer[2][2], { ...desktop, method: "first_visit" });
    await evaluate(`document.getElementById("help-close").click()`);
    // Win level 1 by pasting its solution, copy it, and paste it again: one win.
    await evaluate(PASTE_WIN);
    await evaluate(SETTLE);
    await evaluate(`(() => {
      const clipboardData = new DataTransfer();
      document.dispatchEvent(new ClipboardEvent("copy", { clipboardData, bubbles: true, cancelable: true }));
    })()`);
    await evaluate(PASTE_WIN);
    await evaluate(SETTLE);
    const level1 = { ...desktop, level: 1 };
    assert.deepEqual(await eventsNamed("paste_solution"), [{ ...level1, success: true }, { ...level1, success: true }]);
    assert.deepEqual(await eventsNamed("level_end"), [{ ...level1, level_name: "C1-L01", success: true }], "one level_end for one win");
    assert.deepEqual(await eventsNamed("copy_solution"), [level1]);
    // Next: level_start once level 2 has stayed a second; a quick ◀ ▶ there and back sends one more.
    await evaluate(nextLevel);
    await evaluate(SETTLE);
    assert.deepEqual(await eventsNamed("level_start"), [], "level_start waits for the level to stay");
    await delay(1300);
    const level2 = { ...desktop, level: 2, level_name: "C1-L02" };
    assert.deepEqual(await eventsNamed("level_start"), [level2]);
    await evaluate(`document.querySelector('.toolbar [data-command="previous"]').click()`);
    await evaluate(nextLevel);
    await delay(1300);
    assert.deepEqual(await eventsNamed("level_start"), [level2, level2], "one level_start for the level landed on");
    assert.equal((await eventsNamed("level_end")).length, 1, "passing the solved level 1 is no new win");
    // ? opens the help.
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "?", code: "Slash", text: "?" });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "?", code: "Slash" });
    await evaluate(SETTLE);
    assert.deepEqual((await eventsNamed("tutorial_begin")).at(-1), { ...desktop, method: "key" });
    await evaluate(`document.getElementById("help-close").click()`);
    // Actions ▸ Classic look, there and back: each select_look carries the look it chose.
    for (let k = 0; k < 2; k += 1) {
      await evaluate(`document.getElementById("classic-look").click()`);
      await evaluate(SETTLE);
    }
    assert.deepEqual(await eventsNamed("select_look"), [{ ...desktop, look: "modern" }, desktop], "select_look, to modern and back to classic");
    assert.equal((await evaluate(FACTS)).look, "classic", "back in the classic look");
    // The game picker, then a paste the game rejects.
    await evaluate(`(() => {
      const select = document.getElementById("game-select");
      select.value = "2";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    })()`);
    await evaluate(SETTLE);
    assert.deepEqual(await eventsNamed("select_game"), [{ game: 2, layout: "desktop", look: "classic" }]);
    await evaluate(PASTE_WIN);
    assert.deepEqual((await eventsNamed("paste_solution")).at(-1), { game: 2, layout: "desktop", look: "classic", level: 1, success: false });
    assert.deepEqual(pageErrors, [], "no errors on the page");
    // A gtag that throws (a broken or blocked script) changes nothing for the game: level 1 still wins.
    await forgetAll();
    await load({ ...scenario, query: `${scenario.query}&reload=2` }, pageUrl);
    await analyticsStarted();
    await evaluate(`window.gtag = () => { throw new Error("gtag is broken"); }`);
    await evaluate(PASTE_WIN);
    await evaluate(SETTLE);
    const won = await evaluate(WIN_FACTS);
    await evaluate(nextLevel);
    await delay(1300);
    const after = await evaluate(FACTS);
    await forgetAll();
    assert.ok(won.drawn.includes(CLICK_NEXT), `level 1 won with a throwing gtag: ${JSON.stringify(won.drawn)}`);
    assert.ok(!after.message.includes("stopped working"), after.message);
  },
  async "modern-desktop-2x"() {
    await hook();
    perfRows.push({ name: "modern-desktop-2x", ...(await coldPaintTime(SCENARIOS.find(({ name }) => name === "modern-desktop-2x"))) });
  },
  async "modern-portrait"() {
    await hook();
    perfRows.push({ name: "modern-portrait", ...(await coldPaintTime(SCENARIOS.find(({ name }) => name === "modern-portrait"))) });
  },
  // Task 13's toolbar re-wrap loop: across widths 912–944 at 667 px, each width change resizes the
  // session within 1 s (one or two session.resize calls), the canvas then has the backing that
  // width's CSS size and render scale give, and nothing changes for the next 400 ms; Levels keeps
  // its slot (visibility, never display: none) at every width.
  async "modern-desktop-narrow"(facts) {
    await hook();
    const SNAPSHOT = `(() => {
      const levels = document.getElementById("levels-open");
      const canvas = document.getElementById("lightbox");
      const box = canvas.getBoundingClientRect();
      return JSON.stringify({
        backing: [canvas.width, canvas.height],
        css: [box.width, box.height],
        viewport: document.documentElement.clientWidth,
        header: document.querySelector("header").getBoundingClientRect().height,
        resizes: window.__calls.resizes.length,
        slot: levels.getClientRects().length > 0 && getComputedStyle(levels).display !== "none",
      });
    })()`;
    const settled = [];
    let previous = JSON.parse(await evaluate(SNAPSHOT)).backing;
    for (let width = 912; width <= 944; width += 1) {
      await evaluate("window.__calls.resizes.length = 0");
      const start = Date.now();
      await send("Emulation.setDeviceMetricsOverride", { width, height: facts.height, deviceScaleFactor: 1, mobile: false });
      // The session is resized for the new width (the page debounces it) within 1 s...
      let last = await evaluate(SNAPSHOT);
      while (JSON.parse(last).resizes === 0 && Date.now() - start <= 1000) {
        await delay(20);
        last = await evaluate(SNAPSHOT);
      }
      assert.ok(JSON.parse(last).resizes >= 1, `at ${width} px no session.resize within 1 s: ${last}`);
      // ...then nothing changes for 400 ms, the last change no later than 1 s after the width's.
      let changedAt = Date.now();
      while (Date.now() - changedAt < 400 && Date.now() - start < 1600) {
        await delay(40);
        const now = await evaluate(SNAPSHOT);
        if (now !== last) {
          last = now;
          changedAt = Date.now();
        }
      }
      const state = JSON.parse(last);
      settled.push({ width, ms: changedAt - start, ...state });
      assert.ok(changedAt - start <= 1000 && Date.now() - changedAt >= 400, `at ${width} px the page settled ${changedAt - start} ms after the change: ${last}`);
      assert.ok(state.resizes >= 1 && state.resizes <= 2, `at ${width} px: ${state.resizes} session resizes`);
      // The settled canvas is this width's: as wide as the viewport, its backing at the render scale.
      const scale = glue.render_scale(state.css[0], state.css[1], 1, false);
      assertNear(state.css[0], state.viewport, `at ${width} px the canvas spans the viewport`);
      assert.deepEqual(state.backing, [Math.round(state.css[0] * scale), Math.round(state.css[1] * scale)], `at ${width} px backing = CSS size × render scale`);
      assert.notDeepEqual(state.backing, previous, `at ${width} px the backing changed from the last width's`);
      previous = state.backing;
      assert.ok(state.slot, `at ${width} px Levels keeps its slot, never display: none`);
    }
    const slowest = settled.reduce((a, b) => (b.ms > a.ms ? b : a));
    console.log(`modern-desktop-narrow: widths 912–944 settle, slowest ${slowest.ms} ms at ${slowest.width} px; toolbar heights ${[...new Set(settled.map(({ header }) => header))].join(", ")} px`);
    await resizeViewport({ width: facts.width, height: facts.height, deviceScaleFactor: 1, mobile: false });
  },
  // R8 (spec §16): C1's longest hint on the 390 × 664 phone. The body shrinks to 11 px at the
  // least, and only at 11 px may it clip (ruling 22): the size reached and the clip are recorded.
  async "modern-portrait-bars"() {
    try {
      for (let k = 0; k < LONGEST_SLOT; k += 1) {
        await typeKey("L", "KeyL", "L");
        await typeKey("=", "Equal", "=");
      }
      await evaluate(SETTLE);
      await shoot("modern-portrait-bars-longest-hint");
      const facts = await evaluate(FACTS);
      const body = facts.captions.find((caption) => caption.className === "cap-body");
      assert.equal(body?.text, HINTS[LONGEST_SLOT], `level ${LONGEST_SLOT + 1}'s hint in the card`);
      r8 = { level: LONGEST_SLOT + 1, length: HINTS[LONGEST_SLOT].length, fontSize: body.fontSize, clipped: body.clips };
      console.log(`R8: C1 level ${r8.level}'s hint (${r8.length} characters) at 390 × 664: ${r8.fontSize} px, ${r8.clipped ? "clipped" : "fits"}`);
      assert.ok(body.fontSize >= 11, `the body at ${body.fontSize} px`);
      assert.ok(!body.clips || body.fontSize === 11, `the body clips at ${body.fontSize} px`);
    } finally {
      await forgetAll();
    }
  },
  // The level sheet opens from the toolbar's Levels (one open_levels each time), hides the
  // captions and changes the canvas; Escape closes it.
  async "modern-sheet"() {
    await hook();
    await analyticsStarted();
    const before = await lightboxHash();
    const sheetFacts = [];
    const expanded = () => evaluate(`document.getElementById("levels-open").getAttribute("aria-expanded")`);
    assert.equal(await expanded(), "false", "Levels starts collapsed");
    // First a scripted click; then the keyboard path (review I2): Levels focused, Enter, and
    // Escape with the focus still on the button.
    for (let k = 1; k <= 2; k += 1) {
      if (k === 1) {
        await evaluate(`document.getElementById("levels-open").click()`);
      } else {
        await evaluate(`document.getElementById("levels-open").focus()`);
        await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, text: "\r" });
        await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
      }
      // The captions follow the sheet's panel (Task 12): two frames in, both are mid-fade.
      const fadingOut = await evaluate(CAPTIONS_MID_FADE);
      assert.ok(fadingOut.moving && fadingOut.spans > 0 && fadingOut.fading, `opening ${k}: the captions fade out with the panel fading in: ${JSON.stringify(fadingOut)}`);
      // The sheet fades in and out (Task 11): its pictures are taken once it has settled.
      assert.ok(await evaluate(IDLE), `opening ${k}: the sheet's fade in settles`);
      const open = await evaluate(FACTS);
      if (k === 1) await shoot("modern-sheet");
      const focus = await evaluate("document.activeElement?.id ?? null");
      sheetFacts.push({ open, focus, expanded: await expanded(), sheet: await evaluate("window.__session.sheet_open()"), hash: await lightboxHash(), events: await eventsNamed("open_levels") });
      await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
      // The carry from Task 11: the captions no longer come back at once over the fading panel.
      const fadingIn = await evaluate(CAPTIONS_MID_FADE);
      assert.ok(fadingIn.moving && fadingIn.spans > 0 && fadingIn.fading, `opening ${k}: the captions fade in with the panel fading out: ${JSON.stringify(fadingIn)}`);
      assert.ok(await evaluate(IDLE), `opening ${k}: the sheet's fade out settles`);
      sheetFacts.at(-1).closed = { facts: await evaluate(FACTS), expanded: await expanded(), sheet: await evaluate("window.__session.sheet_open()"), hash: await lightboxHash() };
    }
    const opened = { game: 1, layout: "portrait", look: "modern" };
    for (const [index, { open, focus, expanded: openExpanded, sheet, hash, events, closed }] of sheetFacts.entries()) {
      assert.ok(sheet && open.captionsVisibility === "hidden", `opening ${index + 1}: the sheet is open and the captions hidden: ${open.captionsVisibility}`);
      assert.equal(openExpanded, "true", `opening ${index + 1}: Levels is expanded`);
      if (index === 1) assert.equal(focus, "levels-open", "the keyboard path keeps the focus on Levels");
      assert.notEqual(hash, before, "the sheet is on the canvas");
      assert.deepEqual(events, Array(index + 1).fill(opened), "one open_levels per opening");
      assert.ok(!closed.sheet && closed.facts.captionsVisibility === "visible" && closed.facts.captionsOpacity === "1", `opening ${index + 1}: Escape closed the sheet and the captions are back`);
      assert.equal(closed.expanded, "false", `opening ${index + 1}: Levels is collapsed again`);
      assert.equal(closed.hash, before, "the canvas is as it was");
    }
    // Levels follows the other ways the sheet closes: a chip chosen (level 1's, which is open),
    // and a resize to a canvas with a level picker of its own (one without keeps the sheet).
    // A sheet that a resize closes (or a resize in the middle of the sheet's fade) is gone from
    // the canvas at once, a cold paint, while the captions still fade back in by CSS over 200 ms:
    // accepted (Task 12 review M2), cosmetic.
    await evaluate(`document.getElementById("levels-open").click()`);
    assert.ok(await evaluate(IDLE), "the sheet's fade in settles before a chip is chosen");
    const canvas = await evaluate(CANVAS);
    await tap(...clientOf(canvas, centreOf(await evaluate("[...window.__session.chip_rect(0)]"))));
    assert.ok(!(await evaluate("window.__session.sheet_open()")), "a chip closed the sheet");
    assert.equal(await expanded(), "false", "a chip choice collapses Levels");
    await evaluate(`document.getElementById("levels-open").click()`);
    await evaluate(SETTLE);
    try {
      await resizeViewport({ width: 844, height: 390, deviceScaleFactor: 3, mobile: true });
      assert.ok(await evaluate("window.__session.sheet_open()"), "the sheet survives a resize to landscape");
      assert.equal(await expanded(), "true", "and Levels stays expanded");
      await resizeViewport({ width: 1600, height: 1000, deviceScaleFactor: 1, mobile: true });
      assert.ok(await evaluate("window.__session.has_inline_picker() && !window.__session.sheet_open()"), "an inline picker closes the sheet");
      assert.equal(await expanded(), "false", "a resize that closes the sheet collapses Levels");
    } finally {
      await send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 3, mobile: true });
    }
  },
  // The won card on a phone: the touch wording, #win-live, and Next ▶ answering a tap at the drawn
  // pill's centre and its four corners (its hit rect is the union of its own and the drawn pill).
  async "modern-won"() {
    await hook();
    try {
      // Task 12's wake loop: the win animates frame after frame with no input...
      const before = await evaluate(RENDERS);
      await evaluate(PASTE_WIN);
      await evaluate(FRAMES);
      const animated = (await evaluate(RENDERS)) - before;
      assert.ok(animated >= 3, `the win animates on its own: ${animated} paints in 4 frames`);
      assert.ok(await evaluate("window.__session.next_wake(performance.now()) >= 0"), "the win is still running");
      // ...a resize in the middle of it (landscape; the session was still waking when it came:
      // review M7) paints the new size cold and settles...
      await resizeViewport({ width: 844, height: 390, deviceScaleFactor: 3, mobile: true });
      const midCue = await evaluate("window.__calls.resizes[0]");
      assert.ok(midCue.wake >= 0, `the resize landed mid-win: ${JSON.stringify(midCue)}`);
      assert.ok(await evaluate(IDLE), "a resize mid-win settles");
      const resized = await lightboxHash();
      await shoot("modern-won-resized-mid-win");
      // ...then the page is idle: no frame is requested, nothing paints.
      const settled = await evaluate(RENDERS);
      await evaluate(TWELVE_FRAMES);
      assert.equal(await evaluate(RENDERS), settled, "idle: nothing paints once the cues have run out");
      // The picture is the one a calm resize to that size gives.
      await resizeViewport({ width: 390, height: 844, deviceScaleFactor: 3, mobile: true });
      assert.ok(await evaluate(IDLE), "back to portrait, idle");
      await resizeViewport({ width: 844, height: 390, deviceScaleFactor: 3, mobile: true });
      assert.ok(await evaluate(IDLE), "landscape again, idle");
      assert.equal(await lightboxHash(), resized, "the resize mid-win left no trace of the win's animation");
      await resizeViewport({ width: 390, height: 844, deviceScaleFactor: 3, mobile: true });
      assert.ok(await evaluate(IDLE), "the won card settles");
      const won = await evaluate(FACTS);
      await shoot("modern-won");
      assert.equal(won.captions.find((caption) => caption.className === "cap-body")?.text, "Tap Next ▶ or pick a level from Levels.", "the touch wording");
      assert.equal(won.winLive, "You win!", "#win-live reads the win");
      // Read once: a win text typeset as a span (not lettered) is hidden from screen readers; a
      // lettered one has no span, and #win-live alone reads it.
      const winSpan = won.captions.find((caption) => caption.text === won.winLive);
      if (winSpan) assert.equal(winSpan.ariaHidden, "true", "the win span is aria-hidden");
      // The Next ▶ control: the points below the card's body that answer a press (nothing else
      // there does), then the drawn pill around them: the pixels brighter than the card.
      const next = await evaluate(`(() => {
        const session = window.__session;
        const canvas = document.getElementById("lightbox");
        const box = canvas.getBoundingClientRect();
        const k = canvas.width / box.width;
        const top = Math.ceil((document.querySelector("#captions > .cap-body").getBoundingClientRect().bottom - box.top) * k);
        let hit = null;
        for (let y = top; y < canvas.height; y += 2) {
          for (let x = 0; x < canvas.width; x += 2) {
            if (session.inert(x, y)) continue;
            hit = hit ? [Math.min(hit[0], x), Math.min(hit[1], y), Math.max(hit[2], x + 2), Math.max(hit[3], y + 2)] : [x, y, x + 2, y + 2];
          }
        }
        if (hit === null) return null;
        // The pill is looked for 12 px around the hit rect too: it is drawn, not assumed inside.
        const [l, t, r, b] = [Math.max(0, hit[0] - 12), Math.max(0, hit[1] - 12), Math.min(canvas.width, hit[2] + 12), Math.min(canvas.height, hit[3] + 12)];
        const data = canvas.getContext("2d").getImageData(l, t, r - l, b - t).data;
        let pill = null;
        for (let y = 0; y < b - t; y += 1) {
          for (let x = 0; x < r - l; x += 1) {
            const i = (y * (r - l) + x) * 4;
            if (Math.max(data[i], data[i + 1], data[i + 2]) < 48) continue;
            pill = pill ? [Math.min(pill[0], l + x), Math.min(pill[1], t + y), Math.max(pill[2], l + x + 1), Math.max(pill[3], t + y + 1)] : [l + x, t + y, l + x + 1, t + y + 1];
          }
        }
        return { hit, pill };
      })()`);
      assert.ok(next?.pill, `a Next ▶ pill under the body: ${JSON.stringify(next)}`);
      const canvas = await evaluate(CANVAS);
      const [l, t, r, b] = next.pill;
      assert.ok((r - l) * canvas.sx >= 44 && (b - t) * canvas.sy >= 28, `a pill-sized Next ▶: ${JSON.stringify(next)}`);
      // The centre, and each corner 2 CSS px inside the pill's bounds.
      const inset = 2 / canvas.sx;
      const points = { centre: centreOf(next.pill), "top left": [l + inset, t + inset], "top right": [r - inset, t + inset], "bottom left": [l + inset, b - inset], "bottom right": [r - inset, b - inset] };
      nextPill = { hit: next.hit, pill: next.pill, taps: {} };
      for (const [where, point] of Object.entries(points)) {
        assert.equal(await slotNow(), 0, `on level 1 before the ${where} tap`);
        assert.ok(await evaluate("window.__session.won() && window.__session.next_open()"), "Next ▶ shows");
        await tap(...clientOf(canvas, point));
        const slot = await slotNow();
        nextPill.taps[where] = slot === 1;
        assert.equal(slot, 1, `a tap at the drawn pill's ${where} ${clientOf(canvas, point).map((v) => v.toFixed(1))} went to level 2`);
        await typeKey("-", "Minus", "-");
        await evaluate(PASTE_WIN);
        await evaluate(SETTLE);
      }
    } finally {
      await forgetAll();
    }
  },
  // Look precedence (spec §11): the default is modern; Actions ▸ Classic look is stored and
  // survives a reload; ?look= wins over it and is never stored; toggling back stores modern.
  async "modern-reload"() {
    const scenario = SCENARIOS.find(({ name }) => name === "modern-reload");
    const pageUrl = await evaluate("location.origin + location.pathname");
    const lookNow = () => evaluate(`({ look: document.documentElement.dataset.look, stored: localStorage.getItem("chromatron.look"), checked: document.getElementById("classic-look").getAttribute("aria-checked") })`);
    try {
      await evaluate(`document.getElementById("classic-look").click()`);
      await evaluate(SETTLE);
      assert.deepEqual(await lookNow(), { look: "classic", stored: "classic", checked: "true" }, "Classic look, stored");
      await load({ ...scenario, query: "&reload=1" }, pageUrl);
      const reloaded = await evaluate(FACTS);
      await shoot("modern-reload-classic");
      assert.deepEqual(await lookNow(), { look: "classic", stored: "classic", checked: "true" }, "the Classic look survives a reload");
      assert.equal(reloaded.framePixels, expected.frame, "the engine's frame, pixel for pixel");
      await load({ ...scenario, query: "&reload=2&look=modern" }, pageUrl);
      assert.deepEqual(await lookNow(), { look: "modern", stored: "classic", checked: "false" }, "?look=modern wins, and is not stored");
      await load({ ...scenario, query: "&reload=3" }, pageUrl);
      assert.equal((await lookNow()).look, "classic", "the stored choice again without ?look=");
      await evaluate(`document.getElementById("classic-look").click()`);
      await evaluate(SETTLE);
      assert.deepEqual(await lookNow(), { look: "modern", stored: "modern", checked: "false" }, "toggled back to modern, stored");
      checkModernDesktop({ ...(await evaluate(FACTS)), storedLook: null });
    } finally {
      await forgetAll();
    }
  },
  // The small phones with their bars (ruling 19): the page is taller than the screen, and a touch
  // drag from the card (inert) scrolls it by hand until the card's end is in view, without one
  // press reaching the game; a tray piece still drags onto the board.
  "modern-375-bars": (facts) => scrollsByHand("modern-375-bars", facts),
  "modern-360-bars": (facts) => scrollsByHand("modern-360-bars", facts),
  "modern-motion": () => motionFrames("modern-motion"),
  "modern-reduced": () => reducedFrames("modern-reduced"),
  "modern-reduced-live": () => reducedLive("modern-reduced-live"),
  async "modern-light-desktop"() {
    const t = await theme();
    assert.equal(t.theme, "light");
    assert.equal(t.color, "#eef1f6");
    assert.equal(t.page, "rgb(238, 241, 246)");
    assert.equal(t.body, "rgb(58, 69, 102)", "the hint in the light body tone (#3a4566)");
    const corner = await evaluate(`(() => Array.from(document.getElementById("lightbox").getContext("2d").getImageData(2, 2, 1, 1).data.slice(0, 3)))()`);
    assert.ok(corner.every((c) => c >= 230), `light page: ${corner}`);
  },
  async "modern-light-portrait"() {
    const t = await theme();
    assert.equal(t.theme, "light");
    assert.equal(t.color, "#eef1f6");
    assert.equal(t.body, "rgb(58, 69, 102)", "the hint in the light body tone (#3a4566)");
  },
  async "classic-light"() {
    const t = await theme();
    assert.equal(t.theme, "dark", "classic keeps its dark page");
    assert.equal(t.color, "#25272a");
    assert.equal(t.page, "rgb(27, 28, 30)");
    // The first paint, before main.js and the wasm: already light for a modern visitor, and
    // still dark for a stored Classic or ?look=classic (the head script, not a CSS default).
    const light = { look: "modern", theme: "light", colors: ["#eef1f6", "#eef1f6"], page: "rgb(238, 241, 246)", toolbar: "rgb(255, 255, 255)", started: false };
    const dark = { look: "classic", theme: "dark", colors: ["#25272a", "#25272a"], page: "rgb(27, 28, 30)", toolbar: "rgb(37, 39, 42)", started: false };
    assert.deepEqual(await firstPaint(null), light, "a light system's first paint is light");
    assert.deepEqual(await firstPaint("classic"), dark, "a stored Classic's first paint stays dark");
    assert.deepEqual(await firstPaint(null, "&look=classic"), dark, "?look=classic's first paint stays dark");
  },
  // Review Focus 5 in a browser (final review M3): under a dark system in classic, the scheme turns
  // light (the page stays dark), then the player switches Classic look off: the modern page and
  // canvas come up light.
  async "classic-theme-live"() {
    try {
      assert.equal((await theme()).theme, "dark");
      await setMedia(LIGHT);
      await evaluate(SETTLE);
      const classic = await theme();
      assert.deepEqual([classic.theme, classic.color, classic.page], ["dark", "#25272a", "rgb(27, 28, 30)"], "classic keeps its dark page under the light scheme");
      await evaluate(`document.getElementById("classic-look").click()`);
      await hook();
      await evaluate(SETTLE);
      const modern = await theme();
      assert.equal(await evaluate("document.documentElement.dataset.look"), "modern");
      assert.deepEqual([modern.theme, modern.color, modern.page], ["light", "#eef1f6", "rgb(238, 241, 246)"], "the modern look comes up light");
      const corner = await evaluate(`(() => Array.from(document.getElementById("lightbox").getContext("2d").getImageData(2, 2, 1, 1).data.slice(0, 3)))()`);
      assert.ok(corner.every((c) => c >= 230), `the canvas is the light page: ${corner}`);
    } finally {
      await forgetAll().catch(() => {});
    }
  },
  async "modern-theme-live"() {
    await hook();
    const before = await theme();
    assert.equal(before.theme, "dark");
    const dark = await lightboxHash();
    await setMedia(LIGHT);
    await evaluate(SETTLE);
    const after = await theme();
    assert.equal(after.theme, "light", "the page follows at once");
    assert.equal(before.body, "rgb(195, 203, 230)", "the dark hint (#c3cbe6)");
    assert.equal(after.body, "rgb(58, 69, 102)", "the captions change colour");
    assert.notEqual(await lightboxHash(), dark, "the canvas repaints in light");
    const corner = await evaluate(`(() => Array.from(document.getElementById("lightbox").getContext("2d").getImageData(2, 2, 1, 1).data.slice(0, 3)))()`);
    assert.ok(corner.every((c) => c >= 230), `the repaint is the light page: ${corner}`);
    assert.equal(after.origin, before.origin, "no reload (the document's time origin survives the flip)");
    await setMedia(withScheme());
    await evaluate(SETTLE);
    assert.equal(await lightboxHash(), dark, "dark again, the same pixels");
  },
  async "modern-light-motion"() {
    await hook();
    await motionFrames("modern-light-motion");
    const dark = perfRows.find((row) => row.name === "modern-motion animation frames");
    const light = perfRows.find((row) => row.name === "modern-light-motion animation frames");
    if (dark && light) console.log(`perf: modern-light-motion p90 / modern-motion p90 = ${(light.p90 / dark.p90).toFixed(2)} (${light.p90.toFixed(1)} / ${dark.p90.toFixed(1)} ms)`);
  },
  "modern-landscape-bars-844": (facts) => landscapeBars("modern-landscape-bars-844", facts),
  "modern-landscape-bars-667": (facts) => landscapeBars("modern-landscape-bars-667", facts),
  "modern-landscape-bars-740": (facts) => landscapeBars("modern-landscape-bars-740", facts),
  "modern-landscape-bars-800": (facts) => landscapeBars("modern-landscape-bars-800", facts),
  // The compact card in landscape (Task 14): C2's level name sits between the lettered header and
  // "(won)", overlapping neither; its width is recorded here and at 740 × 390.
  async "modern-landscape"(facts) {
    await hook();
    try {
      // The game switched under the open level sheet (it stays open): the Help keeps none of the
      // last game's credits (Task 12 review M3); the new game's come with the captions when the
      // sheet closes.
      assert.ok((await evaluate(FACTS)).creditsHelp.length > 0, "C1's credits in the Help");
      await evaluate(`document.getElementById("levels-open").click()`);
      assert.ok(await evaluate(IDLE), "the sheet's fade in settles");
      await evaluate(`(() => {
        const select = document.getElementById("game-select");
        select.value = "2";
        select.dispatchEvent(new Event("change", { bubbles: true }));
      })()`);
      await evaluate(SETTLE);
      const switched = await evaluate(FACTS);
      assert.ok(await evaluate("window.__session.sheet_open() && window.__session.game() === 2"), "C2 under the open sheet");
      assert.equal(switched.creditsHelp, "", "no credits of C1 in the Help under the sheet");
      await typeKey("Escape", "Escape");
      assert.ok(await evaluate(IDLE), "the sheet's fade out settles");
      assert.ok((await evaluate(FACTS)).creditsHelp.length > 0, "C2's credits in the Help once the sheet closes");
      compactNames.push({ size: "844 × 390", state: "unsolved", ...(await compactName()) });
      // Won with the cheat key, then away and back: the level shows "(won)".
      await typeKey("z", "KeyZ", "z");
      await typeKey("=", "Equal", "=");
      await typeKey("-", "Minus", "-");
      await evaluate(SETTLE);
      await shoot("modern-landscape-c2-won");
      compactNames.push({ size: "844 × 390", state: "solved", ...(await compactName()) });
      await resizeViewport({ width: 740, height: 390, deviceScaleFactor: 3, mobile: true });
      await shoot("modern-landscape-740-c2-won");
      compactNames.push({ size: "740 × 390", state: "solved", ...(await compactName()) });
    } finally {
      await send("Emulation.setDeviceMetricsOverride", { width: facts.width, height: facts.height, deviceScaleFactor: facts.ratio, mobile: true });
      await forgetAll();
    }
    for (const name of compactNames) {
      assert.ok(name.gapToHeader !== null, `the lettered header left of the name: ${JSON.stringify(name)}`);
      console.log(`modern-landscape: C2's compact name at ${name.size}, ${name.state}: "${name.text}" in ${name.width.toFixed(1)} px${name.ellipsised ? " (ellipsised)" : ""}; ${name.gapToHeader.toFixed(1)} px right of the lettering${name.gapToBadge === null ? "" : `, ${name.gapToBadge.toFixed(1)} px left of "(won)"`}`);
      assert.ok(name.gapToHeader >= 0, `the name overlaps the lettered header: ${JSON.stringify(name)}`);
      if (name.gapToBadge !== null) assert.ok(name.gapToBadge >= 0, `the name overlaps "(won)": ${JSON.stringify(name)}`);
    }
    assert.ok(compactNames.some((name) => name.state === "solved" && name.gapToBadge !== null), "a solved level shows (won)");
  },
};

// Spec §14's web rows and the checks recorded for the report: printed at the end.
const perfRows = [];
let r8 = null;
let nextPill = null;
const compactNames = [];
const swipeStarts = [];
const landscapeBarsRows = [];
const sheetScrolls = [];

// The compact card's level name (CSS px): its box, the text's left edge (right-aligned, so the box
// less the text's width, unless it is ellipsised), the gap to the lettered header on its left (the
// rightmost bright canvas pixel on its row, scanning left until something answers a press) and to
// "(won)"'s text on its right.
async function compactName() {
  return evaluate(`(() => {
    const session = window.__session;
    const name = document.querySelector("#captions > .cap-title-compact");
    const badge = document.querySelector("#captions > .cap-badge");
    const canvas = document.getElementById("lightbox");
    const box = canvas.getBoundingClientRect();
    const k = canvas.width / box.width;
    const textOf = (span) => { const range = document.createRange(); range.selectNodeContents(span); return range.getBoundingClientRect(); };
    const r = name.getBoundingClientRect();
    const textWidth = textOf(name).width;
    const textLeft = Math.max(r.left, r.right - textWidth);
    const [top, bottom] = [Math.round((r.top - box.top) * k), Math.round((r.bottom - box.top) * k)];
    const data = canvas.getContext("2d").getImageData(0, top, canvas.width, bottom - top).data;
    let lettering = null;
    for (let x = Math.floor((textLeft - box.left) * k) - 1; x >= 0 && lettering === null; x -= 1) {
      if (!session.inert(x, (top + bottom) / 2)) break;
      for (let y = 0; y < bottom - top && lettering === null; y += 1) {
        const i = (y * canvas.width + x) * 4;
        if (Math.max(data[i], data[i + 1], data[i + 2]) >= 160) lettering = x + 1;
      }
    }
    return {
      text: name.textContent,
      width: r.width,
      ellipsised: name.scrollWidth > name.clientWidth,
      gapToHeader: lettering === null ? null : textLeft - (box.left + lettering / k),
      gapToBadge: badge ? textOf(badge).left - r.right : null,
    };
  })()`);
}

// A landscape phone with its bars (review I1): Beside (the tray right of the board), the board's
// cell and the tray's slot recorded, then level 1 won and Next ▶, found on the card under the
// won text (the points there that answer a press), on screen and taken by a tap to level 2.
async function landscapeBars(name, facts) {
  await hook();
  try {
    // Turn clockwise and Full screen, off the short rail, are in the Actions menu, whole and on
    // screen; the menu's Turn clockwise toggles the rail's.
    const ITEMS = `(() => {
      const item = (id) => { const e = document.getElementById(id); const r = e.getBoundingClientRect(); return { shown: e.getClientRects().length > 0, top: r.top, bottom: r.bottom, right: r.right, checked: e.getAttribute("aria-checked") }; };
      return { clockwise: item("clockwise-item"), fullscreen: item("fullscreen-item"), fullscreenEnabled: document.fullscreenEnabled, pressed: document.getElementById("clockwise").getAttribute("aria-pressed") };
    })()`;
    await evaluate(`document.getElementById("actions").click()`);
    await evaluate(SETTLE);
    const menu = await evaluate(ITEMS);
    await shoot(`${name}-actions`);
    for (const [what, item] of [["clockwise", menu.clockwise], ...(menu.fullscreenEnabled ? [["fullscreen", menu.fullscreen]] : [])]) {
      assert.ok(item.shown && item.top >= 0 && item.bottom <= facts.height + 0.5 && item.right <= facts.width + 0.5, `${name}: the menu's ${what} item on screen: ${JSON.stringify(item)}`);
    }
    assert.deepEqual([menu.clockwise.checked, menu.pressed], ["false", "false"], `${name}: taps turn anticlockwise at first`);
    await evaluate(`document.getElementById("clockwise-item").click()`);
    await evaluate(SETTLE);
    const toggled = await evaluate(ITEMS);
    assert.deepEqual([toggled.clockwise.checked, toggled.pressed], ["true", "true"], `${name}: the menu's Turn clockwise toggled the rail's`);
    await evaluate(`document.getElementById("clockwise").click()`);
    const canvas = await evaluate(CANVAS);
    const [corner, slot] = [await cellRect(14), await cellRect(225)];
    assert.ok(slot[0] >= corner[2], `${name}: Beside, the tray ${slot} right of the board ${corner}`);
    const cell = (corner[2] - corner[0]) * canvas.sx;
    const pitch = (slot[2] - slot[0]) * canvas.sx;
    assert.ok(pitch >= 28 - 0.5, `${name}: a tray slot of ${pitch} CSS px`);
    // The tray's shape: 6×4, or 8×3 when that keeps its slots bigger (spec §6 short landscape).
    const cols = await evaluate(`(() => {
      const top = window.__session.cell_rect(225)[1];
      let k = 1;
      while (k < 24 && window.__session.cell_rect(225 + k)[1] === top) k += 1;
      return k;
    })()`);
    assert.ok(cols === 6 || cols === 8, `${name}: a tray ${cols} slots across`);
    await evaluate(PASTE_WIN);
    await evaluate(SETTLE);
    await shoot(`${name}-won`);
    assert.ok(await evaluate("window.__session.won() && window.__session.next_open()"), `${name}: won, Next ▶ shows`);
    const won = await evaluate(FACTS);
    checkModernLandscapeBars(won);
    const next = await evaluate(`(() => {
      const session = window.__session;
      const canvas = document.getElementById("lightbox");
      const box = canvas.getBoundingClientRect();
      const k = canvas.width / box.width;
      const body = document.querySelector("#captions > .cap-body").getBoundingClientRect();
      const [left, right] = [Math.floor((body.left - box.left) * k), Math.ceil((body.right - box.left) * k)];
      const [top, bottom] = [Math.ceil((body.bottom - box.top) * k), Math.min(canvas.height, Math.floor((body.bottom + 50 - box.top) * k))];
      let hit = null;
      for (let y = top; y < bottom; y += 2) {
        for (let x = left; x < right; x += 2) {
          if (session.inert(x, y)) continue;
          hit = hit ? [Math.min(hit[0], x), Math.min(hit[1], y), Math.max(hit[2], x + 2), Math.max(hit[3], y + 2)] : [x, y, x + 2, y + 2];
        }
      }
      return hit;
    })()`);
    assert.ok(next, `${name}: Next ▶ under the won text`);
    const [x, y] = clientOf(canvas, centreOf(next));
    assert.ok(y <= facts.height && clientOf(canvas, [next[2], next[3]])[1] <= facts.height + 0.5, `${name}: Next ▶ ${next} on screen`);
    await tap(x, y);
    assert.equal(await slotNow(), 1, `${name}: a tap on Next ▶ at (${x.toFixed(1)}, ${y.toFixed(1)}) went to level 2`);
    landscapeBarsRows.push({ view: name, canvas: [facts.lightbox.width, facts.lightbox.height], cell, tray: `${cols}×${24 / cols}`, slot: pitch, next: [x, y] });
    console.log(`${name}: canvas ${facts.lightbox.width} × ${facts.lightbox.height}, Beside, board cell ${cell.toFixed(1)} px, tray ${cols}×${24 / cols} at ${pitch.toFixed(1)} px, Next ▶ tapped at (${x.toFixed(1)}, ${y.toFixed(1)})`);
  } finally {
    await forgetAll();
  }
}

// The hand scroll of a short phone with its bars (ruling 19), then a tray-to-board drag.
async function scrollsByHand(name, facts) {
  await hook();
  try {
    await evaluate("window.__calls.pointer_down = 0; window.__calls.pointer_up = 0");
    // Swipes up from the card: a point on screen inside the box its captions span (the header row
    // to the body's last line; the body itself may start below the screen), in the body's column
    // and inert, until the page is scrolled to its end.
    for (let swipe = 0; swipe < 4 && (await evaluate("scrollY")) < facts.overflowY - 1; swipe += 1) {
      const canvas = await evaluate(CANVAS);
      const captions = (await evaluate(FACTS)).captions.filter((caption) => caption.className !== "cap-credit");
      const body = captions.find((caption) => caption.className === "cap-body")?.rect;
      assert.ok(body, `${name}: the card's body`);
      const card = { top: Math.min(...captions.map(({ rect }) => rect.top)), bottom: body.bottom };
      const x = body.left + 10;
      const y = Math.min(card.bottom - 4, facts.height - 8);
      assert.ok(y >= card.top && y > facts.header.bottom && x < body.right, `${name}: the swipe starts on the card ${JSON.stringify(card)} at (${x}, ${y})`);
      assert.ok(await evaluate(`window.__session.inert(${backingOf(canvas, [x, y]).join(", ")})`), `${name}: the card is inert at (${x}, ${y})`);
      swipeStarts.push({ view: name, at: [x, y], card, body });
      const to = Math.max(facts.header.bottom + 10, y - facts.overflowY - 40);
      await touch("touchStart", x, y);
      for (let step = 1; step <= 10; step += 1) await touch("touchMove", x, y + ((to - y) * step) / 10);
      await touch("touchEnd");
      await evaluate(SETTLE);
    }
    const scrolled = await evaluate(FACTS);
    const presses = await evaluate("[window.__calls.pointer_down, window.__calls.pointer_up]");
    await shoot(`${name}-scrolled`);
    assertNear(scrolled.scrollY, facts.overflowY, "a swipe on the card scrolled the page to its end", 1);
    assert.deepEqual(presses, [0, 0], "the panning presses never reached the game");
    assert.ok(scrolled.lightbox.bottom <= scrolled.height + 1, `the card's end is in view: ${scrolled.lightbox.bottom}`);
    const lowest = Math.max(...scrolled.captions.map((caption) => caption.rect.bottom));
    assert.ok(lowest <= scrolled.height, `the card's last line is in view: ${lowest}`);
    // Back at the top, a touch drag carries the reflector in tray slot 0 (cell 225) onto board
    // cell (8, 7) (cell 113): the canvas keeps touch-action: none, so the page does not pan.
    await evaluate("window.scrollTo(0, 0)");
    await evaluate(SETTLE);
    let canvas = await evaluate(CANVAS);
    const trayY = clientOf(canvas, centreOf(await cellRect(225)))[1];
    if (trayY > facts.height - 30) await evaluate(`window.scrollBy(0, ${trayY - (facts.height - 30)})`);
    await evaluate(SETTLE);
    canvas = await evaluate(CANVAS);
    const [slotRect, cell] = [await cellRect(225), await cellRect(113)];
    const from = clientOf(canvas, centreOf(slotRect));
    const to = clientOf(canvas, centreOf(cell));
    assert.ok(to[1] > facts.header.bottom && from[1] < facts.height, `${name}: the tray slot ${from} and the cell ${to} on screen`);
    const before = [await lightboxHash(slotRect), await lightboxHash(cell)];
    const scrollY = await evaluate("scrollY");
    await touchDrag(from, to);
    const after = [await lightboxHash(slotRect), await lightboxHash(cell)];
    assert.ok(after[0] !== before[0] && after[1] !== before[1], `${name}: the piece moved from the tray onto the board`);
    assert.deepEqual(await evaluate("[window.__calls.pointer_down, window.__calls.pointer_up]"), [1, 1], "the drag's press reached the game (the press counter counts)");
    assert.equal(await evaluate("scrollY"), scrollY, "the drag did not scroll the page");
    // The open sheet (review M3): swipes up from its first chip, past the slop, pan the page
    // until its last row is on screen; no chip is chosen, and the sheet stays open.
    await evaluate("window.scrollTo(0, 0)");
    await evaluate(`document.getElementById("levels-open").click()`);
    await evaluate(SETTLE);
    await evaluate("window.__calls.pointer_up = 0");
    const lastChipBottom = async () => {
      const c = await evaluate(CANVAS);
      return clientOf(c, [0, (await evaluate("[...window.__session.chip_rect(49)]"))[3]])[1];
    };
    const start = { bottom: await lastChipBottom(), slot: await slotNow() };
    for (let swipe = 0; swipe < 4 && (await lastChipBottom()) > facts.height; swipe += 1) {
      const c = await evaluate(CANVAS);
      const [x, y] = clientOf(c, centreOf(await evaluate("[...window.__session.chip_rect(0)]")));
      const from = Math.min(y, facts.height - 8);
      await touch("touchStart", x, from);
      for (let step = 1; step <= 10; step += 1) await touch("touchMove", x, from - (step * Math.max(60, from - facts.header.bottom - 10)) / 10);
      await touch("touchEnd");
      await evaluate(SETTLE);
    }
    const end = { bottom: await lastChipBottom(), scrollY: await evaluate("scrollY"), slot: await slotNow(), open: await evaluate("window.__session.sheet_open()"), ups: await evaluate("window.__calls.pointer_up") };
    await shoot(`${name}-sheet-scrolled`);
    sheetScrolls.push({ view: name, start, end });
    assert.ok(start.bottom > facts.height, `${name}: the sheet's last row starts below the screen (${start.bottom})`);
    assert.ok(end.bottom <= facts.height + 0.5 && end.scrollY > 0, `${name}: swipes on the chips scrolled the last row into view: ${JSON.stringify(end)}`);
    assert.ok(end.open && end.slot === start.slot && end.ups === 0, `${name}: the swipes chose nothing: ${JSON.stringify(end)}`);
    await typeKey("Escape", "Escape");
    await evaluate(SETTLE);
  } finally {
    // Clean-up never throws over the assertion that brought it here.
    await evaluate(`document.querySelector("#actions-menu [data-command=reset]")?.click(); window.scrollTo(0, 0)`).catch(() => {});
    await forgetAll().catch(() => {});
  }
}

let failures = 0;
try {
  const origin = await serve();
  await connect();
  await send("Runtime.enable");
  await send("Page.enable");
  await send("Network.enable");
  // The first visit opens the help once; mark it seen so that the pictures show the game.
  await setMedia(withScheme());
  await load({ name: "first-visit", width: 1400, height: 1000, ratio: 1, touch: false, query: "&look=classic" }, origin);
  assert.deepEqual(pageErrors, [], "no errors on the page at the first visit");
  await evaluate(`localStorage.setItem("chromatron.helpSeen", "1")`);
  for (const scenario of SCENARIOS) {
    // Media features emulated from the load (reduced motion, a scheme), restored after the view.
    await setMedia(withScheme(scenario.media));
    await load(scenario, origin);
    const facts = await evaluate(FACTS);
    writeFileSync(join(outDir, `${scenario.name}.json`), `${JSON.stringify(facts, null, 2)}\n`);
    await shoot(scenario.name);
    try {
      assert.equal(facts.fatal, false, "the page started");
      assert.ok(!facts.message.includes("stopped working"), facts.message);
      assert.ok(CHECKS[scenario.name], `no checks for the ${scenario.name} view`);
      CHECKS[scenario.name](facts);
      await ACTIONS[scenario.name]?.(facts);
      assert.deepEqual(pageErrors, [], "no errors on the page");
      if (!(scenario.query ?? "").includes("analytics=debug")) {
        assert.equal(await evaluate("window.dataLayer === undefined"), true, "no analytics off the site");
      }
      console.log(`ok: ${scenario.name}`);
    } catch (error) {
      failures += 1;
      console.error(`FAIL ${scenario.name}: ${error.message}`);
    } finally {
      await setMedia(withScheme());
    }
  }
  // The `?perf` animation rows (spec §14), against ANIM_CAP.
  for (const row of perfRows.filter((row) => row.p90 !== undefined)) {
    console.log(`perf: ${row.name}: p90 ${row.p90.toFixed(1)} ms (worst ${row.worst.toFixed(1)} ms, ${row.count} frames; putImageData p90 ${row.putP90.toFixed(1)} ms), cap ${ANIM_CAP} ms`);
  }
  // analytics.js loaded, and nothing ever went to Google.
  const google = requested.filter((url) => /(^|\.)googletagmanager\.com$/.test(new URL(url).hostname));
  if (google.length > 0 || !requested.some((url) => url.includes("/analytics.js?v="))) {
    failures += 1;
    console.error(`FAIL network: analytics.js requested ${requested.filter((url) => url.includes("/analytics.js")).length} times, Google ${JSON.stringify(google)}`);
  }
  // What the checks measured, for the record (the ?perf rows, R8, Next ▶, the compact name).
  writeFileSync(join(outDir, "measured.json"), `${JSON.stringify({ perf: perfRows, r8, nextPill, compactNames, swipeStarts, landscapeBarsRows, sheetScrolls }, null, 2)}\n`);
  console.log(`web-shots: ${SCENARIOS.length - failures} of ${SCENARIOS.length} views as expected; pictures in ${outDir}`);
} catch (error) {
  failures += 1;
  console.error(`web-shots: ${error?.stack ?? error}`);
} finally {
  await finish(failures === 0 ? 0 : 1);
}
