// Chromatron in the browser: wires the page (game picker, toolbar, canvases, input, storage) to
// the wasm Session. Event-driven: a change schedules one animation frame; nothing runs while idle.
// Every URL is relative, so the page works under any path (GitHub Pages, `just web-serve`).

// `just web` replaces this with the wasm's hash, so the files of one build always load together.
const BUILD = "__BUILD__";
const FRAME_WIDTH = 640;
const FRAME_HEIGHT = 480;
// The app's text: Lucida Grande 13 pt, black, left-aligned, word-wrapped to each box.
const FONT = '13px "Lucida Grande", "Lucida Sans Unicode", "Helvetica Neue", Arial, sans-serif';
const LINE_HEIGHT = 16; // frame pixels
const BASELINE = 12; // the first baseline below a box's top, frame pixels
const GUTTER = 8; // CSS pixels kept around the game outside full screen
const GAMES = [1, 2, 3, 4];
// `?layout=touch` (or `desktop`) forces a layout whatever the pointer: a debug and test hook
// (`just web-shots`). It is never stored.
const forcedLayout = new URLSearchParams(location.search).get("layout");
const coarsePointer = matchMedia("(pointer: coarse)");
// Spec §9.3: the visitor's reduced-motion setting, followed live (the Session settles a running
// animation when it turns on).
const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)");
// `?look=classic` (or `modern`) forces a look over the stored choice, for tests; never stored.
const forcedLook = new URLSearchParams(location.search).get("look");

const $ = (id) => document.getElementById(id);
const stage = $("stage");
const board = $("game");
const pixels = $("pixels");
const text = $("text");
const pixelContext = pixels.getContext("2d");
const textContext = text.getContext("2d");
const lightbox = $("lightbox");
const lightboxContext = lightbox.getContext("2d");
const header = document.querySelector("header");
const footer = document.querySelector("footer");
const boardPart = $("board-part");
const toolboxPart = $("toolbox-part");
const smallViewport = $("small-viewport");
const themeColor = document.querySelector('meta[name="theme-color"]');
const toolbarNext = document.querySelector('.toolbar [data-command="next"]');
// The ▶'s own aria-label (none: its hidden "Next" names it), put back when its highlight goes.
const toolbarNextLabel = toolbarNext.getAttribute("aria-label");

// localStorage that never throws: reads say whether storage answered at all.
const storage = {
  read(key) {
    try {
      return { ok: true, value: window.localStorage.getItem(key) };
    } catch {
      return { ok: false, value: null };
    }
  },
  write(key, value) {
    try {
      window.localStorage.setItem(key, value);
      return true;
    } catch {
      return false;
    }
  },
};

let wasm;
let session;
let Flag;
let displayScale;
// The look on screen, "classic" or "modern" (wantedLook); "" before the first layout.
let look = "";
let Look;
let renderScale;
let stageHeight;
// The modern canvas's backing px per CSS px, as the session was last told.
let pxPerCss = 1;
// The last size given to session.resize, so an unchanged layout pass doesn't repeat it.
let lightboxSize = "";
// ImageData over the session's modern canvas in wasm memory; rebuilt after a resize or when
// wasm memory grows (which detaches the old buffer).
let lightboxImage = null;
// `?perf`: the page's own log of its last 512 modern paints for the checks (spec §14): the `now`
// given to render_at, when the paint began, render_at's and putImageData's times (ms), and how
// many rects and px it put. Off unless asked for; no cost when off.
const perfLog = new URLSearchParams(location.search).has("perf") ? (window.__perf = { frames: [] }).frames : null;
function notePerf(at, wall, render, put, rects, px) {
  perfLog.push({ at, wall, render, put, rects, px, look });
  if (perfLog.length > 512) perfLog.shift();
}
let LevelState;
// web/touch_layout.js: the pure layout and touch rules.
let touchLayout;
// "desktop", "portrait" or "landscape" (touchLayout.layoutMode); "" before the first layout.
let mode = "";
// The canvases that show the frame, each {canvas, context, frame}: the whole frame, or in
// portrait the board and the toolbox. `views` holds the ones on screen.
let frameView;
let boardView;
let toolboxView;
let views = [];
// Whether the RGBA frame in wasm memory holds a rendered frame yet.
let framed = false;
let layoutPending = false;
// A live resize stretches the modern canvas (CSS fills the stage with it) and resizes the session
// only once the size has been still this long: each session.resize repaints cold and takes its
// buffers anew. Going fullscreen and a pixel-ratio change resize at once (resizeNow).
const RESIZE_SETTLE_MS = 150;
let resizeTimer = 0;
let resizeNow = false;
let game = 0;
const started = new Set();
// Games this tab changed since it last stored them: only these are written, so a tab left open
// never overwrites progress that another tab stored. If two tabs change the same game, the last
// one to store it wins (a known limit).
const changed = new Set();
// Games whose stored save could not be read, or could not be kept aside when it was damaged:
// their save is never overwritten (the app's locked ProgressFile).
const locked = new Set();
let wheelOn = storage.read("chromatron.wheel").value !== "0";
// The saved "Turn clockwise" choice; `clockwise` is it in a touch layout and off on the desktop.
let clockwiseWanted = storage.read("chromatron.clockwise").value === "1";
let clockwise = false;
let scale = 1;
// The pending requestAnimationFrame's id, 0 when none: one frame chain at a time (paint drops it).
let frameRequest = 0;
// The last paint's `now`: paints never go back in time (a direct paint reads the clock after the
// frame's timestamp).
let paintedAt = 0;
let textBoxes = [];
let messageTimer = 0;
// The game's own "click for the next level" text (Session.click_next), read once per game select.
let clickNext = "";
// What a touch layout says instead of it: there is no spacebar, but there are a Next level button
// (portrait) and the level numbers and ▶ (landscape).
const TOUCH_NEXT = {
  portrait: "Tap Next level or pick a level from Levels.",
  landscape: "Tap a level number or ▶ for the next level.",
};
// The wasm's CaptionStyle and CaptionRole codes (caption_style, caption_role), and each style's CSS
// class (captionClasses); Hidden has none (screen readers only).
let CaptionStyle;
let CaptionRole;
let captionClasses;
// What the modern touch layouts say instead of click_next while Next ▶ shows (ruling 21).
const MODERN_TOUCH_NEXT = "Tap Next ▶ or pick a level from Levels.";
// Body captions shrink through these sizes (CSS px) until they fit; at the last they clip.
const BODY_SIZES = [15, 13, 11];
// Portrait: shown right under "You win!" while the level is won and a next level is open.
const nextButton = document.createElement("button");
nextButton.type = "button";
nextButton.id = "next-level";
nextButton.hidden = true;
nextButton.textContent = "Next level ▶";
// A ⌘/Ctrl+C or V whose copy or paste event has not fired yet (see clipboardShortcut).
let copyPending = false;
let pastePending = false;
// web/analytics.js, loaded after the first paint (loadAnalytics); the few events before it wait in
// `earlyEvents`.
let analytics;
const earlyEvents = [];
// A level shown after a user action is sent this long after the last change, so scrolling through
// levels sends only the one landed on.
const LEVEL_START_DELAY = 1000;
let levelStartTimer = 0;
// The level the win check saw last, and whether it was won then (noteWin).
let winGame = 0;
let winSlot = -1;
let winWon = false;

// Sends a game event, with the game, the layout and the look, to analytics (a no-op off the site).
function track(name, params) {
  const event = { game, layout: mode, look, ...params };
  if (analytics) analytics.track(name, event);
  else if (earlyEvents.length < 16) earlyEvents.push([name, event]);
}

// The current level as the events name it.
function levelParams() {
  const level = session.slot() + 1;
  return { game, level, level_name: `C${game}-L${String(level).padStart(2, "0")}` };
}

// The level changed by a user action: level_start once it has stayed for LEVEL_START_DELAY.
function levelChanged() {
  clearTimeout(levelStartTimer);
  levelStartTimer = setTimeout(() => track("level_start", levelParams()), LEVEL_START_DELAY);
}

// level_end once per change into the won state on the level shown; a level that comes up already
// won (another level, another game, a reload) is not a new win.
function noteWin() {
  const slot = session.slot();
  const won = session.won();
  if (won && !winWon && game === winGame && slot === winSlot) track("level_end", { ...levelParams(), success: true });
  winGame = game;
  winSlot = slot;
  winWon = won;
}

