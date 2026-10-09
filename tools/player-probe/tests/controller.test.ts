import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { test } from "node:test";
import { Controller } from "../src/controller.js";
import { BROKER_ARGS, PLAYER_ID, VERSION, type BusMessage, type Reply } from "../src/protocol.js";

class Message implements BusMessage {
  sender: unknown = PLAYER_ID;
  uniqueToken: unknown = "player-subscription";
  payload: unknown = { version: VERSION, subscribe: true };
  isSubscription: unknown = true;
  replies: Reply[] = [];
  cancelled = false;
  respond(reply: Reply): void { this.replies.push(reply); }
  cancel(): void { this.cancelled = true; }
}

function controller() {
  return new Controller(() => spawn("/workspace/.local/player-probe/criterion-broker-probe", [...BROKER_ARGS], {
    shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
  }));
}

test("an authenticated subscription observes actual broker execution and confirmed cleanup", async () => {
  const bridge = controller();
  try {
  const subscribed = new Message();
  await bridge.attach(subscribed);
  assert.equal(subscribed.replies[0]?.returnValue, true);
  const ping = new Message();
  ping.isSubscription = false;
  ping.payload = { version: VERSION };
  await bridge.ping(ping);
  const result = ping.replies[0];
  assert.ok(result?.returnValue);
  assert.equal(result.counter, 1);
  assert.ok(Number.isInteger(result.pid) && result.pid > 0);
  const close = new Message();
  close.isSubscription = false;
  close.payload = { version: VERSION };
  await bridge.close(close);
  const closed = close.replies[0];
  assert.ok(closed?.returnValue);
  assert.equal(closed.state, "closed");
  assert.equal(closed.cleanupConfirmed, true);
  assert.equal(closed.stopAcknowledged, true);
  assert.equal(closed.exitCode, 0);
  assert.equal(subscribed.cancelled, true);
  } finally { await bridge.dispose(); }
});

test("cancelling while the child starts retires the pending subscription without publication", async () => {
  const bridge = controller();
  try {
    const subscribed = new Message();
    const attaching = bridge.attach(subscribed);
    await bridge.cancel(subscribed);
    await attaching;
    assert.deepEqual(subscribed.replies, []);
  } finally { await bridge.dispose(); }
});

test("disposed service admission cannot spawn a replacement child", async () => {
  const bridge = controller();
  try {
    await bridge.dispose();
    const subscribed = new Message();
    await bridge.attach(subscribed);
    assert.deepEqual(subscribed.replies, [{ returnValue: false, errorCode: "closed" }]);
    assert.equal(subscribed.cancelled, true);
  } finally { await bridge.dispose(); }
});

function oneShot(sender: unknown = PLAYER_ID): Message {
  const message = new Message();
  message.sender = sender;
  message.payload = { version: VERSION };
  message.isSubscription = false;
  return message;
}

for (const [name, change, expected] of [
  ["missing sender", (m: Message) => { m.sender = undefined; }, "unauthorized"],
  ["foreign sender with spoofed payload", (m: Message) => { m.sender = "foreign.app"; m.payload = { version: VERSION, subscribe: true, sender: PLAYER_ID }; }, "unauthorized"],
  ["unknown payload field", (m: Message) => { m.payload = { version: VERSION, subscribe: true, executable: "/bin/sh" }; }, "invalidRequest"],
  ["not a subscription", (m: Message) => { m.isSubscription = false; }, "invalidRequest"],
  ["version mismatch", (m: Message) => { m.payload = { version: "0.2.0", subscribe: true }; }, "versionMismatch"],
  ["oversized token", (m: Message) => { m.uniqueToken = "x".repeat(129); }, "invalidRequest"],
] as const) {
  test(`rejected request never starts a process: ${name}`, async () => {
    let launches = 0;
    const bridge = new Controller(() => { launches += 1; throw new Error("must not execute"); });
    const message = new Message();
    change(message);
    await bridge.attach(message);
    assert.deepEqual(message.replies, [{ returnValue: false, errorCode: expected }]);
    assert.equal(launches, 0);
    assert.equal(message.cancelled, message.isSubscription === true);
    await bridge.dispose();
  });
}

