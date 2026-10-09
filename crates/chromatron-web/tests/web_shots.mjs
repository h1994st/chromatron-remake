// `just web-shots [dir]`: serves dist/web/ and photographs it in headless Google Chrome in five views:
// a desktop (1400 × 1000 at 1×); a phone in landscape (844 × 390) and in portrait, both full height
// (390 × 844) and with Safari's bars shown (390 × 664), at devicePixelRatio 3 with touch emulation,
// so `(pointer: coarse)` matches; and a mouse at 390 × 844 with `?layout=touch`. For each view it
// writes <name>.png and <name>.json (the page's layout facts) into <dir> and checks them.
// A local development aid, not run in CI: it needs Google Chrome ($CHROME overrides the path). The
// pictures show the games' sprites: inside the repo they may only go under re/out/, dist/ or
// target/, which git ignores. Every way out (the end, an error, Ctrl-C, SIGTERM, the 90 s limit)
// stops Chrome and removes its temporary profile.
// Usage: node web_shots.mjs <dist/web> <dir>

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createReadStream, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, isAbsolute, join, normalize, relative, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

const [webDir, outDir] = process.argv.slice(2);
assert.ok(webDir && outDir, "usage: node web_shots.mjs <dist/web> <dir>");
const repo = fileURLToPath(new URL("../../..", import.meta.url));
const inRepo = relative(repo, resolve(outDir));
assert.ok(
  inRepo.startsWith("..") || isAbsolute(inRepo) || /^(re\/out|dist|target)(\/|$)/.test(inRepo),
  `${outDir} is in the repo but not git-ignored; use re/out/web-shots/… or a directory outside the repo`,
);
const CHROME = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
assert.ok(existsSync(CHROME), `no Google Chrome at ${CHROME}; set CHROME to its executable`);
mkdirSync(outDir, { recursive: true });
const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm" };

let chrome;
let server;
let profile;
let socket;
let finishing;
// Every way out comes here: stop Chrome (SIGTERM, then SIGKILL after 3 s), wait until it has
// exited, remove its profile, and exit with `code`.
function finish(code) {
  finishing ??= (async () => {
    server?.close();
    if (chrome && chrome.exitCode === null && chrome.signalCode === null) {
      const exited = new Promise((resolve) => chrome.once("exit", resolve));
      chrome.kill();
      const force = setTimeout(() => chrome.kill("SIGKILL"), 3000);
      await exited;
      clearTimeout(force);
    }
    if (profile) rmSync(profile, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
    process.exit(code);
  })();
  return finishing;
}
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) process.on(signal, () => finish(1));
for (const event of ["uncaughtException", "unhandledRejection"]) {
  process.on(event, (error) => {
    console.error(`web-shots: ${error?.stack ?? error}`);
    finish(1);
  });
}
// The script never hangs: after 90 s it gives up the same way.
setTimeout(() => {
  console.error("web-shots: timed out after 90 s");
  finish(1);
}, 90_000).unref();