async function loadAnalytics() {
  try {
    const module = await import(`./analytics.js?v=${BUILD}`);
    module.start(location);
    analytics = module;
    for (const [name, event] of earlyEvents.splice(0)) analytics.track(name, event);
  } catch {
    // Analytics never stops the game.
  }
}

function say(message) {
  const element = $("message");
  element.textContent = message;
  clearTimeout(messageTimer);
  if (message) messageTimer = setTimeout(() => { element.textContent = ""; }, 6000);
}

function schedule() {
  if (frameRequest === 0) frameRequest = requestAnimationFrame(paint);
}

// Tells the player once per game per page load that progress is not being kept.
const warned = new Set();
function warnUnsaved(n) {
  if (warned.has(n)) return;
  warned.add(n);
  say(`Progress for Chromatron ${n} can't be saved in this browser.`);
}

function persist(n, saveText) {
  if (saveText === undefined || locked.has(n)) return;
  if (storage.write(`chromatron${n}.save`, saveText)) changed.delete(n);
  else warnUnsaved(n);
}

// Stores game n's save, as the app does on quit, if this tab changed it.
function flush(n) {
  if (changed.has(n)) persist(n, session.quit(n));
}

function rememberSlot() {
  if (game) storage.write(`chromatron${game}.lastLevelSlot`, String(session.slot()));
}

// Carries out what the session asks for after an input.
function apply(flags) {
  if (flags & (Flag.Redraw | Flag.Save | Flag.Slot)) changed.add(game);
  if (flags & Flag.Save) persist(game, session.save_text());
  if (flags & Flag.Slot) {
    rememberSlot();
    levelChanged();
  }
  if (flags & Flag.Redraw) schedule();
  // The modern look's level sheet opened (the toolbar's Levels, the card's pill, a key).
  if (flags & Flag.SheetOpened) track("open_levels", {});
  return flags;
}

function gameFromHash() {
  const n = Number(location.hash.slice(1));
  return GAMES.includes(n) ? n : undefined;
}

function selectGame(n) {
  if (!GAMES.includes(n) || n === game) return;
  // Any drag in the game left behind ends as every drag ends (the piece stays put), before its
  // save is stored.
  endDrag();
  if (game) flush(game);
  const save = storage.read(`chromatron${n}.save`);
  const last = storage.read(`chromatron${n}.lastLevelSlot`);
  if (!save.ok) {
    locked.add(n);
    warnUnsaved(n);
  }
  const slot = Number.parseInt(last.value ?? "", 10);
  const lastSlot = Number.isSafeInteger(slot) && slot >= 0 && slot <= 0xffffffff ? slot : undefined;
  const flags = session.select(n, save.value ?? undefined, lastSlot);
  if (flags & Flag.CorruptSave) {
    if (storage.write(`chromatron${n}.save.bak`, save.value)) {
      console.warn(`chromatron${n}.save is damaged; kept as chromatron${n}.save.bak`);
    } else {
      console.warn(`chromatron${n}.save is damaged and could not be kept aside; it will not be overwritten`);
      locked.add(n);
      warnUnsaved(n);
    }
  }
  game = n;
  // The Help's off-canvas credits are the last game's until the next captions: the paint after
  // this sets them, except under an open level sheet (renderCaptions waits for it to close).
  clearHelpCredits();
  clearTimeout(levelStartTimer);
  winGame = n;
  winSlot = session.slot();
  winWon = session.won();
  started.add(n);
  storage.write("chromatron.game", String(n));
  if (location.hash !== `#${n}`) history.replaceState(null, "", `#${n}`);
  document.title = session.title();
  clickNext = session.click_next();
  $("game-select").value = String(n);
  $("game-short-number").textContent = String(n);
  schedule();
}

// Another tab stored a game's save: a game this tab has not changed starts again from it.
function storedElsewhere(event) {
  const match = /^chromatron([1-4])\.save$/.exec(event.key ?? "");
  if (!match) return;
  const n = Number(match[1]);
  if (!started.has(n) || changed.has(n)) return;
  session.forget(n);
  started.delete(n);
  if (n === game) {
    game = 0;
    selectGame(n);
  }
}

// The engine's RGBA frame in wasm memory as ImageData, without a copy.
function frameImage() {
  const rgba = new Uint8ClampedArray(wasm.memory.buffer, session.rgba_ptr(), FRAME_WIDTH * FRAME_HEIGHT * 4);
  return new ImageData(rgba, FRAME_WIDTH, FRAME_HEIGHT);
}

// Copies frame rect `rect` into each view on screen that shows part of it.
function blit(image, rect) {
  for (const view of views) {
    const part = touchLayout.intersect(rect, view.frame);
    if (part !== null) {
      const { left, top } = view.frame;
      view.context.putImageData(image, -left, -top, part.left, part.top, part.right - part.left, part.bottom - part.top);
    }
  }
}

// Fills the views on screen from the last frame, after a layout change.
function blitAll() {
  if (look === "classic" && framed) blit(frameImage(), frameView.frame);
}

function lightboxPixels() {
  const width = session.canvas_width();
  const height = session.canvas_height();
  if (lightboxImage === null || lightboxImage.data.buffer !== wasm.memory.buffer || lightboxImage.width !== width || lightboxImage.height !== height) {
    lightboxImage = new ImageData(new Uint8ClampedArray(wasm.memory.buffer, session.rgba_ptr(), width * height * 4), width, height);
  }
  return lightboxImage;
}

function paintLightbox(now) {
  // Switched to modern, not laid out yet: sizeLightbox resizes the session and paints at once.
  // Painting the session's old lightbox at its stale size first would paint cold twice in a frame.
  if (lightboxSize === "") return;
  const wall = perfLog ? performance.now() : 0;
  const count = session.render_at(now);
  const rendered = perfLog ? performance.now() : 0;
  let px = 0;
  if (count > 0) {
    const image = lightboxPixels();
    const rects = new Int32Array(wasm.memory.buffer, session.dirty_ptr(), count * 4);
    for (let i = 0; i < count; i += 1) {
      const [left, top, right, bottom] = rects.subarray(i * 4, i * 4 + 4);
      lightboxContext.putImageData(image, 0, 0, left, top, right - left, bottom - top);
      px += (right - left) * (bottom - top);
    }
  }
  if (perfLog) notePerf(now, wall, rendered - wall, performance.now() - rendered, count, px);
  // The typeset text changes far less often than the pixels (never during a drag).
  if (session.captions_changed()) renderCaptions();
  // Spec §9.1 "Idle": the next frame only while a cue runs and the page is seen (ruling M4); the
  // classic look and settled pages never get here with next_wake ≥ 0. A page hidden mid-cue
  // paints once more when it is shown again (the visibilitychange listener).
  if (!document.hidden && session.next_wake(now) >= 0) schedule();
  afterPaint();
}

