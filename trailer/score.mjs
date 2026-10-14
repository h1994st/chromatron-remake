// The trailer's original soundtrack and sound effects, synthesised in plain Node and written as a
// 48 kHz stereo 16-bit WAV of exactly DURATION seconds. Scored like a film trailer to the picture's
// cues in timeline.mjs (the same data the picture is drawn from, so sound and picture cannot
// drift): an atmospheric opening, a riser into a braam on the title, pulses that tighten scene by
// scene (kick and sub at puzzle 1, hats and clap at puzzle 2, an arpeggio over the montage), a fuller
// section for the facts and platforms, a riser that cuts to silence, and the final hit with a long
// tail fading out by the end. A minor, 120 BPM where the picture's cuts use the grid; chords change
// on the picture's events (a target lighting resolves to the tonic) rather than strictly on bars.
//
//   node score.mjs <out.wav>
//
// Deterministic: noise and phases come from PRNGs seeded by name, and no clock or Math.random is
// read. Oscillators are band-limited (PolyBLEP); every envelope has an attack and release of at
// least 2 ms, so no note starts or ends with a click. The master is high-passed, glued by a gentle
// compressor, normalised to TARGET_LUFS (ITU BS.1770 integrated loudness, measured here) and held
// under CEILING_DBTP by a look-ahead limiter on 4× oversampled (true) peaks.

import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { clamp, prng } from "./draw.mjs";
import { BAR, BEAT, DURATION, END, FACTS, INTRO, PLATFORMS, START, cues, sceneAt, storyTime } from "./timeline.mjs";

export const RATE = 48000;
export const TARGET_LUFS = -14;
// Under the −1 dBTP target with room for the AAC encode, which adds a few tenths.
export const CEILING_DBTP = -2;
const LENGTH = DURATION * RATE;
const TAU = 2 * Math.PI;

const sample = (t) => Math.round(t * RATE);
const hz = (midi) => 440 * 2 ** ((midi - 69) / 12);
const cents = (c) => 2 ** (c / 1200);
const dB = (x) => 20 * Math.log10(x);

// --- Building blocks ---------------------------------------------------------------------------------

// A PRNG seeded by a name (its FNV-1a hash into draw.mjs's mulberry32), so each sound's noise is
// fixed and independent of the others: adding a sound never changes another's.
function rng(name) {
  let a = 2166136261;
  for (let i = 0; i < name.length; i += 1) a = Math.imul(a ^ name.charCodeAt(i), 16777619);
  return prng(a);
}

// The PolyBLEP correction for a step at phase 0 of an oscillator advancing dt per sample.
function blep(p, dt) {
  if (p < dt) {
    p /= dt;
    return p + p - p * p - 1;
  }
  if (p > 1 - dt) {
    p = (p - 1) / dt;
    return p * p + p + p + 1;
  }
  return 0;
}

// A band-limited sawtooth (and, with `pulse`, a square) whose frequency may change every sample.
class Osc {
  constructor(phase = 0) {
    this.p = phase;
  }
  saw(f) {
    const dt = f / RATE;
    const y = 2 * this.p - 1 - blep(this.p, dt);
    this.p += dt;
    if (this.p >= 1) this.p -= 1;
    return y;
  }
  square(f) {
    const dt = f / RATE;
    const q = this.p + 0.5 >= 1 ? this.p - 0.5 : this.p + 0.5;
    const y = (this.p < 0.5 ? 1 : -1) + blep(this.p, dt) - blep(q, dt);
    this.p += dt;
    if (this.p >= 1) this.p -= 1;
    return y;
  }
}

// An RBJ-cookbook biquad in transposed direct form II; `set` may be called while running (the
// state is kept), which is how the sweeps work.
class Biquad {
  constructor(type, f, q) {
    this.z1 = 0;
    this.z2 = 0;
    if (type) this.set(type, f, q);
  }
  set(type, f, q = Math.SQRT1_2) {
    const w = (TAU * Math.min(f, RATE * 0.46)) / RATE;
    const cos = Math.cos(w);
    const alpha = Math.sin(w) / (2 * q);
    let b0;
    let b1;
    let b2;
    if (type === "lp") {
      b1 = 1 - cos;
      b0 = b1 / 2;
      b2 = b0;
    } else if (type === "hp") {
      b1 = -(1 + cos);
      b0 = (1 + cos) / 2;
      b2 = b0;
    } else {
      // "bp": 0 dB at the centre.
      b0 = alpha;
      b1 = 0;
      b2 = -alpha;
    }
    const a0 = 1 + alpha;
    this.b0 = b0 / a0;
    this.b1 = b1 / a0;
    this.b2 = b2 / a0;
    this.a1 = (-2 * cos) / a0;
    this.a2 = (1 - alpha) / a0;
    return this;
  }
  run(x) {
    const y = this.b0 * x + this.z1;
    this.z1 = this.b1 * x - this.a1 * y + this.z2;
    this.z2 = this.b2 * x - this.a2 * y;
    return y;
  }
}

// A note's gain u s after it starts, held for `hold` s: a raised-cosine attack over `a`, an
// exponential decay with time constant `decay` towards `sustain`, then a release over `r` that lands
// softly on zero. Attack and release are at least 2 ms.
function env(u, hold, a, decay, sustain, r) {
  if (u <= 0) return 0;
  a = Math.max(a, 0.002);
  r = Math.max(r, 0.002);
  const level = (w) => (w < a ? 0.5 - 0.5 * Math.cos((Math.PI * w) / a) : sustain + (1 - sustain) * Math.exp(-(w - a) / decay));
  if (u <= hold) return level(u);
  const k = (u - hold) / r;
  return k >= 1 ? 0 : level(hold) * (1 - k) ** 2;
}

// A piecewise-linear automation curve through [[t, value], …], held flat outside its points.
function curve(points) {
  return (t) => {
    if (t <= points[0][0]) return points[0][1];
    for (let k = 1; k < points.length; k += 1) {
      if (t < points[k][0]) {
        const [t0, v0] = points[k - 1];
        const [t1, v1] = points[k];
        return v0 + ((v1 - v0) * (t - t0)) / (t1 - t0);
      }
    }
    return points[points.length - 1][1];
  };
}

// Control-rate (1 kHz) tables of a function of time, read back per sample with interpolation.
const CONTROL = 1000;
function control(fn) {
  const table = new Float32Array(DURATION * CONTROL + 2);
  for (let j = 0; j < table.length; j += 1) table[j] = fn(j / CONTROL);
  return table;
}
function read(table, i) {
  const x = (i * CONTROL) / RATE;
  const j = x | 0;
  return table[j] + (table[j + 1] - table[j]) * (x - j);
}

const bus = () => ({ l: new Float32Array(LENGTH), r: new Float32Array(LENGTH) });
const pans = (pan) => [Math.cos(((pan + 1) * Math.PI) / 4), Math.sin(((pan + 1) * Math.PI) / 4)];

