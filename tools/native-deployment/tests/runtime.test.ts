import assert from "node:assert/strict";
import * as fs from "node:fs";
import { dirname, relative } from "node:path";
import { Script } from "node:vm";
import { readNativeRuntime, emitNativeRuntimeProgram, decodeNativeRuntimeFacts } from "../index.js";
import type { NativeRuntimeRead, NativeRuntimeFacts } from "../index.js";
import { RuntimeFixture, executable } from "./runtime-fixture.js";

function facts(result: NativeRuntimeRead): NativeRuntimeFacts {
  assert.equal(result.response.kind, "accepted", "one matching MAIN process with its mapped required and graphics ELF libraries is observed");
  if (result.response.kind !== "accepted") throw new Error("fixture unavailable"); return result.response.facts;
}
const observe = (device: RuntimeFixture) => readNativeRuntime(device.requirements, { deadlineMs: 10000, now: () => 0, execute: device.execute });
async function fixture(name: string, test: (device: RuntimeFixture) => Promise<void>): Promise<void> {
  const device = new RuntimeFixture(); try { await test(device); console.log("PASS " + name); } finally { device.dispose(); }
}
async function refused(device: RuntimeFixture): Promise<void> { assert.deepEqual((await observe(device)).response, { kind: "unavailable", reason: "execution" }); }
async function run(): Promise<void> {
  await fixture("compiled stock16 program observes exact MAIN process and mapped libraries", async device => {
    const observed = facts(await observe(device));
    assert.equal(observed.pid, 42); assert.equal(observed.startTicks, "456"); assert.equal(observed.bootId, "12345678-1234-1234-1234-123456789abc");
    assert.equal(observed.executable.path, executable);
    assert.equal(observed.executable.sha256, "d0e84cc32861277927fdb920507cf854292a3009bf5ab830541e9d47fa73e6b1");
    assert.deepEqual(observed.executable.elf, { class: 32, endian: "little", machine: 40, type: 3, flags: 0x05000200 });
    assert.deepEqual(observed.libraries.map(file => file.soname), ["libSDL2-2.0.so.0", "libc.so.6", "libEGL.so.1", "libGLESv2.so.2"]);
    assert.ok(observed.libraries.every(file => file.path === "/usr/lib/libfixture.so.1.2" && file.bytes === 56));
    const encoded = JSON.stringify(observed); assert.ok(!encoded.includes("criterion-unofficial) S") && !encoded.includes("r-xp") && !encoded.includes(device.root));
    new Script(emitNativeRuntimeProgram(device.requirements, 10000, 0).source);
  });
  await fixture("MAIN proc files may belong to the application UID while the observer is root", async device => {
    fs.chownSync(device.local("/proc/42/stat"), 5000, 5000); fs.chownSync(device.local("/proc/42/maps"), 5000, 5000);
    assert.equal(facts(await observe(device)).pid, 42);
  });
  await fixture("canonical installation path is resolved without a lexical proc-exe assumption", async device => {
    const directory = dirname(executable), alternate = "/media/canonical-app";
    fs.renameSync(device.local(directory), device.local(alternate));
    fs.symlinkSync(relative(dirname(directory), alternate), device.local(directory));
    fs.unlinkSync(device.local("/proc/42/exe")); fs.symlinkSync(alternate + "/criterion-unofficial", device.local("/proc/42/exe"));
    device.write("/proc/42/maps", fs.readFileSync(device.local("/proc/42/maps"), "utf8").replace(executable, alternate + "/criterion-unofficial"));
    assert.equal(facts(await observe(device)).executable.path, alternate + "/criterion-unofficial");
  });
  await fixture("duplicate fixed MAIN processes refuse a runtime identity", async device => {
    fs.mkdirSync(device.local("/proc/43")); fs.copyFileSync(device.local("/proc/42/stat"), device.local("/proc/43/stat"));
    device.write("/proc/43/stat", "43 (criterion-unofficial) S " + Array(18).fill("0").join(" ") + " 457 0\n");
    fs.symlinkSync(executable, device.local("/proc/43/exe")); await refused(device);
  });
  await fixture("a deleted or foreign-path MAIN is not a matching application", async device => {
    for (const path of [executable + " (deleted)", "/tmp/criterion-unofficial"]) {
      fs.unlinkSync(device.local("/proc/42/exe")); fs.symlinkSync(path, device.local("/proc/42/exe")); await refused(device);
    }
  });
  await fixture("missing required library mapping refuses even when its disk file exists", async device => {
    device.write("/proc/42/maps", fs.readFileSync(device.local("/proc/42/maps"), "utf8").split("\n")[0] + "\n"); await refused(device);
  });
  await fixture("mapped inode and device must match the pinned disk file", async device => {
    const original = fs.readFileSync(device.local("/proc/42/maps"), "utf8");
    device.write("/proc/42/maps", original.replace(/ ([0-9]+) \/usr\/lib/, " 999999 /usr/lib")); await refused(device);
    device.write("/proc/42/maps", original.replace(/ ([a-f0-9]+):([a-f0-9]+) ([0-9]+) \/usr\/lib/, " ffff:ffff $3 /usr/lib")); await refused(device);
  });
  await fixture("wrong executable hash and ARM hard-float header refuse admission", async device => {
    const result = await readNativeRuntime({ ...device.requirements, executableSha256: "1".repeat(64) }, { deadlineMs: 10000, now: () => 0, execute: device.execute });
    assert.equal(result.response.kind, "unavailable");
    const bytes = fs.readFileSync(device.local("/usr/lib/libfixture.so.1.2")); bytes.writeUInt32LE(0x05000400, 36); device.write("/usr/lib/libfixture.so.1.2", bytes); await refused(device);
  });
  await fixture("library symlink escape and writable executable refuse before external content reading", async device => {
    fs.unlinkSync(device.local("/lib/libSDL2-2.0.so.0")); fs.symlinkSync("/etc/passwd", device.local("/lib/libSDL2-2.0.so.0")); await refused(device);
    assert.ok(!device.paths.includes("/etc/passwd"));
    fs.unlinkSync(device.local("/lib/libSDL2-2.0.so.0")); fs.symlinkSync("../usr/lib/libfixture.so.1.2", device.local("/lib/libSDL2-2.0.so.0"));
    fs.chmodSync(device.local(executable), 0o777); await refused(device);
  });
  for (const mode of ["boot", "start", "mapping", "file", "replacement", "duplicate"] as const) {
    await fixture(mode + " drift while files are hashed refuses publication", async device => {
      let fired = false;
      device.afterRead = fd => {
        if (fired || !fs.readlinkSync(`/proc/self/fd/${fd}`).endsWith("/usr/lib/libfixture.so.1.2")) return; fired = true;
        if (mode === "boot") device.write("/proc/sys/kernel/random/boot_id", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\n");
        if (mode === "start") device.write("/proc/42/stat", "42 (criterion-unofficial) S " + Array(18).fill("0").join(" ") + " 457 0\n");
        if (mode === "mapping") device.write("/proc/42/maps", fs.readFileSync(device.local("/proc/42/maps"), "utf8").replace("3000-4000 r-xp", "3000-4000 r--p"));
        if (mode === "file") device.write("/usr/lib/libfixture.so.1.2", Buffer.alloc(56));
        if (mode === "replacement") { fs.renameSync(device.local(executable), device.local(executable + ".old")); fs.copyFileSync(device.local(executable + ".old"), device.local(executable)); }
        if (mode === "duplicate") { fs.mkdirSync(device.local("/proc/43")); device.write("/proc/43/stat", "43 (criterion-unofficial) S " + Array(18).fill("0").join(" ") + " 789 0\n"); fs.symlinkSync(executable, device.local("/proc/43/exe")); }
      };
      await refused(device); assert.ok(fired);
    });
  }
  await fixture("overbound proc maps and ELF files refuse without unbounded buffers", async device => {
    device.write("/proc/42/maps", "x".repeat(1024 * 1024 + 1)); await refused(device);
    device.maps(); fs.truncateSync(device.local(executable), 32 * 1024 * 1024 + 1); await refused(device);
  });
  await fixture("proc scan and line budgets refuse excessive inventory", async device => {
    for (let pid = 100; pid < 2148; pid++) fs.mkdirSync(device.local("/proc/" + pid));
    await refused(device);
  });
  await fixture("malformed process state and map framing cannot supply facts", async device => {
    device.write("/proc/42/stat", "42 (criterion-unofficial) Z " + Array(18).fill("0").join(" ") + " 456 0\n"); await refused(device);
    device.write("/proc/42/stat", "42 (criterion-unofficial) S " + Array(18).fill("0").join(" ") + " 456 0\n");
    device.write("/proc/42/maps", "not-a-linux-mapping\n"); await refused(device);
  });
  await fixture("unsupported requirements never dispatch a read", async device => {
    let calls = 0; const execute = async () => { calls++; return { kind: "unresolved" as const, reason: "should-not-run" }; };
    for (const requirements of [{ ...device.requirements, appId: "another-app" }, { ...device.requirements, neededSonames: ["../libc.so.6"] }, { ...device.requirements, neededSonames: ["libc.so.6", "libc.so.6"] }, { ...device.requirements, extra: "private" }]) {
      const result = await readNativeRuntime(requirements as typeof device.requirements, { deadlineMs: 10000, now: () => 0, execute });
      assert.deepEqual(result.response, { kind: "unavailable", reason: "requirements" });
    }
    assert.equal(calls, 0);
  });
  await fixture("nonmatching processes do not become output identities", async device => {
    fs.mkdirSync(device.local("/proc/90")); fs.symlinkSync("/bin/unrelated-private-name", device.local("/proc/90/exe"));
    assert.ok(!JSON.stringify(facts(await observe(device))).includes("unrelated-private-name"));
  });
  await fixture("original deadline and closure gate one read without retry", async device => {
    let calls = 0;
    const execute = async (program: Parameters<typeof device.execute>[0]) => { calls++; return device.execute(program); };
    assert.deepEqual((await readNativeRuntime(device.requirements, { deadlineMs: 10, now: () => 10, execute })).response, { kind: "unavailable", reason: "deadline" }); assert.equal(calls, 0);
    let clock = 0;
    const late = await readNativeRuntime(device.requirements, { deadlineMs: 10000, now: () => clock, execute: async program => { const result = await execute(program); assert.ok(result.kind === "closed" && result.exitCode === 0); clock = 10000; return result; } });
    assert.deepEqual(late.response, { kind: "unavailable", reason: "deadline" }); assert.equal(calls, 1);
    const unresolved = await readNativeRuntime(device.requirements, { deadlineMs: 10000, now: () => 0, execute: async () => ({ kind: "unresolved", reason: "private-do-not-publish" }) });
    assert.deepEqual(unresolved.response, { kind: "unavailable", reason: "closure" });
    let reads = 0; const backwards = await readNativeRuntime(device.requirements, { deadlineMs: 10000, now: () => reads++ ? 0 : 1, execute });
    assert.deepEqual(backwards.response, { kind: "unavailable", reason: "deadline" });
  });
  await fixture("response bytes and captured requirements are validated after closure", async device => {
    const raw = await device.execute(emitNativeRuntimeProgram(device.requirements, 10000, 0)); assert.ok(raw.kind === "closed");
    const input = JSON.parse(raw.stdout.toString()) as NativeRuntimeFacts;
    const malformed = [Buffer.from([0xff]), Buffer.from("[]"), Buffer.from(JSON.stringify({ ...input, pid: 0 })), Buffer.from(JSON.stringify({ ...input, extra: "private" })), Buffer.alloc(65537)];
    for (const bytes of malformed) assert.throws(() => decodeNativeRuntimeFacts(bytes, device.requirements));
    const mutable = { ...device.requirements, neededSonames: [...device.requirements.neededSonames] };
    const result = await readNativeRuntime(mutable, { deadlineMs: 10000, now: () => 0, execute: async () => { mutable.executableSha256 = "1".repeat(64); mutable.neededSonames.push("evil.so"); return raw; } });
    assert.equal(result.response.kind, "accepted");
    for (const outcome of [{ ...raw, signal: "SIGTERM", exitCode: null }, { ...raw, timedOut: true }, { ...raw, stderr: Buffer.alloc(4097) }]) {
      const response = await readNativeRuntime(device.requirements, { deadlineMs: 10000, now: () => 0, execute: async () => outcome }); assert.equal(response.response.kind, "unavailable");
    }
  });
}
void run().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
