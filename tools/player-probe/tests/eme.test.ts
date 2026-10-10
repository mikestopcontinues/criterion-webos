import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import { Client, type Requests } from "../src/client.js";
import { EmeProbe, type BridgeWitness, type EmeEnvironment, type EmeDependencies } from "../src/eme.js";
import { VERSION } from "../src/protocol.js";

class Clock {
  time = 0;
  private timers = new Map<() => void, number>();
  schedule = (delay: number, callback: () => void) => {
    this.timers.set(callback, this.time + delay);
    return () => { this.timers.delete(callback); };
  };
  advance(milliseconds: number): void {
    this.time += milliseconds;
    for (const [callback, at] of this.timers) if (at <= this.time) {
      this.timers.delete(callback); callback();
    }
  }
}
const flush = async () => { for (let n = 0; n < 12; n++) await Promise.resolve(); };
const hardware: MediaKeySystemConfiguration = {
  label: "hw-video", initDataTypes: ["cenc"], sessionTypes: ["temporary"],
  distinctiveIdentifier: "not-allowed", persistentState: "not-allowed",
  audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"', robustness: "SW_SECURE_CRYPTO", encryptionScheme: "cenc" }],
  videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.640028"', robustness: "HW_SECURE_ALL", encryptionScheme: "cenc" }],
};
const software: MediaKeySystemConfiguration = {
  label: "sw-crypto", initDataTypes: ["cenc"], sessionTypes: ["temporary"],
  distinctiveIdentifier: "not-allowed", persistentState: "not-allowed",
  audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"', robustness: "SW_SECURE_CRYPTO", encryptionScheme: "cenc" }],
  videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.640028"', robustness: "SW_SECURE_CRYPTO", encryptionScheme: "cenc" }],
};
function fixture() {
  const clock = new Clock();
  const environment: EmeEnvironment = { player: true, secure: true, topLevel: true, visible: true };
  let witness: BridgeWitness | undefined;
  let owner: EmeProbe;
  let counter = 0;
  let pingDelay = 0;
  const messages: Parameters<Requests["request"]>[1][] = [];
  const running = () => ({ returnValue: true, version: VERSION, state: "running", pid: 123,
    counter, subscribers: 1, stopAcknowledged: false, cleanupConfirmed: false, exitCode: null });
  const client = new Client({ request: (_service, options) => {
    messages.push(options);
    if (options.method === "attach") options.onSuccess(running());
    if (options.method === "ping") { clock.advance(pingDelay); counter++; options.onSuccess(running()); }
    return { cancel() {} };
  } }, (value) => {
    witness = value !== "unavailable" && value.state === "running"
      ? { pid: value.pid, counter: value.counter, at: clock.time } : undefined;
    if (!witness) owner?.stop();
  });
  const calls: { keySystem: string; configurations: MediaKeySystemConfiguration[] }[] = [];
  const published: string[] = [];
  let answer: (index: number) => Promise<unknown> = (index) => Promise.resolve({
    keySystem: "com.widevine.alpha", getConfiguration: () => index === 0 ? hardware : software,
  });
  const dependencies: EmeDependencies = { environment: () => environment, bridge: () => witness,
    ping: () => client.action("ping"), request: (keySystem, configurations) => {
      calls.push({ keySystem, configurations }); return answer(calls.length - 1);
    }, now: () => clock.time, schedule: clock.schedule, publish: (report) => published.push(report) };
  owner = new EmeProbe(dependencies);
  return { owner, client, clock, environment, calls, published, messages,
    answer: (next: typeof answer) => { answer = next; },
    witness: (next: BridgeWitness | undefined) => { witness = next; },
    running, dependencies, pingDelay: (delay: number) => { pingDelay = delay; },
  };
}

test("the actual owner measures two independent exact configurations once after the Client ping", async () => {
  const f = fixture();
  try {
    assert.equal(await f.client.attach(), true);
    await f.owner.run(); await f.owner.run();
    assert.equal(f.calls.length, 2);
    assert.deepEqual(f.calls, [
      { keySystem: "com.widevine.alpha", configurations: [hardware] },
      { keySystem: "com.widevine.alpha", configurations: [software] },
    ]);
    assert.deepEqual(f.messages.map((message) => message.method), ["attach", "ping"]);
    assert.match(f.owner.report(), /hw-video.*resolved/);
    assert.match(f.owner.report(), /sw-crypto.*resolved/);
    assert.ok(f.owner.report().length <= 1024);
    await flush();
  } finally { f.owner.stop(); f.client.dispose(); }
});