// Renders a mono sound into `out` at `t0` for `len` s, panned: fn(u) gives its sample u s in.
function voice(out, t0, len, pan, fn) {
  const [gl, gr] = pans(pan);
  const i0 = sample(t0);
  const n = sample(len);
  for (let k = Math.max(0, -i0); k < n && i0 + k < LENGTH; k += 1) {
    const x = fn(k / RATE);
    out.l[i0 + k] += x * gl;
    out.r[i0 + k] += x * gr;
  }
}
// The same for a stereo sound: fn(u, lr) fills lr with the left and right samples.
function voice2(out, t0, len, fn) {
  const lr = [0, 0];
  const i0 = sample(t0);
  const n = sample(len);
  for (let k = Math.max(0, -i0); k < n && i0 + k < LENGTH; k += 1) {
    fn(k / RATE, lr);
    out.l[i0 + k] += lr[0];
    out.r[i0 + k] += lr[1];
  }
}

// Times from `from` (inclusive) to `to` (exclusive) every `step` s.
function steps(from, to, step) {
  const out = [];
  for (let k = 0; from + k * step < to - 1e-9; k += 1) out.push(Math.round((from + k * step) * 1e6) / 1e6);
  return out;
}

// --- Harmony -----------------------------------------------------------------------------------------

// Chords as [root pitch class, third]: A minor and its relatives, with E major as the dominant.
const CHORDS = { Am: [9, 3], F: [5, 4], C: [0, 4], G: [7, 4], E: [4, 4] };
const cueAt = (kind, scene, colour) => cues.find((cue) => cue.kind === kind && cue.scene === scene && (!colour || cue.colour === colour)).t;
const IMPACTS = cues.filter((cue) => cue.kind === "impact").map((cue) => cue.t);

// The music stops here, a held breath before the final hit.
const SILENCE = END.hit - 0.45;

// The chord from each time on. It moves on the picture's events: the mix scene's white target and
// the montage finale resolve to A minor, the facts counter rises on G and lands on A minor, and the
// platforms build through G and E (the dominant) into the final hit.
const HARMONY = [
  [INTRO.impact, "Am"],
  [START.bend + BAR, "F"],
  [START["mix-title"], "C"],
  [cueAt("lit", "mix", "yellow"), "F"],
  [cueAt("lit", "mix", "white"), "Am"],
  [START.filter, "F"],
  [START.solve, "G"],
  [cueAt("lit", "solve", "white"), "Am"],
  [START.facts + BAR / 2, "F"],
  [FACTS.count, "G"],
  [FACTS.counted, "Am"],
  [START.platforms, "F"],
  [START.end - BAR, "G"],
  [START.end - BAR / 2, "E"],
  [END.hit, "Am"],
];
function chordAt(t) {
  let name = "Am";
  for (const [from, chord] of HARMONY) if (t >= from - 1e-9) name = chord;
  return name;
}
const triad = (name) => {
  const [root, third] = CHORDS[name];
  return [root, (root + third) % 12, (root + 7) % 12];
};
// The MIDI note of pitch class `pc` in [lo, lo + 12).
const place = (pc, lo) => lo + ((((pc - lo) % 12) + 12) % 12);

// A target's chime, pitched to its colour: red, green and blue are A, C and E, the notes of the
// tonic triad, with a fifth above each, so mixed colours sound their notes together and white is
// the whole A minor (add 9) chord.
const CHIMES = {
  red: [81, 88],
  green: [84, 91],
  blue: [88, 95],
  yellow: [81, 84, 88, 91],
  magenta: [81, 88, 95],
  cyan: [84, 88, 91, 95],
  white: [69, 76, 81, 84, 88, 95],
};

// Where the picture's speed ramps slow time: 1 at normal speed, down to the ramp's slowest. The
// music dips under a low-pass there, and the drums sit out.
function speedAt(t) {
  const scene = sceneAt(t);
  const h = 0.002;
  return (storyTime(scene, t + h) - storyTime(scene, t - h)) / (2 * h);
}

// --- Instruments ---------------------------------------------------------------------------------------

// Each group is a stereo bus, processed and sent to the reverb and delay at the mixdown.
const GROUPS = ["drone", "pad", "bass", "kick", "snare", "hat", "arp", "hits", "riser", "ui", "fx", "whoosh", "final"];

