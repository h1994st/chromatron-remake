// The trailer's capture tool: serves trailer/ over HTTP, opens stage.html in headless Google Chrome
// (as crates/chromatron-web/tests/web_shots.mjs does) at an exact viewport with device scale 1,
// draws through window.trailer and takes Page.captureScreenshot.
//
//   node reel.mjs stills [<dir> [<name>…]]
//     The brand stills: web/og.png, web/apple-touch-icon.png, assets/promo/banner.jpg,
//     assets/promo/cover-portrait.jpg, its English twin assets/promo/cover-portrait-en.jpg (whose
//     QR code is read back) and the three video thumbnails
//     assets/promo/thumbnail-{a,b,c}.jpg (`just social-images`), or with <dir> (under dist/ or
//     outside the repo) the named stills, or all, into <dir> for review. The PNGs are squeezed with
//     png_squeeze.mjs; every output's size and byte cap is checked.
//   node reel.mjs video <dir> [<wav>]
//     Both videos, rendered side by side in two tabs: every frame's PNG is piped into ffmpeg
//     (H.264 High, yuv420p, 60 fps, +faststart), with <wav> muxed in as AAC if given, into
//     <dir>/chromatron-remake-{landscape,portrait}.mp4; each is then checked with ffprobe, and its
//     loudness (ebur128) against the soundtrack's targets.
//   node reel.mjs remux <dir> <wav>
//     Lays <wav> onto the videos already in <dir>, the picture copied, and checks them the same way.
//   node reel.mjs sheet <dir> [<shape>…]
//     A contact sheet of the keyframes (scenes.mjs's KEYFRAMES) per shape, <dir>/<shape>.png, after
//     checking that a frame rendered alone matches the same frame rendered in sequence.
//   node reel.mjs strip <png> <shape> <from> <to> <every>
//     A sheet of every <every>th frame from <from> to <to> s, to judge a move.
//   node reel.mjs frames <dir> <shape> <t>…
//     Full-size frames at the given times, <dir>/<shape>-<t>.png.
//
// Every way out (the end, an error, Ctrl-C, SIGTERM, the mode's time limit) stops Chrome and
// ffmpeg and removes the temporary files. $CHROME overrides Chrome's path.

