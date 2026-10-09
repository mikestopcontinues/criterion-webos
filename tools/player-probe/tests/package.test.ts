import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { admitBroker, ipkData, ipkMembers } from "../src/package.js";

test("TV packaging rejects the real host CPU executable", () => {
  const host = readFileSync("/workspace/.local/player-probe/criterion-broker-probe");
  assert.throws(() => admitBroker(host), /invalidBroker/);
});

test("TV packaging rejects oversized and truncated executable inputs", () => {
  assert.throws(() => admitBroker(Buffer.alloc(2 * 1024 * 1024 + 1)), /invalidBroker/);
  assert.throws(() => admitBroker(Buffer.from("not an ELF")), /invalidBroker/);
});

// Fixed ar layout from the pinned official CLI; compressed content is not decoded here.
export function archiveFixture(data: Buffer): Buffer {
  const members = [["debian-binary", Buffer.from("2.0\n")], ["control.tar.gz", Buffer.from("control")], ["data.tar.gz", data]] as const;
  const chunks: Buffer[] = [Buffer.from("!<arch>\n")];
  for (const [name, body] of members) {
    const header = `${name.padEnd(16)}${"0".padEnd(12)}${"0".padEnd(6)}${"0".padEnd(6)}${"100644".padEnd(8)}${String(body.length).padEnd(10)}\x60\n`;
    chunks.push(Buffer.from(header), body);
    if (body.length % 2) chunks.push(Buffer.from("\n"));
  }
  return Buffer.concat(chunks);
}

test("archive default stays probe-sized while a bounded native limit admits larger data", () => {
  const large = archiveFixture(Buffer.alloc(2 * 1024 * 1024));
  assert.throws(() => ipkData(large), /invalidIpk/);
  assert.equal(ipkData(large, 4 * 1024 * 1024).length, 2 * 1024 * 1024);
  assert.equal(ipkMembers(large, 4 * 1024 * 1024).get("control.tar.gz")?.toString(), "control");
});
test("archive byte limits cannot disable the admission bound", () => {
  const bytes = archiveFixture(Buffer.from("data"));
  for (const limit of [0, -1, 1.5, Infinity, 64 * 1024 * 1024 + 1]) assert.throws(() => ipkData(bytes, limit), /invalidIpk/);
  const large = archiveFixture(Buffer.alloc(10 * 1024 * 1024));
  assert.equal(ipkData(large, 12 * 1024 * 1024).length, 10 * 1024 * 1024);
});
test("ar member names must preserve their exact ASCII bytes", () => {
  const bytes = archiveFixture(Buffer.from("data"));
  const position = bytes.indexOf(Buffer.from("data.tar.gz"));
  bytes[position] = 0xe4; // lossy ASCII decoding would turn this into 'd'.
  assert.throws(() => ipkData(bytes), /invalidIpk/);
});
