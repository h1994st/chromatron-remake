// `just web`: how much of a wasm module's code is one crate's, from the function names the build
// keeps until wasm-opt drops them (mangled Rust names contain the crate name).
// Usage: node wasm_ledger.mjs <module.wasm> <crate>

import { readFileSync } from "node:fs";

const [file, crate] = process.argv.slice(2);
if (!file || !crate) {
  console.error("usage: node wasm_ledger.mjs <module.wasm> <crate>");
  process.exit(2);
}
const bytes = readFileSync(file);
let at = 8; // after the magic number and version
const leb = () => {
  let result = 0;
  let shift = 0;
  let byte;
  do {
    byte = bytes[at++];
    result += (byte & 0x7f) * 2 ** shift;
    shift += 7;
  } while (byte & 0x80);
  return result;
};
const limits = () => {
  const flags = bytes[at++];
  leb();
  if (flags & 1) leb();
};
const bodies = [];
const names = new Map();
let imported = 0;
while (at < bytes.length) {
  const id = bytes[at++];
  const size = leb();
  const end = at + size;
  if (id === 2) {
    for (let n = leb(); n > 0; n -= 1) {
      at += leb(); // module name
      at += leb(); // field name
      const kind = bytes[at++];
      if (kind === 0) {
        leb();
        imported += 1;
      } else if (kind === 1) {
        at += 1;
        limits();
      } else if (kind === 2) limits();
      else if (kind === 3) at += 2;
      else if (kind === 4) {
        at += 1;
        leb();
      }
    }
  } else if (id === 10) {
    for (let n = leb(); n > 0; n -= 1) {
      const body = leb();
      bodies.push(body);
      at += body;
    }
  } else if (id === 0) {
    const length = leb();
    const name = bytes.toString("utf8", at, at + length);
    at += length;
    while (name === "name" && at < end) {
      const sub = bytes[at++];
      const subEnd = leb() + at;
      if (sub === 1) {
        for (let n = leb(); n > 0; n -= 1) {
          const index = leb();
          const nameLength = leb();
          names.set(index, bytes.toString("utf8", at, at + nameLength));
          at += nameLength;
        }
      }
      at = subEnd;
    }
  }
  at = end;
}
if (names.size === 0) {
  console.log(`${crate}: no function names in ${file}`);
} else {
  let total = 0;
  let ours = 0;
  bodies.forEach((size, i) => {
    total += size;
    if ((names.get(imported + i) ?? "").includes(crate)) ours += size;
  });
  console.log(`${crate}: ${ours} of ${total} code bytes (${((100 * ours) / total).toFixed(1)}%), before wasm-opt`);
}