import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { copyFileSync, createReadStream, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, extname, isAbsolute, join, normalize, relative, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { KEYFRAMES, SHAPES } from "./scenes.mjs";
import { RATE } from "./score.mjs";
import { STILLS } from "./stills.mjs";
import { DURATION, FPS } from "./timeline.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const repo = resolve(here, "..");
const CHROME = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const TYPES = { ".html": "text/html; charset=utf-8", ".mjs": "text/javascript", ".js": "text/javascript" };

// Where `just social-images` puts each still, PNG bits per channel for the squeeze (or JPEG
// quality), and its byte cap. Each still's size is in stills.mjs.
const STILL_OUTPUTS = {
  og: { path: "web/og.png", bits: 6, maxBytes: 300_000 },
  icon: { path: "web/apple-touch-icon.png", bits: 8, maxBytes: 50_000 },
  banner: { path: "assets/promo/banner.jpg", quality: 88, maxBytes: 700_000 },
  "cover-portrait": { path: "assets/promo/cover-portrait.jpg", quality: 88, maxBytes: 700_000 },
  "cover-portrait-en": { path: "assets/promo/cover-portrait-en.jpg", quality: 90, maxBytes: 1_500_000 },
  // Video thumbnails may be up to 2 MB; ours stay well under.
  "thumbnail-a": { path: "assets/promo/thumbnail-a.jpg", quality: 88, maxBytes: 400_000 },
  "thumbnail-b": { path: "assets/promo/thumbnail-b.jpg", quality: 88, maxBytes: 400_000 },
  "thumbnail-c": { path: "assets/promo/thumbnail-c.jpg", quality: 88, maxBytes: 400_000 },
};

let chrome;
let server;
let profile;
let work;
const children = new Set();
const sockets = new Set();
let finishing;
function finish(code) {
  finishing ??= (async () => {
    server?.close();
    for (const socket of sockets) socket.close();
    for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    if (chrome && chrome.exitCode === null && chrome.signalCode === null) {
      const exited = new Promise((done) => chrome.once("exit", done));
      chrome.kill();
      const force = setTimeout(() => chrome.kill("SIGKILL"), 3000);
      await exited;
      clearTimeout(force);
    }
    for (const dir of [profile, work]) if (dir) rmSync(dir, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
    // Let piped output drain first: process.exit drops pending writes to a pipe.
    await new Promise((done) => process.stdout.write("", done));
    process.exit(code);
  })();
  return finishing;
}
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) process.on(signal, () => finish(1));
for (const event of ["uncaughtException", "unhandledRejection"]) {
  process.on(event, (error) => {
    // Once on the way out, the calls cut short by stopping Chrome are not news.
    if (!finishing) console.error(`reel: ${error?.stack ?? error}`);
    finish(1);
  });
}
// Each mode gives up after its own limit, the same way.
function timeLimit(seconds) {
  setTimeout(() => {
    console.error(`reel: timed out after ${seconds} s`);
    finish(1);
  }, seconds * 1000).unref();
}
// A temporary directory removed on every way out.
function workDir() {
  work ??= mkdtempSync(join(tmpdir(), "chromatron-reel-work-"));
  return work;
}

// Serves trailer/ only.
function serve() {
  server = createServer((request, response) => {
    const path = normalize(decodeURIComponent(new URL(request.url, "http://localhost").pathname)).replace(/^[/\\]+/, "") || "stage.html";
    const file = join(here, path);
    if (path.startsWith("..") || !existsSync(file) || !statSync(file).isFile()) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
    createReadStream(file).pipe(response);
  });
  return new Promise((done) => server.listen(0, "127.0.0.1", () => done(`http://127.0.0.1:${server.address().port}/`)));
}

// Starts headless Chrome with a throw-away profile; returns its DevTools port.
async function launch() {
  assert.ok(existsSync(CHROME), `no Google Chrome at ${CHROME}; set CHROME to its executable`);
  profile = mkdtempSync(join(tmpdir(), "chromatron-reel-"));
  chrome = spawn(
    CHROME,
    ["--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"],
    { stdio: "ignore" },
  );
  const portFile = join(profile, "DevToolsActivePort");
  let port = "";
  while (!port) {
    assert.ok(chrome.exitCode === null && chrome.signalCode === null, `Chrome exited at start (${CHROME})`);
    if (existsSync(portFile)) port = readFileSync(portFile, "utf8").split("\n")[0].trim();
    if (!port) await delay(100);
  }
  // Chrome dying mid-run ends the run at once, rather than leaving its calls waiting.
  chrome.once("exit", (code, signal) => {
    if (finishing) return;
    console.error(`reel: Chrome exited mid-run (${signal ?? `code ${code}`})`);
    finish(1);
  });
  return port;
}

// A CDP connection to one tab: send(method, params), evaluate(expression) and its page errors.
async function attach(url) {
  const socket = new WebSocket(url);
  sockets.add(socket);
  await new Promise((done, reject) => {
    socket.addEventListener("open", done, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  let lastId = 0;
  const calls = new Map();
  const errors = [];
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.id !== undefined) {
      const call = calls.get(message.id);
      calls.delete(message.id);
      if (message.error) call.reject(new Error(`${call.method}: ${message.error.message}`));
      else call.resolve(message.result);
    } else if (message.method === "Runtime.exceptionThrown") {
      const details = message.params.exceptionDetails;
      errors.push(details.exception?.description ?? details.text);
    } else if (message.method === "Runtime.consoleAPICalled" && message.params.type === "error") {
      errors.push(message.params.args.map((arg) => arg.value ?? arg.description).join(" "));
    }
  });
  // A closed connection fails every call still waiting on it, and every later one.
  socket.addEventListener("close", () => {
    for (const call of calls.values()) call.reject(new Error(`${call.method}: Chrome closed the connection`));
    calls.clear();
  });
  const send = (method, params = {}) =>
    new Promise((done, reject) => {
      if (socket.readyState !== WebSocket.OPEN) {
        reject(new Error(`${method}: Chrome closed the connection`));
        return;
      }
      lastId += 1;
      calls.set(lastId, { method, resolve: done, reject });
      socket.send(JSON.stringify({ id: lastId, method, params }));
    });
  const evaluate = async (expression) => {
    const { result, exceptionDetails } = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
    return result.value;
  };
  await send("Runtime.enable");
  return { send, evaluate, errors };
}

// The first tab, or with `fresh` a new one.
async function tab(port, fresh) {
  const targets = fresh ? [await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json()] : await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  return attach(targets.find((target) => target.type === "page").webSocketDebuggerUrl);
}

// Opens the stage in `page` at width × height CSS px, at device scale 1, and waits for its modules.
async function openStage(page, origin, width, height) {
  await page.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await page.send("Page.navigate", { url: `${origin}stage.html` });
  for (let tries = 0; !(await page.evaluate("window.trailerReady === true").catch(() => false)); tries += 1) {
    assert.ok(tries < 100, "the stage did not start");
    await delay(50);
  }
  assert.deepEqual(page.errors, [], "the stage has no errors");
}

// Renders frame time t in `shape` and returns the PNG (CDP's fast encoder: it is lossless either way).
async function frame(page, t, shape) {
  const [width, height] = SHAPES[shape];
  await page.evaluate(`trailer.render(${t}, ${JSON.stringify(shape)})`);
  assert.deepEqual(page.errors, [], `frame ${t} s draws with no errors`);
  const { data } = await page.send("Page.captureScreenshot", { format: "png", optimizeForSpeed: true, clip: { x: 0, y: 0, width, height, scale: 1 } });
  return Buffer.from(data, "base64");
}

// The pixel size of a PNG (its IHDR) or a baseline or progressive JPEG (its SOF marker).
function imageSize(bytes) {
  if (bytes.subarray(1, 4).toString("latin1") === "PNG") return [bytes.readUInt32BE(16), bytes.readUInt32BE(20)];
  assert.ok(bytes[0] === 0xff && bytes[1] === 0xd8, "a PNG or a JPEG");
  for (let at = 2; at < bytes.length; ) {
    assert.equal(bytes[at], 0xff, "a JPEG marker");
    const marker = bytes[at + 1];
    if (marker >= 0xc0 && marker <= 0xc2) return [bytes.readUInt16BE(at + 7), bytes.readUInt16BE(at + 5)];
    at += 2 + bytes.readUInt16BE(at + 2);
  }
  throw new Error("a JPEG with no SOF marker");
}

// Review output may only go outside the repo or under dist/ (git-ignored); never under re/, which
// is for reverse engineering only.
function reviewDir(dir) {
  const inRepo = relative(repo, resolve(dir));
  assert.ok(
    inRepo.startsWith("..") || isAbsolute(inRepo) || /^dist(\/|$)/.test(inRepo),
    `${dir} is in the repo but not under dist/; use dist/trailer/review/… or a directory outside the repo`,
  );
  mkdirSync(resolve(dir), { recursive: true });
  return resolve(dir);
}

// Runs a command to completion, failing loudly.
function run(command, args, options = {}) {
  return execFileSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], ...options });
}

