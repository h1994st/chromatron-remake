// Pure rules for the page's layouts and touch input, with no DOM access: `main.js` uses them in
// the browser and `just web-check` tests them in Node (crates/chromatron-web/tests/touch_layout_check.mjs).
// Rects are frame pixels {left, top, right, bottom}; boxes are CSS pixels {left, top, width, height},
// as getBoundingClientRect() reports them.

/** CSS pixels a touch or pen moves from its press before the page treats it as a drag. */
export const TOUCH_SLOP = 8;
/** CSS pixels a touch drag's point is lifted above the finger, so the piece shows above it. */
export const LIFT = 40;
/** A frame point every hit test misses; a piece dragged there is drawn wholly off the frame. */
export const OFF_FRAME = Object.freeze([-100, -100]);
/** Portrait: the smallest toolbox scale (36 px slots); when the screen is shorter, the page scrolls. */
export const TOOLBOX_MIN = 1.4;
/** Portrait: the largest toolbox scale (2x the original), so a tall screen keeps gutters beside it to swipe on. */
export const TOOLBOX_MAX = 2;
/** No view ever gets a smaller scale, so the game never vanishes. */
export const MIN_SCALE = 0.25;

/**
 * "desktop", "portrait" or "landscape". The touch layouts are for a coarse primary pointer and
 * follow the orientation. `forced` is the page's `?layout=` value, a debug and test hook: "touch"
 * forces a touch layout, "desktop" the desktop one; anything else is ignored.
 */
export function layoutMode({ forced, coarse, width, height }) {
  const touch = forced === "touch" || (forced !== "desktop" && coarse);
  if (!touch) return "desktop";
  return height > width ? "portrait" : "landscape";
}

/** A touch layout snaps to a sharp scale only when that loses less than this share of the fit. */
export const SNAP_LOSS = 0.03;

/**
 * The CSS scale for `fit` (the largest scale that fits) at `ratio` device pixels per CSS pixel: the
 * exact fit, so the game uses every pixel, unless the sharp step just below it (a whole number of
 * device pixels per frame pixel: thirds at 3×, halves at 2×) loses less than SNAP_LOSS of it. With
 * `image-rendering: pixelated`, an exact fit's uneven pixels are barely visible at 2× and up.
 * 1 for an empty or broken fit, so the game never vanishes.
 */
export function fillScale(fit, ratio) {
  if (!Number.isFinite(fit) || fit <= 0) return 1;
  const pixels = Number.isFinite(ratio) && ratio > 0 ? ratio : 1;
  const steps = Math.floor(fit * pixels + 1e-9);
  const sharp = steps / pixels;
  return steps >= 1 && sharp >= fit * (1 - SNAP_LOSS) ? sharp : fit;
}

const spanX = (rect) => rect.right - rect.left;
const spanY = (rect) => rect.bottom - rect.top;

/**
 * Portrait: fill scales for the board and the toolbox stacked in `height` CSS px (the small
 * viewport below the toolbar, less the gaps) and `width` wide. The board takes the full width,
 * whatever the height. The toolbox takes the height left, up to the board's width, and at least
 * TOOLBOX_MIN (unless the board is narrower than that), and at most TOOLBOX_MAX: on a screen too
 * short for it, the page scrolls further, and on a wide one the toolbox stays centred with gutters.
 */
export function portraitScales({ width, height, ratio, board, toolbox }) {
  const boardScale = fillScale(Math.max(MIN_SCALE, width / spanX(board)), ratio);
  const widthFit = (spanX(board) * boardScale) / spanX(toolbox);
  const heightFit = (height - boardScale * spanY(board)) / spanY(toolbox);
  const exact = Math.max(MIN_SCALE, Math.min(widthFit, Math.max(TOOLBOX_MIN, heightFit), TOOLBOX_MAX));
  const sharp = fillScale(exact, ratio);
  // A snap never takes the toolbox under TOOLBOX_MIN or over TOOLBOX_MAX; then the exact fit stays.
  return { board: boardScale, toolbox: sharp >= TOOLBOX_MIN && sharp <= TOOLBOX_MAX ? sharp : exact };
}

/** `[left, top, right, bottom]`, as Session returns rects, as a rect. */
export function rectOf([left, top, right, bottom]) {
  return { left, top, right, bottom };
}

/** The part of rect `a` inside rect `b`, or null when they do not overlap. */
export function intersect(a, b) {
  const left = Math.max(a.left, b.left);
  const top = Math.max(a.top, b.top);
  const right = Math.min(a.right, b.right);
  const bottom = Math.min(a.bottom, b.bottom);
  return left < right && top < bottom ? { left, top, right, bottom } : null;
}

/** The look to show: `forced` (?look=) if it names one, else the stored choice, else modern. */
export function lookFrom({ forced, stored }) {
  if (forced === "classic" || forced === "modern") return forced;
  return stored === "classic" ? "classic" : "modern";
}