function instruments(G) {
  // Kick: a sine swept down from a punch to 44 Hz, with a short click, softly saturated.
  function kick(t0, vel = 1) {
    const noise = rng(`kick ${t0}`);
    const hp = new Biquad("hp", 1800, 0.7);
    let phase = 0;
    voice(G.kick, t0, 0.5, 0, (u) => {
      phase += (TAU * (44 + 115 * Math.exp(-u / 0.03) + 220 * Math.exp(-u / 0.0035))) / RATE;
      const body = Math.sin(phase) * env(u, 0.42, 0.002, 0.16, 0, 0.08);
      const click = hp.run(noise() * 2 - 1) * env(u, 0.004, 0.002, 0.004, 0, 0.004) * 0.5;
      return Math.tanh(1.6 * (body + click)) * vel;
    });
  }

  // Clap (or, with `tone`, a snare): three noise bursts and a tail through band-passes, wide.
  function clap(t0, vel = 1, { tone = 0, bright = 1 } = {}) {
    const nl = rng(`clap L ${t0}`);
    const nr = rng(`clap R ${t0}`);
    const bl = new Biquad("bp", 1250 * bright, 0.9);
    const br = new Biquad("bp", 1380 * bright, 0.9);
    const hl = new Biquad("hp", 450, 0.7);
    const hr = new Biquad("hp", 450, 0.7);
    let phase = 0;
    voice2(G.snare, t0, 0.32, (u, lr) => {
      let e = 0.8 * env(u - 0.03, 0.18, 0.003, 0.07, 0, 0.06);
      for (const offset of [0, 0.011, 0.023]) e += env(u - offset, 0.004, 0.002, 0.006, 0, 0.004);
      let body = 0;
      if (tone) {
        phase += (TAU * tone * (1 + 0.5 * Math.exp(-u / 0.01))) / RATE;
        body = Math.sin(phase) * env(u, 0.09, 0.002, 0.05, 0, 0.04) * 0.5;
      }
      lr[0] = (hl.run(bl.run(nl() * 2 - 1)) * 2.2 * e + body) * vel;
      lr[1] = (hr.run(br.run(nr() * 2 - 1)) * 2.2 * e + body) * vel;
    });
  }

  // Hats: high-passed noise with a resonant band on top; open ones ring longer.
  function hat(t0, vel = 1, { open = false, pan = 0 } = {}) {
    const noise = rng(`hat ${t0}`);
    const hp = new Biquad("hp", 7200, 0.8);
    const band = new Biquad("bp", 10500, 1.4);
    const hold = open ? 0.16 : 0.04;
    voice(G.hat, t0, hold + 0.04, pan, (u) => {
      const x = hp.run(noise() * 2 - 1);
      return (x * 0.6 + band.run(x)) * env(u, hold, 0.002, open ? 0.12 : 0.025, 0, 0.04) * vel;
    });
  }

  // Bass: a sine sub (saturated at the mixdown so phones hear its harmonics) and, from the
  // montage on, a plucked saw an octave up through a low-pass for definition.
  function bass(t0, len, midi, vel = 1, mid = 0) {
    const f = hz(midi);
    let phase = 0;
    voice(G.bass, t0, len + 0.03, 0, (u) => {
      phase += (TAU * f) / RATE;
      return Math.sin(phase) * env(u, len, 0.004, Infinity, 1, 0.03) * vel;
    });
    if (mid <= 0) return;
    const osc = new Osc(0.5);
    const lp = new Biquad();
    let k = 0;
    voice(G.bass, t0, len + 0.03, 0, (u) => {
      if (k++ % 16 === 0) lp.set("lp", 260 + 1100 * Math.exp(-u / 0.06), 1.6);
      return lp.run(osc.saw(2 * f)) * env(u, len, 0.003, 0.25, 0.4, 0.03) * vel * mid;
    });
  }

  // Pads: each chord tone as three detuned band-limited saws spread across the stereo field, with a
  // slow drift. The resonant low-pass and the sidechain ducking act on the whole group.
  function pad(t0, t1, name, attack, release) {
    const random = rng(`pad ${t0}`);
    const voicing = [place(CHORDS[name][0], 40), ...triad(name).map((pc) => place(pc, 55))];
    for (const midi of voicing) {
      for (const [detune, pan] of [
        [-9, -0.65],
        [0, 0],
        [8, 0.65],
      ]) {
        const osc = new Osc(random());
        const f = hz(midi) * cents(detune);
        const drift = random() * TAU;
        voice(G.pad, t0, t1 - t0 + release, pan, (u) => osc.saw(f * (1 + 0.0016 * Math.sin(TAU * 0.21 * u + drift))) * env(u, t1 - t0, attack, Infinity, 1, release) * 0.16);
      }
    }
  }

  // The opening drone: a low A as a sine and two detuned saws through a dark low-pass, breathing.
  function drone(level) {
    const a = new Osc(0.1);
    const b = new Osc(0.6);
    const lp = new Biquad("lp", 280, 1.2);
    let phase = 0;
    voice(G.drone, 0, 9.6, 0, (u) => {
      phase += (TAU * hz(33)) / RATE;
      const breath = 0.85 + 0.15 * Math.sin(TAU * 0.4 * u - Math.PI / 2);
      return (Math.sin(phase) * 0.7 + lp.run(a.saw(hz(45) * cents(-6)) + b.saw(hz(45) * cents(7))) * 0.5) * level(u) * breath;
    });
  }

  // The drifting spark before the ignition: sparse crackles, denser as it nears the ignition.
  function crackle(t0, t1) {
    const random = rng("crackle");
    let t = t0;
    for (;;) {
      t += -Math.log(1 - random()) / (5 + 30 * ((t - t0) / (t1 - t0)) ** 2);
      if (t >= t1) break;
      const noise = rng(`grain ${t}`);
      const hp = new Biquad("hp", 4500, 0.8);
      const gain = 0.25 + 0.6 * random() * ((t - t0) / (t1 - t0));
      voice(G.fx, t, 0.012, random() * 1.2 - 0.6, (u) => hp.run(noise() * 2 - 1) * env(u, 0.004, 0.002, 0.002, 0, 0.004) * gain);
    }
  }

  // Arpeggio pluck: a saw and a square through a low-pass that closes as the note decays.
  function pluck(t0, midi, vel, pan) {
    const osc = new Osc(0);
    const sub = new Osc(0.25);
    const lp = new Biquad();
    const f = hz(midi);
    let k = 0;
    voice(G.arp, t0, 0.2, pan, (u) => {
      if (k++ % 16 === 0) lp.set("lp", 700 + 3800 * vel * Math.exp(-u / 0.05), 2.2);
      return lp.run(osc.saw(f) * 0.7 + sub.square(f / 2) * 0.35) * env(u, 0.14, 0.002, 0.09, 0.1, 0.05) * vel;
    });
  }

  // --- Cinematic hits -------------------------------------------------------------------------------

  // A braam: a power chord of detuned saws that blats in a little flat, through a low-pass that
  // opens hard and slowly closes, driven into saturation.
  function braam(out, t0, root, { size = 1, minor = false, len = 2.6, decay = 0.75 } = {}) {
    const random = rng(`braam ${t0}`);
    const base = place(root, 40);
    const notes = [base, base + 7, base + 12, base + 19, ...(minor ? [base + 15] : [])];
    const left = [];
    const right = [];
    for (const midi of notes) {
      for (const detune of [-14, -5, 4, 13]) {
        left.push([new Osc(random()), hz(midi) * cents(detune)]);
        right.push([new Osc(random()), hz(midi) * cents(-detune * 0.9 + 1)]);
      }
    }
    const norm = 1 / Math.sqrt(left.length);
    const ll = new Biquad();
    const lr = new Biquad();
    let k = 0;
    voice2(out, t0, len, (u, out2) => {
      if (k++ % 16 === 0) {
        const open = u < 0.07 ? Math.sin(((u / 0.07) * Math.PI) / 2) : Math.exp(-(u - 0.07) / 0.6);
        ll.set("lp", 140 + 1600 * size * open, 1.3);
        lr.set("lp", (140 + 1600 * size * open) * 1.04, 1.3);
      }
      const bend = 2 ** ((-1 / 12) * Math.exp(-u / 0.03));
      let sl = 0;
      let sr = 0;
      for (const [osc, f] of left) sl += osc.saw(f * bend);
      for (const [osc, f] of right) sr += osc.saw(f * bend);
      const e = env(u, len - 0.4, 0.008, decay, 0, 0.4);
      out2[0] = Math.tanh(2.2 * ll.run(sl * norm)) * e * size;
      out2[1] = Math.tanh(2.2 * lr.run(sr * norm)) * e * size;
    });
  }

  // A sub drop: a sine gliding down from `from` to `to` Hz.
  function subDrop(out, t0, { from = 90, to = 30, glide = 1.1, len = 2.2, gain = 1 } = {}) {
    let phase = 0;
    voice(out, t0, len, 0, (u) => {
      phase += (TAU * (to + (from - to) * Math.exp(-u / (glide / 3)))) / RATE;
      return Math.tanh(1.4 * Math.sin(phase)) * env(u, len - 0.3, 0.003, 0.9, 0, 0.3) * gain;
    });
  }

  // A boom: a long, low kick.
  function boom(out, t0, gain, len = 1.6) {
    let phase = 0;
    voice(out, t0, len, 0, (u) => {
      phase += (TAU * (36 + 70 * Math.exp(-u / 0.05))) / RATE;
      return Math.sin(phase) * env(u, len - 0.3, 0.002, 0.45, 0, 0.3) * gain;
    });
  }

  // A crash: wide band-limited noise with a darkening tail.
  function crash(out, t0, gain, len = 1.6) {
    const nl = rng(`crash L ${t0}`);
    const nr = rng(`crash R ${t0}`);
    const hl = new Biquad("hp", 400, 0.7);
    const hr = new Biquad("hp", 400, 0.7);
    const ll = new Biquad();
    const lr = new Biquad();
    let k = 0;
    voice2(out, t0, len, (u, out2) => {
      if (k++ % 16 === 0) {
        ll.set("lp", 2800 + 5000 * Math.exp(-u / 0.25), 0.7);
        lr.set("lp", 2900 + 5000 * Math.exp(-u / 0.25), 0.7);
      }
      const e = env(u, len - 0.3, 0.002, 0.35, 0, 0.3) * gain;
      out2[0] = ll.run(hl.run(nl() * 2 - 1)) * e;
      out2[1] = lr.run(hr.run(nr() * 2 - 1)) * e;
    });
  }

  // An impact: braam, sub drop, boom and crash on the chord of the moment.
  function impact(out, t0, size, opts = {}) {
    braam(out, t0, CHORDS[chordAt(t0 + 0.01)][0], { size, ...opts });
    subDrop(out, t0, { gain: size, ...(opts.drop ?? {}) });
    boom(out, t0, size, opts.len ?? 1.6);
    crash(out, t0, 0.45 * size, opts.len ?? 1.6);
  }

  // A riser over [t0, t1]: noise through a band-pass sweeping up, and three detuned saws gliding
  // two octaves up under a tremolo that tightens, swelling to the end, where it stops dead.
  function riser(t0, t1, size = 1) {
    const len = t1 - t0;
    const nl = rng(`riser L ${t0}`);
    const nr = rng(`riser R ${t0}`);
    const random = rng(`riser ${t0}`);
    const bl = new Biquad();
    const br = new Biquad();
    const lp = new Biquad();
    const saws = [-10, 0, 9].map((detune) => [new Osc(random()), cents(detune)]);
    let tremolo = 0;
    let k = 0;
    voice2(G.riser, t0, len, (u, lr) => {
      const p = u / len;
      if (k++ % 16 === 0) {
        const fc = 250 * (7000 / 250) ** p;
        bl.set("bp", fc, 2.2);
        br.set("bp", fc * 1.08, 2.2);
        lp.set("lp", 500 + 5000 * p * p, 1);
      }
      const amp = p ** 2.2 * env(u, len - 0.012, 0.02, Infinity, 1, 0.012) * size;
      tremolo += (TAU * (3 + 17 * p * p)) / RATE;
      const f = hz(57 + 24 * p ** 1.5);
      let tone = 0;
      for (const [osc, detune] of saws) tone += osc.saw(f * detune);
      const pulse = lp.run(tone / 3) * (0.62 + 0.38 * Math.sin(tremolo)) * 0.45;
      lr[0] = (bl.run(nl() * 2 - 1) * 1.6 + pulse) * amp;
      lr[1] = (br.run(nr() * 2 - 1) * 1.6 + pulse) * amp;
    });
  }

  // --- Sound effects --------------------------------------------------------------------------------

  // A beam igniting: a snap, a whoomp of noise through a rising band-pass, a low thump, and the
  // beam's hum powering up from a fifth below onto the chord's root.
  function ignite(t0, size, hum) {
    const noise = rng(`ignite ${t0}`);
    const hp = new Biquad("hp", 3000, 0.7);
    voice(G.fx, t0, 0.02, 0, (u) => hp.run(noise() * 2 - 1) * env(u, 0.006, 0.002, 0.004, 0, 0.01) * 0.9 * size);
    const band = new Biquad();
    let k = 0;
    voice(G.whoosh, t0, 0.55, 0, (u) => {
      if (k++ % 16 === 0) band.set("bp", 250 * 12 ** Math.min(1, u / 0.3), 1.4);
      return band.run(noise() * 2 - 1) * env(u, 0.3, 0.01, 0.12, 0, 0.22) * 1.3 * size;
    });
    let thump = 0;
    voice(G.fx, t0, 0.3, 0, (u) => {
      thump += (TAU * (52 + 40 * Math.exp(-u / 0.03))) / RATE;
      return Math.sin(thump) * env(u, 0.2, 0.002, 0.08, 0, 0.1) * 0.8 * size;
    });
    const root = place(CHORDS[chordAt(t0 + 0.01)][0], 45);
    const a = new Osc(0.3);
    const b = new Osc(0.8);
    const tone = new Biquad("bp", 900, 0.8);
    let shimmer = 0;
    voice(G.fx, t0, hum + 0.3, 0, (u) => {
      const bend = 2 ** ((-7 / 12) * Math.exp(-u / 0.04));
      shimmer += (TAU * 6) / RATE;
      const x = tone.run(a.saw(hz(root) * bend) + 0.7 * b.saw(hz(root + 7) * bend * cents(4)));
      return x * (0.85 + 0.15 * Math.sin(shimmer)) * env(u, hum, 0.01, 0.5, 0.4, 0.3) * 0.7 * size;
    });
  }

  // A soft UI tick: a short sine blip with a little of its third harmonic.
  function tick(t0, f, gain, pan) {
    let phase = 0;
    voice(G.ui, t0, 0.07, pan, (u) => {
      phase += (TAU * f) / RATE;
      return (Math.sin(phase) + 0.25 * Math.sin(3 * phase)) * env(u, 0.05, 0.002, 0.012, 0, 0.02) * gain;
    });
  }
  function thud(t0, f, gain, len = 0.12) {
    let phase = 0;
    voice(G.ui, t0, len, 0, (u) => {
      phase += (TAU * f * (1 + 0.6 * Math.exp(-u / 0.01))) / RATE;
      return Math.sin(phase) * env(u, len - 0.04, 0.002, len / 4, 0, 0.04) * gain;
    });
  }

  // A beam rerouting: a quick rising zap and a hiss.
  function zap(t0) {
    const osc = new Osc(0);
    const band = new Biquad();
    const noise = rng(`zap ${t0}`);
    const hp = new Biquad("hp", 2600, 0.7);
    let k = 0;
    voice(G.fx, t0, 0.26, 0.1, (u) => {
      const f = 330 * 8 ** Math.min(1, u / 0.16);
      if (k++ % 16 === 0) band.set("bp", f * 1.5, 2);
      return (band.run(osc.saw(f)) * 1.4 + hp.run(noise() * 2 - 1) * 0.25) * env(u, 0.18, 0.003, 0.07, 0, 0.08);
    });
  }

  // A target lighting: a strummed FM chime chord of the colour's notes, plus a few sparkles for
  // its burst of particles.
  function chime(out, t0, notes, gain, { len = 1.8, sparkle = 7 } = {}) {
    const random = rng(`chime ${t0}`);
    const each = gain / Math.sqrt(notes.length);
    notes.forEach((midi, j) => {
      const f = hz(midi);
      let carrier = 0;
      let modulator = 0;
      const pan = notes.length > 1 ? (j / (notes.length - 1)) * 1.2 - 0.6 : 0;
      voice(out, t0 + j * 0.016, len, pan, (u) => {
        carrier += (TAU * f) / RATE;
        modulator += (TAU * 2 * f) / RATE;
        const index = 2.2 * Math.exp(-u / 0.15) + 0.2;
        const glint = Math.sin(4.2 * carrier) * Math.exp(-u / 0.05) * 0.2;
        return (Math.sin(carrier + index * Math.sin(modulator)) + glint) * env(u, len - 0.3, 0.003, len / 3.2, 0, 0.3) * each;
      });
    });
    for (let s = 0; s < sparkle; s += 1) {
      const f = hz([93, 96, 100, 105][Math.floor(random() * 4)]);
      let phase = 0;
      voice(out, t0 + 0.03 + random() * 0.42, 0.1, random() * 1.6 - 0.8, (u) => {
        phase += (TAU * f) / RATE;
        return Math.sin(phase) * env(u, 0.06, 0.002, 0.03, 0, 0.04) * gain * 0.12;
      });
    }
  }

  // A whoosh over `len` s peaking `peak` of the way through: wide noise through a band-pass that
  // rises and falls, with low air under it, panned left to right.
  function whoosh(t0, len, peak, gain, { from = 350, top = 2600, to = 700 } = {}) {
    const nl = rng(`whoosh L ${t0}`);
    const nr = rng(`whoosh R ${t0}`);
    const bl = new Biquad();
    const br = new Biquad();
    const air = new Biquad("lp", 380, 0.7);
    let k = 0;
    voice2(G.whoosh, t0, len, (u, lr) => {
      const p = u / len;
      if (k++ % 16 === 0) {
        const fc = p < peak ? from * (top / from) ** (p / peak) : top * (to / top) ** ((p - peak) / (1 - peak));
        bl.set("bp", fc, 1);
        br.set("bp", fc * 1.1, 1);
      }
      const shape = p < peak ? Math.sin(((p / peak) * Math.PI) / 2) ** 2 : Math.cos((((p - peak) / (1 - peak)) * Math.PI) / 2) ** 2;
      const [gl, gr] = pans(-0.7 + 1.4 * p);
      const low = air.run(nl() + nr() - 1) * 0.6;
      lr[0] = (bl.run(nl() * 2 - 1) * 1.6 + low) * shape * gl * gain;
      lr[1] = (br.run(nr() * 2 - 1) * 1.6 + low) * shape * gr * gain;
    });
  }

  // A title landing at t0, its lettering written from `delay` s later over `len` s: a soft low hit
  // and a single chime as it lands, then the beam writing it, a bright laser tone gliding up a
  // fifth and moving left to right with the lettering.
  function title(t0, delay, len) {
    const a = new Osc(0.2);
    const b = new Osc(0.7);
    const band = new Biquad();
    let vibrato = 0;
    let k = 0;
    voice2(G.fx, t0 + delay, len + 0.15, (u, lr) => {
      const p = clamp(u / len);
      const f = hz(76 + 5 * p * p) * (1 + 0.004 * Math.sin((vibrato += (TAU * 7) / RATE)));
      if (k++ % 16 === 0) band.set("bp", f * 2, 1.6);
      const x = band.run(a.saw(f) + b.saw(f * cents(9))) * env(u, len, 0.02, Infinity, 1, 0.15) * 1.3;
      const [pl, pr] = pans(-0.6 + 1.2 * p);
      lr[0] = x * pl;
      lr[1] = x * pr;
    });
    boom(G.hits, t0, 0.5, 0.8);
    chime(G.fx, t0, [81, 88], 0.55, { len: 1.4, sparkle: 0 });
  }

  return { kick, clap, hat, bass, pad, drone, crackle, pluck, braam, subDrop, boom, crash, impact, riser, ignite, tick, thud, zap, chime, whoosh, title };
}

