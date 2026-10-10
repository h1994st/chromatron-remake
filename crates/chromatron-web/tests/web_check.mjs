// `just web-check`: loads the built site's wasm in Node and plays every game through the same
// Session API the page uses, then checks the size budgets and that every page URL is relative.
// It also checks the social preview tags, the JSON-LD and the two PNGs that go with them.
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
// The only absolute URL in the page's scripts: Google's gtag.js, which analytics.js injects on the site.
const SCRIPT_URLS = { "analytics.js": ["https://www.googletagmanager.com/gtag/js"] };
// The site's own address, which only the canonical link and the OG and Twitter tags spell out.
const SITE = "https://shengtuo.me/chromatron-remake/";
// The OG and Twitter tags the page must carry, with their exact values where they matter.
const METAS = {
  "og:type": "website",
  "og:site_name": undefined,
  "og:title": undefined,
  "og:description": undefined,
  "og:url": SITE,
  "og:image": `${SITE}og.png`,
  "og:image:width": "1200",
  "og:image:height": "630",
  "og:image:alt": undefined,
  "og:locale": "en_US",
  "twitter:card": "summary_large_image",
  "twitter:title": undefined,
  "twitter:description": undefined,
  "twitter:image": `${SITE}og.png`,
  "twitter:image:alt": undefined,
};
// The social preview image and the touch icon (made by `just social-images`): their sizes and byte caps.
const IMAGES = {
  "og.png": { width: 1200, height: 630, maxBytes: 300_000 },
  "apple-touch-icon.png": { width: 180, height: 180, maxBytes: 50_000 },
};