// The modern card's captions as HTML spans over the canvas (spec §10), each at its backing-px rect
// / pxPerCss CSS px. Consecutive credits share a rect and join with " · " (ruling 6); a compact
// card gives them an empty rect, and they go into the Help sheet instead. The win text, lettered
// on the canvas or not, is read out through #win-live.
function renderCaptions() {
  const box = $("captions");
  const sheetOpen = session.sheet_open();
  box.classList.toggle("sheet", sheetOpen);
  // The sheet covers the card: the captions shown fade out with its panel fading in (style.css),
  // and are set anew when it closes, fading in as the panel fades out.
  if (sheetOpen) return;
  const spans = [];
  const credits = [];
  let live = "";
  for (let i = 0; i < session.caption_count(); i += 1) {
    const style = session.caption_style(i);
    const role = session.caption_role(i);
    let words = session.caption_text(i).replace(/\r\n?/g, "\n");
    if (role === CaptionRole.ClickNext && mode !== "desktop" && session.next_open()) words = MODERN_TOUCH_NEXT;
    if (role === CaptionRole.Win) live = words;
    if (style === CaptionStyle.Hidden) continue;
    const [left, top, right, bottom] = session.caption_rect(i);
    if (style === CaptionStyle.Credit) {
      const last = credits.at(-1);
      if (last && last.left === left && last.top === top) last.words += ` · ${words}`;
      else credits.push({ left, top, right, bottom, words });
      continue;
    }
    spans.push({ left, top, right, bottom, words, role, className: captionClasses.get(style), alpha: session.caption_alpha(i) });
  }
  for (const credit of credits) {
    if (credit.right <= credit.left) continue;
    // Stacked's credits line runs under the card across the canvas, centred as in the comps;
    // Beside's sits in the column right of the board, left-aligned.
    const centred = Math.abs(credit.left - (lightbox.width - credit.right)) <= 2;
    spans.push({ ...credit, className: "cap-credit", alpha: 1, centred });
  }
  const offCanvas = credits.filter((credit) => credit.right <= credit.left).map((credit) => credit.words).join(" · ");
  const creditsHelp = $("credits-help");
  creditsHelp.textContent = offCanvas;
  creditsHelp.hidden = offCanvas === "";
  box.replaceChildren(
    ...spans.map(({ left, top, right, bottom, words, role, className, alpha, centred }) => {
      const span = document.createElement("span");
      span.className = className;
      span.textContent = words;
      // A win text that can't be lettered is a Title span; #win-live already reads it out.
      if (role === CaptionRole.Win) span.setAttribute("aria-hidden", "true");
      const height = (bottom - top) / pxPerCss;
      Object.assign(span.style, {
        left: `${left / pxPerCss}px`,
        top: `${top / pxPerCss}px`,
        width: `${(right - left) / pxPerCss}px`,
        height: `${height}px`,
        opacity: alpha === 1 ? "" : String(alpha),
        textAlign: centred ? "center" : "",
        // A title's one line, centred in its row (a plain block, so it can ellipsise).
        lineHeight: className === "cap-title" ? `${height}px` : "",
      });
      return span;
    }),
  );
  widenCompactName(box);
  // Body text shrinks 15 → 13 → 11 px until it fits, with line height 1.3 at 11 (ruling 22, as
  // card_min assumes); at 11 it clips (spec §10).
  for (const span of box.querySelectorAll(".cap-body")) {
    for (const size of BODY_SIZES) {
      span.style.fontSize = `${size}px`;
      span.style.lineHeight = size === BODY_SIZES.at(-1) ? "1.3" : "";
      if (span.scrollHeight <= span.clientHeight + 1) break;
    }
  }
  // The sheet hides the card's captions; the win text stays put under it (renderCaptions returns
  // above), so closing the sheet does not read it out again.
  const winLive = $("win-live");
  if (winLive.textContent !== live) winLive.textContent = live;
}

// A compact card's level name (halation's rect ends at the "(won)" slot, which Rust can't size to
// its text) grows rightwards on the header row: to 8 px left of the badge's measured text when
// there is one, else across the empty badge slot. That slot is BADGE_W wide and starts
// BADGE_NAME_GAP right of the name's rect (tableau's card_parts, in pt = CSS px), and it ends
// 10 pt left of the Levels pill, so the name never reaches the pill.
const BADGE_W = 44;
const BADGE_NAME_GAP = 4;
const BADGE_TEXT_GAP = 8;
function widenCompactName(box) {
  const name = box.querySelector(".cap-title-compact");
  if (name === null) return;
  const left = parseFloat(name.style.left);
  const right = left + parseFloat(name.style.width);
  const slotRight = right + BADGE_NAME_GAP + BADGE_W;
  const badge = box.querySelector(".cap-badge");
  let edge = slotRight;
  if (badge !== null) {
    const range = document.createRange();
    range.selectNodeContents(badge);
    edge = range.getBoundingClientRect().left - box.getBoundingClientRect().left - BADGE_TEXT_GAP;
  }
  if (edge > right) name.style.width = `${Math.min(edge, slotRight) - left}px`;
}

// The classic look has no captions: the overlay (hidden by CSS), the Help's credits and the win
// text all go.
function clearCaptions() {
  $("captions").replaceChildren();
  clearHelpCredits();
  $("win-live").textContent = "";
}

// Empties and hides the Help's off-canvas credits (a compact card's).
function clearHelpCredits() {
  $("credits-help").textContent = "";
  $("credits-help").hidden = true;
}

// What follows every paint, in either look.
function afterPaint() {
  updateNextButton();
  showNewLevel();
  noteWin();
  updateLevelsExpanded();
}

// The toolbar's Levels says whether what it opens is open: the modern sheet (it closes with
// Escape, the scrim, the close chip, a chip, or a resize to an inline picker, each followed by a
// paint) or the classic dialog.
function updateLevelsExpanded() {
  const open = look === "modern" ? session.sheet_open() : levelsDialog.open;
  setAttributeIfChanged($("levels-open"), "aria-expanded", String(open));
}

// Paints at `t`, the requestAnimationFrame timestamp (performance.now()'s clock), so every frame of
// one vsync agrees on the time (ruling M4). A direct call (a resize) paints at performance.now()
// and drops a frame still pending, which would only paint again.
function paint(t) {
  cancelAnimationFrame(frameRequest);
  frameRequest = 0;
  const now = Math.max(t ?? performance.now(), paintedAt);
  paintedAt = now;
  if (look === "modern") {
    paintLightbox(now);
    return;
  }
  const count = session.render_at(now);
  if (count > 0) {
    framed = true;
    const image = frameImage();
    const rects = new Int32Array(wasm.memory.buffer, session.dirty_ptr(), count * 4);
    for (let i = 0; i < count; i += 1) {
      const [left, top, right, bottom] = rects.subarray(i * 4, i * 4 + 4);
      blit(image, { left, top, right, bottom });
    }
    if (levelsDialog.open) refreshLevels(image);
  }
  if (session.text_changed()) {
    textBoxes = [];
    for (let i = 0; i < session.text_count(); i += 1) {
      const [left, top, right, bottom] = session.text_rect(i);
      textBoxes.push({ text: session.text(i), left, top, right, bottom });
    }
    if (mode === "portrait") listText();
    else drawText();
  }
  afterPaint();
}

// The game and level the last paint showed; 0 and -1 before the first.
let shownGame = 0;
let shownSlot = -1;
// Portrait: another level or game (by any path) scrolls the page back to the top, so its board is
// fully in view.
function showNewLevel() {
  const slot = session.slot();
  if (game === shownGame && slot === shownSlot) return;
  shownGame = game;
  shownSlot = slot;
  if (mode === "portrait" && window.scrollY > 0) window.scrollTo({ top: 0 });
}

// A text box's string as this layout shows it: the game's click_next, matched by equality, is
// reworded on the touch layouts and stays the game's own on the desktop.
function shownText(string) {
  const touchText = TOUCH_NEXT[mode];
  return touchText !== undefined && clickNext !== "" && string === clickNext ? touchText : string;
}

// Portrait: the frame's text boxes as HTML paragraphs, in the engine's order (the win text, the
// level name, the hint, the notices).
function listText() {
  $("text-list").replaceChildren(
    ...textBoxes.map((box) => {
      const paragraph = document.createElement("p");
      paragraph.textContent = shownText(box.text).replace(/\r\n?/g, "\n");
      return paragraph;
    }),
  );
  updateNextButton();
}

// The Next level button shows in classic portrait while the level is won and the next one is open
// (on the last level the toolbar's Next does nothing, so there is no button). It sits after the
// first text, "You win!"; listText rebuilds the list, so this puts it back. The toolbar's ▶ is
// highlighted meanwhile, so the win shows even with the texts scrolled below the screen. The
// modern look has its own Next ▶ on the canvas.
function updateNextButton() {
  const list = $("text-list");
  const show = look === "classic" && mode === "portrait" && session.won() && session.slot_state(session.slot() + 1) !== LevelState.Locked;
  if (nextButton.hidden !== !show) nextButton.hidden = !show;
  if (toolbarNext.classList.contains("won") !== show) {
    toolbarNext.classList.toggle("won", show);
    if (show) toolbarNext.setAttribute("aria-label", "Next level: this one is solved");
    else if (toolbarNextLabel === null) toolbarNext.removeAttribute("aria-label");
    else toolbarNext.setAttribute("aria-label", toolbarNextLabel);
  }
  if (!show) {
    nextButton.remove();
  } else if (nextButton.parentElement !== list) {
    const first = list.firstElementChild;
    if (first) first.after(nextButton);
    else list.append(nextButton);
  }
}