// --- Arrangement ---------------------------------------------------------------------------------------

// The drums drop out for the last eighth before each impact after the intro, so the hit lands
// into a gap; and they sit out wherever a speed ramp slows time.
const DROPS = IMPACTS.filter((t) => t > 4).map((t) => [t - BEAT / 2, t]);
const dropped = (t) => DROPS.some(([a, b]) => t >= a - 1e-9 && t < b - 1e-9);
const slowed = (t) => speedAt(t) < 0.6;
const playing = (t) => !dropped(t) && !slowed(t) && t < SILENCE;

// The kicks, for both the drums and the sidechain. Pulses tighten: every second beat through the
// bend (and a pickup into its title), every beat from the mix to the end card.
const KICKS = [...steps(START.bend, START["mix-title"], 2 * BEAT), START["mix-title"] - BEAT, ...steps(START.mix, START.end, BEAT)].filter(playing).sort((a, b) => a - b);
// The end roll starts on the first beat of the end riser.
const ROLL = Math.ceil(PLATFORMS.riser / BEAT) * BEAT;

function arrange(G) {
  const I = instruments(G);

  // The opening: near silence, the drone breathing in, the spark crackling; the drone swells under
  // the riser, ducks under the title hit, and comes back for the second title card.
  const mixTitle = START["mix-title"];
  I.drone(curve([[0, 0], [0.3, 0.05], [INTRO.ignite, 0.09], [INTRO.impact, 0.5], [INTRO.impact + 0.3, 0.35], [START.bend, 0.2], [START.bend + 0.6, 0], [mixTitle - 0.1, 0], [mixTitle + 0.15, 0.3], [START.mix, 0.25], [START.mix + BEAT, 0]]));
  I.crackle(INTRO.spark, INTRO.ignite);

  // Pads from the title hit, chord by chord.
  const spans = HARMONY.map(([from, name], k) => [from, HARMONY[k + 1]?.[0] ?? DURATION, name]).filter(([from]) => from < SILENCE);
  for (const [from, to, name] of spans) I.pad(from, Math.min(to, SILENCE), name, from === INTRO.impact ? 0.8 : 0.3, 0.5);

  // Drums.
  for (const t of KICKS) I.kick(t, t < START.mix ? 0.75 : 1);
  for (const t of steps(START.mix + BEAT, PLATFORMS.riser, 2 * BEAT).filter(playing)) I.clap(t, t >= START.facts ? 1 : 0.85);
  for (const t of steps(START.mix + BEAT / 2, START.split, BEAT).filter(playing)) I.hat(t, 0.8, { pan: 0.2 });
  for (const t of steps(START.split, START.end, BEAT / 4).filter(playing)) {
    const step = Math.round((t % BEAT) / (BEAT / 4));
    const open = t >= START.facts && step === 2;
    I.hat(t, [0.35, 0.55, 1, 0.55][step] * (t >= START.facts ? 1 : 0.85), { open, pan: step % 2 ? -0.25 : 0.25 });
  }
  // The end roll: snares tightening from quarters to sixteenths to thirty-seconds, rising.
  const roll = [...steps(ROLL, START.end - BEAT, BEAT / 2), ...steps(START.end - BEAT, START.end, BEAT / 4), ...steps(START.end, SILENCE - 0.02, BEAT / 8)];
  for (const t of roll) {
    const p = (t - ROLL) / (SILENCE - ROLL);
    I.clap(t, 0.25 + 0.75 * p, { tone: 180 + 160 * p, bright: 1 + 0.6 * p });
  }

  // Bass: a heartbeat with the kick through the bend, eighths from the mix, sixteenths in the last
  // two beats of the build.
  const root = (t) => place(CHORDS[chordAt(t + 0.01)][0], 28);
  const last = START.end - 2 * BEAT;
  for (const t of KICKS.filter((t) => t < mixTitle)) I.bass(t, t % 1 ? 0.3 : 0.6, root(t), 0.75);
  for (const t of steps(START.mix, last, BEAT / 2).filter((t) => !dropped(t))) I.bass(t, 0.2, root(t), 1, t >= START.split ? (t >= START.facts ? 0.8 : 0.6) : 0);
  for (const t of steps(last, START.end, BEAT / 4)) I.bass(t, 0.1, root(t), 1, 0.9);

  // The arpeggio, over the montage and on through the facts and platforms.
  const PATTERN = [0, 1, 2, 3, 2, 1, 3, 1];
  for (const t of steps(START.split, START.end, BEAT / 4).filter((t) => !dropped(t))) {
    const name = chordAt(t + 0.001);
    const base = place(CHORDS[name][0], 64);
    const [, third] = CHORDS[name];
    const notes = [base, base + third, base + 7, base + 12];
    const step = Math.round(t / (BEAT / 4));
    const level = t < START.facts ? 0.7 : t < START.platforms ? 0.85 : 1;
    I.pluck(t, notes[PATTERN[step % 8]], (step % 4 === 0 ? 1 : 0.75) * level, step % 2 ? 0.3 : -0.3);
  }

  // The picture's cues. Lit targets that light together sound as one chord.
  const lit = new Map();
  for (const cue of cues) {
    const { t, kind } = cue;
    const shared = (other) => cues.some((c) => c.kind === other && Math.abs(c.t - t) < 1e-6);
    if (kind === "cut") I.whoosh(t - 0.24, 0.4, 0.6, shared("whoosh") ? 0.55 : 0.8);
    else if (kind === "whoosh") I.whoosh(t, cue.dur, 0.35, 0.5, { from: 250, top: 1600, to: 400 });
    // The end riser runs into the held breath before the final hit, not into the hit.
    else if (kind === "riser") I.riser(t, cue.scene === "platforms" ? SILENCE : t + cue.dur, cue.scene === "intro" ? 0.8 : 1);
    else if (kind === "impact") I.impact(G.hits, t, cue.scene === "intro" ? 1.1 : cue.scene === "facts" ? 1 : 0.85);
    else if (kind === "title") I.title(t, cue.delay, cue.dur);
    else if (kind === "final") {
      const len = DURATION - t;
      I.impact(G.final, t, 1.3, { minor: true, len, decay: 1.1, drop: { from: 110, to: 28, glide: 0.9, len } });
      I.chime(G.final, t, [57, 64, 69, 72, 76, 81], 0.6, { len, sparkle: 10 });
    } else if (kind === "ignite") {
      if (cue.scene === "intro") I.ignite(t, 1, INTRO.impact - t);
      else if (cue.scene === "end") I.ignite(t, 0.8, SILENCE - t - 0.3);
      else I.ignite(t, 0.7, 0.6);
    } else if (kind === "beam") I.zap(t);
    else if (kind === "grab") {
      I.tick(t, 2350, 0.8, -0.2);
      I.tick(t + 0.012, 3520, 0.35, -0.2);
    } else if (kind === "drop") {
      if (cue.scene === "facts") {
        // A card landing: a weightier thud and a tick.
        I.thud(t, 82, 0.9, 0.22);
        I.tick(t, 1760, 0.6, 0);
      } else {
        I.tick(t, 1560, 0.9, 0.2);
        I.thud(t, 170, 0.5);
      }
    } else if (kind === "rotate") {
      I.tick(t, 2000, 0.7, -0.1);
      I.tick(t + 0.05, 2670, 0.7, 0.1);
    } else if (kind === "lit") {
      const key = t.toFixed(6);
      if (!lit.has(key)) lit.set(key, new Set());
      for (const note of CHIMES[cue.colour]) lit.get(key).add(note);
      lit.get(key).count = (lit.get(key).count ?? 0) + 1;
    }
  }
  for (const [key, notes] of lit) I.chime(G.fx, Number(key), [...notes].sort((a, b) => a - b), Math.min(1.7, 1.1 + 0.2 * (notes.count - 1)));
}

