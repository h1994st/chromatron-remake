// `just web-canvas-fnv`: the web's pixel-identity check. For each game (1–4) at the desktop
// (2560 × 1600 @2) and the phone (780 × 1480 @2, touch) sizes, a fresh Session paints the modern
// canvas cold in one theme, and this prints an FNV-1a hash of its RGBA bytes:
// `C<n> <w>x<h>@<s>: <hex>`. Then the wasm's own incremental frames on that canvas, a win's cue
// and a level change's fade: `C<n> <w>x<h>@<s> <cue>+<ms>: <hex>`. A speed change must leave
// every line as it was.
// Usage: node canvas_fnv.mjs <dist/web> <dark|light>

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [webDir, theme] = process.argv.slice(2);
assert.ok(
  webDir && (theme === "dark" || theme === "light"),
  "usage: node canvas_fnv.mjs <dist/web> <dark|light>",
);

const glue = await import(pathToFileURL(join(webDir, "chromatron_web.js")).href);
const wasm = glue.initSync({ module: readFileSync(join(webDir, "chromatron_web_bg.wasm")) });
const { Look, Session } = glue;

// [width, height, backing px per CSS px, touch]
const EASELS = [
  [2560, 1600, 2, false],
  [780, 1480, 2, true],
];

function fnv1a(bytes) {
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) {
    h = Math.imul(h ^ bytes[i], 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, "0");
}

// Each game's cheat key and whether it is typed with Control.
const CHEATS = { 1: ["L", false], 2: ["z", false], 3: ["^", false], 4: ["o", true] };

for (let game = 1; game <= 4; game++) {
  for (const [width, height, scale, touch] of EASELS) {
    const session = new Session();
    session.set_theme(theme === "dark");
    session.select(game);
    session.set_look(Look.Modern);
    session.resize(width, height, scale, [0, 0, 0, 0], touch);
    // The canvas after each paint (read afresh: wasm memory can grow and move).
    const canvas = () =>
      fnv1a(new Uint8Array(wasm.memory.buffer, session.rgba_ptr(), width * height * 4));
    const size = `C${game} ${width}x${height}@${scale}`;
    session.render_at(0);
    console.log(`${size}: ${canvas()}`);
    // Then incremental wasm frames over that canvas: a cheat-key win's cue and the level
    // change after it (its fade), each a few frames in and settled.
    const [cheat, ctrl] = CHEATS[game];
    session.key(cheat, ctrl, false, false);
    for (const dt of [0, 120, 500, 3000]) {
      session.render_at(1000 + dt);
      console.log(`${size} win+${dt}: ${canvas()}`);
    }
    session.key("=", false, false, false);
    for (const dt of [0, 90, 400, 3000]) {
      session.render_at(5000 + dt);
      console.log(`${size} level+${dt}: ${canvas()}`);
    }
    session.free();
  }
}