// FNV-1a over rect [left, top, right, bottom] of a 640-pixel-wide RGBA frame; the page runs the same
// function over its canvases (FACTS), so equal hashes mean equal pixels.
function fnvRect(rgba, [left, top, right, bottom], width = 640) {
  let hash = 2166136261;
  for (let y = top; y < bottom; y += 1) {
    for (let i = (y * width + left) * 4; i < (y * width + right) * 4; i += 1) hash = Math.imul(hash ^ rgba[i], 16777619);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

// The engine's own first frame of Chromatron 1, and lumen's regions, from the built wasm.
const glue = await import(pathToFileURL(join(webDir, "chromatron_web.js")).href);
const wasm = glue.initSync({ module: readFileSync(join(webDir, "chromatron_web_bg.wasm")) });
const reference = new glue.Session();
reference.select(1, undefined, undefined);
reference.render();
const BOARD = [...reference.board_rect()];
const TOOLBOX = [...reference.toolbox_rect()];
const CLICK_NEXT = reference.click_next();
assert.ok(CLICK_NEXT.length > 0, "the game has a click_next text");
// What the touch layouts say instead (web/main.js TOUCH_NEXT).
const TOUCH_PORTRAIT = "Tap Next level or pick a level from Levels.";
const TOUCH_LANDSCAPE = "Tap a level number or ▶ for the next level.";
const rgba = new Uint8Array(wasm.memory.buffer, reference.rgba_ptr(), 640 * 480 * 4).slice();
const { LIFT, TEXT_ROOM, TOOLBOX_MIN, fillScale, portraitScales, rectOf } = await import(pathToFileURL(join(webDir, "touch_layout.js")).href);
const expected = { frame: fnvRect(rgba, [0, 0, 640, 480]), board: fnvRect(rgba, BOARD), toolbox: fnvRect(rgba, TOOLBOX) };
// Where today's page (Task 4's run, Chrome 154 on macOS) shows the desktop game at 1400 × 1000.
const DESKTOP_GAME = { left: 380, top: 255, width: 640, height: 480 };

function serve() {
  server = createServer((request, response) => {
    const path = normalize(decodeURIComponent(new URL(request.url, "http://localhost").pathname)).replace(/^[/\\]+/, "") || "index.html";
    const file = join(webDir, path);
    if (path.startsWith("..") || !existsSync(file) || !statSync(file).isFile()) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
    createReadStream(file).pipe(response);
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(`http://127.0.0.1:${server.address().port}/`)));
}

// Starts headless Chrome with a throw-away profile and connects to its page over the DevTools protocol.
async function connect() {
  profile = mkdtempSync(join(tmpdir(), "chromatron-shots-"));
  chrome = spawn(
    CHROME,
    ["--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"],
    { stdio: "ignore" },
  );
  const portFile = join(profile, "DevToolsActivePort");
  let port = "";
  while (!port) {
    if (existsSync(portFile)) port = readFileSync(portFile, "utf8").split("\n")[0].trim();
    if (!port) await delay(100);
  }
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(targets.find((target) => target.type === "page").webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  socket.addEventListener("message", receive);
}

// The Chrome DevTools Protocol: calls, and the page's errors.
let lastId = 0;
const calls = new Map();
const pageErrors = [];
function receive(event) {
  const message = JSON.parse(event.data);
  if (message.id !== undefined) {
    const call = calls.get(message.id);
    calls.delete(message.id);
    if (message.error) call.reject(new Error(`${call.method}: ${message.error.message}`));
    else call.resolve(message.result);
  } else if (message.method === "Runtime.exceptionThrown") {
    const details = message.params.exceptionDetails;
    pageErrors.push(details.exception?.description ?? details.text);
  } else if (message.method === "Runtime.consoleAPICalled" && message.params.type === "error") {
    pageErrors.push(message.params.args.map((arg) => arg.value ?? arg.description).join(" "));
  }
}
function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    lastId += 1;
    calls.set(lastId, { method, resolve, reject });
    socket.send(JSON.stringify({ id: lastId, method, params }));
  });
}
async function evaluate(expression) {
  const { result, exceptionDetails } = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  return result.value;
}

// Level 1 of Chromatron 1 solved by pasting its stored solution (tests/solutions/game1/level-01.txt).
const PASTE_WIN = `(() => {
  const clipboardData = new DataTransfer();
  clipboardData.setData("text/plain", "1-01-hI");
  document.dispatchEvent(new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true }));
})()`;

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

