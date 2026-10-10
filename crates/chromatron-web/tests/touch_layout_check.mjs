// `just web-check`: the page's pure layout and touch rules (web/touch_layout.js), in Node.
// Usage: node touch_layout_check.mjs <a directory holding touch_layout.js>

import assert from "node:assert/strict";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [dir] = process.argv.slice(2);
assert.ok(dir, "usage: node touch_layout_check.mjs <a directory holding touch_layout.js>");
const t = await import(pathToFileURL(join(dir, "touch_layout.js")).href);
const near = (actual, expected, message) =>
  assert.ok(Math.abs(actual - expected) < 1e-9, `${message}: ${actual}, expected ${expected}`);

// Which layout.
const mode = (forced, coarse, width, height) => t.layoutMode({ forced, coarse, width, height });
assert.equal(mode(null, false, 1400, 1000), "desktop");
assert.equal(mode(null, false, 390, 844), "desktop", "a narrow window with a mouse");
assert.equal(mode(null, true, 390, 844), "portrait");
assert.equal(mode(null, true, 844, 390), "landscape");
assert.equal(mode(null, true, 600, 600), "landscape", "a square screen");
assert.equal(mode("touch", false, 390, 844), "portrait", "?layout=touch with a mouse");
assert.equal(mode("touch", false, 1400, 1000), "landscape");
assert.equal(mode("desktop", true, 390, 844), "desktop", "?layout=desktop on a phone");
assert.equal(mode("phone", false, 390, 844), "desktop", "an unknown value is ignored");

// Fill: the exact fit, unless a sharp step (scale × devicePixelRatio whole) loses under 3 %.
assert.equal(t.SNAP_LOSS, 0.03);
near(t.fillScale(390 / 360, 3), 390 / 360, "a 360 px board fills a 390 px phone at 3x (sharp 1x would lose 8 %)");
near(t.fillScale(390 / 480, 3), 390 / 480, "landscape at 3x stays exact (sharp 2/3 would lose 18 %)");
near(t.fillScale(390 / 480, 2), 390 / 480, "landscape at 2x stays exact (sharp 1/2 would lose 38 %)");
near(t.fillScale(2.5, 3), 2.5, "7/3 would lose 6.7 %");
near(t.fillScale(1.02, 3), 1, "a sharp step 2 % below snaps");
near(t.fillScale(0.68, 3), 2 / 3, "a third step 2 % below snaps");
near(t.fillScale(1.02, 1), 1, "whole steps at 1x snap too");
near(t.fillScale(1, 3), 1, "an exact fit on a step stays");
near(t.fillScale(1.5, 1.25), 1.5, "a fractional ratio: 0.8 would lose 47 %");
near(t.fillScale(0.2, 2), 0.2, "below one device pixel per frame pixel: the exact fit");
near(t.fillScale(1.5, 0), 1.5, "a broken ratio counts as 1x");
for (const broken of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
  assert.equal(t.fillScale(broken, 2), 1, `fit ${broken}`);
}

// Portrait scales (lumen's BOARD_RECT and TOOLBOX_RECT).
const board = t.rectOf([48, 18, 408, 378]);
const toolbox = t.rectOf([446, 6, 604, 112]);
assert.equal(t.TEXT_ROOM, undefined, "the texts no longer take a share of the screen");
assert.equal(t.TOOLBOX_MIN, 1.4);
assert.equal(t.TOOLBOX_MAX, 2);
// A 390 × 844 phone: 763 px below a 45 px toolbar and 36 px of gaps.
const phone = t.portraitScales({ width: 390, height: 763, ratio: 3, board, toolbox });
near(phone.board, 390 / 360, "the board fills a 390 px phone: 26 px cells");
near(phone.toolbox, t.TOOLBOX_MAX, "the toolbox capped at 2x, not as wide as the board");
near(158 * phone.toolbox, 316, "316 px wide");
assert.ok((390 - 158 * phone.toolbox) / 2 >= 30, "gutters of 30 px or more each side");
// The same phone with its browser bars shown: 583 px.
const bars = t.portraitScales({ width: 390, height: 583, ratio: 3, board, toolbox });
near(bars.board, 390 / 360, "the board still fills the width");
near(bars.toolbox, (583 - 390) / 106, "the toolbox fills the height left (1.82x; the 5/3 step would lose 8 %)");
// Too short for a 1.4x toolbox below the full-width board: the toolbox keeps 1.4x, the page scrolls.
const short = t.portraitScales({ width: 390, height: 500, ratio: 3, board, toolbox });
near(short.board, 390 / 360, "a short screen's board still fills the width");
near(short.toolbox, 1.4, "36 px toolbox slots");
const tablet = t.portraitScales({ width: 820, height: 900, ratio: 2, board, toolbox });
near(tablet.board, 820 / 360, "a tablet's board fills its width");
near(tablet.toolbox, 1.4, "its toolbox keeps 1.4x below it");
const roomy = t.portraitScales({ width: 820, height: 1099, ratio: 2, board, toolbox });
near(roomy.toolbox, t.TOOLBOX_MAX, "a tall tablet's toolbox is capped at 2x");
const tiny = t.portraitScales({ width: 390, height: 60, ratio: 3, board, toolbox });
assert.deepEqual(tiny, short, "no height at all: the same as a short screen");
const narrow = t.portraitScales({ width: 60, height: 60, ratio: 3, board, toolbox });
near(narrow.board, t.MIN_SCALE, "a board too narrow for the screen: still visible");
near(narrow.toolbox, (360 * t.MIN_SCALE) / 158, "and the toolbox no wider than it");