test("a pending access times out after six seconds and late settlement closes only its witness", async () => {
  const f = fixture();
  let resolve: ((value: unknown) => void) | undefined;
  let inspected = 0;
  const pending = new Promise<unknown>((done) => { resolve = done; });
  f.answer(() => pending);
  let running: Promise<void> | undefined;
  try {
    await f.client.attach();
    let finished = false;
    running = f.owner.run().then(() => { finished = true; });
    await flush();
    assert.equal(f.calls.length, 1);
    f.clock.advance(6000); await flush();
    assert.equal(finished, true);
    assert.match(f.owner.report(), /hw-video.*settled=false.*timeout/);
    const publications = f.published.length;
    resolve?.({ keySystem: "com.widevine.alpha", getConfiguration: () => { inspected++; return hardware; } });
    await flush(); await f.owner.run();
    assert.match(f.owner.report(), /hw-video.*settled=true.*timeout/);
    assert.equal(f.calls.length, 1);
    assert.equal(inspected, 0);
    assert.equal(f.published.length, publications);
  } finally { f.owner.stop(); resolve?.({}); await running; f.client.dispose(); }
});

for (const field of ["player", "secure", "topLevel", "visible"] as const) {
  test(`the ${field} guard refuses before the action-time ping or any EME submission`, async () => {
    const f = fixture();
    try {
      await f.client.attach(); f.environment[field] = false;
      await f.owner.run();
      assert.equal(f.calls.length, 0);
      assert.equal(f.messages.length, 1);
    } finally { f.owner.stop(); f.client.dispose(); }
  });
}

test("an expired current broker witness after the awaited response suppresses projection and the second query", async () => {
  const f = fixture();
  let inspected = 0;
  try {
    await f.client.attach();
    f.answer(() => {
      f.clock.advance(1001);
      return Promise.resolve({ keySystem: "com.widevine.alpha", getConfiguration: () => { inspected++; return hardware; } });
    });
    await f.owner.run();
    assert.equal(f.calls.length, 1);
    assert.equal(inspected, 0);
    assert.equal(f.published.length, 0);
  } finally { f.owner.stop(); f.client.dispose(); }
});

test("access resolution cannot admit a malformed or permission-expanded selected configuration", async () => {
  for (const selected of [{ ...hardware, persistentState: "required" }, { ...hardware, externalField: "private-synthetic-value" }, { ...hardware, sessionTypes: ["persistent-license"] }]) {
    const f = fixture();
    try {
      await f.client.attach();
      f.answer(() => Promise.resolve({ keySystem: "com.widevine.alpha", getConfiguration: () => selected }));
      await f.owner.run();
      assert.equal(f.calls.length, 1);
      assert.equal(f.owner.report().includes("invalid"), true);
      assert.equal(f.owner.report().includes("private-synthetic-value"), false);
    } finally { f.owner.stop(); f.client.dispose(); }
  }
});

test("absent and null selected schemes stay distinct and cannot claim cenc", async () => {
  const f = fixture();
  try {
    await f.client.attach();
    const selected = { ...hardware,
      audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"', robustness: "SW_SECURE_CRYPTO" }],
      videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.640028"', robustness: "HW_SECURE_ALL", encryptionScheme: null }],
    };
    f.answer((index) => Promise.resolve({ keySystem: "com.widevine.alpha", getConfiguration: () => index === 0 ? selected : software }));
    await f.owner.run();
    assert.equal(f.calls.length, 2);
    assert.equal(f.owner.report().includes("audioScheme=absent videoScheme=null cenc=false"), true);
    assert.equal(f.owner.report().includes("audioScheme=cenc videoScheme=cenc cenc=true"), true);
  } finally { f.owner.stop(); f.client.dispose(); }
});

test("the original twelve seconds includes ping and bounds the second query without a fresh budget", async () => {
  const f = fixture();
  let first: ((value: unknown) => void) | undefined;
  let second: ((value: unknown) => void) | undefined;
  const pendingFirst = new Promise<unknown>((resolve) => { first = resolve; });
  const pendingSecond = new Promise<unknown>((resolve) => { second = resolve; });
  let running: Promise<void> | undefined;
  try {
    await f.client.attach(); f.pingDelay(2999);
    f.answer((index) => index === 0 ? pendingFirst : pendingSecond);
    running = f.owner.run(); await flush();
    assert.equal(f.clock.time, 2999); assert.equal(f.calls.length, 1);
    f.clock.advance(5999);
    f.messages[0]?.onSuccess(f.running());
    first?.({ keySystem: "com.widevine.alpha", getConfiguration: () => hardware });
    await flush(); assert.equal(f.calls.length, 2);
    f.clock.advance(3002); await flush(); await running;
    assert.equal(f.clock.time, 12000);
    assert.equal(f.owner.report().includes("sw-crypto attempted=true settled=false timeout"), true);
  } finally { f.owner.stop(); first?.({}); second?.({}); await running; f.client.dispose(); }
});

