// The QR encoder against the standard's worked values; `just social-images` also scans the
// rendered code back in Chrome.

import assert from "node:assert/strict";
import { test } from "node:test";
import { encodeQr, formatBits, reedSolomon, versionBits } from "./qr.mjs";

test("Reed–Solomon matches the standard's 1-M HELLO WORLD codewords", () => {
  const data = [32, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236, 17, 236, 17];
  assert.deepEqual(reedSolomon(data, 10), [196, 35, 39, 119, 235, 215, 231, 226, 93, 23]);
});

test("format bits match the standard's table", () => {
  const bits = (level, mask) => formatBits(level, mask).toString(2).padStart(15, "0");
  assert.equal(bits("L", 0), "111011111000100");
  assert.equal(bits("L", 4), "110011000101111");
  assert.equal(bits("M", 0), "101010000010010");
  assert.equal(bits("Q", 0), "011010101011111");
  assert.equal(bits("H", 0), "001011010001001");
});

test("version bits match the standard's table", () => {
  assert.equal(versionBits(7).toString(2).padStart(18, "0"), "000111110010010100");
  assert.equal(versionBits(10).toString(2).padStart(18, "0"), "001010010011010011");
});

test("the page's address takes the smallest version for its level", () => {
  const url = "https://shengtuo.me/chromatron-remake/";
  assert.equal(encodeQr(url, { level: "M" }).version, 3);
  assert.equal(encodeQr(url, { level: "Q" }).version, 4);
  assert.equal(encodeQr(url, { level: "H" }).version, 5);
  assert.throws(() => encodeQr("x".repeat(300), { level: "H" }), /do not fit/);
});

test("a symbol has its finders, timing and dark module", () => {
  const { size, dark } = encodeQr("https://shengtuo.me/chromatron-remake/", { level: "Q" });
  assert.equal(size, 33);
  for (const [cx, cy] of [[3, 3], [size - 4, 3], [3, size - 4]]) {
    assert.ok(dark(cx, cy) && dark(cx - 3, cy - 3) && !dark(cx - 2, cy - 2) && dark(cx - 1, cy - 1), `finder at ${cx}, ${cy}`);
  }
  for (let i = 8; i < size - 8; i += 1) assert.equal(dark(6, i), i % 2 === 0, `timing at row ${i}`);
  assert.ok(dark(8, size - 8), "the dark module");
});