async function stills(dir, names) {
  const out = dir === undefined ? null : reviewDir(dir);
  const chosen = names.length ? names : Object.keys(STILL_OUTPUTS);
  for (const name of chosen) assert.ok(STILL_OUTPUTS[name] && STILLS[name], `no still named ${name}`);
  timeLimit(120);
  const origin = await serve();
  const page = await tab(await launch());
  for (const name of chosen) {
    const { path, bits, quality, maxBytes } = STILL_OUTPUTS[name];
    const { width, height } = STILLS[name];
    const file = out ? join(out, `${name}${extname(path)}`) : join(repo, path);
    mkdirSync(dirname(file), { recursive: true });
    await openStage(page, origin, width, height);
    await page.evaluate(`trailer.still(${JSON.stringify(name)})`);
    assert.deepEqual(page.errors, [], `${name} draws with no errors`);
    const format = quality ? { format: "jpeg", quality } : { format: "png" };
    const { data } = await page.send("Page.captureScreenshot", { ...format, clip: { x: 0, y: 0, width, height, scale: 1 } });
    writeFileSync(file, Buffer.from(data, "base64"));
    // A still with a QR code: Chrome's barcode reader reads the saved image back to its address,
    // at full size and at a third (as a post in a phone's feed shows it).
    if (STILLS[name].scan) {
      const type = quality ? "image/jpeg" : "image/png";
      for (const scale of [1, 1 / 3]) {
        const read = await page.evaluate(`(async () => {
          if (typeof BarcodeDetector === "undefined") return "no BarcodeDetector in this Chrome";
          const image = await createImageBitmap(await (await fetch("data:${type};base64,${data}")).blob(), {
            resizeWidth: Math.round(${width * scale}),
            resizeHeight: Math.round(${height * scale}),
            resizeQuality: "high",
          });
          return (await new BarcodeDetector({ formats: ["qr_code"] }).detect(image)).map((code) => code.rawValue);
        })()`);
        assert.deepEqual(read, [STILLS[name].scan], `${name}'s QR code reads back as its address at ${scale === 1 ? "full size" : "a third"}`);
      }
    }
    if (bits) execFileSync(process.execPath, [join(repo, "crates/chromatron-web/tests/png_squeeze.mjs"), file, String(bits)], { stdio: "inherit" });
    const bytes = readFileSync(file);
    assert.deepEqual(imageSize(bytes), [width, height], `${name} is ${width} × ${height}`);
    assert.ok(bytes.length <= maxBytes, `${name} is ${bytes.length} bytes, over ${maxBytes}`);
    console.log(`${String(bytes.length).padStart(8)} bytes  ${relative(process.cwd(), file)} (${width} × ${height})`);
  }
}