test("projection time is inside the original deadline and cannot retain an admitted selection", async () => {
  const f = fixture();
  try {
    await f.client.attach();
    f.answer(() => Promise.resolve({ keySystem: "com.widevine.alpha", getConfiguration: () => { f.clock.advance(12000); return hardware; } }));
    await f.owner.run();
    assert.equal(f.calls.length, 1);
    assert.equal(f.owner.report().includes("cenc=true"), false);
  } finally { f.owner.stop(); f.client.dispose(); }
});

test("a synchronous submission failure is settled, whereas an unrecognized rejected error stops discovery", async () => {
  for (const synchronous of [true, false]) {
    const f = fixture();
    try {
      await f.client.attach();
      f.answer(() => {
        const error = { name: "unknown-private-name", get message(): never { throw new Error("forbidden message getter"); } };
        if (synchronous) throw error;
        return Promise.reject(error);
      });
      await f.owner.run();
      assert.equal(f.calls.length, 1);
      assert.equal(f.owner.report().includes("settled=true"), true);
      assert.equal(f.owner.report().includes("error=other"), true);
      assert.equal(f.owner.report().includes("unknown-private-name"), false);
    } finally { f.owner.stop(); f.client.dispose(); }
  }
});

test("missing API, missing attachment and stale snapshot refuse before ping", async () => {
  for (const refusal of ["api", "attachment", "stale"] as const) {
    const f = fixture();
    try {
      await f.client.attach();
      if (refusal === "api") f.dependencies.request = undefined;
      if (refusal === "attachment") f.witness(undefined);
      if (refusal === "stale") f.clock.advance(1001);
      await f.owner.run();
      assert.equal(f.calls.length, 0); assert.equal(f.messages.length, 1);
    } finally { f.owner.stop(); f.client.dispose(); }
  }
});

test("the actual Client rejects changed broker identity while issued access settles without projection", async () => {
  const f = fixture();
  let resolve: ((value: unknown) => void) | undefined;
  let inspected = 0;
  const pending = new Promise<unknown>((done) => { resolve = done; });
  f.answer(() => pending);
  try {
    await f.client.attach(); const running = f.owner.run(); await flush();
    assert.equal(f.calls.length, 1);
    const publications = f.published.length;
    f.messages[0]?.onSuccess({ ...f.running(), pid: 999 });
    await flush(); await running;
    resolve?.({ keySystem: "com.widevine.alpha", getConfiguration: () => { inspected++; return hardware; } });
    await flush();
    assert.equal(f.calls.length, 1); assert.equal(inspected, 0);
    assert.equal(f.owner.report().includes("settled=true interrupted"), true);
    assert.equal(f.published.length, publications);
  } finally { f.owner.stop(); resolve?.({}); f.client.dispose(); }
});

test("a settled unsupported result allows only the other declared measurement without touching forbidden access surfaces", async () => {
  const f = fixture();
  let forbidden = 0;
  const trap = () => { forbidden++; throw new Error("forbidden fixture surface"); };
  try {
    await f.client.attach();
    const access = { keySystem: "com.widevine.alpha", getConfiguration: () => software };
    for (const key of ["createMediaKeys", "createSession", "generateRequest", "update", "setServerCertificate", "getStatusForPolicy"]) {
      Object.defineProperty(access, key, { get: trap });
    }
    f.answer((index) => index === 0 ? Promise.reject({ name: "NotSupportedError", get message(): never { return trap(); } }) : Promise.resolve(access));
    await f.owner.run();
    assert.equal(f.calls.length, 2); assert.equal(forbidden, 0);
    assert.equal(f.owner.report().includes("refused error=unsupported"), true);
    assert.deepEqual(f.calls[1]?.configurations, [software]);
  } finally { f.owner.stop(); f.client.dispose(); }
});

