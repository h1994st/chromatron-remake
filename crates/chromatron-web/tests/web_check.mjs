// `just web-check`: loads the built site's wasm in Node and plays every game through the same
// Session API the page uses, then checks the size budgets and that every page URL is relative.
// Usage: node web_check.mjs <dist/web> <tests/solutions>

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";

const [webDir, solutionsDir] = process.argv.slice(2);
assert.ok(webDir && solutionsDir, "usage: node web_check.mjs <dist/web> <tests/solutions>");

const MAX_WASM_BYTES = 600_000;
const MAX_PAGE_GZIP_BYTES = 150_000;
const RGBA_BYTES = 640 * 480 * 4;
const BACKGROUND = [0xa4, 0xa4, 0xa4, 0xff];
// Each game's cheat key as the browser reports it, and whether Control is held.
const CHEATS = { 1: ["L", false], 2: ["z", false], 3: ["^", false], 4: ["o", true] };

// The only absolute URLs the page may carry: its links to the originals and to the source.
const EXTERNAL_HOSTS = ["silverspaceship.com", "github.com"];

// The files, their sizes and their URLs.
const files = readdirSync(webDir).sort();
for (const required of ["index.html", "style.css", "main.js", "chromatron_web.js", "chromatron_web_bg.wasm"]) {
  assert.ok(files.includes(required), `${required} is in ${webDir}`);
}
const wasmBytes = readFileSync(join(webDir, "chromatron_web_bg.wasm"));
assert.ok(wasmBytes.length < MAX_WASM_BYTES, `wasm is ${wasmBytes.length} bytes, over ${MAX_WASM_BYTES}`);
let gzipped = 0;
for (const file of files) gzipped += gzipSync(readFileSync(join(webDir, file)), { level: 9 }).length;
assert.ok(gzipped < MAX_PAGE_GZIP_BYTES, `the page is ${gzipped} bytes gzip, over ${MAX_PAGE_GZIP_BYTES}`);
const html = readFileSync(join(webDir, "index.html"), "utf8");
for (const [, url] of html.matchAll(/\b(?:src|href)="([^"]*)"/g)) {
  if (/^(?:data:|#)/.test(url)) continue;
  if (url.startsWith("https://")) {
    assert.ok(EXTERNAL_HOSTS.includes(new URL(url).hostname), `${url} in index.html is a known external link`);
    continue;
  }
  assert.ok(!url.startsWith("/") && !url.includes("://"), `${url} in index.html is relative`);
}
// `just web` stamps every page URL with a hash of the shipped files (as templates), so one deploy's files load together.
const main = readFileSync(join(webDir, "main.js"), "utf8");
const build = /const BUILD = "([0-9a-f]{12})";/.exec(main)?.[1];
assert.ok(build, "main.js carries a 12-hex build stamp");
const unstamp = (text) => text.replaceAll(build, "__BUILD__");
const expected = createHash("sha256")
  .update(wasmBytes)
  .update(readFileSync(join(webDir, "chromatron_web.js")))
  .update(unstamp(main))
  .update(readFileSync(join(webDir, "style.css")))
  .update(unstamp(html))
  .digest("hex")
  .slice(0, 12);
assert.equal(build, expected, "the stamp hashes every shipped file");
assert.ok(!html.includes("__BUILD__") && !main.includes("__BUILD__"), "the build hash is stamped");
assert.ok(html.includes(`main.js?v=${build}`) && html.includes(`style.css?v=${build}`), "index.html");
assert.ok(main.includes(`const BUILD = "${build}";`), "main.js carries the build hash");
assert.match(main, /import\(`\.\/chromatron_web\.js\?v=\$\{BUILD\}`\)/);
assert.match(main, /new URL\(`chromatron_web_bg\.wasm\?v=\$\{BUILD\}`, import\.meta\.url\)/);
execFileSync(process.execPath, ["--check", join(webDir, "main.js")], { stdio: "inherit" });

// Play.
const glue = await import(pathToFileURL(join(webDir, "chromatron_web.js")).href);
const wasm = glue.initSync({ module: wasmBytes });
const { Flag, Session, display_scale: displayScale } = glue;
const has = (flags, flag) => (flags & flag) !== 0;
assert.equal(displayScale(1280, 960), 2);
assert.equal(displayScale(320, 480), 0.5);

let solved = 0;
for (const game of [1, 2, 3, 4]) {
  const session = new Session();
  assert.ok(has(session.select(game, undefined, undefined), Flag.Redraw));
  assert.equal(session.render(), 1);
  const rgba = new Uint8Array(wasm.memory.buffer, session.rgba_ptr(), RGBA_BYTES);
  assert.deepEqual([...rgba.subarray(0, 4)], BACKGROUND, `game ${game}: background at (0, 0)`);
  assert.deepEqual([...new Int32Array(wasm.memory.buffer, session.dirty_ptr(), 4)], [0, 0, 640, 480]);
  assert.ok(session.text_changed() && session.text_count() > 0, `game ${game}: text boxes`);
  assert.match(session.title(), new RegExp(`^Chromatron ${game} \\(v`));
  assert.equal(session.key("/", false, false, false) & Flag.Consumed, 0, "an unread key stays the browser's");
  assert.equal(session.dragging(), false);

  let gameSolved = 0;
  const dir = join(solutionsDir, `game${game}`);
  for (const file of readdirSync(dir).filter((f) => /^level-\d\d\.txt$/.test(f)).sort()) {
    const level = Number(file.slice(6, 8));
    const solution = readFileSync(join(dir, file), "utf8").trim();
    const [cheat, ctrl] = CHEATS[game];
    while (session.slot() < level - 1) {
      const slot = session.slot();
      assert.ok(has(session.key(cheat, ctrl, false, false), Flag.Consumed), `game ${game}: cheat`);
      session.key("=", false, false, false);
      assert.equal(session.slot(), slot + 1, `game ${game}: = after the cheat`);
    }
    assert.ok(has(session.paste(solution), Flag.Redraw), `${solution} pastes`);
    assert.ok(session.won(), `${solution} wins`);
    assert.ok(session.render() > 0, `${solution}: a frame renders`);
    assert.ok(session.copy().startsWith(`${game}-${String(level).padStart(2, "0")}-`));

    const resumed = new Session();
    const flags = resumed.select(game, session.save_text(), level - 1);
    assert.ok(!has(flags, Flag.CorruptSave), `${solution}: the save reloads`);
    assert.equal(resumed.slot(), level - 1);
    assert.ok(resumed.won(), `${solution}: the saved board comes back`);
    assert.ok(resumed.forget(game) && resumed.game() === 0, `${solution}: forget`);
    resumed.free();
    solved += 1;
    gameSolved += 1;
  }
  assert.ok(gameSolved >= 1, `game ${game}: no solution fixture played in ${dir}`);
  session.free();
}
console.log(`wasm ${wasmBytes.length} bytes, page ${gzipped} bytes gzip`);
console.log(`ok: ${solved} solutions won through the browser build`);