// --- Mixdown and master --------------------------------------------------------------------------------

// The pads' low-pass, their level, and the sends, by section.
const PAD_CUTOFF = [
  [INTRO.impact, 350],
  [START["bend-title"], 600],
  [START.bend, 700],
  [START["mix-title"] - BEAT, 1100],
  [START["mix-title"], 650],
  [START.mix, 900],
  [START.split - 0.2, 1700],
  [START.split, 1500],
  [START.facts, 2400],
  [START.platforms, 2700],
  [PLATFORMS.riser, 2700],
  [SILENCE, 5500],
];
const PAD_LEVEL = [[INTRO.impact, 0.55], [START.bend, 0.65], [START["mix-title"], 0.55], [START.mix, 0.75], [START.split, 0.9], [START.facts, 1]];
// The build: the drums, bass, pads and arpeggio grow scene by scene, swelling into the end.
const BUILD = [[0, 0.5], [START["mix-title"], 0.5], [START.mix, 0.62], [START.split - 0.1, 0.68], [START.split, 0.82], [START.facts - 0.1, 0.86], [START.facts, 0.95], [START.platforms, 1], [PLATFORMS.riser, 1], [SILENCE, 1.15]];
// Group: [gain, reverb send, delay send].
const MIX = {
  drone: [0.5, 0.3, 0],
  pad: [0.42, 0.3, 0],
  bass: [0.44, 0, 0],
  kick: [0.62, 0.04, 0],
  snare: [0.26, 0.22, 0.04],
  hat: [0.09, 0.08, 0],
  arp: [0.16, 0.22, 0.3],
  hits: [0.5, 0.25, 0],
  riser: [0.22, 0.25, 0],
  ui: [0.2, 0.12, 0.08],
  fx: [0.2, 0.35, 0.22],
  whoosh: [0.2, 0.15, 0],
};

