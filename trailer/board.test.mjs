// `just trailer-test`: the trailer's beam tracer on small hand-made boards.

import assert from "node:assert/strict";
import { test } from "node:test";
import { BLUE, BOARDS, DARK, GREEN, RED, WHITE, beamChange, playback, targetAt, trace } from "./board.mjs";

const board = (w, h, ...pieces) => ({ w, h, pieces });

test("a beam runs straight to the board's edge", () => {
  const { segments } = trace(board(4, 1, { x: 0, y: 0, kind: "emitter", color: RED, dir: 0 }));
  assert.deepEqual(segments, [{ x0: 0, y0: 0, x1: 3.5, y1: 0, dir: 0, mask: RED, d0: 0, d1: 3.5 }]);
});

test("mirrors turn beams by their angle", () => {
  // East into "\" goes south; north into "/" goes east.
  const down = trace(board(3, 3, { x: 0, y: 0, kind: "emitter", color: RED, dir: 0 }, { x: 2, y: 0, kind: "mirror", angle: 1 }, { x: 2, y: 2, kind: "target", color: RED }));
  assert.ok(down.solved);
  const right = trace(board(3, 3, { x: 0, y: 2, kind: "emitter", color: GREEN, dir: 6 }, { x: 0, y: 0, kind: "mirror", angle: 3 }, { x: 2, y: 0, kind: "target", color: GREEN }));
  assert.ok(right.solved);
  // A beam along a mirror's line is blocked.
  const along = trace(board(3, 1, { x: 0, y: 0, kind: "emitter", color: RED, dir: 0 }, { x: 1, y: 0, kind: "mirror", angle: 0 }, { x: 2, y: 0, kind: "target", color: RED }));
  assert.ok(!along.solved);
});

test("a splitter sends one beam out two ways", () => {
  const { targets, bounces } = trace(
    board(3, 3, { x: 0, y: 1, kind: "emitter", color: BLUE, dir: 0 }, { x: 1, y: 1, kind: "splitter", dir: 0 }, { x: 1, y: 0, kind: "target", color: BLUE }, { x: 1, y: 2, kind: "target", color: BLUE }),
  );
  assert.deepEqual(
    targets.map((target) => target.lit),
    [true, true],
  );
  assert.deepEqual(bounces, [{ x: 1, y: 1, mask: BLUE }]);
  // From the side it is blocked.
  const side = trace(board(3, 2, { x: 1, y: 0, kind: "emitter", color: BLUE, dir: 2 }, { x: 1, y: 1, kind: "splitter", dir: 0 }));
  assert.equal(side.segments[0].y1, 0.5);
});

test("filters pass only their colours and targets add colours up", () => {
  const { targets } = trace(
    board(
      4,
      3,
      { x: 0, y: 1, kind: "emitter", color: WHITE, dir: 0 },
      { x: 1, y: 1, kind: "filter", color: RED },
      { x: 3, y: 1, kind: "target", color: RED | GREEN },
      { x: 3, y: 0, kind: "emitter", color: GREEN, dir: 2 },
    ),
  );
  // The green emitter's beam stops at the target: red from the left and green from above make yellow.
  assert.equal(targets[0].mask, RED | GREEN);
  assert.ok(targets[0].lit);
});

test("diagonal steps are √2 long and each channel is its own segment", () => {
  const { segments } = trace(board(3, 3, { x: 0, y: 2, kind: "emitter", color: RED | BLUE, dir: 7 }));
  assert.equal(segments.length, 2);
  for (const segment of segments) assert.ok(Math.abs(segment.d1 - 2.5 * Math.SQRT2) < 1e-9);
  assert.deepEqual(
    segments.map((segment) => segment.mask),
    [RED, BLUE],
  );
});

test("crossing beams of two colours are found", () => {
  const { crossings } = trace(board(3, 3, { x: 0, y: 1, kind: "emitter", color: RED, dir: 0 }, { x: 1, y: 0, kind: "emitter", color: BLUE, dir: 2 }));
  assert.deepEqual(crossings, [{ x: 1, y: 1, mask: RED | BLUE }]);
});

test("anti-parallel beams on one row keep their own directions and distances", () => {
  // Red runs east into the green emitter's face and green runs west into red's.
  const { segments } = trace(board(4, 1, { x: 0, y: 0, kind: "emitter", color: RED, dir: 0 }, { x: 3, y: 0, kind: "emitter", color: GREEN, dir: 4 }));
  assert.deepEqual(segments, [
    { x0: 0, y0: 0, x1: 2.5, y1: 0, dir: 0, mask: RED, d0: 0, d1: 2.5 },
    { x0: 3, y0: 0, x1: 0.5, y1: 0, dir: 4, mask: GREEN, d0: 0, d1: 2.5 },
  ]);
});