// Words that fit `width` frame pixels per line; a word longer than a line breaks between
// characters; \r, \n and \r\n start a new line.
function wrapLines(context, string, width) {
  const lines = [];
  for (const paragraph of string.split(/\r\n|\r|\n/)) {
    let line = "";
    for (const word of paragraph.split(" ")) {
      const candidate = line === "" ? word : `${line} ${word}`;
      if (line !== "" && context.measureText(candidate).width > width) {
        lines.push(line);
        line = word;
      } else {
        line = candidate;
      }
      while (line.length > 1 && context.measureText(line).width > width) {
        let cut = line.length - 1;
        while (cut > 1 && context.measureText(line.slice(0, cut)).width > width) cut -= 1;
        lines.push(line.slice(0, cut));
        line = line.slice(cut);
      }
    }
    lines.push(line);
  }
  return lines;
}

// The text layer: display size × devicePixelRatio backing pixels, drawn in frame units.
function drawText() {
  const ratio = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(FRAME_WIDTH * scale * ratio));
  const height = Math.max(1, Math.round(FRAME_HEIGHT * scale * ratio));
  if (text.width !== width || text.height !== height) {
    text.width = width;
    text.height = height;
  }
  const context = textContext;
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, width, height);
  context.setTransform(width / FRAME_WIDTH, 0, 0, height / FRAME_HEIGHT, 0, 0);
  context.font = FONT;
  context.fillStyle = "#000";
  context.textAlign = "left";
  context.textBaseline = "alphabetic";
  const drawn = textBoxes.map((box) => shownText(box.text));
  // The canvas can't be read back as text: the strings it draws are exposed for the page checks.
  text.dataset.drawn = JSON.stringify(drawn);
  for (const [index, box] of textBoxes.entries()) {
    const lines = wrapLines(context, drawn[index], box.right - box.left);
    context.save();
    context.beginPath();
    context.rect(box.left, box.top, box.right - box.left, box.bottom - box.top);
    context.clip();
    for (let i = 0; i < lines.length && box.top + (i + 1) * LINE_HEIGHT <= box.bottom; i += 1) {
      context.fillText(lines[i], box.left, box.top + BASELINE + i * LINE_HEIGHT);
    }
    context.restore();
  }
}

// A canvas showing frame rect `rect` ([left, top, right, bottom]), one backing pixel per frame pixel.
function partView(canvas, rect) {
  const frame = touchLayout.rectOf(rect);
  canvas.width = frame.right - frame.left;
  canvas.height = frame.bottom - frame.top;
  return { canvas, context: canvas.getContext("2d"), frame };
}

function sizeView(view, viewScale) {
  view.canvas.style.width = `${(view.frame.right - view.frame.left) * viewScale}px`;
  view.canvas.style.height = `${(view.frame.bottom - view.frame.top) * viewScale}px`;
}

// The look to show (spec §11): ?look= wins and is never stored; then the stored choice; then modern.
function wantedLook() {
  return touchLayout.lookFrom({ forced: forcedLook, stored: storage.read("chromatron.look").value });
}

// Switches the look (spec §11), storing it as the player's choice when `store`: the drag ends (the
// piece stays put), the session starts the look cold, and the page lays out again.
function setLook(next, { store = false } = {}) {
  if (store) storage.write("chromatron.look", next);
  if (next === look) return;
  closeMenu();
  endDrag();
  look = next;
  const root = document.documentElement;
  root.dataset.look = look;
  // Theme-ready (ruling 1): phase 2's modern look is dark only.
  root.dataset.theme = "dark";
  $("classic-look").setAttribute("aria-checked", String(look === "classic"));
  themeColor.content = look === "modern" ? "#06070b" : "#25272a";
  lightboxImage = null;
  lightboxSize = "";
  clearTimeout(resizeTimer);
  resizeTimer = 0;
  // Classic sizes its stage with CSS alone; the modern look has its own level sheet.
  if (look === "classic") {
    clearStageHeight();
    clearCaptions();
  } else if (levelsDialog.open) levelsDialog.close();
  placeFooter();
  apply(session.set_look(look === "modern" ? Look.Modern : Look.Classic));
  updateLevelsButton();
  requestLayout();
}

// Drops the stage height the modern look set (sizeLightbox), so CSS sizes the stage again.
function clearStageHeight() {
  stage.style.height = "";
  stage.style.minHeight = "";
  document.documentElement.classList.remove("stage-grown");
}

// The credits footer. Modern: always at the end of the Help sheet, followed by a compact card's
// credits (spec §12). Classic: it follows the texts in portrait, and sits at the end of the help
// in landscape, which fits one screen, instead of below the page.
function placeFooter() {
  if (look === "modern") helpDialog.querySelector(".sheet").append(footer, $("credits-help"));
  else if (mode === "landscape") helpDialog.querySelector(".sheet").append(footer);
  else if (mode === "portrait") $("texts").append(footer);
  else if (footer.parentElement !== document.body) document.body.append(footer);
}

// The toolbar's Levels: in modern whenever the canvas has no level picker of its own (it opens the
// sheet, and is the keyboard path to it); in classic in portrait, whose parts leave out the strip.
// The modern desktop keeps its slot while it is not shown (visibility: hidden): the desktop toolbar
// wraps, and whether the canvas has a picker is not monotonic in its height, so a button that came
// and went could wrap and unwrap the toolbar, and resize the canvas, forever.
function updateLevelsButton() {
  const button = $("levels-open");
  const show = look === "modern" ? !session.has_inline_picker() : mode === "portrait";
  const keepSlot = look === "modern" && mode === "desktop";
  const hidden = !show && !keepSlot;
  const visibility = show || hidden ? "" : "hidden";
  if (button.hidden !== hidden) button.hidden = hidden;
  if (button.style.visibility !== visibility) button.style.visibility = visibility;
}

// The "Turn clockwise" toggle's state, on the rail's button and its Actions menu item.
function showClockwise() {
  $("clockwise").setAttribute("aria-pressed", String(clockwise));
  $("clockwise-item").setAttribute("aria-checked", String(clockwise));
}

// Switches to layout `next`. The regions under a finger change, so any drag ends (the piece stays
// put); the game itself carries on.
function setMode(next) {
  if (next === mode) return;
  if (mode !== "") endDrag();
  mode = next;
  const root = document.documentElement;
  root.dataset.layout = mode;
  root.classList.toggle("touch", mode !== "desktop");
  // The first layout picks the look; later layouts keep the one on screen, so a rotation never
  // undoes a toggle (with ?look= forced, or storage that refused it).
  setLook(look || wantedLook());
  $("wheel").hidden = mode !== "desktop";
  $("clockwise").hidden = mode === "desktop";
  $("clockwise-item").hidden = mode === "desktop";
  if (mode !== "portrait" && levelsDialog.open) levelsDialog.close();
  // A hidden toggle stays off: the desktop's taps (mouse clicks) never turn clockwise by surprise.
  // A touch layout restores the saved choice; neither case writes it.
  clockwise = mode !== "desktop" && clockwiseWanted;
  showClockwise();
  document.body.style.paddingLeft = "";
  // The last layout's stage height; the modern look sets this one's (sizeLightbox).
  clearStageHeight();
  views = mode === "portrait" ? [boardView, toolboxView] : [frameView];
  placeFooter();
  updateLevelsButton();
  blitAll();
  if (look === "classic" && mode === "portrait") listText();
  else updateNextButton();
}

function layout() {
  const width = window.innerWidth;
  const height = window.innerHeight;
  setMode(touchLayout.layoutMode({ forced: forcedLayout, coarse: coarsePointer.matches, width, height }));
  // Landscape: edge to edge beside the toolbar, which runs down the left side.
  if (mode === "landscape") document.body.style.paddingLeft = `${header.offsetWidth}px`;
  if (look === "modern") {
    sizeLightbox();
    return;
  }
  const ratio = window.devicePixelRatio || 1;
  if (mode === "portrait") {
    // The board and the toolbox fill the small viewport (100svh) below the toolbar, with a gap
    // above, between and below them; the texts follow further down the page. Safari's bars
    // collapsing as the page scrolls changes nothing.
    const style = getComputedStyle(stage);
    const gaps = parseFloat(style.paddingTop) + 2 * parseFloat(style.rowGap);
    const room = smallViewport.getBoundingClientRect().height - header.getBoundingClientRect().height - gaps;
    const scales = touchLayout.portraitScales({
      width: document.documentElement.clientWidth,
      height: room,
      ratio,
      board: boardView.frame,
      toolbox: toolboxView.frame,
    });
    sizeView(boardView, scales.board);
    sizeView(toolboxView, scales.toolbox);
    return;
  }
  if (mode === "landscape") {
    // CSS sizes the stage to the small viewport (100svh), so Safari's bars collapsing changes nothing.
    const fit = Math.min((document.documentElement.clientWidth - header.offsetWidth) / FRAME_WIDTH, stage.clientHeight / FRAME_HEIGHT);
    scale = touchLayout.fillScale(fit, ratio);
  } else {
    const gutter = document.fullscreenElement ? 0 : GUTTER;
    scale = displayScale(stage.clientWidth - 2 * gutter, stage.clientHeight - 2 * gutter);
  }
  board.style.width = `${FRAME_WIDTH * scale}px`;
  board.style.height = `${FRAME_HEIGHT * scale}px`;
  drawText();
}