// Freeverb (Schroeder–Moorer): eight damped combs into four allpasses per side.
function freeverb(inL, inR, { room = 0.84, damp = 0.3 } = {}) {
  const scale = RATE / 44100;
  const combs = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
  const allpasses = [556, 441, 341, 225];
  const make = (spread) => ({
    combs: combs.map((n) => ({ buf: new Float32Array(Math.round((n + spread) * scale)), i: 0, store: 0 })),
    alls: allpasses.map((n) => ({ buf: new Float32Array(Math.round((n + spread) * scale)), i: 0 })),
  });
  const sides = [make(0), make(23)];
  const outL = new Float32Array(LENGTH);
  const outR = new Float32Array(LENGTH);
  const pre = sample(0.02);
  const hp = [new Biquad("hp", 180, 0.7), new Biquad("hp", 180, 0.7)];
  const lp = [new Biquad("lp", 7500, 0.7), new Biquad("lp", 7500, 0.7)];
  for (let i = 0; i < LENGTH; i += 1) {
    const x = i >= pre ? (inL[i - pre] + inR[i - pre]) * 0.5 : 0;
    for (let s = 0; s < 2; s += 1) {
      const side = sides[s];
      const input = lp[s].run(hp[s].run(x)) * 0.03;
      let sum = 0;
      for (const comb of side.combs) {
        const y = comb.buf[comb.i];
        comb.store = y * (1 - damp) + comb.store * damp;
        comb.buf[comb.i] = input + comb.store * room;
        comb.i = comb.i + 1 === comb.buf.length ? 0 : comb.i + 1;
        sum += y;
      }
      for (const all of side.alls) {
        const b = all.buf[all.i];
        all.buf[all.i] = sum + b * 0.5;
        all.i = all.i + 1 === all.buf.length ? 0 : all.i + 1;
        sum = b - sum;
      }
      (s ? outR : outL)[i] = sum;
    }
  }
  return { l: outL, r: outR };
}

// A ping-pong delay, a dotted eighth, darkening on every repeat.
function delay(inL, inR, { time = 0.375, feedback = 0.38 } = {}) {
  const n = sample(time);
  const bl = new Float32Array(n);
  const br = new Float32Array(n);
  const lp = [new Biquad("lp", 3200, 0.7), new Biquad("lp", 3200, 0.7)];
  const hp = new Biquad("hp", 300, 0.7);
  const outL = new Float32Array(LENGTH);
  const outR = new Float32Array(LENGTH);
  let j = 0;
  for (let i = 0; i < LENGTH; i += 1) {
    const yl = bl[j];
    const yr = br[j];
    bl[j] = hp.run((inL[i] + inR[i]) * 0.5) + lp[1].run(yr) * feedback;
    br[j] = lp[0].run(yl) * feedback;
    j = j + 1 === n ? 0 : j + 1;
    outL[i] = yl;
    outR[i] = yr;
  }
  return { l: outL, r: outR };
}

