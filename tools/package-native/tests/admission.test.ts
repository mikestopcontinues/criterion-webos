import assert from "node:assert/strict";
import { test } from "node:test";
import { admitExecutable } from "../src/admission.js";

// Literal ELF metadata only; never executed or submitted to the packaging CLI.
export function metadataFixture(): Buffer {
  const bytes = Buffer.alloc(180);
  Buffer.from([0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]).copy(bytes);
  bytes.writeUInt16LE(3, 16); // PIE
  bytes.writeUInt16LE(40, 18); // ARM
  bytes.writeUInt32LE(1, 20);
  bytes.writeUInt32LE(52, 28);
  bytes.writeUInt32LE(0x05000200, 36); // EABI5, soft-float
  bytes.writeUInt16LE(52, 40);
  bytes.writeUInt16LE(32, 42);
  bytes.writeUInt16LE(3, 44);
  bytes.writeUInt32LE(1, 52); // PT_LOAD
  bytes.writeUInt32LE(180, 52 + 16);
  bytes.writeUInt32LE(180, 52 + 20);
  bytes.writeUInt32LE(5, 52 + 24);
  bytes.writeUInt32LE(3, 84); // PT_INTERP
  bytes.writeUInt32LE(148, 84 + 4);
  bytes.writeUInt32LE(19, 84 + 16);
  bytes.writeUInt32LE(0x6474e551, 116); // PT_GNU_STACK
  bytes.writeUInt32LE(6, 116 + 24); // Read/write, no execute
  bytes.write("/lib/ld-linux.so.3\0", 148, "ascii");
  return bytes;
}

test("stock-target metadata with an executable stack cannot enter a native package", () => {
  const safe = metadataFixture();
  assert.doesNotThrow(() => admitExecutable(safe));
  const executableStack = Buffer.from(safe);
  executableStack.writeUInt32LE(7, 116 + 24);
  assert.throws(() => admitExecutable(executableStack), /invalidExecutable/);
});

for (const [name, change] of [
  ["wrong machine", (b: Buffer) => b.writeUInt16LE(183, 18)],
  ["hard-float ABI", (b: Buffer) => b.writeUInt32LE(0x05000400, 36)],
  ["unspecified float ABI", (b: Buffer) => b.writeUInt32LE(0x05000000, 36)],
  ["wrong interpreter", (b: Buffer) => b.write("/bad/ld-linux.so.3", 148)],
  ["missing stack declaration", (b: Buffer) => b.writeUInt32LE(0, 116)],
  ["duplicate interpreter", (b: Buffer) => b.writeUInt32LE(3, 116)],
  ["missing executable segment", (b: Buffer) => b.writeUInt32LE(4, 52 + 24)],
  ["truncated program-header table", (b: Buffer) => b.writeUInt32LE(170, 28)],
  ["segment outside the file", (b: Buffer) => b.writeUInt32LE(181, 52 + 16)],
] as const) {
  test(`native package rejects ${name}`, () => {
    const bytes = metadataFixture();
    change(bytes);
    assert.throws(() => admitExecutable(bytes), /invalidExecutable/);
  });
}

test("malformed or oversized executable input is rejected without reading headers", () => {
  assert.throws(() => admitExecutable(Buffer.from("no ELF")), /invalidExecutable/);
  assert.throws(() => admitExecutable(Buffer.alloc(32 * 1024 * 1024 + 1)), /invalidExecutable/);
});
