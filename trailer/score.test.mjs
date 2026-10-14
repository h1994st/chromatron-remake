// `just trailer-test`: the soundtrack's format, determinism, loudness and its sync to the cues.

import assert from "node:assert/strict";
import { test } from "node:test";
import { CEILING_DBTP, RATE, TARGET_LUFS, loudness, renderScore, truePeak, wavBytes } from "./score.mjs";
import { DURATION, END, cues } from "./timeline.mjs";

const score = renderScore();
const rms = ({ l, r }, from, to) => {
  let sum = 0;
  for (let i = Math.round(from * RATE); i < Math.round(to * RATE); i += 1) sum += (l[i] * l[i] + r[i] * r[i]) / 2;
  return 10 * Math.log10(sum / ((to - from) * RATE) + 1e-20);
};

test("a 48 kHz stereo 16-bit WAV of exactly the trailer's length", () => {
  assert.equal(score.l.length, DURATION * RATE);
  assert.equal(score.r.length, DURATION * RATE);
  const wav = wavBytes(score);
  assert.equal(wav.toString("ascii", 0, 4), "RIFF");
  assert.equal(wav.readUInt16LE(22), 2, "stereo");
  assert.equal(wav.readUInt32LE(24), 48000, "48 kHz");
  assert.equal(wav.readUInt16LE(34), 16, "16-bit");
  assert.equal(wav.readUInt32LE(40), DURATION * RATE * 4, "data size");
});

test("deterministic: a second render is identical", () => {
  assert.ok(wavBytes(renderScore()).equals(wavBytes(score)));
});

test("on the loudness target, under the true-peak ceiling", () => {
  assert.ok(Math.abs(loudness(score.l, score.r) - TARGET_LUFS) < 0.2, `${score.lufs} LUFS`);
  assert.ok(truePeak(score.l, score.r) <= CEILING_DBTP + 0.05, `${score.peak} dBTP`);
});

test("hits land on their cues, with a held breath before the final one", () => {
  for (const cue of cues.filter((one) => one.kind === "impact" || one.kind === "final")) {
    assert.ok(rms(score, cue.t, cue.t + 0.03) > rms(score, cue.t - 0.04, cue.t - 0.01) + 6, `${cue.kind} at ${cue.t} s`);
  }
  assert.ok(rms(score, END.hit - 0.38, END.hit - 0.01) < -80, "silence before the final hit");
  assert.ok(rms(score, DURATION - 0.01, DURATION) < -50, "faded out by the end");
});
