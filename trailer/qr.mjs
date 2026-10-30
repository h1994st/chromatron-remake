// A QR code encoder for the brand stills' link to the page (ISO/IEC 18004): byte mode, versions 1–10,
// any error-correction level, the mask with the lowest penalty. `encodeQr` returns the symbol as a
// size × size grid of dark (true) and light modules, without the quiet zone.

// Error-correction codewords per block, then the blocks as [count, data codewords] groups, for
// versions 1–10 at each level.
const BLOCKS = {
  L: [[7, [1, 19]], [10, [1, 34]], [15, [1, 55]], [20, [1, 80]], [26, [1, 108]], [18, [2, 68]], [20, [2, 78]], [24, [2, 97]], [30, [2, 116]], [18, [2, 68], [2, 69]]],
  M: [[10, [1, 16]], [16, [1, 28]], [26, [1, 44]], [18, [2, 32]], [24, [2, 43]], [16, [4, 27]], [18, [4, 31]], [22, [2, 38], [2, 39]], [22, [3, 36], [2, 37]], [26, [4, 43], [1, 44]]],
  Q: [[13, [1, 13]], [22, [1, 22]], [18, [2, 17]], [26, [2, 24]], [18, [2, 15], [2, 16]], [24, [4, 19]], [18, [2, 14], [4, 15]], [22, [4, 18], [2, 19]], [20, [4, 16], [4, 17]], [24, [6, 19], [2, 20]]],
  H: [[17, [1, 9]], [28, [1, 16]], [22, [2, 13]], [16, [4, 9]], [22, [2, 11], [2, 12]], [28, [4, 15]], [26, [4, 13], [1, 14]], [26, [4, 14], [2, 15]], [24, [4, 12], [4, 13]], [28, [6, 15], [2, 16]]],
};
// The level's two format bits.
const LEVEL_BITS = { L: 1, M: 0, Q: 3, H: 2 };
// The alignment patterns' centres (both axes), versions 1–10.
const ALIGNMENT = [[], [6, 18], [6, 22], [6, 26], [6, 30], [6, 34], [6, 22, 38], [6, 24, 42], [6, 26, 46], [6, 28, 50]];

// Multiplication in GF(256) modulo x^8 + x^4 + x^3 + x^2 + 1.
function gfMultiply(x, y) {
  let z = 0;
  for (let i = 7; i >= 0; i -= 1) {
    z = (z << 1) ^ ((z >>> 7) * 0x11d);
    z ^= ((y >>> i) & 1) * x;
  }
  return z;
}

// The Reed–Solomon error-correction codewords of `data`, `degree` of them.
export function reedSolomon(data, degree) {
  const divisor = new Array(degree).fill(0);
  divisor[degree - 1] = 1;
  let root = 1;
  for (let i = 0; i < degree; i += 1) {
    for (let j = 0; j < degree; j += 1) {
      divisor[j] = gfMultiply(divisor[j], root);
      if (j + 1 < degree) divisor[j] ^= divisor[j + 1];
    }
    root = gfMultiply(root, 2);
  }
  const result = new Array(degree).fill(0);
  for (const byte of data) {
    const factor = byte ^ result.shift();
    result.push(0);
    for (let i = 0; i < degree; i += 1) result[i] ^= gfMultiply(divisor[i], factor);
  }
  return result;
}

// The 15 format bits for `level` and `mask`, BCH-coded and masked.
export function formatBits(level, mask) {
  const data = (LEVEL_BITS[level] << 3) | mask;
  let rem = data;
  for (let i = 0; i < 10; i += 1) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
  return ((data << 10) | rem) ^ 0x5412;
}

// The 18 version bits (versions 7 and up), BCH-coded.
export function versionBits(version) {
  let rem = version;
  for (let i = 0; i < 12; i += 1) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25);
  return (version << 12) | rem;
}