function safeInsets() {
  const style = getComputedStyle($("safe-probe"));
  return [style.paddingTop, style.paddingRight, style.paddingBottom, style.paddingLeft].map((v) => parseFloat(v) || 0);
}

// Sizes the modern canvas, on every layout: its CSS height (portrait, and a desktop window or a
// landscape phone too short for the card: stage_height), then its backing store to that CSS size × the render scale
// (spec D2), and tells the session (which ends any drag: the layout changed under it). Debounced
// (see RESIZE_SETTLE_MS) unless `now`, resizeNow is set, or the canvas has no size yet.
function sizeLightbox(now = false) {
  clearTimeout(resizeTimer);
  resizeTimer = 0;
  const immediate = now || resizeNow || lightboxSize === "";
  const touch = mode !== "desktop";
  const fullscreen = document.fullscreenElement !== null;
  const [top, right, bottom, left] = touchLayout.canvasInsets(mode, safeInsets(), fullscreen);
  // The desktop's full screen shows the stage alone, as wide as the screen.
  const width = mode === "desktop" && fullscreen ? stage.getBoundingClientRect().width : document.documentElement.clientWidth - (mode === "landscape" ? header.offsetWidth : 0);
  let height;
  if (mode === "portrait") {
    // The small viewport below the toolbar; never less than Stacked's minimum (the page scrolls
    // then, and only then).
    const room = smallViewport.getBoundingClientRect().height - header.getBoundingClientRect().height;
    height = stageHeight(width - left - right, room - top - bottom, touch) + top + bottom;
    stage.style.height = `${height}px`;
  } else if (mode === "desktop" && !fullscreen) {
    // The stage's own box (CSS fills the window below the toolbar with it), unless the window is
    // too short for the card: then the stage grows and the page scrolls.
    stage.style.minHeight = "";
    const room = stage.getBoundingClientRect().height;
    height = stageHeight(width - left - right, room - top - bottom, touch) + top + bottom;
    if (height > room) stage.style.minHeight = `${height}px`;
    else height = room;
  } else if (mode === "landscape" && !fullscreen) {
    // CSS sizes the stage to the small viewport (100svh). Beside fits every phone there, its
    // browser's bars shown or not (spec §6 "Revision: short landscape"); a canvas too small even
    // for that grows to stage_height, and the page scrolls by hand (ruling 19).
    stage.style.minHeight = "";
    const room = stage.getBoundingClientRect().height;
    height = stageHeight(width - left - right, room - top - bottom, touch) + top + bottom;
    if (height > room) stage.style.minHeight = `${height}px`;
    else height = room;
    document.documentElement.classList.toggle("stage-grown", height > room);
  } else {
    // Full screen: the stage's own box (the screen), never grown.
    stage.style.minHeight = "";
    document.documentElement.classList.remove("stage-grown");
    height = stage.getBoundingClientRect().height;
  }
  const scale = renderScale(width - left - right, height - top - bottom, window.devicePixelRatio || 1, touch);
  const backingWidth = Math.max(1, Math.round(width * scale));
  const backingHeight = Math.max(1, Math.round(height * scale));
  // A layout pass that changes nothing must not end a drag or repaint cold. The touch layout is
  // part of the size: the same pixels laid out for touch or not arrange differently.
  const size = `${backingWidth}x${backingHeight}@${scale}:${top},${right},${bottom},${left}:${touch}`;
  // The captions are placed for the last size: hidden while the canvas stretches to a new one,
  // shown again by the cold paint that places them anew (or when the size comes back). At once:
  // only the sheet's own opacity fades (style.css).
  const captions = $("captions");
  const pending = size !== lightboxSize && !immediate;
  const visibility = pending ? "hidden" : "";
  if (captions.style.visibility !== visibility) captions.style.visibility = visibility;
  if (size === lightboxSize) return;
  if (pending) {
    resizeTimer = setTimeout(() => {
      if (look === "modern") sizeLightbox(true);
    }, RESIZE_SETTLE_MS);
    return;
  }
  resizeNow = false;
  lightboxSize = size;
  pxPerCss = scale;
  if (lightbox.width !== backingWidth) lightbox.width = backingWidth;
  if (lightbox.height !== backingHeight) lightbox.height = backingHeight;
  lightboxImage = null;
  apply(session.resize(backingWidth, backingHeight, pxPerCss, [top, right, bottom, left], touch));
  // The new arrangement may or may not have a level picker on the canvas.
  updateLevelsButton();
  // Setting the size cleared the canvas: paint it now, not on the next frame, so it never shows
  // blank.
  paint();
}

// Lays out on the next frame, never inside a ResizeObserver callback: a size change made there
// is reported as a "ResizeObserver loop" error, which the page would show as a crash.
function requestLayout() {
  if (layoutPending) return;
  layoutPending = true;
  requestAnimationFrame(() => {
    layoutPending = false;
    layout();
  });
}