// The cap holds for every ratio and height, and a snap never goes over it.
for (const ratio of [1, 2, 3, 13])
  for (let height = 0; height <= 1500; height += 7) {
    const r = t.portraitScales({ width: 390, height, ratio, board, toolbox });
    assert.ok(r.toolbox <= t.TOOLBOX_MAX + 1e-9 && r.toolbox >= t.TOOLBOX_MIN - 1e-9, `${ratio}x ${height}: ${r.toolbox}`);
  }

// A snap never takes the toolbox under TOOLBOX_MIN (13 px per CSS px: 18/13 = 1.385 would snap from 1.42).
{
  const boardScale = 19 / 13;
  const height = boardScale * 360 + 1.42 * 106;
  const kept = t.portraitScales({ width: 360 * boardScale, height, ratio: 13, board, toolbox });
  near(kept.board, boardScale, "the board is on a sharp step");
  near(kept.toolbox, 1.42, "the exact fit, not the 1.385 snap");
  assert.ok(kept.toolbox >= t.TOOLBOX_MIN);
}

// Rects.
assert.deepEqual(t.intersect(t.rectOf([430, 0, 490, 50]), toolbox), t.rectOf([446, 6, 490, 50]));
assert.equal(t.intersect(t.rectOf([88, 88, 113, 113]), toolbox), null);
assert.deepEqual(t.intersect(t.rectOf([0, 0, 640, 480]), board), board);
assert.equal(t.intersect(t.rectOf([408, 0, 446, 480]), board), null, "rects that only touch do not overlap");

// Client points to frame points.
const full = { frame: t.rectOf([0, 0, 640, 480]), box: { left: 8, top: 50, width: 1280, height: 960 } };
assert.deepEqual(t.framePointAt([full], 8, 50), [0, 0]);
assert.deepEqual(t.framePointAt([full], 1287, 1009), [639.5, 479.5]);
assert.deepEqual(t.framePointAt([full], 0, 40), [-4, -5], "one view maps points outside it beyond the frame");
const portrait = [
  { frame: board, box: { left: 15, top: 60, width: 360, height: 360 } },
  { frame: toolbox, box: { left: 37, top: 432, width: 316, height: 212 } },
];
assert.deepEqual(t.framePointAt(portrait, 15, 60), [48, 18], "the board's top-left");
assert.deepEqual(t.framePointAt(portrait, 315, 240), [348, 198]);
assert.deepEqual(t.framePointAt(portrait, 65, 460), [460, 20], "toolbox slot 0's centre at 2x");
assert.deepEqual(t.framePointAt(portrait, 200, 425), [...t.OFF_FRAME], "between the views");
assert.deepEqual(t.framePointAt(portrait, 375, 100), [...t.OFF_FRAME], "a box's right edge is outside it");

assert.deepEqual(t.framePointAt([], 10, 10), [...t.OFF_FRAME], "no views");
assert.deepEqual(t.framePointAt([{ frame: board, box: { left: 0, top: 0, width: 0, height: 0 } }], 0, 0), [...t.OFF_FRAME], "an empty box");
assert.deepEqual(t.toFrame({ frame: board, box: { left: 0, top: 0, width: 360, height: 0 } }, 5, 5), [...t.OFF_FRAME]);
assert.deepEqual(t.framePointAt(portrait, 100, 90 - t.LIFT), [...t.OFF_FRAME], "a lifted point above the board's top edge is off the frame");
// Frame -> screen -> frame at fractional scales.
for (const [bs, ts] of [[1.0833, 2.468], [0.7321, 1.4], [2.5, 3.1]]) {
  const views = [
    { frame: board, box: { left: 11.5, top: 60.25, width: 360 * bs, height: 360 * bs } },
    { frame: toolbox, box: { left: 20.75, top: 70.25 + 360 * bs, width: 158 * ts, height: 106 * ts } },
  ];
  for (const [view, scale, [fx, fy]] of [[views[0], bs, [123.4, 200.7]], [views[0], bs, [48.5, 18.5]], [views[1], ts, [460.25, 20.5]], [views[1], ts, [600, 100]]]) {
    const cx = view.box.left + (fx - view.frame.left) * scale;
    const cy = view.box.top + (fy - view.frame.top) * scale;
    const [x, y] = t.framePointAt(views, cx, cy);
    near(x, fx, "round trip x");
    near(y, fy, "round trip y");
  }
}