// Whether mask `mask` flips the module at column x, row y.
const MASKS = [
  (x, y) => (x + y) % 2 === 0,
  (x, y) => y % 2 === 0,
  (x) => x % 3 === 0,
  (x, y) => (x + y) % 3 === 0,
  (x, y) => (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0,
  (x, y) => ((x * y) % 2) + ((x * y) % 3) === 0,
  (x, y) => (((x * y) % 2) + ((x * y) % 3)) % 2 === 0,
  (x, y) => (((x + y) % 2) + ((x * y) % 3)) % 2 === 0,
];

// The data codewords for `bytes` in byte mode, padded to `capacity`.
function dataCodewords(bytes, version, capacity) {
  const bits = [];
  const put = (value, length) => {
    for (let i = length - 1; i >= 0; i -= 1) bits.push((value >>> i) & 1);
  };
  put(0b0100, 4);
  put(bytes.length, version < 10 ? 8 : 16);
  for (const byte of bytes) put(byte, 8);
  put(0, Math.min(4, capacity * 8 - bits.length));
  put(0, (8 - (bits.length % 8)) % 8);
  const codewords = [];
  for (let i = 0; i < bits.length; i += 8) codewords.push(bits.slice(i, i + 8).reduce((byte, bit) => (byte << 1) | bit, 0));
  for (let pad = 0xec; codewords.length < capacity; pad ^= 0xec ^ 0x11) codewords.push(pad);
  return codewords;
}

// The codewords in symbol order: the blocks' data interleaved, then their error correction.
function interleave(data, version, level) {
  const [ecLength, ...groups] = BLOCKS[level][version - 1];
  const blocks = [];
  let at = 0;
  for (const [count, length] of groups) {
    for (let i = 0; i < count; i += 1) {
      const block = data.slice(at, at + length);
      at += length;
      blocks.push({ data: block, ec: reedSolomon(block, ecLength) });
    }
  }
  const out = [];
  const longest = Math.max(...blocks.map((block) => block.data.length));
  for (let i = 0; i < longest; i += 1) for (const block of blocks) if (i < block.data.length) out.push(block.data[i]);
  for (let i = 0; i < ecLength; i += 1) for (const block of blocks) out.push(block.ec[i]);
  return out;
}

// The penalty score of a finished grid (the standard's four rules); the encoder keeps the lowest.
function penalty(grid, size) {
  let score = 0;
  const at = (x, y) => grid[y * size + x];
  for (let pass = 0; pass < 2; pass += 1) {
    for (let a = 0; a < size; a += 1) {
      const line = [];
      for (let b = 0; b < size; b += 1) line.push(pass === 0 ? at(b, a) : at(a, b));
      // Runs of five or more in a row.
      let run = 1;
      for (let b = 1; b <= size; b += 1) {
        if (b < size && line[b] === line[b - 1]) run += 1;
        else {
          if (run >= 5) score += run - 2;
          run = 1;
        }
      }
      // Finder-like 1:1:3:1:1 patterns with four light modules on either side.
      const padded = [0, 0, 0, 0, ...line.map(Number), 0, 0, 0, 0];
      for (let b = 0; b + 11 <= padded.length; b += 1) {
        const s = padded.slice(b, b + 11).join("");
        if (s === "10111010000" || s === "00001011101") score += 40;
      }
    }
  }
  // 2 × 2 blocks of one colour.
  for (let y = 0; y + 1 < size; y += 1) {
    for (let x = 0; x + 1 < size; x += 1) {
      const c = at(x, y);
      if (c === at(x + 1, y) && c === at(x, y + 1) && c === at(x + 1, y + 1)) score += 3;
    }
  }
  // The dark share's distance from half, in 5% steps.
  const dark = grid.reduce((sum, module) => sum + module, 0);
  score += Math.floor(Math.abs(dark * 20 - size * size * 10) / (size * size)) * 10;
  return score;
}

// The QR code of `text` (UTF-8, byte mode) at `level` ("L", "M", "Q" or "H"), in the smallest
// version 1–10 that holds it: { version, size, level, mask, dark(x, y) }.
export function encodeQr(text, { level = "M" } = {}) {
  const bytes = [...new TextEncoder().encode(text)];
  let version = 1;
  const capacityOf = (v) => {
    const [, ...groups] = BLOCKS[level][v - 1];
    return groups.reduce((sum, [count, length]) => sum + count * length, 0);
  };
  while (version <= 10 && 4 + (version < 10 ? 8 : 16) + bytes.length * 8 > capacityOf(version) * 8) version += 1;
  if (version > 10) throw new Error(`${bytes.length} bytes do not fit a version 10-${level} QR code`);
  const size = version * 4 + 17;
  const grid = new Uint8Array(size * size);
  const reserved = new Uint8Array(size * size);
  const set = (x, y, dark) => {
    if (x < 0 || y < 0 || x >= size || y >= size) return;
    grid[y * size + x] = dark ? 1 : 0;
    reserved[y * size + x] = 1;
  };

  // The function patterns: timing, finders (with their separators), alignment, the format and
  // version areas.
  for (let i = 0; i < size; i += 1) {
    set(6, i, i % 2 === 0);
    set(i, 6, i % 2 === 0);
  }
  for (const [cx, cy] of [[3, 3], [size - 4, 3], [3, size - 4]]) {
    for (let dy = -4; dy <= 4; dy += 1) {
      for (let dx = -4; dx <= 4; dx += 1) {
        const ring = Math.max(Math.abs(dx), Math.abs(dy));
        set(cx + dx, cy + dy, ring !== 2 && ring !== 4);
      }
    }
  }
  const centres = ALIGNMENT[version - 1];
  for (const cy of centres) {
    for (const cx of centres) {
      const onFinder = (cx === 6 && cy === 6) || (cx === 6 && cy === size - 7) || (cx === size - 7 && cy === 6);
      if (onFinder) continue;
      for (let dy = -2; dy <= 2; dy += 1) for (let dx = -2; dx <= 2; dx += 1) set(cx + dx, cy + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
    }
  }
  const drawFormat = (mask) => {
    const bits = formatBits(level, mask);
    const bit = (i) => ((bits >>> i) & 1) === 1;
    for (let i = 0; i <= 5; i += 1) set(8, i, bit(i));
    set(8, 7, bit(6));
    set(8, 8, bit(7));
    set(7, 8, bit(8));
    for (let i = 9; i < 15; i += 1) set(14 - i, 8, bit(i));
    for (let i = 0; i < 8; i += 1) set(size - 1 - i, 8, bit(i));
    for (let i = 8; i < 15; i += 1) set(8, size - 15 + i, bit(i));
    set(8, size - 8, true);
  };
  drawFormat(0);
  if (version >= 7) {
    const bits = versionBits(version);
    for (let i = 0; i < 18; i += 1) {
      const a = size - 11 + (i % 3);
      const b = Math.floor(i / 3);
      set(a, b, ((bits >>> i) & 1) === 1);
      set(b, a, ((bits >>> i) & 1) === 1);
    }
  }

  // The codewords, in the zigzag of two-column strips from the bottom right.
  const codewords = interleave(dataCodewords(bytes, version, capacityOf(version)), version, level);
  let i = 0;
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5;
    for (let vert = 0; vert < size; vert += 1) {
      for (let j = 0; j < 2; j += 1) {
        const x = right - j;
        const y = ((right + 1) & 2) === 0 ? size - 1 - vert : vert;
        if (reserved[y * size + x]) continue;
        if (i < codewords.length * 8) grid[y * size + x] = (codewords[i >>> 3] >>> (7 - (i & 7))) & 1;
        i += 1;
      }
    }
  }

  // The mask with the lowest penalty.
  const masked = (mask) => {
    const out = grid.slice();
    for (let y = 0; y < size; y += 1) for (let x = 0; x < size; x += 1) if (!reserved[y * size + x] && MASKS[mask](x, y)) out[y * size + x] ^= 1;
    return out;
  };
  let best = null;
  for (let mask = 0; mask < 8; mask += 1) {
    drawFormat(mask);
    const candidate = masked(mask);
    const score = penalty(candidate, size);
    if (best === null || score < best.score) best = { mask, score, modules: candidate };
  }
  drawFormat(best.mask);
  const modules = masked(best.mask);
  return { version, size, level, mask: best.mask, dark: (x, y) => modules[y * size + x] === 1 };
}
