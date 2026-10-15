// The headless-Chrome rig behind `just web-shots`: the static server for dist/web/, Chrome and its
// temporary profile (every way out stops both), the CDP session (send, evaluate, input), page
// loads and settles, the Session hook, the modern canvas's geometry, and hashing. web_shots.mjs
// holds the views and their checks.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createReadStream, existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

// FNV-1a over a byte array, as JS source, so Node and the page run the very same function.
export const FNV = "((bytes) => { let h = 2166136261; for (let i = 0; i < bytes.length; i += 1) h = Math.imul(h ^ bytes[i], 16777619); return h >>> 0; })";
export const fnv = new Function(`return ${FNV}`)();

// FNV-1a over rect [left, top, right, bottom] of a `width`-pixel-wide RGBA frame, as hex.
export function fnvRect(rgba, [left, top, right, bottom], width = 640) {
  const rows = new Uint8Array((right - left) * (bottom - top) * 4);
  for (let y = top; y < bottom; y += 1) rows.set(rgba.subarray((y * width + left) * 4, (y * width + right) * 4), (y - top) * (right - left) * 4);
  return fnv(rows).toString(16).padStart(8, "0");
}

// The page's ?perf frames that began at or after `since` (performance.now() in the page).
export const perfFrames = (since) => evaluate(`window.__perf.frames.filter((f) => f.wall >= ${since})`);

// Where the rig serves the page from (dist/web/), where `shoot` writes, and the Chrome executable:
// web_shots.mjs checks its arguments, then sets them here before `serve` and `connect`.
let webDir;
let outDir;
let chromePath;
export function configure(paths) {
  ({ webDir, outDir, chrome: chromePath } = paths);
}

const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm" };

let chrome;
let server;
let profile;
let socket;
let finishing;
// Every way out comes here: stop Chrome (SIGTERM, then SIGKILL after 3 s), wait until it has
// exited, remove its profile, and exit with `code`.
export function finish(code) {
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
// The script never hangs: after 240 s it gives up the same way.
setTimeout(() => {
  console.error("web-shots: timed out after 240 s");
  finish(1);
}, 240_000).unref();

export function serve() {
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
export async function connect() {
  profile = mkdtempSync(join(tmpdir(), "chromatron-shots-"));
  chrome = spawn(
    chromePath,
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

// The Chrome DevTools Protocol: calls, the page's errors, and every URL it requested.
let lastId = 0;
const calls = new Map();
export const pageErrors = [];
export const requested = [];
export function receive(event) {
  const message = JSON.parse(event.data);
  if (message.method === "Network.requestWillBeSent") {
    requested.push(message.params.request.url);
  } else if (message.method === "Fetch.requestPaused") {
    // Fetch is enabled only to hold a script back (web-shots' first-paint view): every request
    // it pauses fails, as a blocked one.
    send("Fetch.failRequest", { requestId: message.params.requestId, errorReason: "BlockedByClient" });
  } else if (message.id !== undefined) {
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
export function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    lastId += 1;
    calls.set(lastId, { method, resolve, reject });
    socket.send(JSON.stringify({ id: lastId, method, params }));
  });
}
export async function evaluate(expression) {
  const { result, exceptionDetails } = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  return result.value;
}

// Four animation frames: the page has laid out (a layout waits for the next frame) and painted.
export const FRAMES = `new Promise((resolve) => {
  let frames = 4;
  const next = () => (--frames === 0 ? resolve(true) : requestAnimationFrame(next));
  requestAnimationFrame(next);
})`;

// Resolves once the page has no frame pending (the hooked modern Session's next_wake is −1; `load`
// hooks every modern page; a classic page, never hooked, is idle), then after four more animation
// frames (FRAMES: a layout waits for the next frame): the modern canvas may be animating after an
// input. Rejects if the page is still waking after 3 s (every cue ends within SETTLE_MS, 2.5 s).
export const SETTLE = `new Promise((resolve, reject) => {
  const start = performance.now();
  const idle = () => !window.__session || window.__session.next_wake(performance.now()) < 0;
  const wait = () => {
    if (idle()) ${FRAMES}.then(resolve);
    else if (performance.now() - start > 3000) reject(new Error("SETTLE: the page never went idle"));
    else setTimeout(wait, 20);
  };
  wait();
})`;

// Emulates CSS media features (e.g. prefers-reduced-motion), live; `[]` restores the defaults.
export const setMedia = (features) => send("Emulation.setEmulatedMedia", { features });

// SETTLE, then frames until the session's cues have run out (next_wake −1; at most 3 s), and
// SETTLE again: a picture after a cue (the level sheet's fade) is its settled one. Resolves to
// whether the page is idle (next_wake −1) at the end, for the caller to assert.
export const IDLE = `(async () => {
  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
  const idle = () => window.__session.next_wake(performance.now()) < 0;
  for (let k = 0; k < 4; k += 1) await frame();
  for (let k = 0; k < 180 && !idle(); k += 1) await frame();
  for (let k = 0; k < 4; k += 1) await frame();
  return idle();
})()`;

export async function load({ name, width, height, ratio, touch, query = "" }, origin) {
  await evaluate("window.shotStale = true");
  pageErrors.length = 0;
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: ratio, mobile: touch });
  await send("Emulation.setTouchEmulationEnabled", touch ? { enabled: true, maxTouchPoints: 5 } : { enabled: false });
  await send("Page.navigate", { url: `${origin}?shot=${name}${query}#1` });
  // Started: main.js has laid the page out (its first layout sets data-layout, after the Session
  // exists), or says it can't start. (data-look is no signal: index.html sets it before the wasm.)
  const started = `!window.shotStale && (document.documentElement.dataset.layout !== undefined || !document.getElementById("fatal").hidden)`;
  for (let tries = 0; !(await evaluate(started).catch(() => false)); tries += 1) {
    assert.ok(tries < 100, `${name}: the page did not start`);
    await delay(100);
  }
  // A modern page is hooked at once, so that every SETTLE on it waits until it is idle.
  if (await evaluate(`document.documentElement.dataset.look === "modern" && document.getElementById("fatal").hidden`)) await hook();
  await evaluate(SETTLE);
}

// One touch-point event at viewport CSS pixel (x, y).
export function touch(type, x = 0, y = 0) {
  return send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x, y }] });
}