test("wrong key system, selected codec or robustness and throwing platform getters stop without retaining values", async () => {
  const cases: unknown[] = [
    { keySystem: "private-wrong-system", getConfiguration: () => hardware },
    { get keySystem(): never { throw new Error("private-getter-failure"); }, getConfiguration: () => hardware },
    { keySystem: "com.widevine.alpha", getConfiguration(): never { throw new Error("private-getter-failure"); } },
    { keySystem: "com.widevine.alpha", getConfiguration: () => ({ ...hardware,
      videoCapabilities: [{ contentType: "private-wrong-codec", robustness: "HW_SECURE_ALL", encryptionScheme: "cenc" }] }) },
    { keySystem: "com.widevine.alpha", getConfiguration: () => ({ ...hardware,
      videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.640028"', robustness: "", encryptionScheme: "cenc" }] }) },
    { keySystem: "com.widevine.alpha", getConfiguration: () => ({ ...hardware,
      audioCapabilities: [hardware.audioCapabilities?.[0], hardware.audioCapabilities?.[0]] }) },
  ];
  for (const value of cases) {
    const f = fixture();
    try {
      await f.client.attach(); f.answer(() => Promise.resolve(value)); await f.owner.run();
      assert.equal(f.calls.length, 1);
      assert.equal(f.owner.report().includes("invalid"), true);
      assert.equal(f.owner.report().includes("private-"), false);
      assert.equal(f.published[f.published.length - 1]?.includes("invalid"), true);
    } finally { f.owner.stop(); f.client.dispose(); }
  }
});

class Element {
  textContent: string | null = null;
  disabled = true;
  hidden = false;
  readonly listeners = new Map<string, () => void>();
  addEventListener(event: string, listener: () => void): void { this.listeners.set(event, listener); }
  removeEventListener(event: string): void { this.listeners.delete(event); }
  focus(): void {}
  click(): void { this.listeners.get("click")?.(); }
}

for (const role of ["player", "ui"] as const) for (const pending of [false, true]) {
  test(`actual WAM ${role} composition preserves explicit once ownership${pending ? " through pending pagehide" : " on relaunch"}`, async () => {
    const elements = new Map(["status", "capabilities", "transfer", "ping", "close", "eme", "eme-results"].map((id) => [id, new Element()]));
    const events = new Map<string, () => void>();
    let counter = 0;
    let requests = 0;
    let forbidden = 0;
    let inspected = 0;
    let settle: ((value: unknown) => void) | undefined;
    const delayed = new Promise<unknown>((resolve) => { settle = resolve; });
    const trap = () => { forbidden++; throw new Error("forbidden fixture surface"); };
    const bus: Requests = { request: (_service, options) => {
      if (options.method === "ping") counter++;
      options.onSuccess({ returnValue: true, version: VERSION, state: "running", pid: 123,
        counter, subscribers: 1, stopAcknowledged: false, cleanupConfirmed: false, exitCode: null });
      return { cancel() {} };
    } };
    const window = { webOS: { service: bus, platformBack: trap }, webOSSystem: { activate() {} }, isSecureContext: true,
      addEventListener: (event: string, listener: () => void) => events.set(event, listener),
      removeEventListener: (event: string) => events.delete(event), top: {} };
    window.top = window;
    const sandbox = { PROBE_ROLE: role, window,
      document: { getElementById: (id: string) => elements.get(id), querySelector: (selector: string) => elements.get(selector.slice(1)),
        visibilityState: "visible", addEventListener() {}, removeEventListener() {} },
      navigator: { requestMediaKeySystemAccess: () => {
        const index = requests++;
        if (pending) return delayed;
        return Promise.resolve({ keySystem: "com.widevine.alpha", getConfiguration: () => index === 0 ? hardware : software,
          createMediaKeys: trap, getStatusForPolicy: trap });
      } }, MediaSource: trap, MediaKeys: trap, MediaKeySession: trap, Audio: trap, fetch: trap,
      XMLHttpRequest: trap, localStorage: { getItem: trap, setItem: trap },
      performance: { now: () => 0 }, setTimeout, clearTimeout, exports: {},
      require: createRequire(join(__dirname, "../src/wam.js")),
    };
    try {
      runInNewContext(readFileSync(join(__dirname, "../src/wam.js"), "utf8"), sandbox);
      await flush(); assert.equal(requests, 0);
      elements.get("eme")?.click(); await flush();
      assert.equal(requests, role === "player" ? pending ? 1 : 2 : 0);
      const retainedText = elements.get("eme-results")?.textContent;
      if (pending) {
        events.get("pagehide")?.();
        settle?.({ keySystem: "com.widevine.alpha", getConfiguration: () => { inspected++; return hardware; } });
        await flush();
        assert.equal(elements.get("eme-results")?.textContent, retainedText);
        assert.equal(inspected, 0);
      }
      events.get("webOSRelaunch")?.(); elements.get("eme")?.click(); await flush();
      assert.equal(requests, role === "player" ? pending ? 1 : 2 : 0);
      assert.equal(forbidden, 0);
      if (role === "player" && !pending) assert.equal(elements.get("eme-results")?.textContent?.includes("cenc=true"), true);
      const html = readFileSync("/workspace/tools/player-probe/packaging/index.html", "utf8");
      assert.equal(html.includes('id="eme"'), true);
      assert.equal(html.includes("connect-src 'none'; media-src 'none'"), true);
    } finally { events.get("pagehide")?.(); }
  });
}
