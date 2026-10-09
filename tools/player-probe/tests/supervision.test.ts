import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { test } from "node:test";
import { Broker } from "../src/broker.js";

function fixture(mode: string) {
  return new Broker(spawn(process.execPath, ["/workspace/tools/player-probe/dist/tests/child-fixture.js", mode], {
    shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
  }));
}

for (const mode of ["wrong-pid", "stderr", "oversized", "duplicate", "no-pong"]) {
  test(`malformed or unresponsive pipe peer is closed and reaped: ${mode}`, async () => {
    const broker = fixture(mode);
    try {
      try { await broker.started(); if (mode === "duplicate" || mode === "no-pong") await broker.ping(); } catch { /* The closed child remains the oracle. */ }
      const cleanup = await broker.closed;
      assert.equal(cleanup.confirmed, true);
      assert.equal(broker.state, "closed");
    } finally { await broker.close(); }
  });
}

test("ignoring stop and SIGTERM still requires actual close after SIGKILL", async () => {
  const broker = fixture("ignore-stop");
  try {
    await broker.started();
    const cleanup = await broker.close();
    assert.equal(cleanup.confirmed, true);
    assert.equal(cleanup.exitCode, null);
    assert.equal(broker.stopAcknowledged, false);
    assert.equal(broker.state, "closed");
  } finally { await broker.close(); }
});

test("parent exit with inherited pipes remains unconfirmed until actual pipe close", async () => {
  const broker = fixture("inherited-pipe");
  try {
    await broker.started();
    const cleanup = await broker.close();
    assert.equal(cleanup.confirmed, false);
    assert.equal(broker.state, "failed");
    assert.equal((await broker.closed).confirmed, true);
    assert.equal(broker.state, "closed");
  } finally { await broker.close(); }
});
