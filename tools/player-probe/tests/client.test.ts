import assert from "node:assert/strict";
import { test } from "node:test";
import { Client, type Request, type Requests } from "../src/client.js";
import { SERVICE_ID, VERSION } from "../src/protocol.js";

class Bus implements Requests {
  calls: { service: string; options: Parameters<Requests["request"]>[1]; cancelled: boolean }[] = [];
  request(service: string, options: Parameters<Requests["request"]>[1]): Request {
    const call = { service, options, cancelled: false };
    this.calls.push(call);
    return { cancel: () => { call.cancelled = true; } };
  }
}
const running = { returnValue: true, version: VERSION, state: "running", counter: 0, pid: 123,
  subscribers: 1, stopAcknowledged: false, cleanupConfirmed: false, exitCode: null };

test("client disposal settles an outstanding action and rejects late publication", async () => {
  const bus = new Bus();
  const published: unknown[] = [];
  const client = new Client(bus, (value) => published.push(value));
  const attaching = client.attach();
  bus.calls[0]?.options.onSuccess(running);
  assert.equal(await attaching, true);
  const pending = client.action("ping");
  await Promise.resolve();
  client.dispose();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const result = await Promise.race([pending, new Promise<string>((resolve) => { timer = setTimeout(() => resolve("unsettled"), 50); })]);
  clearTimeout(timer);
  assert.equal(result, false);
  bus.calls[1]?.options.onSuccess({ ...running, counter: 1 });
  assert.equal(published.length, 1);
  assert.equal(bus.calls.every((call) => call.cancelled), true);
});

test("client validates returned identity and counts at its external bus seam", async () => {
  const bus = new Bus();
  const published: unknown[] = [];
  const client = new Client(bus, (value) => published.push(value));
  const attaching = client.attach();
  assert.equal(bus.calls[0]?.service, `luna://${SERVICE_ID}`);
  bus.calls[0]?.options.onSuccess({ ...running, pid: "untrusted" });
  assert.equal(await attaching, false);
  assert.deepEqual(published, ["unavailable"]);
  assert.equal(bus.calls[0]?.cancelled, true);
  client.dispose();
});

test("a late ping reply cannot reopen confirmed terminal cleanup", async () => {
  const bus = new Bus();
  const published: unknown[] = [];
  const client = new Client(bus, (value) => published.push(value));
  try {
    const attaching = client.attach();
    bus.calls[0]?.options.onSuccess(running);
    await attaching;
    const ping = client.action("ping");
    await Promise.resolve();
    const closed = { ...running, state: "closed", counter: 1, cleanupConfirmed: true, exitCode: 0, stopAcknowledged: true, subscribers: 0 };
    bus.calls[0]?.options.onSuccess(closed);
    bus.calls[1]?.options.onSuccess({ ...running, counter: 1 });
    assert.equal(await ping, false);
    assert.deepEqual(published, [running, closed]);
  } finally { client.dispose(); }
});
