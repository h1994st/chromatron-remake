// `just social-images`: shrinks a PNG that headless Chrome wrote, with no dependency. It rounds every
// channel to <bits> bits (6 keeps the dark gradients smooth and drops the dither that defeats
// compression), drops alpha, picks the best PNG filter per row and deflates at level 9.
// Usage: node png_squeeze.mjs <png> <bits 1-8>

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { crc32, deflateSync, inflateSync } from "node:zlib";

const [path, bitsArg] = process.argv.slice(2);
const bits = Number(bitsArg);
assert.ok(path && Number.isInteger(bits) && bits >= 1 && bits <= 8, "usage: node png_squeeze.mjs <png> <bits 1-8>");

const png = readFileSync(path);
assert.equal(png.toString("latin1", 1, 4), "PNG", `${path} is a PNG`);
let width;
let height;
let channels;
const idat = [];
for (let at = 8; at < png.length; ) {
  const length = png.readUInt32BE(at);
  const type = png.toString("latin1", at + 4, at + 8);
  const data = png.subarray(at + 8, at + 8 + length);
  if (type === "IHDR") {
    width = data.readUInt32BE(0);
    height = data.readUInt32BE(4);
    assert.ok(data[8] === 8 && (data[9] === 2 || data[9] === 6) && data[12] === 0, `${path} is 8-bit RGB or RGBA, not interlaced`);
    channels = data[9] === 6 ? 4 : 3;
  } else if (type === "IDAT") {
    idat.push(data);
  }
  at += 12 + length;
}

const paeth = (a, b, c) => {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
};
// The predictor of PNG filter `f` for byte x of a row with `bpp` bytes per pixel.
const predict = (f, row, above, x, bpp) => {
  const a = x >= bpp ? row[x - bpp] : 0;
  const b = above ? above[x] : 0;
  const c = x >= bpp && above ? above[x - bpp] : 0;
  return [0, a, b, (a + b) >> 1, paeth(a, b, c)][f];
};

// Decode.
const raw = inflateSync(Buffer.concat(idat));
const inStride = width * channels;
const pixels = Buffer.alloc(height * inStride);
for (let y = 0; y < height; y++) {
  const filter = raw[y * (inStride + 1)];
  const row = pixels.subarray(y * inStride, (y + 1) * inStride);
  const above = y ? pixels.subarray((y - 1) * inStride, y * inStride) : null;
  for (let x = 0; x < inStride; x++) row[x] = raw[y * (inStride + 1) + 1 + x] + predict(filter, row, above, x, channels);
}

// Round to `bits` bits per channel, as RGB.
const step = 1 << (8 - bits);
const stride = width * 3;
const rgb = Buffer.alloc(height * stride);
for (let i = 0, j = 0; j < rgb.length; i += channels, j += 3) {
  for (let k = 0; k < 3; k++) rgb[j + k] = Math.min(255, Math.round(pixels[i + k] / step) * step);
}

// Encode, each row with the filter whose output has the smallest sum of absolute differences.
const filtered = Buffer.alloc(height * (stride + 1));
const candidate = Buffer.alloc(stride + 1);
for (let y = 0; y < height; y++) {
  const row = rgb.subarray(y * stride, (y + 1) * stride);
  const above = y ? rgb.subarray((y - 1) * stride, y * stride) : null;
  let best = Infinity;
  for (let f = 0; f < 5; f++) {
    candidate[0] = f;
    let sum = 0;
    for (let x = 0; x < stride; x++) {
      const d = (row[x] - predict(f, row, above, x, 3)) & 255;
      candidate[x + 1] = d;
      sum += d < 128 ? d : 256 - d;
    }
    if (sum < best) {
      best = sum;
      candidate.copy(filtered, y * (stride + 1));
    }
  }
}
const chunk = (type, data) => {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, "latin1");
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
};
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(width, 0);
ihdr.writeUInt32BE(height, 4);
ihdr[8] = 8;
ihdr[9] = 2;
const squeezed = Buffer.concat([
  png.subarray(0, 8),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(filtered, { level: 9, memLevel: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);
writeFileSync(path, squeezed);
