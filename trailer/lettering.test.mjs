// `just trailer-test`: the lettering's glyph set and line layout.

import assert from "node:assert/strict";
import { test } from "node:test";
import { CHARSET, GLYPHS, WEIGHT, layoutLine } from "./lettering.mjs";

test("every character in the set has a glyph on the cap grid", () => {
  for (const ch of CHARSET) {
    const g = GLYPHS[ch];
    assert.ok(g, `a glyph for ${JSON.stringify(ch)}`);
    assert.ok(Number.isFinite(g.w) && g.w >= 0, `${ch} has a width`);
    if (ch !== " ") assert.ok(g.paths.length > 0, `${ch} has strokes`);
    for (const { pts } of g.paths) {
      for (const [x, y] of pts) {
        assert.ok(x >= -0.01 && x <= g.w + 0.1, `${ch}: x ${x} in its box`);
        assert.ok(y >= -0.01 && y <= 1.1, `${ch}: y ${y} on the cap grid`);
      }
    }
  }
  assert.equal(Object.keys(GLYPHS).length, [...CHARSET].length, "no glyph outside the set");
});

test("a line's layout is deterministic and adds up", () => {
  const text = "4 classic games · 200 levels!";
  const a = layoutLine(text, { track: 0.3 });
  const b = layoutLine(text, { track: 0.3 });
  assert.deepEqual(a, b);
  assert.equal(a.width.toFixed(6), "27.980000");
  // Without kerned pairs the width is the glyph widths plus one advance gap per letter pair.
  const plain = layoutLine("CHROMATRON", { track: 0.36 });
  const sum = [..."CHROMATRON"].reduce((total, ch) => total + GLYPHS[ch].w, 0);
  assert.ok(Math.abs(plain.width - (sum + 9 * (WEIGHT + 0.36))) < 1e-9);
  // Kerning only ever tightens.
  assert.ok(layoutLine("LT").width < GLYPHS.L.w + GLYPHS.T.w + WEIGHT + 0.3);
  assert.throws(() => layoutLine("é"), /no glyph/);
});