// The kick's sidechain: how far each sample sits inside a duck (0–1).
function sidechain() {
  const shape = new Float32Array(LENGTH);
  for (const t of KICKS) {
    const i0 = sample(t);
    for (let k = 0; k < sample(0.45) && i0 + k < LENGTH; k += 1) {
      const u = k / RATE;
      const s = u < 0.006 ? 0.5 - 0.5 * Math.cos((Math.PI * u) / 0.006) : Math.exp(-(u - 0.006) / 0.11);
      if (s > shape[i0 + k]) shape[i0 + k] = s;
    }
  }
  return shape;
}

function mixdown(G) {
  const duck = sidechain();
  // Pads: a resonant low-pass following the arrangement, their level, ducked by the kick.
  const cutoff = control(curve(PAD_CUTOFF));
  const level = control(curve(PAD_LEVEL));
  const pl = new Biquad();
  const pr = new Biquad();
  // A high-pass leaves the low end to the sub and the kick.
  const hl = new Biquad("hp", 110, 0.7);
  const hr = new Biquad("hp", 110, 0.7);
  for (let i = 0; i < LENGTH; i += 1) {
    if (i % 16 === 0) {
      const f = read(cutoff, i);
      pl.set("lp", f, 1.8);
      pr.set("lp", f * 1.02, 1.8);
    }
    const g = read(level, i) * (1 - 0.6 * duck[i]);
    G.pad.l[i] = hl.run(pl.run(G.pad.l[i])) * g;
    G.pad.r[i] = hr.run(pr.run(G.pad.r[i])) * g;
  }
  // Bass: soft saturation, ducked by the kick.
  for (let i = 0; i < LENGTH; i += 1) {
    const g = 1 - 0.7 * duck[i];
    G.bass.l[i] = Math.tanh(1.8 * G.bass.l[i]) * g;
    G.bass.r[i] = Math.tanh(1.8 * G.bass.r[i]) * g;
  }

  // Sum the groups and their sends; the music groups go through the slow-motion dip.
  const music = bus();
  const sfx = bus();
  const toReverb = bus();
  const toDelay = bus();
  const build = control(curve(BUILD));
  // The music and hits make a little room (−3.5 dB) while a title is written.
  const titles = cues.filter((cue) => cue.kind === "title").map((cue) => cue.t);
  const titleRoom = control((t) => 1 - 0.33 * Math.max(0, ...titles.map((at) => Math.min(clamp((t - at) / 0.06), clamp((at + 0.75 - t) / 0.25)))));
  for (const [name, [gain, reverb, echo]] of Object.entries(MIX)) {
    const into = ["drone", "pad", "bass", "kick", "snare", "hat", "arp"].includes(name) ? music : sfx;
    const builds = into === music && name !== "drone";
    const ducks = into === music || name === "hits";
    const { l, r } = G[name];
    for (let i = 0; i < LENGTH; i += 1) {
      const g = (builds ? gain * read(build, i) : gain) * (ducks ? read(titleRoom, i) : 1);
      const xl = l[i] * g;
      const xr = r[i] * g;
      into.l[i] += xl;
      into.r[i] += xr;
      toReverb.l[i] += xl * reverb;
      toReverb.r[i] += xr * reverb;
      toDelay.l[i] += xl * echo;
      toDelay.r[i] += xr * echo;
    }
  }
  const dip = control((t) => clamp((1 - speedAt(t)) / 0.82));
  const dl = new Biquad();
  const dr = new Biquad();
  for (let i = 0; i < LENGTH; i += 1) {
    const d = read(dip, i);
    if (i % 16 === 0) {
      const f = 19000 * (650 / 19000) ** d;
      dl.set("lp", f, 0.75);
      dr.set("lp", f, 0.75);
    }
    music.l[i] = dl.run(music.l[i]) * (1 - 0.25 * d);
    music.r[i] = dr.run(music.r[i]) * (1 - 0.25 * d);
  }
  const room = freeverb(toReverb.l, toReverb.r);
  const echo = delay(toDelay.l, toDelay.r);

  // Everything but the final hit stops at SILENCE (a 40 ms fade); the final hit has its own long
  // reverb, and the whole mix fades out by the end.
  const finalRoom = freeverb(
    G.final.l.map((x) => x * 0.5),
    G.final.r.map((x) => x * 0.5),
    { room: 0.9, damp: 0.25 },
  );
  const out = bus();
  const fadeIn = sample(0.005);
  for (let i = 0; i < LENGTH; i += 1) {
    const t = i / RATE;
    const gate = t < SILENCE ? 1 : t < SILENCE + 0.04 ? 0.5 + 0.5 * Math.cos((Math.PI * (t - SILENCE)) / 0.04) : 0;
    const fade = t < 29.2 ? 1 : Math.cos(((Math.PI / 2) * (t - 29.2)) / (DURATION - 29.2)) ** 2;
    const start = i < fadeIn ? i / fadeIn : 1;
    out.l[i] = ((music.l[i] + sfx.l[i] + room.l[i] + echo.l[i]) * gate + G.final.l[i] * 0.55 + finalRoom.l[i]) * fade * start;
    out.r[i] = ((music.r[i] + sfx.r[i] + room.r[i] + echo.r[i]) * gate + G.final.r[i] * 0.55 + finalRoom.r[i]) * fade * start;
  }
  return out;
}

// A stereo-linked glue compressor (feed-forward, soft knee) on an RMS detector.
function glue({ l, r }, { threshold, ratio = 2, attack = 0.01, release = 0.15, knee = 6 }) {
  const up = 1 - Math.exp(-1 / (attack * RATE));
  const down = 1 - Math.exp(-1 / (release * RATE));
  let power = 0;
  for (let i = 0; i < LENGTH; i += 1) {
    const x = (l[i] * l[i] + r[i] * r[i]) / 2;
    power += (x - power) * (x > power ? up : down);
    const over = 10 * Math.log10(power + 1e-12) - threshold;
    const cut = over <= -knee / 2 ? 0 : over >= knee / 2 ? over * (1 - 1 / ratio) : ((1 - 1 / ratio) * (over + knee / 2) ** 2) / (2 * knee);
    const g = 10 ** (-cut / 20);
    l[i] *= g;
    r[i] *= g;
  }
}

// --- Measuring ---------------------------------------------------------------------------------------

