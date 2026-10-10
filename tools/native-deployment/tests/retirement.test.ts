import assert from "node:assert/strict";
import * as fs from "node:fs";
import { dirname, relative } from "node:path";
import { Script, runInNewContext } from "node:vm";
import { readNativeRuntime, type NativeRuntimeRead, type NativeRuntimeFacts } from "../runtime.js";
import { readNativeRetirement, emitNativeRetirementProgram, type NativeRetirementRead } from "../retirement.js";
import type { ActualReadOutcome, StockNode16ReadProgram } from "../contract.js";
import { RuntimeFixture, executable } from "./runtime-fixture.js";

/** VM filesystem adapter only; target /proc/exe links resolve within this fixture. */
class RetirementFixture extends RuntimeFixture {
  runtime = { platform: "linux", arch: "arm", versions: { node: "16.20.2" }, geteuid: () => 0, hrtime: process.hrtime };
  executeRetirement = async (program: StockNode16ReadProgram): Promise<ActualReadOutcome> => {
    let stdout = "", stderr = "", exitCode = 0;
    const filesystem = new Proxy(fs, { get: (target, key) => {
      const value: unknown = Reflect.get(target, key); if (typeof value !== "function") return value;
      if (key === "readSync") return (...args: unknown[]) => {
        const result: unknown = Reflect.apply(value, target, args); if (typeof args[0] === "number") this.afterRead?.(args[0]); return result;
      };
      if (!["openSync", "lstatSync", "readlinkSync", "opendirSync", "realpathSync"].includes(String(key))) return value;
      return (...args: unknown[]) => {
        if (typeof args[0] === "string") {
          const path = args[0]; this.paths.push(path); this.beforeFs?.(String(key), path);
          assert.ok(path.startsWith("/") && !path.includes("/../"), path);
          const local = /^\/proc\/self\/fd\/\d+(?:\/|$)/.test(path) ? path : this.local(path === "/" ? "" : path);
          args[0] = key === "openSync" && path.endsWith("/exe") ? this.local(fs.readlinkSync(local)) : local;
        }
        const result: unknown = Reflect.apply(value, target, args);
        return key === "realpathSync" && typeof result === "string" ? result.slice(this.root.length) : result;
      };
    } });
    runInNewContext(program.source, { Buffer, require: (name: string): unknown => {
      if (name === "node:fs") return filesystem;
      if (name === "node:crypto") return require(name) as unknown;
      throw new Error("unexpected module: " + name);
    }, process: { ...this.runtime, stdout: { write: (text: string) => { stdout += text; } }, stderr: { write: (text: string) => { stderr += text; } },
      get exitCode() { return exitCode; }, set exitCode(value: number) { exitCode = value; } },
    }, { timeout: Math.min(10000, program.timeoutMs) });
    return { kind: "closed", exitCode, signal: null, timedOut: false, stdout: Buffer.from(stdout), stderr: Buffer.from(stderr) };
  };
  process(pid: number, start: string, path: string | null): void {
    fs.mkdirSync(this.local("/proc/" + pid));
    this.write(`/proc/${pid}/stat`, `${pid} (unrelated-private-name) S ` + Array(18).fill("0").join(" ") + ` ${start} 0\n`);
    if (path) fs.symlinkSync(path, this.local(`/proc/${pid}/exe`)); else { this.write(`/proc/${pid}/maps`, ""); fs.mkdirSync(this.local(`/proc/${pid}/task/${pid}`), { recursive: true }); }
  }
  gone(): void { fs.rmSync(this.local("/proc/42"), { recursive: true }); }
}
const observe = (device: RuntimeFixture) => readNativeRuntime(device.requirements, { deadlineMs: 10000, now: () => 0, execute: device.execute });
const retire = (device: RetirementFixture, admitted: NativeRuntimeRead) => readNativeRetirement(admitted, device.requirements, { deadlineMs: 10000, now: () => 0, execute: device.executeRetirement });
async function fixture(name: string, test: (device: RetirementFixture, admitted: NativeRuntimeRead) => Promise<void>): Promise<void> {
  const device = new RetirementFixture();
  try {
    const admitted = await observe(device); assert.equal(admitted.response.kind, "accepted");
    await test(device, admitted); console.log("PASS " + name);
  } finally { device.dispose(); }
}
function uncertain(result: NativeRetirementRead, reason: "admission" | "deadline" | "closure" | "execution" | "response" = "execution"): void {
  assert.deepEqual(result.response, { kind: "uncertain", reason });
}
async function run(): Promise<void> {
  await fixture("admitted MAIN positively absent after retirement", async (device, admitted) => {
    device.gone(); const retired = await retire(device, admitted);
    assert.equal(retired.response.kind, "retired", "the admitted MAIN is positively gone under the same boot, with its installed file intact and no replacement MAIN");
    if (retired.response.kind !== "retired") throw new Error("fixture uncertain");
    assert.deepEqual(retired.response.facts, { schemaVersion: 1, appId: "com.mikestopcontinues.criterion.unofficial",
      bootId: "12345678-1234-1234-1234-123456789abc", pid: 42, startTicks: "456",
      executable: { path: executable, bytes: 56, sha256: "d0e84cc32861277927fdb920507cf854292a3009bf5ab830541e9d47fa73e6b1",
        device: admitted.response.kind === "accepted" ? admitted.response.facts.executable.device : "", inode: admitted.response.kind === "accepted" ? admitted.response.facts.executable.inode : "" } });
    new Script(emitNativeRetirementProgram(admitted, device.requirements, 10000, 0).source);
    assert.ok(!JSON.stringify(retired.response).includes(device.root));
  });
  await fixture("still alive owned PID is uncertainty", async (device, admitted) => { uncertain(await retire(device, admitted)); });
  await fixture("reused owned PID cannot certify retirement even with a foreign executable", async (device, admitted) => {
    device.write("/proc/42/stat", "42 (unrelated) S " + Array(18).fill("0").join(" ") + " 999 0\n");
    fs.unlinkSync(device.local("/proc/42/exe")); fs.symlinkSync("/usr/lib/libfixture.so.1.2", device.local("/proc/42/exe")); uncertain(await retire(device, admitted));
  });
  for (const path of [executable, "/tmp/criterion-unofficial", executable + " (deleted)"]) {
    await fixture("new or foreign MAIN path refuses: " + path, async (device, admitted) => { device.gone(); device.process(43, "789", path); uncertain(await retire(device, admitted)); });
  }
  await fixture("captured executable inode under a different name refuses", async (device, admitted) => {
    device.gone(); fs.linkSync(device.local(executable), device.local("/usr/lib/renamed-private")); device.process(43, "789", "/usr/lib/renamed-private"); uncertain(await retire(device, admitted));
  });
  await fixture("stable unrelated and exeless empty-map processes stay private", async (device, admitted) => {
    device.gone(); device.process(90, "900", "/usr/lib/libfixture.so.1.2"); device.process(91, "901", null);
    const result = await retire(device, admitted); assert.equal(result.response.kind, "retired");
    assert.ok(!JSON.stringify(result.response).includes("unrelated-private-name") && !JSON.stringify(result.response).includes("libfixture"));
  });
  await fixture("exe-less leader with another surviving TID cannot certify MAIN absence", async (device, admitted) => {
    device.gone(); device.process(90, "900", null);
    fs.mkdirSync(device.local("/proc/90/task/91"));
    uncertain(await retire(device, admitted));
  });
  await fixture("unavailable or symlinked task directory is uncertainty", async (device, admitted) => {
    device.gone(); device.process(90, "900", null);
    fs.renameSync(device.local("/proc/90/task"), device.local("/proc/90/task-hidden"));
    uncertain(await retire(device, admitted));
    fs.symlinkSync("task-hidden", device.local("/proc/90/task")); uncertain(await retire(device, admitted));
  });
  for (const mode of ["another-tid", "replacement", "missing-leader"] as const) {
    await fixture("exe-less task " + mode + " drift around map observation is uncertainty", async (device, admitted) => {
      device.gone(); device.process(90, "900", null); let fired = false;
      device.beforeFs = (operation, path) => {
        if (fired || operation !== "lstatSync" || !path.endsWith("/maps")) return; fired = true;
        if (mode === "another-tid") fs.mkdirSync(device.local("/proc/90/task/91"));
        if (mode === "replacement") {
          fs.renameSync(device.local("/proc/90/task"), device.local("/proc/90/task-old"));
          fs.mkdirSync(device.local("/proc/90/task/90"), { recursive: true });
        }
        if (mode === "missing-leader") fs.rmSync(device.local("/proc/90/task/90"), { recursive: true });
      };
      uncertain(await retire(device, admitted)); assert.ok(fired);
    });
  }
  await fixture("exeless process with mappings or unreadable metadata is uncertainty", async (device, admitted) => {
    device.gone(); device.process(90, "900", null); device.write("/proc/90/maps", "1000-2000 r-xp 00000000 00:00 1 /unknown\n"); uncertain(await retire(device, admitted));
    device.write("/proc/90/maps", ""); fs.unlinkSync(device.local("/proc/90/stat")); uncertain(await retire(device, admitted));
  });
  await fixture("changed kernel boot cannot certify retirement", async (device, admitted) => {
    device.gone(); device.write("/proc/sys/kernel/random/boot_id", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\n"); uncertain(await retire(device, admitted));
  });
  await fixture("missing, replaced, changed or writable installed MAIN stays uncertain", async (device, admitted) => {
    device.gone(); const original = fs.readFileSync(device.local(executable));
    fs.chmodSync(device.local(executable), 0o777); uncertain(await retire(device, admitted)); fs.chmodSync(device.local(executable), 0o755);
    device.write(executable, Buffer.alloc(original.length)); uncertain(await retire(device, admitted)); device.write(executable, original);
    fs.renameSync(device.local(executable), device.local(executable + ".old")); uncertain(await retire(device, admitted));
    fs.copyFileSync(device.local(executable + ".old"), device.local(executable)); uncertain(await retire(device, admitted));
  });
  await fixture("canonical installed path may contain symlink ancestry", async device => {
    const directory = dirname(executable), alternate = "/media/canonical-app";
    fs.renameSync(device.local(directory), device.local(alternate)); fs.symlinkSync(relative(dirname(directory), alternate), device.local(directory));
    fs.unlinkSync(device.local("/proc/42/exe")); fs.symlinkSync(alternate + "/criterion-unofficial", device.local("/proc/42/exe"));
    device.write("/proc/42/maps", fs.readFileSync(device.local("/proc/42/maps"), "utf8").replace(executable, alternate + "/criterion-unofficial"));
    const admitted = await observe(device); assert.equal(admitted.response.kind, "accepted"); device.gone(); assert.equal((await retire(device, admitted)).response.kind, "retired");
  });
  for (const mode of ["boot", "owned-pid", "new-main", "file", "canonical-path"] as const) {
    await fixture(mode + " drift during executable hashing refuses publication", async (device, admitted) => {
      device.gone(); let fired = false;
      device.afterRead = fd => {
        if (fired || !fs.readlinkSync(`/proc/self/fd/${fd}`).endsWith("criterion-unofficial")) return; fired = true;
        if (mode === "boot") device.write("/proc/sys/kernel/random/boot_id", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\n");
        if (mode === "owned-pid") device.process(42, "999", "/usr/lib/libfixture.so.1.2");
        if (mode === "new-main") device.process(43, "999", executable);
        if (mode === "file") device.write(executable, Buffer.alloc(56));
        if (mode === "canonical-path") { fs.renameSync(device.local(executable), device.local(executable + ".old")); fs.symlinkSync("criterion-unofficial.old", device.local(executable)); }
      };
      uncertain(await retire(device, admitted)); assert.ok(fired);
    });
  }
  for (const mode of ["start", "exe", "gone", "new"] as const) {
    await fixture("proc census " + mode + " drift refuses publication", async (device, admitted) => {
      device.gone(); device.process(90, "900", "/usr/lib/libfixture.so.1.2"); let reads = 0;
      device.beforeFs = (operation, path) => {
        if (operation !== "opendirSync" || !/^\/proc\/self\/fd\//.test(path) || fs.readlinkSync(path) !== device.local("/proc") || ++reads !== 2) return;
        if (mode === "start") device.write("/proc/90/stat", "90 (unrelated) S " + Array(18).fill("0").join(" ") + " 999 0\n");
        if (mode === "exe") { fs.unlinkSync(device.local("/proc/90/exe")); fs.symlinkSync(executable, device.local("/proc/90/exe")); }
        if (mode === "gone") fs.rmSync(device.local("/proc/90"), { recursive: true });
        if (mode === "new") device.process(91, "999", null);
      };
      uncertain(await retire(device, admitted)); assert.equal(reads, 2);
    });
  }
  await fixture("full scan and proc/file buffers are bounded", async (device, admitted) => {
    device.gone(); device.process(90, "900", null); device.write("/proc/90/stat", "x".repeat(4097)); uncertain(await retire(device, admitted));
    device.write("/proc/90/stat", "90 (unrelated) S " + Array(18).fill("0").join(" ") + " 900 0\n"); device.write("/proc/90/maps", "x".repeat(1024 * 1024 + 1)); uncertain(await retire(device, admitted));
    fs.rmSync(device.local("/proc/90"), { recursive: true });
    for (let pid = 100; pid < 2149; pid++) device.process(pid, String(pid), null);
    uncertain(await retire(device, admitted));
  });
  await fixture("only the exact closed admitted positive runtime proof dispatches", async (device, admitted) => {
    let calls = 0; const execute = async () => { calls++; return { kind: "unresolved" as const, reason: "not-issued" }; };
    assert.ok(admitted.outcome?.kind === "closed" && admitted.response.kind === "accepted");
    const outcome = admitted.outcome, response = admitted.response;
    const bad: NativeRuntimeRead[] = [
      { outcome: null, response }, { outcome, response: { kind: "unavailable", reason: "execution" } },
      { outcome: { ...outcome, exitCode: 1 }, response }, { outcome: { ...outcome, timedOut: true }, response },
      { outcome: { ...outcome, signal: "SIGTERM" }, response }, { outcome: { kind: "unresolved", reason: "unknown-close" }, response },
      { outcome: { ...outcome, stdout: Buffer.alloc(65537) }, response }, { outcome: { ...outcome, stderr: Buffer.alloc(4097) }, response },
      { outcome, response: { ...response, facts: { ...response.facts, pid: 43 } } },
      { outcome, response: { ...response, facts: { ...response.facts, extra: "unexpected" } as NativeRuntimeFacts } },
    ];
    for (const proof of bad) uncertain(await readNativeRetirement(proof, device.requirements, { deadlineMs: 10000, now: () => 0, execute }), "admission");
    uncertain(await readNativeRetirement(admitted, { ...device.requirements, executableSha256: "1".repeat(64) }, { deadlineMs: 10000, now: () => 0, execute }), "admission");
    assert.equal(calls, 0);
  });
  await fixture("requirements and admitted facts are captured before await", async (device, admitted) => {
    device.gone(); assert.ok(admitted.response.kind === "accepted" && admitted.outcome?.kind === "closed");
    const mutable = { ...device.requirements, neededSonames: [...device.requirements.neededSonames] };
    const projected = { ...admitted.response.facts, executable: { ...admitted.response.facts.executable } };
    const original = { ...admitted.outcome, stdout: Buffer.from(admitted.outcome.stdout) };
    const result = await readNativeRetirement({ outcome: original, response: { kind: "accepted", facts: projected } }, mutable, {
      deadlineMs: 10000, now: () => 0, execute: async program => {
        mutable.executableSha256 = "1".repeat(64); mutable.neededSonames.push("other.so"); projected.pid = 43; projected.executable.sha256 = "2".repeat(64); original.stdout.fill(0);
        return device.executeRetirement(program);
      },
    });
    assert.equal(result.response.kind, "retired"); if (result.response.kind === "retired") { assert.equal(result.response.facts.pid, 42); assert.equal(result.response.facts.executable.sha256, "d0e84cc32861277927fdb920507cf854292a3009bf5ab830541e9d47fa73e6b1"); }
  });
  await fixture("original deadline and unknown closure never become retirement", async (device, admitted) => {
    device.gone(); let calls = 0; const execute = async (program: StockNode16ReadProgram) => { calls++; return device.executeRetirement(program); };
    uncertain(await readNativeRetirement(admitted, device.requirements, { deadlineMs: 10, now: () => 10, execute }), "deadline"); assert.equal(calls, 0);
    let now = 0; uncertain(await readNativeRetirement(admitted, device.requirements, { deadlineMs: 10000, now: () => now, execute: async program => { const result = await execute(program); now = 10000; return result; } }), "deadline"); assert.equal(calls, 1);
    let clockReads = 0; uncertain(await readNativeRetirement(admitted, device.requirements, { deadlineMs: 10000, now: () => clockReads++ ? 0 : 1, execute }), "deadline");
    uncertain(await readNativeRetirement(admitted, device.requirements, { deadlineMs: 10000, now: () => 0, execute: async () => ({ kind: "unresolved", reason: "private-unknown" }) }), "closure");
    uncertain(await readNativeRetirement(admitted, device.requirements, { deadlineMs: 10000, now: () => 0, execute: async () => { throw new Error("no closure"); } }), "closure");
  });
  await fixture("execution failure and malformed response cannot certify absence", async (device, admitted) => {
    device.gone(); const raw = await device.executeRetirement(emitNativeRetirementProgram(admitted, device.requirements, 10000, 0)); assert.ok(raw.kind === "closed");
    for (const outcome of [{ ...raw, exitCode: 1 }, { ...raw, timedOut: true }, { ...raw, signal: "SIGTERM" }]) uncertain(await readNativeRetirement(admitted, device.requirements, { deadlineMs: 10000, now: () => 0, execute: async () => outcome }));
    for (const stdout of [Buffer.from([0xff]), Buffer.from("[]"), Buffer.from(raw.stdout.toString().replace('"pid":42', '"pid":43')), Buffer.from(raw.stdout.toString().replace('"schemaVersion":1', '"schemaVersion":1,"extra":true')), Buffer.alloc(65537)]) {
      uncertain(await readNativeRetirement(admitted, device.requirements, { deadlineMs: 10000, now: () => 0, execute: async () => ({ ...raw, stdout }) }), "response");
    }
  });
  await fixture("inner monotonic deadline and exact stock runtime are enforced", async (device, admitted) => {
    device.gone(); device.runtime.versions.node = "24.20.0"; uncertain(await retire(device, admitted)); device.runtime.versions.node = "16.20.2";
    let time = 0n; device.runtime.hrtime = { ...process.hrtime, bigint: () => (time += 8000000000n) } as typeof process.hrtime;
    uncertain(await retire(device, admitted));
  });
}
void run().catch(error => { console.error(error); process.exitCode = 1; });
