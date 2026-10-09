import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { admitBroker } from "../src/package.js";

test("TV packaging rejects the real host CPU executable", () => {
  const host = readFileSync("/workspace/.local/player-probe/criterion-broker-probe");
  assert.throws(() => admitBroker(host), /invalidBroker/);
});

test("TV packaging rejects oversized and truncated executable inputs", () => {
  assert.throws(() => admitBroker(Buffer.alloc(2 * 1024 * 1024 + 1)), /invalidBroker/);
  assert.throws(() => admitBroker(Buffer.from("not an ELF")), /invalidBroker/);
});