// ITU-R BS.1770-4 integrated loudness in LUFS (K-weighted, 400 ms blocks, gated) at 48 kHz.
export function loudness(l, r) {
  const n = l.length;
  const sums = new Float64Array(n + 1);
  for (const x of [l, r]) {
    // The two K-weighting stages (a high shelf, then a high-pass) in direct form I.
    let [x1, x2, y1, y2, z1, z2] = [0, 0, 0, 0, 0, 0];
    let acc = 0;
    for (let i = 0; i < n; i += 1) {
      const y = 1.53512485958697 * x[i] - 2.69169618940638 * x1 + 1.19839281085285 * x2 + 1.69065929318241 * y1 - 0.73248077421585 * y2;
      const z = y - 2 * y1 + y2 + 1.99004745483398 * z1 - 0.99007225036621 * z2;
      x2 = x1;
      x1 = x[i];
      z2 = z1;
      z1 = z;
      y2 = y1;
      y1 = y;
      acc += z * z;
      sums[i + 1] += acc;
    }
  }
  const block = sample(0.4);
  const hop = sample(0.1);
  const blocks = [];
  for (let i = 0; i + block <= n; i += hop) blocks.push((sums[i + block] - sums[i]) / block);
  const lufs = (z) => -0.691 + 10 * Math.log10(z);
  const loud = blocks.filter((z) => lufs(z) > -70);
  const relative = lufs(loud.reduce((a, b) => a + b, 0) / loud.length) - 10;
  const gated = loud.filter((z) => lufs(z) > relative);
  return lufs(gated.reduce((a, b) => a + b, 0) / gated.length);
}

// Per-sample true-peak estimate: the larger of |x| at the sample and at the three 4×-oversampled
// points after it (windowed-sinc interpolation), over both channels.
const TAPS = 12;
const PHASES = [0.25, 0.5, 0.75].map((f) => {
  const kernel = new Float64Array(2 * TAPS);
  for (let k = -TAPS + 1; k <= TAPS; k += 1) {
    const d = f - k;
    kernel[k + TAPS - 1] = (Math.sin(Math.PI * d) / (Math.PI * d)) * (0.5 + 0.5 * Math.cos((Math.PI * d) / (TAPS + 0.5)));
  }
  return kernel;
});
function peaks(l, r) {
  const out = new Float32Array(l.length);
  for (const x of [l, r]) {
    for (let i = 0; i < x.length; i += 1) {
      let m = Math.abs(x[i]);
      if (i >= TAPS - 1 && i + TAPS < x.length) {
        for (const kernel of PHASES) {
          let y = 0;
          for (let k = 0; k < kernel.length; k += 1) y += kernel[k] * x[i + k - TAPS + 1];
          if (Math.abs(y) > m) m = Math.abs(y);
        }
      }
      if (m > out[i]) out[i] = m;
    }
  }
  return out;
}
export function truePeak(l, r) {
  return dB(peaks(l, r).reduce((a, b) => Math.max(a, b), 0));
}

// A look-ahead limiter on the true peaks: the gain each sample needs, its minimum over ±1.5 ms,
// a 120 ms release, then smoothed over the look-ahead so the gain glides down before each peak.
function limit(l, r, ceiling) {
  const need = peaks(l, r);
  const top = 10 ** (ceiling / 20);
  for (let i = 0; i < LENGTH; i += 1) need[i] = need[i] > top ? top / need[i] : 1;
  const ahead = sample(0.0015);
  const held = new Float32Array(LENGTH);
  const queue = new Int32Array(LENGTH);
  let head = 0;
  let tail = 0;
  for (let i = 0; i < LENGTH + ahead; i += 1) {
    if (i < LENGTH) {
      while (tail > head && need[queue[tail - 1]] >= need[i]) tail -= 1;
      queue[tail++] = i;
    }
    const centre = i - ahead;
    if (centre < 0) continue;
    while (queue[head] < centre - ahead) head += 1;
    held[centre] = need[queue[head]];
  }
  const rise = 1 - Math.exp(-1 / (0.12 * RATE));
  let g = 1;
  for (let i = 0; i < LENGTH; i += 1) {
    g = Math.min(held[i], g + (1 - g) * rise);
    held[i] = g;
  }
  const sums = new Float64Array(LENGTH + 1);
  for (let i = 0; i < LENGTH; i += 1) sums[i + 1] = sums[i] + held[i];
  const half = ahead >> 1;
  const outL = new Float32Array(LENGTH);
  const outR = new Float32Array(LENGTH);
  for (let i = 0; i < LENGTH; i += 1) {
    const a = Math.max(0, i - half);
    const b = Math.min(LENGTH, i + half + 1);
    const gain = (sums[b] - sums[a]) / (b - a);
    outL[i] = l[i] * gain;
    outR[i] = r[i] * gain;
  }
  return { l: outL, r: outR };
}

// --- Rendering -----------------------------------------------------------------------------------------

// The finished stereo mix as { l, r } Float32Arrays of DURATION × RATE samples, with its measured
// loudness (LUFS) and true peak (dBTP).
export function renderScore() {
  const G = Object.fromEntries(GROUPS.map((name) => [name, bus()]));
  arrange(G);
  const mix = mixdown(G);
  for (const ch of [mix.l, mix.r]) {
    const hp = new Biquad("hp", 22, Math.SQRT1_2);
    for (let i = 0; i < LENGTH; i += 1) ch[i] = hp.run(ch[i]);
  }
  // Bring it near the target, glue it, then find the gain that lands the limited mix on target.
  let gain = 10 ** ((TARGET_LUFS - 2 - loudness(mix.l, mix.r)) / 20);
  for (const ch of [mix.l, mix.r]) for (let i = 0; i < LENGTH; i += 1) ch[i] *= gain;
  glue(mix, { threshold: -16, ratio: 2 });
  gain = 10 ** ((TARGET_LUFS - loudness(mix.l, mix.r)) / 20);
  let out;
  let lufs;
  for (let pass = 0; pass < 4; pass += 1) {
    out = limit(
      mix.l.map((x) => x * gain),
      mix.r.map((x) => x * gain),
      CEILING_DBTP,
    );
    lufs = loudness(out.l, out.r);
    if (Math.abs(lufs - TARGET_LUFS) < 0.05) break;
    gain *= 10 ** ((TARGET_LUFS - lufs) / 20);
  }
  return { ...out, lufs, peak: truePeak(out.l, out.r) };
}

// 16-bit PCM WAV bytes of a stereo mix, with seeded TPDF dither.
export function wavBytes({ l, r }) {
  const n = l.length;
  const bytes = Buffer.alloc(44 + n * 4);
  bytes.write("RIFF", 0);
  bytes.writeUInt32LE(36 + n * 4, 4);
  bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16);
  bytes.writeUInt16LE(1, 20);
  bytes.writeUInt16LE(2, 22);
  bytes.writeUInt32LE(RATE, 24);
  bytes.writeUInt32LE(RATE * 4, 28);
  bytes.writeUInt16LE(4, 32);
  bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36);
  bytes.writeUInt32LE(n * 4, 40);
  const dither = rng("dither");
  for (let i = 0; i < n; i += 1) {
    for (const [c, x] of [
      [0, l[i]],
      [1, r[i]],
    ]) {
      const q = Math.round(x * 32767 + dither() - dither());
      bytes.writeInt16LE(Math.max(-32768, Math.min(32767, q)), 44 + i * 4 + c * 2);
    }
  }
  return bytes;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [file] = process.argv.slice(2);
  if (!file) {
    console.error("usage: node score.mjs <out.wav>");
    process.exit(2);
  }
  const begun = Date.now();
  const score = renderScore();
  writeFileSync(resolve(file), wavBytes(score));
  console.log(`${file}: ${DURATION} s, ${RATE / 1000} kHz stereo 16-bit, ${score.lufs.toFixed(1)} LUFS, ${score.peak.toFixed(1)} dBTP (rendered in ${((Date.now() - begun) / 1000).toFixed(1)} s)`);
}