// A real right mouse press and release at viewport point (x, y); whether the page default-prevented
// the contextmenu event that followed (recorded by a bubbling listener on the document).
export async function rightClick(x, y) {
  await evaluate(`window.__menus = []; document.addEventListener("contextmenu", (event) => window.__menus.push(event.defaultPrevented), { once: true })`);
  await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "right", buttons: 2, clickCount: 1 });
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "right", clickCount: 1 });
  await evaluate(SETTLE);
  return evaluate(`window.__menus`);
}

export async function shoot(name) {
  const { data } = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(outDir, `${name}.png`), Buffer.from(data, "base64"));
}

// A key typed on the page (CDP), as the browser reports it.
export async function typeKey(key, code, text) {
  await send("Input.dispatchKeyEvent", { type: "keyDown", key, code, ...(text ? { text } : {}) });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key, code });
}

// Modern views reach the page's own Session through its wasm-bindgen module (the same URL is the
// same module instance): every Session method is wrapped to keep the instance (window.__session)
// and count the presses that reach it, its paints (render_at) and the resizes (each with the
// session's next_wake just before it: ≥ 0 when it landed mid-cue). Paint times come from the
// page's own `?perf` log (window.__perf). A test hook only: the page itself is unchanged.
export const HOOK = `(async () => {
  if (window.__calls) return window.__session !== undefined;
  const url = performance.getEntriesByType("resource").map((entry) => entry.name).find((name) => /\\/chromatron_web\\.js\\?v=/.test(name));
  const { Session } = await import(url);
  const calls = (window.__calls = { pointer_down: 0, pointer_up: 0, render_at: 0, resizes: [] });
  const proto = Session.prototype;
  for (const name of Object.getOwnPropertyNames(proto)) {
    const original = Object.getOwnPropertyDescriptor(proto, name).value;
    if (typeof original !== "function" || name === "constructor") continue;
    proto[name] = function (...args) {
      window.__session = this;
      if (name === "pointer_down" || name === "pointer_up" || name === "render_at") calls[name] += 1;
      if (name === "resize") calls.resizes.push({ at: performance.now(), width: args[0], height: args[1], wake: proto.next_wake.call(this, performance.now()) });
      return original.apply(this, args);
    };
  }
  // Any Session call keeps the instance: a key the game does not read.
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "F13", bubbles: true }));
  return window.__session !== undefined;
})()`;
export async function hook() {
  assert.ok(await evaluate(HOOK), "the page's Session is reachable");
}

// The modern canvas: its client box, and CSS px per backing px.
export const CANVAS = `(() => {
  const canvas = document.getElementById("lightbox");
  const r = canvas.getBoundingClientRect();
  return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, sx: r.width / canvas.width, sy: r.height / canvas.height, width: canvas.width, height: canvas.height };
})()`;
// Backing point (x, y) of `canvas` (CANVAS) in client CSS px, and back.
export const clientOf = (canvas, [x, y]) => [canvas.left + x * canvas.sx, canvas.top + y * canvas.sy];
export const backingOf = (canvas, [x, y]) => [(x - canvas.left) / canvas.sx, (y - canvas.top) / canvas.sy];
export const centreOf = ([left, top, right, bottom]) => [(left + right) / 2, (top + bottom) / 2];
// FNV-1a over backing rect [left, top, right, bottom] of the modern canvas (all of it by default).
export function lightboxHash(rect) {
  return evaluate(`(() => {
    const canvas = document.getElementById("lightbox");
    const [l, t, r, b] = ${JSON.stringify(rect ?? null)} ?? [0, 0, canvas.width, canvas.height];
    const data = canvas.getContext("2d").getImageData(l, t, r - l, b - t).data;
    return ${FNV}(data);
  })()`);
}

// Sets the viewport (CDP device metrics) and waits for the page's debounced session.resize, then
// for its paint (hooked views only).
export async function resizeViewport(metrics) {
  await evaluate("window.__calls.resizes.length = 0");
  await send("Emulation.setDeviceMetricsOverride", metrics);
  for (let tries = 0; (await evaluate("window.__calls.resizes.length")) === 0; tries += 1) {
    assert.ok(tries < 50, `no session.resize within 1 s at ${metrics.width} × ${metrics.height}`);
    await delay(20);
  }
  await evaluate(SETTLE);
}