/**
 * The safe-area insets [top, right, bottom, left] the modern canvas itself must keep clear: the
 * toolbar already covers the top (desktop, portrait) or the left (the landscape rail); in full
 * screen the canvas is the whole screen.
 */
export function canvasInsets(mode, [top, right, bottom, left], fullscreen) {
  if (fullscreen) return [top, right, bottom, left];
  return mode === "landscape" ? [top, right, bottom, 0] : [0, right, bottom, left];
}

/**
 * Client point (`clientX`, `clientY`) in the modern canvas's backing pixels (fractional), through
 * its on-screen `box` and its `backing` size ({width, height}): backing px per CSS px are
 * `backing.width / box.width` across and `backing.height / box.height` down. OFF_FRAME for an
 * empty box or backing.
 */
export function lightboxPoint(box, backing, clientX, clientY) {
  if (!(box.width > 0 && box.height > 0) || !(backing.width > 0 && backing.height > 0)) return [...OFF_FRAME];
  return [((clientX - box.left) * backing.width) / box.width, ((clientY - box.top) * backing.height) / box.height];
}

/**
 * Whether a press pans the page instead of reaching the game: a touch or pen press (`held`) on a
 * point with nothing to press (`inert`), on a page taller than the viewport (`pageTaller`),
 * scrolls the page by hand. The canvas keeps `touch-action: none`, so piece drags are never
 * cancelled by a browser pan.
 */
export function startsPan({ held, inert, pageTaller }) {
  return held && inert && pageTaller;
}

/** Client point (`clientX`, `clientY`) in frame pixels (fractional) through `view` ({frame, box}); OFF_FRAME for an empty box. */
export function toFrame({ frame, box }, clientX, clientY) {
  if (!(box.width > 0 && box.height > 0)) return [...OFF_FRAME];
  return [
    frame.left + ((clientX - box.left) * spanX(frame)) / box.width,
    frame.top + ((clientY - box.top) * spanY(frame)) / box.height,
  ];
}

function contains(box, x, y) {
  return x >= box.left && x < box.left + box.width && y >= box.top && y < box.top + box.height;
}

/**
 * A client point in frame pixels through the views on screen. One view (the whole frame) maps
 * every point, outside ones beyond the frame, as the desktop page always has. With several
 * (portrait), a point maps through the view it is over, and a point between them is OFF_FRAME.
 */
export function framePointAt(views, clientX, clientY) {
  if (views.length === 1) return toFrame(views[0], clientX, clientY);
  const view = views.find((candidate) => contains(candidate.box, clientX, clientY));
  return view === undefined ? [...OFF_FRAME] : toFrame(view, clientX, clientY);
}

/**
 * Whether `pointerType` is a touch or a pen, whose presses wait for TOUCH_SLOP. Anything else
 * ("mouse", "" or a type this page does not know) is handled like a mouse.
 */
export function isHeldPointer(pointerType) {
  return pointerType === "touch" || pointerType === "pen";
}

/** A primary pointer going down at client (x, y). Touch and pen presses wait for TOUCH_SLOP. */
export function startPress(pointerType, x, y) {
  const held = isHeldPointer(pointerType);
  return { pointerType, held, x, y, moved: !held };
}

/**
 * The client point to send for a move of `press`, or null while a touch or pen is still within
 * TOUCH_SLOP of where it went down. From then on every move counts. A touch's point is the finger
 * until the engine is `dragging`, so a piece near a region's top edge still picks up, and then
 * LIFT px above the finger.
 */
export function movePoint(press, x, y, dragging) {
  if (!press.moved) {
    if (Math.hypot(x - press.x, y - press.y) < TOUCH_SLOP) return null;
    press.moved = true;
  }
  return press.pointerType === "touch" && dragging ? [x, y - LIFT] : [x, y];
}

/** Portrait: ms after a press during which page scrolls are ignored (a fling still settling). */
export const SCROLL_GRACE = 150;
/** Portrait: CSS px the page may scroll from where it was at a press before the press ends. */
export const SCROLL_SLOP = 8;

/**
 * Whether a page scroll to `scrollY` at time `now` (ms) ends `press`, which went down at `start`
 * ({scrollY, time}). Only a touch or pen press ends, after SCROLL_GRACE, and only once the page has
 * moved more than SCROLL_SLOP from where it was at the press. A mouse press never ends this way.
 */
export function scrollEndsPress(press, start, scrollY, now) {
  return press.held && now - start.time >= SCROLL_GRACE && Math.abs(scrollY - start.scrollY) > SCROLL_SLOP;
}

/** The client point to send for the release: a tap releases where it went down, a drag where its moves went. */
export function releasePoint(press, x, y, dragging) {
  return press.moved ? movePoint(press, x, y, dragging) : [press.x, press.y];
}

/**
 * Whether a release ends a touch or pen swipe that carried no piece: the page cancels it, so it
 * turns nothing (its release point is away from where the finger went down).
 */
export function isSwipe(press, dragging) {
  return press.held && press.moved && !dragging;
}