// Encodes one shape: frames piped into ffmpeg as PNGs, then the audio muxed in if given.
async function encode(page, origin, shape, out, wav) {
  const [width, height] = SHAPES[shape];
  await openStage(page, origin, width, height);
  const args = ["-hide_banner", "-loglevel", "error", "-y", "-f", "image2pipe", "-framerate", String(FPS), "-c:v", "png", "-i", "-"];
  if (wav) args.push("-i", wav, "-map", "0:v:0", "-map", "1:a:0", "-c:a", "aac", "-b:a", "192k", "-shortest");
  args.push("-c:v", "libx264", "-preset", "slow", "-crf", "18", "-pix_fmt", "yuv420p", "-profile:v", "high", "-r", String(FPS), "-movflags", "+faststart", out);
  const ffmpeg = spawn("ffmpeg", args, { stdio: ["pipe", "inherit", "inherit"] });
  children.add(ffmpeg);
  // ffmpeg ending early is reported as its exit, not as the broken pipe it leaves behind.
  let failed = null;
  const done = new Promise((ok, fail) =>
    ffmpeg.once("exit", (code, signal) => {
      if (code === 0) ok();
      else fail((failed = new Error(`ffmpeg exited with ${signal ?? code} for ${shape}`)));
    }),
  );
  done.catch(() => {});
  ffmpeg.stdin.on("error", () => {});
  const frames = Math.round(DURATION * FPS);
  const begun = Date.now();
  for (let i = 0; i < frames; i += 1) {
    const png = await frame(page, i / FPS, shape);
    if (failed) throw failed;
    if (!ffmpeg.stdin.write(png)) await Promise.race([new Promise((drained) => ffmpeg.stdin.once("drain", drained)), done]);
    if (i % 300 === 299) console.log(`  ${shape}: ${i + 1}/${frames} frames, ${((Date.now() - begun) / (i + 1)).toFixed(0)} ms each`);
  }
  ffmpeg.stdin.end();
  await done;
  verify(out, width, height, Boolean(wav));
  if (wav) loudness(out);
}

// The soundtrack's loudness targets (score.mjs), checked on the encoded AAC.
const LUFS_RANGE = [-15, -13];
const MAX_TRUE_PEAK = -1;

// Reads a video's integrated loudness and true peak with ffmpeg's ebur128 filter, prints them and
// checks them against the targets; messages name it `label`.
function loudness(file, label = file) {
  const result = spawnSync("ffmpeg", ["-hide_banner", "-nostats", "-i", file, "-map", "0:a:0", "-af", "ebur128=peak=true", "-f", "null", "-"], { encoding: "utf8" });
  assert.equal(result.status, 0, `ffmpeg ebur128 failed on ${label}: ${result.stderr}`);
  const summary = result.stderr.slice(result.stderr.lastIndexOf("Summary:"));
  const lufs = Number(summary.match(/I:\s+(-?[\d.]+) LUFS/)?.[1]);
  const peak = Number(summary.match(/Peak:\s+(-?[\d.]+) dBFS/)?.[1]);
  assert.ok(Number.isFinite(lufs) && Number.isFinite(peak), `no ebur128 summary for ${label}`);
  console.log(`${relative(process.cwd(), label)}: ${lufs.toFixed(1)} LUFS integrated, ${peak.toFixed(1)} dBTP true peak (ebur128)`);
  assert.ok(lufs >= LUFS_RANGE[0] && lufs <= LUFS_RANGE[1], `${label}: ${lufs} LUFS, outside ${LUFS_RANGE.join(" to ")}`);
  assert.ok(peak <= MAX_TRUE_PEAK, `${label}: true peak ${peak} dBTP, over ${MAX_TRUE_PEAK}`);
}