// Four animation frames: the page has laid out (a layout waits for the next frame) and painted.
const SETTLE = `new Promise((resolve) => {
  let frames = 4;
  const next = () => (--frames === 0 ? resolve(true) : requestAnimationFrame(next));
  requestAnimationFrame(next);
})`;

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
    let hash = 2166136261;
    for (let i = 0; i < data.length; i += 1) hash = Math.imul(hash ^ data[i], 16777619);
    return (hash >>> 0).toString(16).padStart(8, "0");
  };
  const tools = [...document.querySelectorAll(".toolbar > *")].filter(
    (element) => element.getClientRects().length > 0 && !element.classList.contains("spacer"),
  );
  const stage = document.getElementById("stage");
  const style = getComputedStyle(stage);
  const px = (value) => parseFloat(value) || 0;
  return {
    layout: root.dataset.layout ?? "",
    width: innerWidth,
    height: innerHeight,
    clientWidth: root.clientWidth,
    ratio: devicePixelRatio,
    overflowX: root.scrollWidth - root.clientWidth,
    overflowY: root.scrollHeight - root.clientHeight,
    toolbarRows: new Set(tools.map((element) => Math.round(element.getBoundingClientRect().top))).size,
    toolbarRight: Math.max(0, ...tools.map((element) => element.getBoundingClientRect().right)),
    toolbarColumns: new Set(tools.map((element) => Math.round(element.getBoundingClientRect().left))).size,
    toolbarBottom: Math.max(0, ...tools.map((element) => element.getBoundingClientRect().bottom)),
    tools: tools.map((element) => element.id || element.dataset.command || element.localName),
    stageRoom: stage.clientHeight - px(style.paddingTop) - px(style.paddingBottom) - 2 * px(style.rowGap),
    header: box("header"),
    game: box("#game"),
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
  };
})()`;

async function load({ name, width, height, ratio, touch, query = "" }, origin) {
  await evaluate("window.shotStale = true");
  pageErrors.length = 0;
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: ratio, mobile: touch });
  await send("Emulation.setTouchEmulationEnabled", touch ? { enabled: true, maxTouchPoints: 5 } : { enabled: false });
  await send("Page.navigate", { url: `${origin}?shot=${name}${query}#1` });
  const started = `!window.shotStale && (document.title !== "Chromatron" || !document.getElementById("fatal").hidden)`;
  for (let tries = 0; !(await evaluate(started).catch(() => false)); tries += 1) {
    assert.ok(tries < 100, `${name}: the page did not start`);
    await delay(100);
  }
  await evaluate(SETTLE);
}

// One touch-point event at viewport CSS pixel (x, y).
function touch(type, x = 0, y = 0) {
  return send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x, y }] });
}

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
    let h = 2166136261;
    for (let i = 0; i < data.length; i += 1) h = Math.imul(h ^ data[i], 16777619);
    return h >>> 0;
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

async function shoot(name) {
  const { data } = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(outDir, `${name}.png`), Buffer.from(data, "base64"));
}

const SCENARIOS = [
  { name: "desktop", width: 1400, height: 1000, ratio: 1, touch: false },
  { name: "landscape", width: 844, height: 390, ratio: 3, touch: true },
  { name: "forced-touch", width: 390, height: 844, ratio: 1, touch: false, query: "&layout=touch" },
  // iOS Safari's usable portrait height on a 390 × 844 phone, with its bars shown.
  { name: "portrait-bars", width: 390, height: 664, ratio: 3, touch: true },
  // The full-height phone portrait.
  { name: "portrait", width: 390, height: 844, ratio: 3, touch: true },
];

// Whether `actual` is `wanted` to within `tolerance` CSS px.
function assertNear(actual, wanted, what, tolerance = 0.5) {
  assert.ok(Math.abs(actual - wanted) <= tolerance, `${what}: ${actual}, expected ${wanted}`);
}

