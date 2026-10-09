import assert from "node:assert/strict";
import { spawn } from "node:child_process";

import { test } from "node:test";

const binary = "/workspace/.local/player-probe/criterion-broker-probe";

async function runBroker(input: string | undefined, lease = "10000", maximum = "120000") {
  const child = spawn(binary, ["--lease-ms", lease, "--max-ms", maximum], {
    shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
  });
  const completion = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) => child.once("close", (code, signal) => resolve({ code, signal })));
  let output = "";
  child.stdout.on("data", (bytes: Buffer) => { output += bytes.toString("utf8"); });
  child.stdin.on("error", () => { /* Assert actual close, not write acceptance. */ });
  const timeout = setTimeout(() => child.kill("SIGKILL"), 1500);
  if (input !== undefined) child.stdin.end(input);
  const { code, signal } = await completion;
  clearTimeout(timeout);
  return { code, signal, output, pid: child.pid };
}

test("the broker exits on an expired lease even while its parent pipe stays open", async () => {
  const result = await runBroker(undefined, "100", "1000");
  assert.equal(result.code, 3);
  assert.equal(result.signal, null);
  assert.equal(result.output, `ready ${result.pid}\n`);
});

test("the actual broker acknowledges a numbered ping and exits after stop", async () => {
  const child = spawn(binary, ["--lease-ms", "10000", "--max-ms", "120000"], {
    shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
  });
  const completion = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) => child.once("close", (code, signal) => resolve({ code, signal })));
  let output = "";
  child.stdout.on("data", (bytes: Buffer) => { output += bytes.toString("utf8"); });
  child.stdin.on("error", () => { /* The close result remains the lifecycle oracle. */ });
  const timeout = setTimeout(() => child.kill("SIGKILL"), 1500);
  child.stdin.end("ping 1\nstop\n");
  const { code, signal } = await completion;
  clearTimeout(timeout);
  assert.equal(code, 0);
  assert.equal(signal, null);
  assert.match(output, new RegExp(`^ready ${child.pid}\\npong 1 1 ${child.pid}\\nstopped 1 ${child.pid}\\n$`));
});

for (const [name, input] of [
  ["unknown command", "exec /bin/sh\n"], ["partial frame at EOF", "ping 1"],
  ["oversized line", "x".repeat(49) + "\n"], ["sequence replay", "ping 1\nping 1\n"],
  ["sequence gap", "ping 2\n"], ["leading zero", "ping 01\n"],
  ["out-of-range sequence", "ping 1000001\n"], ["non-ASCII frame", "ping １\n"],
] as const) {
  test(`broker rejects bounded protocol violation: ${name}`, async () => {
    const result = await runBroker(input);
    assert.equal(result.code, 2);
    assert.equal(result.signal, null);
    assert.ok(!result.output.includes("stopped"));
  });
}

test("clean parent-pipe EOF exits without requiring a stop command", async () => {
  const result = await runBroker("");
  assert.equal(result.code, 0);
  assert.equal(result.signal, null);
  assert.equal(result.output, `ready ${result.pid}\n`);
});

test("absolute lifetime expires despite continuing lease renewals", async () => {
  const child = spawn(binary, ["--lease-ms", "100", "--max-ms", "250"], {
    shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
  });
  let sequence = 0;
  let output = "";
  child.stdout.on("data", (bytes: Buffer) => { output += bytes.toString(); });
  child.stdin.on("error", () => { /* Test waits for actual process close. */ });
  const timer = setInterval(() => { sequence += 1; child.stdin.write(`ping ${sequence}\n`); }, 40);
  const safety = setTimeout(() => child.kill("SIGKILL"), 1500);
  const code = await new Promise<number | null>((resolve) => child.once("close", resolve));
  clearInterval(timer);
  clearTimeout(safety);
  assert.equal(code, 3);
  assert.ok(output.includes("pong 5 5"));
});

test("blocked output cannot keep the broker alive beyond its bounded write deadline", async () => {
  const child = spawn(binary, ["--lease-ms", "1000", "--max-ms", "2000"], {
    shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
  });
  child.stdin.on("error", () => { /* A full input pipe retires when the broker exits. */ });
  child.stderr.resume();
  const exit = new Promise<number | null>((resolve) => child.once("exit", resolve));
  const close = new Promise<number | null>((resolve) => child.once("close", resolve));
  const safety = setTimeout(() => child.kill("SIGKILL"), 3000);
  // Deliberately retain stdout. The broker's own output thread cannot drain forever.
  child.stdin.end(Array.from({ length: 10000 }, (_, index) => `ping ${index + 1}\n`).join(""));
  const code = await exit;
  child.stdout.resume();
  const reaped = await close;
  clearTimeout(safety);
  assert.equal(code, 4);
  assert.equal(reaped, 4);
});