// The files, their sizes and their URLs.
const files = readdirSync(webDir).sort();
for (const required of ["index.html", "style.css", "main.js", "touch_layout.js", "analytics.js", "chromatron_web.js", "chromatron_web_bg.wasm", ...Object.keys(IMAGES)]) {
  assert.ok(files.includes(required), `${required} is in ${webDir}`);
}
const wasmBytes = readFileSync(join(webDir, "chromatron_web_bg.wasm"));
assert.ok(wasmBytes.length < MAX_WASM_BYTES, `wasm is ${wasmBytes.length} bytes, over ${MAX_WASM_BYTES}`);
// The page never loads the social preview image or the touch icon, so they have their own caps.
let gzipped = 0;
for (const file of files) {
  if (file in IMAGES) continue;
  gzipped += gzipSync(readFileSync(join(webDir, file)), { level: 9 }).length;
}
assert.ok(gzipped < MAX_PAGE_GZIP_BYTES, `the page is ${gzipped} bytes gzip, over ${MAX_PAGE_GZIP_BYTES}`);
for (const [file, { width, height, maxBytes }] of Object.entries(IMAGES)) {
  const png = readFileSync(join(webDir, file));
  assert.ok(png.length <= maxBytes, `${file} is ${png.length} bytes, over ${maxBytes}`);
  // The PNG signature, then the IHDR chunk: length, type, width, height.
  assert.ok(png.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])), `${file} is a PNG`);
  assert.equal(png.toString("latin1", 12, 16), "IHDR", `${file} starts with IHDR`);
  assert.deepEqual([png.readUInt32BE(16), png.readUInt32BE(20)], [width, height], `${file} is ${width} × ${height}`);
}
const html = readFileSync(join(webDir, "index.html"), "utf8");
const attributes = (tag) => Object.fromEntries([...tag.matchAll(/([\w:-]+)="([^"]*)"/g)].map(([, name, value]) => [name, value]));
const metas = {};
let canonical;
for (const [tag, element] of html.matchAll(/<([a-z]+)\b[^>]*>/g)) {
  const attrs = attributes(tag);
  if (element === "meta" && (attrs.property || attrs.name)) metas[attrs.property || attrs.name] = attrs.content;
  if (element === "link" && attrs.rel === "canonical") canonical = attrs.href;
  // Crawlers need the site's own absolute URLs, but only in the canonical link and the OG and Twitter tags.
  if (element === "meta" && /^[a-z][a-z\d+.-]*:\/\//i.test(attrs.content ?? "")) {
    const key = attrs.property || attrs.name || "";
    assert.ok(/^(?:og|twitter):/.test(key) && attrs.content.startsWith(SITE), `${key} is an OG or Twitter tag with a URL on ${SITE}`);
  }
  for (const url of [attrs.src, attrs.href]) {
    if (url === undefined || /^(?:data:|#)/.test(url)) continue;
    if (url.startsWith(SITE) && element === "link" && attrs.rel === "canonical") continue;
    if (url.startsWith("https://")) {
      assert.ok(EXTERNAL_HOSTS.includes(new URL(url).hostname), `${url} in index.html is a known external link`);
      continue;
    }
    assert.ok(!url.startsWith("/") && !url.includes("://"), `${url} in index.html is relative`);
  }
}
// The social preview: absolute and unstamped, for crawlers.
assert.equal(canonical, SITE, "the canonical link");
for (const [key, value] of Object.entries(METAS)) {
  assert.ok(metas[key], `index.html has ${key}`);
  if (value !== undefined) assert.equal(metas[key], value, key);
}
assert.match(html, /<link rel="apple-touch-icon" href="apple-touch-icon\.png">/);
const jsonLd = [...html.matchAll(/<script type="application\/ld\+json">([^]*?)<\/script>/g)];
assert.equal(jsonLd.length, 1, "index.html has one JSON-LD block");
const game = JSON.parse(jsonLd[0][1]);
assert.equal(game.url, SITE, "the JSON-LD url");
assert.equal(game.image, `${SITE}og.png`, "the JSON-LD image");
// `just web` stamps every page URL with a hash of the shipped files (as templates), so one deploy's files load together.
const main = readFileSync(join(webDir, "main.js"), "utf8");
const build = /const BUILD = "([0-9a-f]{12})";/.exec(main)?.[1];
assert.ok(build, "main.js carries a 12-hex build stamp");
const unstamp = (text) => text.replaceAll(build, "__BUILD__");
const expected = createHash("sha256")
  .update(wasmBytes)
  .update(readFileSync(join(webDir, "chromatron_web.js")))
  .update(unstamp(main))
  .update(readFileSync(join(webDir, "touch_layout.js")))
  .update(readFileSync(join(webDir, "analytics.js")))
  .update(readFileSync(join(webDir, "style.css")))
  .update(unstamp(html))
  .digest("hex")
  .slice(0, 12);
assert.equal(build, expected, "the stamp hashes every shipped file");
assert.ok(!html.includes("__BUILD__") && !main.includes("__BUILD__"), "the build hash is stamped");
assert.ok(html.includes(`main.js?v=${build}`) && html.includes(`style.css?v=${build}`), "index.html");
assert.ok(main.includes(`const BUILD = "${build}";`), "main.js carries the build hash");
assert.match(main, /import\(`\.\/chromatron_web\.js\?v=\$\{BUILD\}`\)/);
assert.match(main, /import\(`\.\/touch_layout\.js\?v=\$\{BUILD\}`\)/);
assert.match(main, /import\(`\.\/analytics\.js\?v=\$\{BUILD\}`\)/);
assert.match(main, /new URL\(`chromatron_web_bg\.wasm\?v=\$\{BUILD\}`, import\.meta\.url\)/);
for (const script of ["main.js", "touch_layout.js", "analytics.js"]) {
  execFileSync(process.execPath, ["--check", join(webDir, script)], { stdio: "inherit" });
  // Our scripts load nothing from elsewhere, but for SCRIPT_URLS.
  const urls = readFileSync(join(webDir, script), "utf8").match(/https?:\/\/[^\s"'`)]+/g) ?? [];
  assert.deepEqual(urls, SCRIPT_URLS[script] ?? [], `the absolute URLs in ${script}`);
}

// Analytics: off away from the site, on there or with ?analytics=debug, which never loads Google's script.
{
  const analytics = await import(pathToFileURL(join(webDir, "analytics.js")).href);
  const appended = [];
  globalThis.window = {};
  globalThis.document = { createElement: () => ({ addEventListener() {} }), head: { append: (element) => appended.push(element) } };
  for (const hostname of ["127.0.0.1", "localhost", "example.com"]) {
    assert.equal(analytics.start({ hostname, search: "" }), false, `no analytics on ${hostname}`);
  }
  analytics.track("level_start", { game: 1 });
  assert.equal(window.dataLayer, undefined, "nothing queued while off");
  assert.equal(analytics.start({ hostname: "127.0.0.1", search: "?analytics=debug" }), true, "?analytics=debug turns it on");
  analytics.track("level_end", { game: 1, success: true });
  const entries = window.dataLayer.map((entry) => [...entry]);
  assert.deepEqual(entries.map(([command, target]) => [command, command === "js" ? null : target]), [["js", null], ["config", "G-ZMH6NLL7C7"], ["event", "level_end"]]);
  assert.deepEqual(entries[1][2], { content_group: "chromatron" });
  assert.equal(appended.length, 0, "the debug mode loads no script");
  window.gtag = () => {
    throw new Error("blocked");
  };
  analytics.track("level_end", { game: 1 });
  globalThis.window = {};
  assert.equal(analytics.start({ hostname: "shengtuo.me", search: "" }), true, "on at the site");
  assert.deepEqual(appended.map((script) => [script.async, script.src]), [[true, "https://www.googletagmanager.com/gtag/js?id=G-ZMH6NLL7C7"]]);
  delete globalThis.window;
  delete globalThis.document;
}

// Play.
const glue = await import(pathToFileURL(join(webDir, "chromatron_web.js")).href);
const wasm = glue.initSync({ module: wasmBytes });
const { Flag, LevelState, Session, display_scale: displayScale } = glue;
const has = (flags, flag) => (flags & flag) !== 0;
assert.equal(displayScale(1280, 960), 2);
assert.equal(displayScale(320, 480), 0.5);

// The Levels picker and the portrait views.
{
  const session = new Session();
  session.select(2, undefined, undefined);
  assert.equal(session.slot_state(0), LevelState.Current);
  assert.equal(session.slot_state(4), LevelState.Unlocked);
  assert.equal(session.slot_state(5), LevelState.Locked);
  assert.equal(session.open_slot(5), 0, "a locked level stays shut");
  assert.ok(has(session.open_slot(3), Flag.Slot) && session.slot() === 3, "an open level opens");
  assert.deepEqual([...session.board_rect()], [48, 18, 408, 378]);
  assert.deepEqual([...session.toolbox_rect()], [446, 6, 604, 112]);
  assert.deepEqual([...session.strip_number_rect(0)], [10, 410, 30, 422]);
  assert.equal(session.strip_number_rect(50).length, 0);
  session.free();
}

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