function watchPixelRatio() {
  matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`).addEventListener(
    "change",
    () => {
      resizeNow = true;
      layout();
      watchPixelRatio();
    },
    { once: true },
  );
}

// A client point in frame pixels (fractional; Rust floors) through the views on screen, measured
// at each event, so it holds wherever the page is scrolled.
function framePoint(clientX, clientY) {
  const live = views.map((view) => ({ frame: view.frame, box: view.canvas.getBoundingClientRect() }));
  return touchLayout.framePointAt(live, clientX, clientY);
}

// A client point in the game's pixels: the classic frame, or the modern canvas's backing px.
function gamePoint(clientX, clientY) {
  if (look !== "modern") return framePoint(clientX, clientY);
  // A stretched canvas waiting for its resize: resize now, so the point lands where it shows.
  if (resizeTimer) sizeLightbox(true);
  return touchLayout.lightboxPoint(lightbox.getBoundingClientRect(), { width: lightbox.width, height: lightbox.height }, clientX, clientY);
}

function isView(target) {
  return target === pixels || target === lightbox || target === boardPart || target === toolboxPart;
}

// Touch and pen presses are primary. While "Turn clockwise" is on, a touch or pen release that
// ends a tap (not a drag) is sent as the secondary button: the tap turns the piece clockwise, and
// a dragged piece still drops (the engine neither starts nor drops a drag on the secondary button).
// Over the level strip the game ignores the secondary button, so a tap there stays primary.
function pressButton(event) {
  return touchLayout.isHeldPointer(event.pointerType) ? 0 : event.button;
}

function releaseButton(event, x, y) {
  if (!touchLayout.isHeldPointer(event.pointerType)) return event.button;
  return clockwise && !session.dragging() && !session.on_strip(x, y) ? 2 : 0;
}

// A field that takes typed text. A <select> is not one: ⌘C/⌘V and the game's keys work on the game picker.
function isTextField(element) {
  return element instanceof Element && element.closest("input, textarea, [contenteditable]") !== null;
}

function isControl(element) {
  return element instanceof Element && element.closest("button, a, input, textarea, select, [contenteditable]") !== null;
}

function hasSelection() {
  const selection = window.getSelection();
  return selection !== null && !selection.isCollapsed;
}

function pasteSolution(solution) {
  const pasted = (apply(session.paste(solution)) & Flag.Redraw) !== 0;
  if (pasted) say("");
  else say("That is not a solution for this level.");
  track("paste_solution", { level: levelParams().level, success: pasted });
}

// The toolbar's Copy, and ⌘/Ctrl+C when the browser fired no copy event.
async function copyWithClipboardApi() {
  const solution = session.copy();
  if (solution === undefined) return;
  track("copy_solution", { level: levelParams().level });
  try {
    await navigator.clipboard.writeText(solution);
    say(`Copied ${solution}`);
  } catch {
    say(`The browser blocked the clipboard. Your solution: ${solution}`);
  }
}

// The toolbar's Paste, and ⌘/Ctrl+V when the browser fired no paste event.
async function pasteWithClipboardApi() {
  let solution;
  try {
    solution = await navigator.clipboard.readText();
  } catch {
    say("The browser did not allow reading the clipboard. Press ⌘V or Ctrl+V instead.");
    return;
  }
  pasteSolution(solution);
}

// ⌘/Ctrl+C and V. The document's copy and paste events come first: they fire in the same task
// as the keydown and clear the pending flag. Safari may fire neither when nothing on the page is
// selected or editable; then the Clipboard API takes over on the next task. One press is
// handled once either way. Returns whether the key was a clipboard shortcut.
function clipboardShortcut(event) {
  if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return false;
  const key = event.key.toLowerCase();
  if (key !== "c" && key !== "v") return false;
  if (event.repeat || isTextField(document.activeElement)) return true;
  if (key === "c") {
    if (hasSelection()) return true;
    copyPending = true;
    setTimeout(() => {
      if (!copyPending) return;
      copyPending = false;
      copyWithClipboardApi();
    }, 0);
  } else {
    pastePending = true;
    setTimeout(() => {
      if (!pastePending) return;
      pastePending = false;
      pasteWithClipboardApi();
    }, 0);
  }
  return true;
}

const actionsMenu = $("actions-menu");
const gameSelect = $("game-select");

// The keys a focused game picker keeps; every other key is the game's (the keydown listener).
function pickerKey(key) {
  return key === "Tab" || key === "Enter" || key === "Escape" || key.startsWith("Arrow");
}

function menuOpen() {
  return typeof actionsMenu.showPopover === "function" ? actionsMenu.matches(":popover-open") : !actionsMenu.hidden;
}

// The Actions menu sits under its button, or beside it when the toolbar runs down the side
// (landscape), always inside the viewport.
function placeMenu() {
  const button = $("actions").getBoundingClientRect();
  const beside = mode === "landscape";
  const left = beside ? button.right + 4 : button.left;
  const top = beside ? button.top : button.bottom + 4;
  const right = document.documentElement.clientWidth - actionsMenu.offsetWidth - 8;
  const bottom = window.innerHeight - actionsMenu.offsetHeight - 8;
  actionsMenu.style.left = `${Math.max(8, Math.min(left, right))}px`;
  actionsMenu.style.top = `${Math.max(8, Math.min(top, bottom))}px`;
}

function closeMenu() {
  if (typeof actionsMenu.hidePopover !== "function") actionsMenu.hidden = true;
  else if (actionsMenu.matches(":popover-open")) actionsMenu.hidePopover();
}

// A popover opened by its button's popovertarget (light dismiss, Escape); a browser without
// popovers toggles it as a plain panel with its own outside-click and Escape dismissal.
// Choosing an item closes it and gives the keys back to the game.
function wireMenu() {
  if (typeof actionsMenu.showPopover === "function") {
    // Placed once shown, when its size can be measured.
    actionsMenu.addEventListener("toggle", (event) => {
      if (event.newState === "open") placeMenu();
    });
  } else {
    actionsMenu.hidden = true;
    $("actions").addEventListener("click", () => {
      actionsMenu.hidden = !actionsMenu.hidden;
      if (!actionsMenu.hidden) placeMenu();
    });
    document.addEventListener("pointerdown", (event) => {
      if (!(event.target instanceof Element) || event.target.closest("#actions-menu, #actions")) return;
      closeMenu();
    });
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") closeMenu();
    });
  }
  // The menu is placed for one window size.
  window.addEventListener("resize", closeMenu);
  window.addEventListener("orientationchange", closeMenu);
  actionsMenu.addEventListener("click", (event) => {
    if (!(event.target instanceof Element) || !event.target.closest("button")) return;
    closeMenu();
    requestAnimationFrame(() => {
      if (isControl(document.activeElement)) document.activeElement.blur();
    });
  });
}

const helpDialog = $("help");
const levelsDialog = $("levels");
// What had the focus when a dialog opened; only a text field gets it back on close.
let dialogOpener = null;
// The pointer the stage has captured, so a dialog or a layout change can let go of it.
let capturedPointer = null;
// The primary pointer's press (touchLayout.startPress) until its release; null when none.
let press = null;
// Where the page was scrolled when `press` went down, and when ({scrollY, time}).
let pressStart = { scrollY: 0, time: 0 };

function dialogOpen() {
  return helpDialog.open || levelsDialog.open;
}

// Ends any drag as the app does on focus loss (the piece stays put) and lets go of the pointer.
function endDrag() {
  press = null;
  if (capturedPointer !== null) {
    try {
      stage.releasePointerCapture(capturedPointer);
    } catch {
      // The capture is already gone.
    }
    capturedPointer = null;
  }
  apply(session.pointer_cancel());
}

// Opens the help; `method` ("button", "key" or "first_visit") is how, for analytics.
function openHelp(method) {
  if (dialogOpen()) return;
  closeMenu();
  endDrag();
  dialogOpener = document.activeElement;
  helpDialog.showModal();
  track("tutorial_begin", { method });
}

// The ⌘ key on Apple platforms, Ctrl elsewhere.
function showPlatformKeys() {
  const platform = navigator.userAgentData?.platform || navigator.platform || "";
  if (/mac|iphone|ipad|ipod/i.test(platform)) return;
  for (const key of helpDialog.querySelectorAll("kbd.mod")) key.textContent = "Ctrl";
}

// Closes `dialog` with `closeButton`, or with a press and a click both on its backdrop.
function wireDialog(dialog, closeButton) {
  let backdropPress = false;
  closeButton.addEventListener("click", () => dialog.close());
  // The dialog has no padding of its own, so a click on it itself is a click on the backdrop.
  // Closes only when the press and the click both landed on the dialog itself, so a text
  // selection dragged from the sheet onto the backdrop does not.
  dialog.addEventListener("pointerdown", (event) => {
    backdropPress = event.target === dialog;
  });
  dialog.addEventListener("click", (event) => {
    const fromBackdrop = backdropPress;
    backdropPress = false;
    if (fromBackdrop && event.target === dialog) dialog.close();
  });
  dialog.addEventListener("close", () => {
    const opener = dialogOpener;
    dialogOpener = null;
    // Keys go back to the game: only a text field gets its focus back.
    if (opener instanceof HTMLElement && opener.isConnected && isTextField(opener)) opener.focus();
    else if (isControl(document.activeElement)) document.activeElement.blur();
  });
}

function wireHelp() {
  showPlatformKeys();
  $("help-open").addEventListener("click", () => openHelp("button"));
  wireDialog(helpDialog, $("help-close"));
}

// The Levels picker (portrait): one button per strip slot, showing the slot's number copied from
// the frame, so it has the strip's own colours. Each is {slot, button, context, frame}.
const levelButtons = [];
let levelWords;

function buildLevels() {
  levelWords = new Map([
    [LevelState.Current, "current"],
    [LevelState.Completed, "solved"],
    [LevelState.Unlocked, "open"],
    [LevelState.Locked, "locked"],
  ]);
  for (let slot = 0; ; slot += 1) {
    const rect = session.strip_number_rect(slot);
    if (rect.length === 0) break;
    const frame = touchLayout.rectOf(rect);
    const canvas = document.createElement("canvas");
    canvas.width = frame.right - frame.left;
    canvas.height = frame.bottom - frame.top;
    canvas.setAttribute("aria-hidden", "true");
    const button = document.createElement("button");
    button.type = "button";
    button.append(canvas);
    button.addEventListener("click", () => {
      // The current level stays as it is: choosing it only closes the picker.
      if (session.slot_state(slot) !== LevelState.Current) apply(session.open_slot(slot));
      levelsDialog.close();
    });
    $("level-grid").append(button);
    levelButtons.push({ slot, button, context: canvas.getContext("2d"), frame });
  }
}

function setAttributeIfChanged(element, name, value) {
  if (element.getAttribute(name) !== value) element.setAttribute(name, value);
}

// Each level's state and, from `image` (the frame) when given, its number.
function refreshLevels(image) {
  for (const { slot, button, context, frame } of levelButtons) {
    const state = session.slot_state(slot);
    const word = levelWords.get(state);
    const locked = state === LevelState.Locked;
    if (button.disabled !== locked) button.disabled = locked;
    setAttributeIfChanged(button, "aria-current", String(state === LevelState.Current));
    setAttributeIfChanged(button, "aria-label", `Level ${slot + 1}, ${word}`);
    setAttributeIfChanged(button, "data-state", word);
    if (image) {
      const { left, top, right, bottom } = frame;
      context.putImageData(image, -left, -top, left, top, right - left, bottom - top);
    }
  }
}

function openLevels() {
  if (dialogOpen()) return;
  closeMenu();
  endDrag();
  dialogOpener = document.activeElement;
  refreshLevels(framed ? frameImage() : null);
  levelsDialog.showModal();
  updateLevelsExpanded();
  levelButtons.find(({ button }) => button.getAttribute("aria-current") === "true")?.button.focus();
  track("open_levels", {});
}

function wireLevels() {
  buildLevels();
  // Modern: the sheet on the canvas (apply tracks open_levels); classic: the HTML dialog.
  $("levels-open").addEventListener("click", () => {
    if (look === "modern") apply(session.command("levels"));
    else openLevels();
  });
  wireDialog(levelsDialog, $("levels-close"));
  levelsDialog.addEventListener("close", updateLevelsExpanded);
}

// First visit only: shows the help once, if storage can say it has not been shown before.
function showHelpOnFirstVisit() {
  const seen = storage.read("chromatron.helpSeen");
  if (seen.ok && seen.value === null && storage.write("chromatron.helpSeen", "1")) openHelp("first_visit");
}

function saveAll() {
  for (const n of [...changed]) flush(n);
  rememberSlot();
}

function wire() {
  // A press that closes the Actions menu only closes it: it does not reach the game.
  let dismissPress = false;
  document.addEventListener(
    "pointerdown",
    (event) => {
      const outside = event.target instanceof Node && !actionsMenu.contains(event.target) && !$("actions").contains(event.target);
      dismissPress = menuOpen() && outside;
      if (dismissPress) closeMenu();
    },
    true,
  );
  // Presses start on a view (the frame, or a portrait part); the stage captures the pointer, so a
  // drag carries on across views and off them.
  stage.addEventListener("pointerdown", (event) => {
    if (!event.isPrimary || !isView(event.target) || dismissPress) return;
    event.preventDefault();
    // preventDefault keeps an old selection alive; clear it, or ⌘C would copy it instead.
    window.getSelection()?.removeAllRanges();
    if (isControl(document.activeElement)) document.activeElement.blur();
    stage.setPointerCapture(event.pointerId);
    capturedPointer = event.pointerId;
    // A touch or pen press waits for TOUCH_SLOP before it drags; the press itself is the real
    // point, so it picks up what is under the finger.
    press = touchLayout.startPress(event.pointerType, event.clientX, event.clientY);
    pressStart = { scrollY: window.scrollY, time: performance.now() };
    const [x, y] = gamePoint(event.clientX, event.clientY);
    // Modern (ruling 19): a touch or pen press with nothing under it, on a page taller than the
    // viewport, pans the page by hand (the canvas keeps touch-action: none, so a browser pan never
    // cancels a piece drag). It never reaches the game.
    if (look === "modern") {
      const pageTaller = document.documentElement.scrollHeight > window.innerHeight;
      press.pans = touchLayout.startsPan({ held: press.held, inert: session.inert(x, y), pageTaller });
      press.lastY = event.clientY;
      if (press.pans) return;
      // The open sheet on such a page (review M3): its chips are tapped, never dragged, so a touch
      // on it that leaves the slop pans the page too, and the rows below the screen come into view.
      press.pansPastSlop = press.held && pageTaller && session.sheet_open();
    }
    apply(session.pointer_down(x, y, pressButton(event), event.shiftKey, event.altKey, event.ctrlKey));
  });
  stage.addEventListener("pointermove", (event) => {
    if (press === null || dialogOpen() || !event.isPrimary || event.buttons === 0) return;
    if (press.pansPastSlop && !press.pans) {
      if (touchLayout.movePoint(press, event.clientX, event.clientY, false) === null) return;
      // The press on the sheet becomes a pan: it reaches nothing more (its release is dropped).
      press.pans = true;
      apply(session.pointer_cancel());
    }
    if (press.pans) {
      // Past the slop, the page follows the finger.
      if (touchLayout.movePoint(press, event.clientX, event.clientY, false) === null) return;
      window.scrollBy(0, press.lastY - event.clientY);
      press.lastY = event.clientY;
      return;
    }
    // A touch picks a piece up at the finger, so a piece near a region's top edge still starts a
    // drag; from then on it sends the point LIFT px above the finger, where the engine draws the piece.
    const wasDragging = session.dragging();
    const point = touchLayout.movePoint(press, event.clientX, event.clientY, wasDragging);
    if (point === null) return;
    const [x, y] = gamePoint(point[0], point[1]);
    apply(session.pointer_move(x, y));
    if (!wasDragging && session.dragging()) {
      // This move started the drag: lift the piece above the finger at once.
      const [liftedX, liftedY] = touchLayout.movePoint(press, event.clientX, event.clientY, true);
      if (liftedY !== point[1]) {
        const [fx, fy] = gamePoint(liftedX, liftedY);
        apply(session.pointer_move(fx, fy));
      }
    }
  });
  stage.addEventListener("pointerup", (event) => {
    if (!event.isPrimary) return;
    capturedPointer = null;
    const ended = press;
    press = null;
    // A pan sends nothing.
    if (ended === null || ended.pans || dialogOpen()) return;
    // A touch that moved without a piece turns nothing: its release is away from its press.
    const dragging = session.dragging();
    if (touchLayout.isSwipe(ended, dragging)) {
      apply(session.pointer_cancel());
      return;
    }
    // A drag drops under the lifted point, where the piece is drawn.
    const [releaseX, releaseY] = touchLayout.releasePoint(ended, event.clientX, event.clientY, dragging);
    const [x, y] = gamePoint(releaseX, releaseY);
    apply(session.pointer_up(x, y, releaseButton(event, x, y)));
  });
  stage.addEventListener("pointercancel", (event) => {
    if (!event.isPrimary) return;
    capturedPointer = null;
    const panned = press?.pans === true;
    press = null;
    if (!panned && !dialogOpen()) apply(session.pointer_cancel());
  });
  // The touch layouts (portrait, and a modern landscape grown to stage_height): a touch on the
  // game never scrolls the page (touch-action: none), but another finger on the texts or the
  // toolbar can. A real scroll under a live touch or pen press ends the drag as
  // on focus loss (the piece stays put), and that press's later moves and release reach nothing.
  // A fling still settling just after the press, and a scroll of a few pixels, leave it alone
  // (touchLayout.scrollEndsPress); the parts are measured at each event, so the drag stays true.
  // A modern pan scrolls the page itself, and goes on.
  window.addEventListener(
    "scroll",
    () => {
      if (press === null || press.pans || mode === "desktop") return;
      if (touchLayout.scrollEndsPress(press, pressStart, window.scrollY, performance.now())) endDrag();
    },
    { passive: true },
  );
  stage.addEventListener("contextmenu", (event) => {
    // A press on a view captures the pointer, so the event can target the stage itself: decide by
    // position too. The portrait texts, the toolbar and the dialogs keep the browser's menu.
    const [x, y] = gamePoint(event.clientX, event.clientY);
    const [offX, offY] = touchLayout.OFF_FRAME;
    if (isView(event.target) || x !== offX || y !== offY) event.preventDefault();
  });
  for (const view of [pixels, lightbox]) view.addEventListener(
    "wheel",
    (event) => {
      // Ctrl+wheel (and trackpad pinch) stays the browser's zoom.
      // Only the desktop layout has the wheel toggle; a touch layout never changes level by scrolling.
      if (mode !== "desktop" || !wheelOn || event.ctrlKey || dialogOpen()) return;
      // A modern window too short for the card grows the canvas and scrolls: the wheel scrolls it.
      if (look === "modern" && document.documentElement.scrollHeight > window.innerHeight) return;
      event.preventDefault();
      const inPixels = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL;
      apply(session.wheel(event.deltaX, event.deltaY, inPixels, event.timeStamp));
    },
    { passive: false },
  );

  document.addEventListener("keydown", (event) => {
    if (dialogOpen() || event.isComposing) return;
    // Escape closes the modern sheet wherever the focus is: after Tab to Levels and Enter, the
    // focus stays on the button, a control that keeps every other key.
    if (event.key === "Escape" && look === "modern" && session.sheet_open()) {
      event.preventDefault();
      apply(session.key("Escape", false, false, false));
      return;
    }
    // A focused game picker keeps only its own keys (pickerKey), so it never swallows Space, r, = or -.
    const control = isControl(event.target) && !(event.target === gameSelect && !pickerKey(event.key));
    if (event.key === "?" && !event.ctrlKey && !event.metaKey && !event.altKey && !control) {
      event.preventDefault();
      openHelp("key");
      return;
    }
    if (clipboardShortcut(event) || control) return;
    // No type-ahead: a typed letter on the picker would switch games.
    if (event.target === gameSelect && event.key.length === 1 && !event.ctrlKey && !event.metaKey) event.preventDefault();
    const altGraph = event.getModifierState("AltGraph");
    if (event.altKey && !altGraph) return;
    const ctrl = event.ctrlKey && !altGraph;
    // Consumed only for keys the game reads; every other key stays the browser's.
    const flags = apply(session.key(event.key, ctrl, event.metaKey, event.repeat));
    if (flags & Flag.Consumed) event.preventDefault();
  });
  document.addEventListener("copy", (event) => {
    copyPending = false;
    if (dialogOpen() || isTextField(document.activeElement) || hasSelection()) return;
    const solution = session.copy();
    if (solution === undefined || !event.clipboardData) return;
    event.clipboardData.setData("text/plain", solution);
    event.preventDefault();
    say(`Copied ${solution}`);
    track("copy_solution", { level: levelParams().level });
  });
  document.addEventListener("paste", (event) => {
    pastePending = false;
    if (dialogOpen() || isTextField(document.activeElement) || !event.clipboardData) return;
    event.preventDefault();
    pasteSolution(event.clipboardData.getData("text/plain"));
  });

  // The game picker gives the keys back to the game once it closes, chosen or not. Arrow keys
  // only open it: on a closed picker they would switch games at once on Windows and Linux.
  const releasePicker = () => requestAnimationFrame(() => gameSelect.blur());
  gameSelect.addEventListener("change", () => {
    const before = game;
    selectGame(Number(gameSelect.value));
    if (game !== before) track("select_game", {});
    releasePicker();
  });
  gameSelect.addEventListener("keyup", (event) => {
    if (event.key === "Enter" || event.key === "Escape") releasePicker();
  });
  gameSelect.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    try {
      gameSelect.showPicker();
    } catch {
      // No showPicker here, or no user activation: the arrow does nothing.
    }
  });
  for (const button of document.querySelectorAll("[data-command]")) {
    button.addEventListener("click", () => apply(session.command(button.dataset.command)));
  }
  // After a pointer click, give the keys back to the game (Space would press the button again).
  document.querySelector("header").addEventListener("click", (event) => {
    const button = event.target instanceof Element ? event.target.closest("button") : null;
    if (button && event.detail > 0) button.blur();
  });
  // The same command as the toolbar's Next.
  nextButton.addEventListener("click", (event) => {
    apply(session.command("next"));
    if (event.detail > 0) nextButton.blur();
  });
  $("copy").addEventListener("click", copyWithClipboardApi);
  $("paste").addEventListener("click", pasteWithClipboardApi);
  // Actions ▸ Classic look: the player's choice, stored (select_look carries the new look).
  $("classic-look").addEventListener("click", () => {
    setLook(look === "classic" ? "modern" : "classic", { store: true });
    track("select_look", {});
  });
  wireMenu();
  wireHelp();
  wireLevels();
  const wheelButton = $("wheel");
  wheelButton.setAttribute("aria-pressed", String(wheelOn));
  wheelButton.addEventListener("click", () => {
    wheelOn = !wheelOn;
    storage.write("chromatron.wheel", wheelOn ? "1" : "0");
    wheelButton.setAttribute("aria-pressed", String(wheelOn));
  });
  const clockwiseButton = $("clockwise");
  clockwiseButton.addEventListener("click", () => {
    clockwise = !clockwise;
    clockwiseWanted = clockwise;
    storage.write("chromatron.clockwise", clockwise ? "1" : "0");
    showClockwise();
  });
  const fullscreenButton = $("fullscreen");
  if (document.fullscreenEnabled) fullscreenButton.hidden = $("fullscreen-item").hidden = false;
  // A short modern landscape's Actions menu holds these two in the rail's place (style.css).
  $("clockwise-item").addEventListener("click", () => clockwiseButton.click());
  $("fullscreen-item").addEventListener("click", () => fullscreenButton.click());
  fullscreenButton.addEventListener("click", () => {
    if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
    else {
      // The desktop shows the game alone; a touch layout keeps its toolbar and parts.
      const target = mode === "desktop" ? stage : document.documentElement;
      target.requestFullscreen().catch(() => say("Full screen is not available here."));
    }
  });

  window.addEventListener("blur", () => {
    endDrag();
    saveAll();
  });
  window.addEventListener("pageshow", (event) => {
    if (event.persisted) for (const n of [...started]) storedElsewhere({ key: `chromatron${n}.save` });
  });
  window.addEventListener("pagehide", saveAll);
  window.addEventListener("storage", storedElsewhere);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") {
      endDrag();
      saveAll();
    } else if (look === "modern") {
      // Shown again: one paint at the time now settles any cue that ended while hidden, and
      // goes on with one still running (frames are never caught up).
      schedule();
    }
  });
  // Spec §9.3: a change of the setting settles a running animation (a repaint shows it settled).
  // Not apply(): this changes no game, so nothing is to be stored.
  reduceMotion.addEventListener("change", (event) => {
    if (session.set_reduced_motion(event.matches) & Flag.Redraw) schedule();
  });
  window.addEventListener("hashchange", () => {
    const n = gameFromHash();
    if (n !== undefined) selectGame(n);
  });
  frameView = { canvas: pixels, context: pixelContext, frame: touchLayout.rectOf([0, 0, FRAME_WIDTH, FRAME_HEIGHT]) };
  boardView = partView(boardPart, session.board_rect());
  toolboxView = partView(toolboxPart, session.toolbox_rect());
  new ResizeObserver(requestLayout).observe(stage);
  window.addEventListener("resize", requestLayout);
  coarsePointer.addEventListener("change", requestLayout);
  document.addEventListener("fullscreenchange", () => {
    resizeNow = true;
    requestLayout();
  });
  watchPixelRatio();
}

function showFatal(error) {
  console.error(error);
  for (const element of [board, boardPart, toolboxPart]) element.hidden = true;
  $("fatal").hidden = false;
}

async function start() {
  try {
    const glue = await import(`./chromatron_web.js?v=${BUILD}`);
    touchLayout = await import(`./touch_layout.js?v=${BUILD}`);
    wasm = await glue.default({
      module_or_path: new URL(`chromatron_web_bg.wasm?v=${BUILD}`, import.meta.url),
    });
    Flag = glue.Flag;
    LevelState = glue.LevelState;
    CaptionStyle = glue.CaptionStyle;
    CaptionRole = glue.CaptionRole;
    captionClasses = new Map([
      [CaptionStyle.Title, "cap-title"],
      [CaptionStyle.TitleCompact, "cap-title-compact"],
      [CaptionStyle.Body, "cap-body"],
      [CaptionStyle.Badge, "cap-badge"],
      [CaptionStyle.Credit, "cap-credit"],
      [CaptionStyle.Pill, "cap-pill"],
    ]);
    displayScale = glue.display_scale;
    Look = glue.Look;
    renderScale = glue.render_scale;
    stageHeight = glue.stage_height;
    session = new glue.Session();
    session.set_reduced_motion(reduceMotion.matches);
    wire();
    const stored = Number(storage.read("chromatron.game").value);
    // The look's styles from the start, so the other look never flashes.
    document.documentElement.dataset.look = wantedLook();
    selectGame(gameFromHash() ?? (GAMES.includes(stored) ? stored : 1));
    layout();
    showHelpOnFirstVisit();
  } catch (error) {
    showFatal(error);
    return;
  }
  // Analytics waits for the first paint: the frame callback runs before it, the timeout after.
  requestAnimationFrame(() => setTimeout(loadAnalytics, 0));
  // A Rust panic (a wasm trap) or a bug after start: say so instead of silently ignoring input.
  const stopped = () => say("Chromatron stopped working. Reload the page to continue.");
  window.addEventListener("error", stopped);
  window.addEventListener("unhandledrejection", stopped);
}

start();
