import assert from "node:assert/strict";
import * as fs from "node:fs";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import { decodePrerequisiteFacts, emitPrerequisiteProgram, readNativePrerequisites,
  type NativePrerequisiteFacts, type NativeRequirements, type PrerequisiteRead } from "../index.js";
import { DeviceFixture } from "./fixture.js";

const requirements: NativeRequirements = {
  appId: "com.mikestopcontinues.criterion.unofficial", executableSha256: "1".repeat(64),
  neededSonames: ["libSDL2-2.0.so.0", "libgcc_s.so.1", "librt.so.1", "libpthread.so.0",
    "libm.so.6", "libdl.so.2", "libc.so.6", "ld-linux.so.3"],
};
const small: NativeRequirements = { ...requirements, neededSonames: ["libSDL2-2.0.so.0", "libc.so.6"] };
const observe = (device: DeviceFixture, input = small) => readNativePrerequisites(input, { deadlineMs: 10000, now: () => 0, execute: device.execute });
function facts(result: PrerequisiteRead): NativePrerequisiteFacts {
  assert.equal(result.response.kind, "accepted");
  if (result.response.kind !== "accepted") throw new Error("unavailable fixture");
  return result.response.facts;
}
async function fixture(name: string, test: (device: DeviceFixture) => Promise<void>): Promise<void> {
  const device = new DeviceFixture();
  try { await test(device); console.log("PASS " + name); } finally { device.dispose(); }
}
function onLibraryRead(device: DeviceFixture, operation: () => void): void {
  let fired = false;
  device.afterRead = (fd) => {
    if (!fired && fs.readlinkSync(`/proc/self/fd/${fd}`).endsWith("/usr/lib/libfixture.so.1.2")) {
      fired = true; operation();
    }
  };
}
async function run(): Promise<void> {
  await fixture("all eight required names plus two graphics names preserve unavailable slots", async (device) => {
    const result = facts(await observe(device, requirements));
    assert.deepEqual(result.libraries.map(item => item.soname), [...requirements.neededSonames, "libEGL.so.1", "libGLESv2.so.2"]);
    assert.deepEqual(result.libraries[1], { soname: "libgcc_s.so.1", kind: "unavailable", reason: "missing" });
    const present = result.libraries[0]; assert.ok(present?.kind === "available");
    assert.equal(present.bytes, 56);
    assert.equal(present.sha256, "d0e84cc32861277927fdb920507cf854292a3009bf5ab830541e9d47fa73e6b1");
    assert.equal(device.getterCalls, 1);
  });
  await fixture("fixed root symlink resolves only inside admitted library roots", async (device) => {
    fs.rmSync(device.local("/lib"), { recursive: true }); fs.symlinkSync("usr/lib", device.local("/lib"));
    for (const name of ["libSDL2-2.0.so.0", "libc.so.6", "libEGL.so.1", "libGLESv2.so.2"]) {
      fs.symlinkSync("libfixture.so.1.2", device.local("/usr/lib/" + name));
    }
    const result = facts(await observe(device)); assert.ok(result.libraries.every(item => item.kind === "available"));
  });
  for (const [name, target] of [["escape", "/etc/passwd"], ["loop", "libSDL2-2.0.so.0"]] as const) {
    await fixture("library " + name + " is unavailable without following unsafe paths", async (device) => {
      fs.unlinkSync(device.local("/lib/libSDL2-2.0.so.0")); fs.symlinkSync(target, device.local("/lib/libSDL2-2.0.so.0"));
      const result = facts(await observe(device));
      assert.deepEqual(result.libraries[0], { soname: "libSDL2-2.0.so.0", kind: "unavailable", reason: "unsafe-path" });
      assert.ok(!device.paths.some(path => path === "/etc/passwd"));
    });
  }
  await fixture("nonregular selected entry is refused before file reading", async (device) => {
    fs.unlinkSync(device.local("/lib/libSDL2-2.0.so.0")); fs.mkdirSync(device.local("/lib/libSDL2-2.0.so.0"));
    assert.deepEqual(facts(await observe(device)).libraries[0], { soname: "libSDL2-2.0.so.0", kind: "unavailable", reason: "unsafe-path" });
  });
  await fixture("foreign-owned library is unavailable", async (device) => {
    fs.chownSync(device.local("/usr/lib/libfixture.so.1.2"), 1000, 1000);
    assert.ok(facts(await observe(device)).libraries.every(item => item.kind === "unavailable" && item.reason === "unsafe-path"));
  });
  await fixture("writable parent is unavailable", async (device) => {
    fs.chmodSync(device.local("/usr/lib"), 0o777);
    assert.ok(facts(await observe(device)).libraries.every(item => item.kind === "unavailable" && item.reason === "unsafe-path"));
  });
  await fixture("oversized library is unavailable before hashing", async (device) => {
    fs.truncateSync(device.local("/usr/lib/libfixture.so.1.2"), 32 * 1024 * 1024 + 1);
    assert.ok(facts(await observe(device)).libraries.every(item => item.kind === "unavailable" && item.reason === "file-bound"));
  });
  await fixture("non-ARM ELF header cannot become ARM metadata", async (device) => {
    const bytes = fs.readFileSync(device.local("/usr/lib/libfixture.so.1.2")); bytes.writeUInt16LE(62, 18);
    device.write("/usr/lib/libfixture.so.1.2", bytes);
    assert.ok(facts(await observe(device)).libraries.every(item => item.kind === "unavailable" && item.reason === "unsupported-elf"));
  });
  for (const mode of ["content", "replacement", "link", "epoch"] as const) {
    await fixture(mode + " changes during library read refuse the complete publication", async (device) => {
      onLibraryRead(device, () => {
        if (mode === "content") {
          const fd = fs.openSync(device.local("/usr/lib/libfixture.so.1.2"), "r+");
          try { fs.writeSync(fd, Buffer.from("evil"), 0, 4, 52); } finally { fs.closeSync(fd); }
        } else if (mode === "replacement") {
          fs.renameSync(device.local("/usr/lib/libfixture.so.1.2"), device.local("/usr/lib/prior"));
          fs.copyFileSync(device.local("/usr/lib/prior"), device.local("/usr/lib/libfixture.so.1.2"));
        } else if (mode === "link") {
          fs.unlinkSync(device.local("/lib/libSDL2-2.0.so.0")); fs.symlinkSync("/etc/passwd", device.local("/lib/libSDL2-2.0.so.0"));
        } else device.write("/proc/sys/kernel/random/boot_id", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\n");
      });
      const result = await observe(device);
      assert.equal(result.outcome?.kind, "closed");
      assert.deepEqual(result.response, { kind: "unavailable", reason: "execution" });
    });
  }
  await fixture("getter signal and timeout remain known closure without model success", async (device) => {
    device.getterStatus = null; device.getterSignal = "SIGKILL"; device.getterTimedOut = true;
    const result = facts(await observe(device));
    assert.deepEqual(result.getter, { kind: "closed", exitCode: null, signal: "SIGKILL", timedOut: true });
    assert.deepEqual(result.systemInfo, { kind: "unavailable", reason: "getter-failed" });
  });
  await fixture("closed getter with malformed reply preserves unrelated observations", async (device) => {
    device.getterResponse = { returnValue: true, modelName: "\u0000", firmwareVersion: "wrong", sdkVersion: "wrong", boardType: "wrong" };
    const result = facts(await observe(device));
    assert.deepEqual(result.systemInfo, { kind: "unavailable", reason: "getter-response" });
    assert.ok(result.libraries[0]?.kind === "available");
  });
  for (const [name, pid] of [["not-issued", 0], ["unresolved", 55]] as const) {
    await fixture("getter " + name + " never becomes a successful system observation", async (device) => {
      device.getterStatus = null; device.getterSignal = null; device.getterPid = pid;
      const result = facts(await observe(device));
      assert.deepEqual(result.getter, { kind: name });
      assert.deepEqual(result.systemInfo, { kind: "unavailable", reason: "getter-failed" });
    });
  }
  await fixture("malformed and heterogeneous CPU information remains explicitly unavailable", async (device) => {
    for (const body of ["Serial : private-only\n", "processor : 0\nCPU architecture : 7\nFeatures : vfp\n\nprocessor : 1\nCPU architecture : 8\nFeatures : vfp\n"]) {
      device.write("/proc/cpuinfo", body);
      assert.deepEqual(facts(await observe(device)).cpu, { kind: "unavailable", reason: "cpu-response" });
    }
  });
  await fixture("bounded decoder rejects altered candidate, extra fields, missing/duplicate files and CPU hash", async (device) => {
    const good = facts(await observe(device));
    for (const change of [
      (value: Record<string, unknown>) => { value.executableSha256 = "2".repeat(64); },
      (value: Record<string, unknown>) => { value.extra = true; },
      (value: Record<string, unknown>) => { value.libraries = []; },
      (value: Record<string, unknown>) => { value.libraries = [good.libraries[0], good.libraries[0], ...good.libraries.slice(2)]; },
      (value: Record<string, unknown>) => { value.cpu = { ...good.cpu, sha256: "0".repeat(64) }; },
      (value: Record<string, unknown>) => { value.getter = { kind: "closed", exitCode: null, signal: "SIGKILL", timedOut: true }; },
    ]) {
      const value = JSON.parse(JSON.stringify(good)) as Record<string, unknown>; change(value);
      assert.throws(() => decodePrerequisiteFacts(Buffer.from(JSON.stringify(value)), small));
    }
    assert.throws(() => decodePrerequisiteFacts(Buffer.alloc(65537), small));
    assert.throws(() => decodePrerequisiteFacts(Buffer.from([0xff]), small));
  });
  await fixture("requirements snapshot survives caller mutation during awaited read", async (device) => {
    const input = { ...small, neededSonames: [...small.neededSonames] };
    const result = await readNativePrerequisites(input, { deadlineMs: 10000, now: () => 0, execute: async (request) => {
      const outcome = await device.execute(request); input.executableSha256 = "2".repeat(64); input.neededSonames.pop(); return outcome;
    } });
    assert.equal(facts(result).executableSha256, "1".repeat(64));
  });
  let calls = 0;
  const never = async () => { calls++; throw new Error("unexpected execution"); };
  for (const input of [
    { ...small, neededSonames: ["../libc.so.6"] }, { ...small, neededSonames: ["libc.so.6", "libc.so.6"] },
    { ...small, neededSonames: Array.from({ length: 11 }, (_, i) => `lib${i}.so`) },
  ]) assert.deepEqual((await readNativePrerequisites(input, { deadlineMs: 10000, now: () => 0, execute: never })).response,
    { kind: "unavailable", reason: "requirements" });
  assert.equal(calls, 0); console.log("PASS malformed requirements never reach the executor");
  assert.equal((await readNativePrerequisites(small, { deadlineMs: 10, now: () => 10, execute: never })).outcome, null);
  assert.equal(calls, 0); console.log("PASS expired request is never issued");
  for (const outcome of [
    { kind: "closed" as const, exitCode: 1, signal: null, timedOut: false, stdout: Buffer.from("{}"), stderr: Buffer.alloc(0) },
    { kind: "closed" as const, exitCode: null, signal: "SIGKILL", timedOut: true, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0) },
  ]) {
    const result = await readNativePrerequisites(small, { deadlineMs: 10000, now: () => 0, execute: async () => outcome });
    assert.equal(result.outcome, outcome); assert.deepEqual(result.response, { kind: "unavailable", reason: "execution" });
  }
  const unknown = { kind: "unresolved" as const, reason: "missing-close-ack" };
  const unresolved = await readNativePrerequisites(small, { deadlineMs: 10000, now: () => 0, execute: async () => unknown });
  assert.equal(unresolved.outcome, unknown); assert.deepEqual(unresolved.response, { kind: "unavailable", reason: "closure" });
  console.log("PASS closure acknowledgement is distinct from exit and response quality");

  const sources = new Map(["index.js", "contract.js", "prerequisites.js"].map(name => ["./" + name, fs.readFileSync(join(__dirname, "..", name), "utf8")]));
  const cache = new Map<string, object>();
  const load = (name: string): object => {
    const existing = cache.get(name); if (existing) return existing;
    const code = sources.get(name); assert.ok(code, "no native imports on construction: " + name);
    const module = { exports: {} }; cache.set(name, module.exports);
    runInNewContext(code, { exports: module.exports, module, require: load, Buffer });
    return module.exports;
  };
  const library = load("./index.js"), emit: unknown = Reflect.get(library, "emitPrerequisiteProgram"); assert.equal(typeof emit, "function");
  if (typeof emit !== "function") throw new Error("missing emitter");
  const request: unknown = Reflect.apply(emit, undefined, [small, 10000, 0]);
  assert.ok(request !== null && typeof request === "object" && typeof Reflect.get(request, "source") === "string");
  assert.ok(emitPrerequisiteProgram(small, 10000, 0).source.length < 128 * 1024);
  console.log("PASS stock Node16 import and emission are inert without native modules");
}
void run().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