// Lays <wav> onto the videos already in <dir> (the picture copied, not re-encoded), so the score
// can be revised in seconds. Each is checked before it replaces the video in <dir>.
async function remux(dir, wav) {
  assert.ok(dir && wav && existsSync(wav), "usage: node reel.mjs remux <dir> <wav>");
  timeLimit(5 * 60);
  for (const [shape, [width, height]] of Object.entries(SHAPES)) {
    const file = join(resolve(dir), `chromatron-remake-${shape}.mp4`);
    assert.ok(existsSync(file), `no ${file}: run just trailer first`);
    const temp = join(workDir(), `${shape}.mp4`);
    run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-i", file, "-i", wav, "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest", "-movflags", "+faststart", temp]);
    verify(temp, width, height, true, file);
    loudness(temp, file);
    copyFileSync(temp, file);
  }
}

// Checks an encoded video with ffprobe: its duration and frame count, size, frame rate, pixel
// format and streams; messages name it `label`.
function verify(file, width, height, audio, label = file) {
  const probe = JSON.parse(run("ffprobe", ["-v", "error", "-show_streams", "-show_format", "-of", "json", file]));
  const video = probe.streams.filter((stream) => stream.codec_type === "video");
  const sound = probe.streams.filter((stream) => stream.codec_type === "audio");
  assert.equal(video.length, 1, `${label}: one video stream`);
  const [v] = video;
  assert.equal(v.codec_name, "h264", `${label}: H.264`);
  assert.equal(v.profile, "High", `${label}: High profile`);
  assert.equal(v.pix_fmt, "yuv420p", `${label}: yuv420p`);
  assert.deepEqual([v.width, v.height], [width, height], `${label}: ${width} × ${height}`);
  assert.equal(v.r_frame_rate, `${FPS}/1`, `${label}: ${FPS} fps`);
  assert.equal(v.nb_frames, String(DURATION * FPS), `${label}: ${DURATION * FPS} frames`);
  const duration = Number(probe.format.duration);
  assert.ok(Math.abs(duration - DURATION) <= 0.05, `${label}: ${duration} s, not ${DURATION} ± 0.05 s`);
  if (audio) {
    assert.equal(sound.length, 1, `${label}: one audio stream`);
    const [a] = sound;
    assert.equal(a.codec_name, "aac", `${label}: AAC audio`);
    assert.equal(a.sample_rate, String(RATE), `${label}: ${RATE / 1000} kHz audio`);
    assert.equal(a.channels, 2, `${label}: stereo audio`);
  } else {
    assert.equal(sound.length, 0, `${label}: no audio stream`);
  }
  const bytes = statSync(file).size;
  console.log(`${relative(process.cwd(), label)}: ${width} × ${height}, ${duration.toFixed(3)} s, ${v.nb_frames} frames at ${FPS} fps, ${audio ? "AAC audio, " : ""}${(bytes / 1e6).toFixed(1)} MB`);
}

async function video(dir, wav) {
  assert.ok(dir, "usage: node reel.mjs video <dir> [<wav>]");
  if (wav) assert.ok(existsSync(wav), `no WAV at ${wav}`);
  const out = resolve(dir);
  mkdirSync(out, { recursive: true });
  timeLimit(40 * 60);
  const origin = await serve();
  const port = await launch();
  const pages = [await tab(port), await tab(port, true)];
  const begun = Date.now();
  await Promise.all(Object.keys(SHAPES).map((shape, i) => encode(pages[i], origin, shape, join(out, `chromatron-remake-${shape}.mp4`), wav)));
  console.log(`rendered both videos in ${((Date.now() - begun) / 1000).toFixed(0)} s`);
}