test("two exact app subscriptions overlap on one process and last release reaps it", async () => {
  const bridge = controller();
  try {
    const player = new Message();
    const ui = new Message();
    ui.sender = "com.mikestopcontinues.criterion.probe.ui";
    ui.uniqueToken = "ui-subscription";
    await bridge.attach(ui);
    await bridge.attach(player);
    const first = ui.replies[0];
    const second = player.replies[0];
    assert.ok(first?.returnValue && second?.returnValue);
    assert.equal(first.pid, second.pid);
    assert.equal(second.subscribers, 2);
    const duplicate = new Message();
    duplicate.uniqueToken = "duplicate-token";
    await bridge.attach(duplicate);
    assert.deepEqual(duplicate.replies, [{ returnValue: false, errorCode: "busy" }]);
    const foreignCancel = new Message();
    foreignCancel.sender = "foreign.app";
    await bridge.cancel(foreignCancel);
    await bridge.cancel(ui);
    const ping = oneShot();
    await bridge.ping(ping);
    const result = ping.replies[0];
    assert.ok(result?.returnValue);
    assert.equal(result.pid, first.pid);
    assert.equal(result.subscribers, 1);
    await bridge.cancel(player);
    assert.throws(() => process.kill(first.pid, 0));
    const replacement = new Message();
    replacement.uniqueToken = "replacement-subscription";
    await bridge.attach(replacement);
    const next = replacement.replies[0];
    assert.ok(next?.returnValue);
    assert.notEqual(next.pid, first.pid);
    assert.equal(next.counter, 0);
    // A cancelled old token cannot release the replacement.
    await bridge.cancel(player);
    const nextPing = oneShot();
    await bridge.ping(nextPing);
    assert.equal(nextPing.replies[0]?.returnValue, true);
  } finally { await bridge.dispose(); }
});

test("heartbeat renews the owned lease and cancellation ends publication", async () => {
  const bridge = new Controller(() => spawn("/workspace/.local/player-probe/criterion-broker-probe", ["--lease-ms", "1500", "--max-ms", "5000"], {
    shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
  }));
  try {
    const message = new Message();
    await bridge.attach(message);
    await new Promise<void>((resolve) => setTimeout(resolve, 2100));
    const latest = message.replies[message.replies.length - 1];
    assert.ok(latest?.returnValue);
    assert.ok(latest.counter >= 2);
    assert.equal(latest.state, "running");
    await bridge.cancel(message);
    const count = message.replies.length;
    await new Promise<void>((resolve) => setTimeout(resolve, 50));
    assert.equal(message.replies.length, count);
  } finally { await bridge.dispose(); }
});

test("cancel during an outstanding ping cannot publish a successful stale result", async () => {
  const bridge = new Controller(() => spawn(process.execPath, ["/workspace/tools/player-probe/dist/tests/child-fixture.js", "slow-pong"], {
    shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
  }));
  try {
    const message = new Message();
    await bridge.attach(message);
    const ping = oneShot();
    const pending = bridge.ping(ping);
    await bridge.cancel(message);
    await pending;
    assert.equal(ping.replies[0]?.returnValue, false);
  } finally { await bridge.dispose(); }
});

test("overlapping one-shot calls from one caller are bounded before awaiting cleanup", async () => {
  const bridge = controller();
  try {
    await bridge.attach(new Message());
    const first = oneShot();
    const second = oneShot();
    const pending = bridge.close(first);
    await bridge.close(second);
    await pending;
    assert.deepEqual(second.replies, [{ returnValue: false, errorCode: "busy" }]);
    assert.equal(first.replies[0]?.returnValue, true);
  } finally { await bridge.dispose(); }
});