test("a splitter's branches carry on the distance from the emitter", () => {
  const { segments } = trace(board(3, 3, { x: 0, y: 1, kind: "emitter", color: RED, dir: 0 }, { x: 2, y: 1, kind: "splitter", dir: 0 }));
  assert.deepEqual(
    segments.map(({ x1, y1, d0, d1 }) => [x1, y1, d0, d1]),
    [
      [2, 1, 0, 2],
      [2, 2.5, 2, 3.5],
      [2, -0.5, 2, 3.5],
    ],
  );
});

test("a filter that takes every colour out ends the beam, and walls and emitters block", () => {
  const blank = trace(board(4, 1, { x: 0, y: 0, kind: "emitter", color: RED, dir: 0 }, { x: 2, y: 0, kind: "filter", color: GREEN }));
  assert.equal(blank.segments.length, 1);
  assert.equal(blank.segments[0].x1, 2);
  assert.deepEqual(blank.bounces, [{ x: 2, y: 0, mask: RED }]);
  for (const kind of ["wall", "emitter"]) {
    const blocked = trace(board(4, 1, { x: 0, y: 0, kind: "emitter", color: BLUE, dir: 0 }, { x: 2, y: 0, kind, color: RED, dir: 0 }));
    assert.equal(blocked.segments.find((segment) => segment.mask === BLUE).x1, 1.5, `a ${kind} stops the beam at its face`);
  }
});

test("a change grows new beams from where they part from the old ones", () => {
  const before = trace(board(4, 4, { x: 0, y: 0, kind: "emitter", color: RED, dir: 0 }));
  const after = trace(board(4, 4, { x: 0, y: 0, kind: "emitter", color: RED, dir: 0 }, { x: 2, y: 0, kind: "mirror", angle: 1 }));
  const change = beamChange(before, after);
  // The run to the mirror is old; the turn south is new from 2 cells on.
  assert.deepEqual(change.fresh, [Infinity, 2]);
  assert.equal(change.grow[RED], 2);
  // The old beam is gone past the mirror's cell: from 2 cells on.
  assert.deepEqual(change.stale, [2]);
  assert.equal(change.drain[RED], 2);
  // From dark, everything is new from the emitter.
  assert.deepEqual(beamChange(DARK, before).grow[RED], 0);
});

test("the trailer's puzzles play out as scripted", () => {
  const speed = 18;
  // A dragged mirror lights the red target after the beam has travelled from the mirror to it.
  const bend = playback(BOARDS.bend, [{ t: 1, kind: "drag", from: [1, 2], to: [4, 4], dur: 1 }], { start: 0, speed });
  assert.deepEqual(
    bend.states.map((state) => [state.cause, state.changed, state.traced.solved]),
    [
      ["start", true, false],
      ["grab", false, false],
      ["drop", true, true],
    ],
  );
  const [target] = bend.targets;
  const lit = target.changes.find((change) => change.mask === RED);
  assert.ok(Math.abs(lit.t - (2 + 3 / speed)) < 1e-9, "the front runs 3 cells from the mirror to the target");
  assert.equal(targetAt(target, 1.9).litAt, null);
  assert.equal(targetAt(target, 3).litAt, lit.t);
  // Each puzzle is solved by its moves, and the finale lights all four targets at once.
  const mix = playback(
    BOARDS.mix,
    [
      { t: 1, kind: "rotate", at: [1, 0] },
      { t: 2, kind: "rotate", at: [6, 6] },
    ],
    { start: 0, speed },
  );
  assert.ok(mix.states.at(-1).traced.solved);
  assert.ok(playback(BOARDS.split, [{ t: 1, kind: "rotate", at: [3, 2] }], { start: 0, speed }).states.at(-1).traced.solved);
  assert.ok(playback(BOARDS.filter, [{ t: 1, kind: "drag", from: [1, 4], to: [4, 3], dur: 0.5 }], { start: 0, speed }).states.at(-1).traced.solved);
  const solve = playback(BOARDS.solve, [{ t: 1, kind: "drag", from: [5, 4], to: [3, 3], dur: 0.5 }], { start: 0, speed });
  assert.ok(solve.states.at(-1).traced.solved);
  const litTimes = solve.targets.map((one) => one.changes.at(-1).t);
  assert.ok(litTimes.every((t) => Math.abs(t - litTimes[0]) < 1e-9));
  // Moves that come before the last change has settled are refused.
  assert.throws(() => playback(BOARDS.bend, [{ t: 0.05, kind: "drag", from: [1, 2], to: [4, 4], dur: 1 }], { start: 0, speed }), /settled/);
});

test("the hero board is solved", () => {
  const traced = trace(BOARDS.hero);
  assert.ok(traced.solved);
  assert.deepEqual(traced.crossings, [{ x: 2, y: 4, mask: RED | BLUE }]);
  // Twice, the same.
  assert.deepEqual(trace(BOARDS.hero), traced);
});