// Tiles PNG files into one sheet with ffmpeg, each scaled to `cell` px wide, `columns` across.
function tile(files, out, { cell, columns }) {
  const list = join(workDir(), `tile-${Date.now()}.txt`);
  writeFileSync(list, files.map((file) => `file '${file}'\n`).join(""));
  const rows = Math.ceil(files.length / columns);
  run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", list, "-vf", `scale=${cell}:-2:flags=lanczos,tile=${columns}x${rows}:padding=6:color=0x202020`, "-frames:v", "1", "-update", "1", out]);
}

async function sheet(dir, shapes) {
  const out = reviewDir(dir);
  const chosen = shapes.length ? shapes : Object.keys(SHAPES);
  timeLimit(10 * 60);
  const origin = await serve();
  const page = await tab(await launch());
  for (const shape of chosen) {
    assert.ok(SHAPES[shape], `no shape ${shape}`);
    const [width, height] = SHAPES[shape];
    await openStage(page, origin, width, height);
    // Purity: a frame rendered first, alone, has the same pixels as after all the others.
    const probe = KEYFRAMES[Math.floor(KEYFRAMES.length / 2)];
    const alone = await frame(page, probe, shape);
    const files = [];
    for (const t of KEYFRAMES) {
      const file = join(workDir(), `${shape}-${t.toFixed(3)}.png`);
      writeFileSync(file, await frame(page, t, shape));
      files.push(file);
    }
    assert.ok(alone.equals(await frame(page, probe, shape)), `${shape}: frame ${probe} s is the same alone and in sequence`);
    const file = join(out, `${shape}.png`);
    tile(files, file, shape === "portrait" ? { cell: 270, columns: 8 } : { cell: 480, columns: 4 });
    console.log(`${relative(process.cwd(), file)}: ${files.length} keyframes (${KEYFRAMES.join(", ")} s)`);
  }
}

async function strip(png, shape, from, to, every) {
  assert.ok(png && SHAPES[shape] && Number.isFinite(from) && Number.isFinite(to) && every >= 1, "usage: node reel.mjs strip <png> <shape> <from> <to> <every>");
  reviewDir(dirname(png));
  timeLimit(10 * 60);
  const origin = await serve();
  const page = await tab(await launch());
  const [width, height] = SHAPES[shape];
  await openStage(page, origin, width, height);
  const files = [];
  for (let i = Math.round(from * FPS); i <= Math.round(to * FPS); i += every) {
    const file = join(workDir(), `${shape}-${i}.png`);
    writeFileSync(file, await frame(page, i / FPS, shape));
    files.push(file);
  }
  tile(files, resolve(png), shape === "portrait" ? { cell: 270, columns: 8 } : { cell: 480, columns: 4 });
  console.log(`${relative(process.cwd(), resolve(png))}: frames ${from}–${to} s every ${every}`);
}

async function frames(dir, shape, times) {
  const out = reviewDir(dir);
  assert.ok(SHAPES[shape], `no shape ${shape}`);
  timeLimit(10 * 60);
  const origin = await serve();
  const page = await tab(await launch());
  const [width, height] = SHAPES[shape];
  await openStage(page, origin, width, height);
  for (const t of times) {
    const begun = Date.now();
    const png = await frame(page, t, shape);
    const file = join(out, `${shape}-${t.toFixed(2)}.png`);
    writeFileSync(file, png);
    console.log(`${relative(process.cwd(), file)} (${Date.now() - begun} ms)`);
  }
}

const [mode, ...rest] = process.argv.slice(2);
if (mode === "stills") await stills(rest[0], rest.slice(1));
else if (mode === "video") await video(rest[0], rest[1]);
else if (mode === "remux") await remux(rest[0], rest[1]);
else if (mode === "sheet") await sheet(rest[0], rest.slice(1));
else if (mode === "strip") await strip(rest[0], rest[1], Number(rest[2]), Number(rest[3]), Number(rest[4]));
else if (mode === "frames") await frames(rest[0], rest[1], rest.slice(2).map(Number));
else {
  console.error("usage: node reel.mjs stills [<dir> [<name>…]] | video <dir> [<wav>] | remux <dir> <wav> | sheet <dir> [<shape>…] | strip <png> <shape> <from> <to> <every> | frames <dir> <shape> <t>…");
  await finish(2);
}
await finish(0);