// Touch tolerance and the lifted drag.
assert.equal(t.TOUCH_SLOP, 8);
assert.equal(t.LIFT, 40);
const mouse = t.startPress("mouse", 100, 100);
assert.deepEqual(t.movePoint(mouse, 101, 100, false), [101, 100], "a mouse moves at once, unlifted");
assert.deepEqual(t.movePoint(mouse, 102, 100, true), [102, 100], "and stays unlifted while dragging");
assert.deepEqual(t.releasePoint(mouse, 102, 103, true), [102, 103]);
assert.equal(t.isSwipe(mouse, false), false, "a mouse release always reaches the game");
assert.deepEqual(t.movePoint(t.startPress("", 0, 0), 1, 0, false), [1, 0], "an unknown pointer is a mouse");
for (const type of ["", "unknown"]) {
  const other = t.startPress(type, 100, 100);
  assert.deepEqual(t.movePoint(other, 120, 130, true), [120, 130], `a "${type}" pointer moves at once, unlifted`);
  assert.deepEqual(t.releasePoint(other, 121, 131, true), [121, 131]);
  assert.equal(t.isSwipe(other, false), false, `a "${type}" release reaches the game, as a mouse's does`);
  assert.equal(t.isHeldPointer(type), false);
}
assert.equal(t.isHeldPointer("touch") && t.isHeldPointer("pen") && !t.isHeldPointer("mouse"), true, "only touch and pen are held");

const tap = t.startPress("touch", 100, 100);
assert.equal(t.movePoint(tap, 105, 104, false), null, "within the slop: held back");
assert.equal(t.movePoint(tap, 100, 107.9, false), null);
assert.deepEqual(t.releasePoint(tap, 106, 103, false), [100, 100], "a tap releases where it went down");
assert.equal(t.isSwipe(tap, false), false);

// The press and the moves before the engine drags are the finger itself, so a piece near a
// region's top edge picks up; once it drags, the point is lifted.
const drag = t.startPress("touch", 100, 100);
assert.deepEqual([drag.x, drag.y], [100, 100], "the press is the finger");
assert.equal(t.movePoint(drag, 104, 104, false), null);
assert.deepEqual(t.movePoint(drag, 100, 108, false), [100, 108], "past the slop, not dragging yet: the finger");
assert.deepEqual(t.movePoint(drag, 92, 100, false), [92, 100], "a sideways move before the drag starts: still the finger");
assert.deepEqual(t.movePoint(drag, 90, 100, true), [90, 100 - t.LIFT], "dragging: lifted");
assert.deepEqual(t.movePoint(drag, 101, 101, true), [101, 101 - t.LIFT], "then every move counts");
assert.deepEqual(t.releasePoint(drag, 150, 300, true), [150, 300 - t.LIFT], "the drop lands under the piece");
assert.deepEqual(t.releasePoint(drag, 150, 300, false), [150, 300], "a release without a drag is not lifted");
assert.equal(t.isSwipe(drag, true), false, "a drag that carries a piece drops it");
assert.equal(t.isSwipe(drag, false), true, "a moved touch without a piece is cancelled");

const edge = t.startPress("touch", 100, 100);
assert.deepEqual(t.movePoint(edge, 108, 100, false), [108, 100], "exactly the slop is a move");
assert.deepEqual(t.releasePoint(edge, 108, 100, true), [108, 100 - t.LIFT], "exactly the slop is a drag");
assert.equal(t.isSwipe(edge, false), true);

const pen = t.startPress("pen", 10, 10);
assert.equal(t.movePoint(pen, 14, 10, false), null, "a pen waits for the slop too");
assert.deepEqual(t.movePoint(pen, 30, 10, true), [30, 10], "but is not lifted");

// A page scroll mid-press (portrait).
assert.equal(t.SCROLL_GRACE, 150);
assert.equal(t.SCROLL_SLOP, 8);
const pressed = t.startPress("touch", 0, 0);
const at = { scrollY: 100, time: 1000 };
assert.equal(t.scrollEndsPress(pressed, at, 160, 1100), false, "a fling settling right after the press is ignored");
assert.equal(t.scrollEndsPress(pressed, at, 160, 1150), true, "after the grace, a real scroll ends the press");
assert.equal(t.scrollEndsPress(pressed, at, 108, 2000), false, "a scroll within the slop does not");
assert.equal(t.scrollEndsPress(pressed, at, 91, 2000), true, "the slop holds both ways");
assert.equal(t.scrollEndsPress(t.startPress("pen", 0, 0), at, 160, 2000), true, "a pen press ends too");
assert.equal(t.scrollEndsPress(t.startPress("mouse", 0, 0), at, 160, 2000), false, "a mouse press never does");

console.log("ok: touch layout rules");