// A portrait page: one screen that never scrolls, with the board, the toolbox and the texts in
// that order, sized as touch_layout.js's portraitScales says for the room the stage leaves.
function checkPortrait(facts) {
  assert.equal(facts.layout, "portrait");
  assert.ok(facts.overflowX <= 0 && facts.overflowY <= 0, `the page does not scroll: ${facts.overflowX}, ${facts.overflowY}`);
  assert.equal(facts.toolbarRows, 1, "one toolbar row");
  assert.ok(facts.toolbarRight <= facts.clientWidth, "the toolbar fits the width");
  assert.deepEqual(facts.tools, ["game-select", "previous", "next", "actions", "levels-open", "clockwise", "fullscreen", "help-open"]);
  assert.equal(facts.game, null, "the whole frame is hidden");
  const scales = portraitScales({
    width: facts.clientWidth,
    height: facts.stageRoom - TEXT_ROOM,
    ratio: facts.ratio,
    board: rectOf(BOARD),
    toolbox: rectOf(TOOLBOX),
  });
  assertNear(facts.board.width, (BOARD[2] - BOARD[0]) * scales.board, "the board's scale");
  assertNear(facts.toolbox.width, (TOOLBOX[2] - TOOLBOX[0]) * scales.toolbox, "the toolbox's scale");
  assert.ok(facts.board.bottom <= facts.toolbox.top && facts.toolbox.bottom <= facts.texts.top, "board, toolbox, then texts");
  assert.ok(facts.texts.bottom <= facts.height + 0.5 && facts.texts.height >= TEXT_ROOM - 0.5, "the texts fit below");
  assert.ok(facts.textCount >= 1, "the texts are HTML");
  assert.equal(facts.boardPixels, expected.board, "the board part shows the engine's board");
  assert.equal(facts.toolboxPixels, expected.toolbox, "the toolbox part shows the engine's toolbox");
}

// What each view must show.
const CHECKS = {
  desktop(facts) {
    assert.equal(facts.layout, "desktop");
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
    assert.ok(facts.board.width >= 0.85 * facts.clientWidth, `the board keeps most of the width: ${facts.board.width}`);
    assert.ok(facts.toolbox.width / (TOOLBOX[2] - TOOLBOX[0]) >= TOOLBOX_MIN - 0.01, "the toolbox slots stay finger-sized");
  },
  portrait(facts) {
    checkPortrait(facts);
    assertNear(facts.board.width, facts.clientWidth, "the board fills the width", 1);
  },
};

// Steps after a view's first picture, which may take more pictures.
const ACTIONS = {
  async portrait(facts) {
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
    // Tapping it is the toolbar's Next: level 2, and the button is gone.
    await touch("touchStart", won.button.x, won.button.y);
    await touch("touchEnd");
    await evaluate(SETTLE);
    const onLevel2 = await evaluate(WIN_FACTS);
    assert.equal(onLevel2.button, null, "the button is hidden on the next level");
    assert.ok(onLevel2.listed.length > 0 && !onLevel2.listed.some((line) => /win/i.test(line)), `level 2 is not won: ${JSON.stringify(onLevel2.listed)}`);
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
    const typeKey = async (key, code, text) => {
      await send("Input.dispatchKeyEvent", { type: "keyDown", key, code, ...(text ? { text } : {}) });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key, code });
    };
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
    // After a win the game's own click_next text stays, and there is no button.
    await evaluate(`document.getElementById("game-select").blur()`);
    await evaluate(PASTE_WIN);
    await evaluate(SETTLE);
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
};

let failures = 0;
try {
  const origin = await serve();
  await connect();
  await send("Runtime.enable");
  await send("Page.enable");
  // The first visit opens the help once; mark it seen so that the pictures show the game.
  await load({ name: "first-visit", width: 1400, height: 1000, ratio: 1, touch: false }, origin);
  assert.deepEqual(pageErrors, [], "no errors on the page at the first visit");
  await evaluate(`localStorage.setItem("chromatron.helpSeen", "1")`);
  for (const scenario of SCENARIOS) {
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
      console.log(`ok: ${scenario.name}`);
    } catch (error) {
      failures += 1;
      console.error(`FAIL ${scenario.name}: ${error.message}`);
    }
  }
  console.log(`web-shots: ${SCENARIOS.length - failures} of ${SCENARIOS.length} views as expected; pictures in ${outDir}`);
} catch (error) {
  failures += 1;
  console.error(`web-shots: ${error?.stack ?? error}`);
} finally {
  await finish(failures === 0 ? 0 : 1);
}
