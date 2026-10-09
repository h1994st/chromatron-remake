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

const $ = (id) => document.getElementById(id);
const stage = $("stage");
const board = $("game");
const pixels = $("pixels");
const text = $("text");
const pixelContext = pixels.getContext("2d");
const textContext = text.getContext("2d");

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
let clockwise = false;
let scale = 1;
let framePending = false;
let textBoxes = [];
let messageTimer = 0;
// A ⌘/Ctrl+C or V whose copy or paste event has not fired yet (see clipboardShortcut).
let copyPending = false;
let pastePending = false;

function say(message) {
  const element = $("message");
  element.textContent = message;
  clearTimeout(messageTimer);
  if (message) messageTimer = setTimeout(() => { element.textContent = ""; }, 6000);
}

function schedule() {
  if (!framePending) {
    framePending = true;
    requestAnimationFrame(paint);
  }
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
  if (flags & Flag.Slot) rememberSlot();
  if (flags & Flag.Redraw) schedule();
  return flags;
}

function gameFromHash() {
  const n = Number(location.hash.slice(1));
  return GAMES.includes(n) ? n : undefined;
}

function selectGame(n) {
  if (!GAMES.includes(n) || n === game) return;
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
  started.add(n);
  storage.write("chromatron.game", String(n));
  if (location.hash !== `#${n}`) history.replaceState(null, "", `#${n}`);
  document.title = session.title();
  for (const button of document.querySelectorAll("[data-game]")) {
    button.setAttribute("aria-current", String(Number(button.dataset.game) === n));
  }
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

function paint() {
  framePending = false;
  const count = session.render();
  if (count > 0) {
    const memory = wasm.memory.buffer;
    const rgba = new Uint8ClampedArray(memory, session.rgba_ptr(), FRAME_WIDTH * FRAME_HEIGHT * 4);
    const image = new ImageData(rgba, FRAME_WIDTH, FRAME_HEIGHT);
    const rects = new Int32Array(memory, session.dirty_ptr(), count * 4);
    for (let i = 0; i < count; i += 1) {
      const [left, top, right, bottom] = rects.subarray(i * 4, i * 4 + 4);
      pixelContext.putImageData(image, 0, 0, left, top, right - left, bottom - top);
    }
  }
  if (session.text_changed()) {
    textBoxes = [];
    for (let i = 0; i < session.text_count(); i += 1) {
      const [left, top, right, bottom] = session.text_rect(i);
      textBoxes.push({ text: session.text(i), left, top, right, bottom });
    }
    drawText();
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
  for (const box of textBoxes) {
    const lines = wrapLines(context, box.text, box.right - box.left);
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

function layout() {
  const gutter = document.fullscreenElement ? 0 : GUTTER;
  scale = displayScale(stage.clientWidth - 2 * gutter, stage.clientHeight - 2 * gutter);
  board.style.width = `${FRAME_WIDTH * scale}px`;
  board.style.height = `${FRAME_HEIGHT * scale}px`;
  drawText();
}

function watchPixelRatio() {
  matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`).addEventListener(
    "change",
    () => {
      layout();
      watchPixelRatio();
    },
    { once: true },
  );
}

// The pointer in frame pixels (fractional; Rust floors).
function framePoint(event) {
  const r = pixels.getBoundingClientRect();
  return [
    ((event.clientX - r.left) * FRAME_WIDTH) / r.width,
    ((event.clientY - r.top) * FRAME_HEIGHT) / r.height,
  ];
}

// Touch and pen presses are primary. While "Turn clockwise" is on, a touch or pen release that
// ends a tap (not a drag) is sent as the secondary button: the tap turns the piece clockwise, and
// a dragged piece still drops (the engine neither starts nor drops a drag on the secondary button).
// Over the level strip the game ignores the secondary button, so a tap there stays primary.
function pressButton(event) {
  return event.pointerType === "mouse" ? event.button : 0;
}

function releaseButton(event, x, y) {
  if (event.pointerType === "mouse") return event.button;
  return clockwise && !session.dragging() && !session.on_strip(x, y) ? 2 : 0;
}

function isTextField(element) {
  return element instanceof Element && element.closest("input, textarea, select, [contenteditable]") !== null;
}

function isControl(element) {
  return element instanceof Element && element.closest("button, a, input, textarea, select, [contenteditable]") !== null;
}

function hasSelection() {
  const selection = window.getSelection();
  return selection !== null && !selection.isCollapsed;
}

function pasteSolution(solution) {
  if (apply(session.paste(solution)) & Flag.Redraw) say("");
  else say("That is not a solution for this level.");
}

// The toolbar's Copy, and ⌘/Ctrl+C when the browser fired no copy event.
async function copyWithClipboardApi() {
  const solution = session.copy();
  if (solution === undefined) return;
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

const helpDialog = $("help");
// What had the focus when the help opened; only a text field gets it back on close.
let helpOpener = null;
let backdropPress = false;
// The pointer the canvas has captured, so the help can let go of it.
let capturedPointer = null;

function helpOpen() {
  return helpDialog.open;
}

function openHelp() {
  if (helpDialog.open) return;
  apply(session.pointer_cancel());
  helpOpener = document.activeElement;
  if (capturedPointer !== null) {
    try {
      pixels.releasePointerCapture(capturedPointer);
    } catch {
      // The capture is already gone.
    }
    capturedPointer = null;
  }
  helpDialog.showModal();
}

// The ⌘ key on Apple platforms, Ctrl elsewhere.
function showPlatformKeys() {
  const platform = navigator.userAgentData?.platform || navigator.platform || "";
  if (/mac|iphone|ipad|ipod/i.test(platform)) return;
  for (const key of helpDialog.querySelectorAll("kbd.mod")) key.textContent = "Ctrl";
}

function wireHelp() {
  showPlatformKeys();
  $("help-open").addEventListener("click", () => openHelp());
  $("help-close").addEventListener("click", () => helpDialog.close());
  // The dialog has no padding of its own, so a click on it itself is a click on the backdrop.
  // Closes only when the press and the click both landed on the dialog itself, so a text
  // selection dragged from the sheet onto the backdrop does not.
  helpDialog.addEventListener("pointerdown", (event) => {
    backdropPress = event.target === helpDialog;
  });
  helpDialog.addEventListener("click", (event) => {
    const press = backdropPress;
    backdropPress = false;
    if (press && event.target === helpDialog) helpDialog.close();
  });
  helpDialog.addEventListener("close", () => {
    const opener = helpOpener;
    helpOpener = null;
    // Keys go back to the game: only a text field gets its focus back.
    if (opener instanceof HTMLElement && opener.isConnected && isTextField(opener)) opener.focus();
    else if (isControl(document.activeElement)) document.activeElement.blur();
  });
}

// First visit only: shows the help once, if storage can say it has not been shown before.
function showHelpOnFirstVisit() {
  const seen = storage.read("chromatron.helpSeen");
  if (seen.ok && seen.value === null && storage.write("chromatron.helpSeen", "1")) openHelp();
}

function saveAll() {
  for (const n of [...changed]) flush(n);
  rememberSlot();
}

function wire() {
  pixels.addEventListener("pointerdown", (event) => {
    if (!event.isPrimary) return;
    event.preventDefault();
    // preventDefault keeps an old selection alive; clear it, or ⌘C would copy it instead.
    window.getSelection()?.removeAllRanges();
    if (isControl(document.activeElement)) document.activeElement.blur();
    pixels.setPointerCapture(event.pointerId);
    capturedPointer = event.pointerId;
    const [x, y] = framePoint(event);
    apply(session.pointer_down(x, y, pressButton(event), event.shiftKey, event.altKey, event.ctrlKey));
  });
  pixels.addEventListener("pointermove", (event) => {
    if (helpOpen() || !event.isPrimary || event.buttons === 0) return;
    const [x, y] = framePoint(event);
    apply(session.pointer_move(x, y));
  });
  pixels.addEventListener("pointerup", (event) => {
    capturedPointer = null;
    if (helpOpen() || !event.isPrimary) return;
    const [x, y] = framePoint(event);
    apply(session.pointer_up(x, y, releaseButton(event, x, y)));
  });
  pixels.addEventListener("pointercancel", (event) => {
    capturedPointer = null;
    if (!helpOpen() && event.isPrimary) apply(session.pointer_cancel());
  });
  pixels.addEventListener("contextmenu", (event) => event.preventDefault());
  pixels.addEventListener(
    "wheel",
    (event) => {
      // Ctrl+wheel (and trackpad pinch) stays the browser's zoom.
      if (!wheelOn || event.ctrlKey || helpOpen()) return;
      event.preventDefault();
      const inPixels = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL;
      apply(session.wheel(event.deltaX, event.deltaY, inPixels, event.timeStamp));
    },
    { passive: false },
  );

  document.addEventListener("keydown", (event) => {
    if (helpOpen() || event.isComposing) return;
    if (event.key === "?" && !event.ctrlKey && !event.metaKey && !event.altKey && !isControl(event.target)) {
      event.preventDefault();
      openHelp();
      return;
    }
    if (clipboardShortcut(event) || isControl(event.target)) return;
    const altGraph = event.getModifierState("AltGraph");
    if (event.altKey && !altGraph) return;
    const ctrl = event.ctrlKey && !altGraph;
    // Consumed only for keys the game reads; every other key stays the browser's.
    const flags = apply(session.key(event.key, ctrl, event.metaKey, event.repeat));
    if (flags & Flag.Consumed) event.preventDefault();
  });
  document.addEventListener("copy", (event) => {
    copyPending = false;
    if (helpOpen() || isTextField(document.activeElement) || hasSelection()) return;
    const solution = session.copy();
    if (solution === undefined || !event.clipboardData) return;
    event.clipboardData.setData("text/plain", solution);
    event.preventDefault();
    say(`Copied ${solution}`);
  });
  document.addEventListener("paste", (event) => {
    pastePending = false;
    if (helpOpen() || isTextField(document.activeElement) || !event.clipboardData) return;
    event.preventDefault();
    pasteSolution(event.clipboardData.getData("text/plain"));
  });

  for (const button of document.querySelectorAll("[data-game]")) {
    button.addEventListener("click", () => selectGame(Number(button.dataset.game)));
  }
  for (const button of document.querySelectorAll("[data-command]")) {
    button.addEventListener("click", () => apply(session.command(button.dataset.command)));
  }
  // After a pointer click, give the keys back to the game (Space would press the button again).
  document.querySelector("header").addEventListener("click", (event) => {
    const button = event.target instanceof Element ? event.target.closest("button") : null;
    if (button && event.detail > 0) button.blur();
  });
  $("copy").addEventListener("click", copyWithClipboardApi);
  $("paste").addEventListener("click", pasteWithClipboardApi);
  wireHelp();
  const wheelButton = $("wheel");
  wheelButton.setAttribute("aria-pressed", String(wheelOn));
  wheelButton.addEventListener("click", () => {
    wheelOn = !wheelOn;
    storage.write("chromatron.wheel", wheelOn ? "1" : "0");
    wheelButton.setAttribute("aria-pressed", String(wheelOn));
  });
  const clockwiseButton = $("clockwise");
  if (navigator.maxTouchPoints > 0 || matchMedia("(pointer: coarse)").matches) {
    clockwiseButton.hidden = false;
  }
  clockwiseButton.addEventListener("click", () => {
    clockwise = !clockwise;
    clockwiseButton.setAttribute("aria-pressed", String(clockwise));
  });
  const fullscreenButton = $("fullscreen");
  if (document.fullscreenEnabled) fullscreenButton.hidden = false;
  fullscreenButton.addEventListener("click", () => {
    if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
    else stage.requestFullscreen().catch(() => say("Full screen is not available here."));
  });

  window.addEventListener("blur", () => {
    apply(session.pointer_cancel());
    saveAll();
  });
  window.addEventListener("pageshow", (event) => {
    if (event.persisted) for (const n of [...started]) storedElsewhere({ key: `chromatron${n}.save` });
  });
  window.addEventListener("pagehide", saveAll);
  window.addEventListener("storage", storedElsewhere);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") {
      apply(session.pointer_cancel());
      saveAll();
    }
  });
  window.addEventListener("hashchange", () => {
    const n = gameFromHash();
    if (n !== undefined) selectGame(n);
  });
  new ResizeObserver(layout).observe(stage);
  watchPixelRatio();
}

function showFatal(error) {
  console.error(error);
  board.hidden = true;
  $("fatal").hidden = false;
}

async function start() {
  try {
    const glue = await import(`./chromatron_web.js?v=${BUILD}`);
    wasm = await glue.default({
      module_or_path: new URL(`chromatron_web_bg.wasm?v=${BUILD}`, import.meta.url),
    });
    Flag = glue.Flag;
    displayScale = glue.display_scale;
    session = new glue.Session();
    wire();
    const stored = Number(storage.read("chromatron.game").value);
    selectGame(gameFromHash() ?? (GAMES.includes(stored) ? stored : 1));
    layout();
    showHelpOnFirstVisit();
  } catch (error) {
    showFatal(error);
    return;
  }
  // A Rust panic (a wasm trap) or a bug after start: say so instead of silently ignoring input.
  const stopped = () => say("Chromatron stopped working. Reload the page to continue.");
  window.addEventListener("error", stopped);
  window.addEventListener("unhandledrejection", stopped);
}

start();
