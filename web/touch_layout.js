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
/** Portrait: CSS pixels the texts keep at least (three lines); longer texts scroll in their box. */
export const TEXT_ROOM = 72;
/** Portrait: the smallest toolbox scale (36 px slots); below it the board shrinks instead. */
export const TOOLBOX_MIN = 1.4;
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
 * Portrait: fill scales for the board and the toolbox stacked in `height` CSS px (what the texts
 * leave) and at most `width` wide. The board takes the full width and the toolbox the height left,
 * up to the board's width. When that would leave the toolbox under TOOLBOX_MIN, the board shrinks
 * instead, so the page never has to scroll.
 */
export function portraitScales({ width, height, ratio, board, toolbox }) {
  let boardFit = width / spanX(board);
  if ((height - boardFit * spanY(board)) / spanY(toolbox) < TOOLBOX_MIN) {
    boardFit = Math.max(MIN_SCALE, (height - TOOLBOX_MIN * spanY(toolbox)) / spanY(board));
  }
  const boardScale = fillScale(boardFit, ratio);
  const toolboxFit = Math.min((spanX(board) * boardScale) / spanX(toolbox), (height - boardScale * spanY(board)) / spanY(toolbox));
  const exact = Math.max(MIN_SCALE, toolboxFit);
  const sharp = fillScale(exact, ratio);
  // A snap never takes the toolbox under TOOLBOX_MIN; then the exact fit stays.
  return { board: boardScale, toolbox: sharp >= TOOLBOX_MIN ? sharp : exact };
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
