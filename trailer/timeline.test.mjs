// `just trailer-test`: the storyboard's timing and the cues derived from it.

import assert from "node:assert/strict";
import { test } from "node:test";
import { BEAT, DURATION, PLAYS, SCENES, cues, frameTime, sceneAt, storyTime } from "./timeline.mjs";

const KINDS = new Set(["cut", "whoosh", "riser", "impact", "title", "final", "ignite", "beam", "grab", "drop", "rotate", "lit"]);

test("scenes tile the 30 s and cut on beats", () => {
  assert.equal(SCENES[0].start, 0);
  assert.equal(SCENES.at(-1).end, DURATION);
  for (let i = 1; i < SCENES.length; i += 1) {
    assert.equal(SCENES[i].start, SCENES[i - 1].end, `${SCENES[i].name} follows on`);
    assert.equal(SCENES[i].start % BEAT, 0, `${SCENES[i].name} starts on a beat`);
  }
  for (const scene of SCENES) {
    for (const shot of scene.shots ?? []) assert.equal(shot.start % BEAT, 0, `${scene.name}'s shot at ${shot.start} starts on a beat`);
  }
  assert.equal(sceneAt(29.999).name, "end");
});

test("every puzzle is solved by the end of its scene", () => {
  for (const [name, play] of Object.entries(PLAYS)) {
    const scene = SCENES.find((one) => one.name === name);
    const last = play.states.at(-1);
    assert.ok(last.traced.solved, `${name} is solved`);
    assert.ok(storyTime(scene, scene.end) > last.t + last.settle, `${name} settles before its cut`);
  }
});

test("speed ramps slow story time and never run it backwards", () => {
  const bend = SCENES.find((scene) => scene.name === "bend");
  let last = -Infinity;
  for (let t = bend.start; t <= bend.end; t += 1 / 240) {
    const tau = storyTime(bend, t);
    assert.ok(tau >= last, "story time is monotonic");
    last = tau;
  }
  assert.ok(storyTime(bend, bend.end) < bend.end, "the ramp leaves story time behind");
  assert.ok(Math.abs(storyTime(bend, frameTime(bend, 6.1)) - 6.1) < 1e-6, "frameTime inverts storyTime");
});

test("cues are complete, in order and inside the picture", () => {
  for (const cue of cues) {
    assert.ok(KINDS.has(cue.kind), `known kind ${cue.kind}`);
    assert.ok(cue.t >= 0 && cue.t < DURATION, `${cue.kind} at ${cue.t} is in the picture`);
    if (cue.kind === "riser" || cue.kind === "whoosh") assert.ok(cue.dur > 0 && cue.t + cue.dur <= DURATION, `${cue.kind} has a duration`);
    if (cue.kind === "lit") assert.ok(cue.colour, "a lit cue has a colour");
  }
  for (let i = 1; i < cues.length; i += 1) assert.ok(cues[i].t >= cues[i - 1].t, "in time order");
  const count = (kind) => cues.filter((cue) => cue.kind === kind).length;
  assert.equal(count("final"), 1);
  assert.equal(count("lit"), 10, "every target that lights");
  assert.equal(count("grab"), 3);
  assert.equal(count("rotate"), 3);
  for (const kind of ["ignite", "whoosh", "impact", "riser"]) assert.ok(count(kind) > 0, `some ${kind} cues`);
  // Every scene after the first opens on a cut.
  for (const scene of SCENES.slice(1)) assert.ok(cues.some((cue) => cue.kind === "cut" && cue.t === scene.start), `a cut at ${scene.start}`);
});
