import { MAIN_APP_ID, type NativeRequirements, type PrerequisiteExecutor,
  type StockNode16ReadProgram, type ActualReadOutcome } from "./contract.js";
import { decodeNativeRuntimeFacts, type NativeRuntimeRead, type NativeRuntimeFacts } from "./runtime.js";

const installedMain = `/media/developer/apps/usr/palm/applications/${MAIN_APP_ID}/criterion-unofficial`;
interface Owned {
  readonly bootId: string; readonly pid: number; readonly startTicks: string;
  readonly executable: { readonly path: string; readonly bytes: number; readonly sha256: string; readonly device: string; readonly inode: string };
}
/** Same-boot owned PID gone and no MAIN in two stable bounded process censuses.
 * Reused PID, newer/foreign MAIN, inaccessible process or drift is uncertainty.
 * This observes process absence, not close ACK, foreground, other resources, or
 * future absence. Exe-less groups need a pinned task census containing only the
 * leader before/after empty-map and exe-absence checks; missing tasks refuse.
 * The canonical phase owns its admitted positive input/authority.
 */
export interface NativeRetirementFacts extends Owned { readonly schemaVersion: 1; readonly appId: typeof MAIN_APP_ID }
export type NativeRetirementRead = { readonly outcome: ActualReadOutcome | null; readonly response:
  { readonly kind: "retired"; readonly facts: NativeRetirementFacts } |
  { readonly kind: "uncertain"; readonly reason: "admission" | "deadline" | "closure" | "execution" | "response" } };
function check(value: unknown): asserts value { if (!value) throw new Error("native-retirement-refused"); }
function record(value: unknown): Record<string, unknown> { check(value !== null && typeof value === "object" && !Array.isArray(value)); return value as Record<string, unknown>; }
function keys(value: Record<string, unknown>, names: readonly string[]): void { check(Object.keys(value).sort().join(",") === [...names].sort().join(",")); }
function same(actual: unknown, expected: unknown): boolean {
  if (expected === null || typeof expected !== "object") return actual === expected;
  if (Array.isArray(expected)) return Array.isArray(actual) && actual.length === expected.length && expected.every((value: unknown, index: number) => same(actual[index], value));
  const a = record(actual), e = record(expected), names = Object.keys(e);
  return Object.keys(a).length === names.length && names.every(name => Object.prototype.hasOwnProperty.call(a, name) && same(a[name], e[name]));
}
function capture(admitted: NativeRuntimeRead, requirements: NativeRequirements): Owned {
  const read = record(admitted); keys(read, ["outcome", "response"]);
  const response = record(read.response); keys(response, ["kind", "facts"]); check(response.kind === "accepted");
  const outcome = record(read.outcome); keys(outcome, ["kind", "exitCode", "signal", "timedOut", "stdout", "stderr"]);
  check(outcome.kind === "closed" && outcome.exitCode === 0 && outcome.signal === null && outcome.timedOut === false);
  check(Buffer.isBuffer(outcome.stdout) && outcome.stdout.length > 0 && outcome.stdout.length <= 65536 && Buffer.isBuffer(outcome.stderr) && outcome.stderr.length <= 4096);
  const original = decodeNativeRuntimeFacts(Buffer.from(outcome.stdout), requirements);
  check(same(response.facts, original));
  return owned(original);
}
function owned(facts: NativeRuntimeFacts): Owned {
  return { bootId: facts.bootId, pid: facts.pid, startTicks: facts.startTicks,
    executable: { path: facts.executable.path, bytes: facts.executable.bytes, sha256: facts.executable.sha256, device: facts.executable.device, inode: facts.executable.inode } };
}
function emit(captured: Owned, deadlineMs: number, nowMs: number): StockNode16ReadProgram {
  check(Number.isSafeInteger(deadlineMs) && Number.isSafeInteger(nowMs) && nowMs >= 0);
  const timeoutMs = Math.min(10000, deadlineMs - nowMs); check(timeoutMs > 0);
  const source = `'use strict';try {const facts=(${stockRetirement.toString()})(${JSON.stringify({ owned: captured, installedMain, budgetMs: Math.min(8000, timeoutMs) })});const output=JSON.stringify(facts);if(Buffer.byteLength(output)>65536)throw new Error('output-bound');process.stdout.write(output+'\\n');}catch {process.stderr.write('native-retirement-refused');process.exitCode=1;}\n`;
  check(Buffer.byteLength(source) <= 128 * 1024);
  return Object.freeze({ source, stdin: "none", deadlineMs, timeoutMs, stdoutLimit: 65536, stderrLimit: 4096 });
}
/** Inert emission from checked compiled JS. No loose PID/path inputs. */
export function emitNativeRetirementProgram(admitted: NativeRuntimeRead, requirements: NativeRequirements, deadlineMs: number, nowMs: number): StockNode16ReadProgram {
  return emit(capture(admitted, requirements), deadlineMs, nowMs);
}
/** Installed MAIN must remain intact until this single read settles. Never retries. */
export async function readNativeRetirement(admitted: NativeRuntimeRead, requirements: NativeRequirements, executor: PrerequisiteExecutor): Promise<NativeRetirementRead> {
  const deadlineMs = executor.deadlineMs, before = executor.now();
  if (!Number.isSafeInteger(deadlineMs) || !Number.isSafeInteger(before) || before < 0 || before >= deadlineMs) return { outcome: null, response: { kind: "uncertain", reason: "deadline" } };
  let captured: Owned, program: StockNode16ReadProgram;
  try { captured = capture(admitted, requirements); program = emit(captured, deadlineMs, before); }
  catch { return { outcome: null, response: { kind: "uncertain", reason: "admission" } }; }
  let outcome: ActualReadOutcome;
  try { outcome = await executor.execute(program); }
  catch { return { outcome: null, response: { kind: "uncertain", reason: "closure" } }; }
  if (outcome.kind === "unresolved") return { outcome, response: { kind: "uncertain", reason: "closure" } };
  const after = executor.now();
  if (!Number.isSafeInteger(after) || after < before || after >= deadlineMs) return { outcome, response: { kind: "uncertain", reason: "deadline" } };
  if (outcome.kind !== "closed" || outcome.timedOut || outcome.exitCode !== 0 || outcome.signal !== null) return { outcome, response: { kind: "uncertain", reason: "execution" } };
  try {
    check(Buffer.isBuffer(outcome.stdout) && outcome.stdout.length > 0 && outcome.stdout.length <= program.stdoutLimit && Buffer.isBuffer(outcome.stderr) && outcome.stderr.length <= program.stderrLimit);
    const bytes = Buffer.from(outcome.stdout), text = bytes.toString("utf8"); check(Buffer.from(text).equals(bytes));
    const facts: NativeRetirementFacts = { schemaVersion: 1, appId: MAIN_APP_ID, ...captured };
    check(same(JSON.parse(text) as unknown, facts));
    return { outcome, response: { kind: "retired", facts } };
  } catch { return { outcome, response: { kind: "uncertain", reason: "response" } }; }
}

/** Fixed read only: no child, Luna, setter, installer or arbitrary executable. */
function stockRetirement(request: { owned: Owned; installedMain: string; budgetMs: number }): NativeRetirementFacts {
  const fs = require("node:fs") as typeof import("node:fs");
  const crypto = require("node:crypto") as typeof import("node:crypto");
  const origin = process.hrtime.bigint(), handles: number[] = [];
  function check(value: unknown): asserts value { if (!value) throw new Error("read-refused"); }
  const bound = () => check(Number((process.hrtime.bigint() - origin) / 1000000n) < request.budgetMs);
  check(process.platform === "linux" && process.arch === "arm" && process.versions.node === "16.20.2" && process.geteuid?.() === 0);
  const stable = (a: import("node:fs").BigIntStats, b: import("node:fs").BigIntStats) =>
    (["dev", "ino", "mode", "uid", "gid", "nlink", "size", "mtimeNs", "ctimeNs"] as const).every(key => a[key] === b[key]);
  function open(name: string, directory: boolean, rootOwned: boolean): { fd: number; name: string; stat: import("node:fs").BigIntStats } {
    bound(); check(handles.length < 64);
    const before = fs.lstatSync(name, { bigint: true });
    check((directory ? before.isDirectory() : before.isFile()) && (!rootOwned || before.uid === 0n) && (before.mode & 0o022n) === 0n);
    const fd = fs.openSync(name, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | (directory ? fs.constants.O_DIRECTORY : fs.constants.O_NONBLOCK)); handles.push(fd);
    const stat = fs.fstatSync(fd, { bigint: true }); check(stable(before, stat)); return { fd, name, stat };
  }
  function verify(pin: ReturnType<typeof open>): void { bound(); check(stable(pin.stat, fs.fstatSync(pin.fd, { bigint: true })) && stable(pin.stat, fs.lstatSync(pin.name, { bigint: true }))); }
  function close(pin: ReturnType<typeof open>): void { fs.closeSync(pin.fd); const at = handles.indexOf(pin.fd); check(at >= 0); handles.splice(at, 1); }
  function read(name: string, maximum: number): Buffer {
    const pin = open(name, false, false);
    try {
      const buffer = Buffer.alloc(maximum + 1); let count = 0;
      while (count < buffer.length) { bound(); const size = fs.readSync(pin.fd, buffer, count, buffer.length - count, null); if (!size) break; count += size; }
      check(count <= maximum); verify(pin); return buffer.subarray(0, count);
    } finally { close(pin); }
  }
  function canonical(value: string): string {
    check(Buffer.byteLength(value) > 1 && Buffer.byteLength(value) <= 4096 && value.startsWith("/") && !/[\x00-\x1f\x7f]/.test(value) && value.slice(1).split("/").every(part => part !== "" && part !== "." && part !== "..")); return value;
  }
  function device(stat: import("node:fs").BigIntStats): string {
    return (((stat.dev >> 8n) & 0xfffn) | ((stat.dev >> 32n) & 0xfffff000n)).toString(16) + ":" + ((stat.dev & 0xffn) | ((stat.dev >> 12n) & 0xffffff00n)).toString(16);
  }
  function missing(name: string): void {
    bound(); try { fs.lstatSync(name); } catch (error) { check(error !== null && typeof error === "object" && "code" in error && error.code === "ENOENT"); return; }
    check(false);
  }
  try {
    const proc = open("/proc", true, true), procPath = `/proc/self/fd/${proc.fd}`;
    const boot = () => {
      const bytes = read(procPath + "/sys/kernel/random/boot_id", 128), value = bytes.toString("utf8").trim();
      check(Buffer.from(bytes.toString("utf8")).equals(bytes) && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(value)); return value;
    };
    check(boot() === request.owned.bootId); missing(procPath + "/" + request.owned.pid);
    check(fs.realpathSync(request.installedMain) === request.owned.executable.path);
    const parts = canonical(request.owned.executable.path).slice(1).split("/"); check(parts.length <= 32);
    const pins: ReturnType<typeof open>[] = [open("/", true, true)];
    let current = pins[0]; check(current !== undefined);
    for (const [index, part] of parts.entries()) { current = open(`/proc/self/fd/${current.fd}/${part}`, index + 1 < parts.length, true); pins.push(current); }
    const file = current;
    check(file.stat.size === BigInt(request.owned.executable.bytes) && file.stat.size >= 52n && file.stat.size <= 32n * 1024n * 1024n && (file.stat.mode & 0o111n) !== 0n && device(file.stat) === request.owned.executable.device && file.stat.ino.toString() === request.owned.executable.inode);
    const hash = crypto.createHash("sha256"), chunk = Buffer.alloc(65536); let count = 0;
    while (count <= request.owned.executable.bytes) { bound(); const size = fs.readSync(file.fd, chunk, 0, Math.min(chunk.length, request.owned.executable.bytes - count + 1), count); if (!size) break; count += size; check(count <= request.owned.executable.bytes); hash.update(chunk.subarray(0, size)); }
    check(count === request.owned.executable.bytes && hash.digest("hex") === request.owned.executable.sha256);
    interface Process { readonly pid: number; readonly start: string; readonly exe: string | null; readonly device: string | null; readonly inode: string | null }
    function identity(name: string, pid: number): string {
      const bytes = read(name + "/stat", 4096), text = bytes.toString("utf8"); check(Buffer.from(text).equals(bytes));
      const at = text.lastIndexOf(") "); check(at >= 3 && text.startsWith(pid + " ("));
      const fields = text.slice(at + 2).trim().split(/\s+/), start = fields[19];
      check(fields[0] !== undefined && /^[RSDZTtXxKWPI]$/.test(fields[0]) && start !== undefined && /^[1-9][0-9]{0,19}$/.test(start)); return start;
    }
    function scan(): Process[] {
      bound(); const result: Process[] = [], directory = fs.opendirSync(procPath); let entries = 0;
      try { for (;;) {
        bound(); const entry = directory.readSync(); if (!entry) break; check(++entries <= 8192);
        if (!/^[1-9][0-9]*$/.test(entry.name)) continue;
        const pid = Number(entry.name); check(Number.isSafeInteger(pid) && pid <= 0x7fffffff && result.length < 2048 && pid !== request.owned.pid);
        const process = open(procPath + "/" + entry.name, true, false), name = `/proc/self/fd/${process.fd}`;
        try {
          const start = identity(name, pid); let exe: string | null = null, exeDevice: string | null = null, inode: string | null = null;
          try { exe = fs.readlinkSync(name + "/exe"); }
          catch (error) { check(error !== null && typeof error === "object" && "code" in error && error.code === "ENOENT"); }
          if (exe === null) {
            const tasks = open(name + "/task", true, false), taskPath = `/proc/self/fd/${tasks.fd}`;
            const leader = open(taskPath + "/" + pid, true, false);
            try {
              const onlyLeader = () => {
                bound(); const directory = fs.opendirSync(taskPath); let entries = 0;
                try { for (;;) {
                  bound(); const entry = directory.readSync(); if (!entry) break;
                  check(++entries <= 64 && entry.name === String(pid) && entry.isDirectory());
                } } finally { directory.closeSync(); }
                check(entries === 1); verify(tasks); verify(leader);
              };
              onlyLeader();
              check(read(name + "/maps", 1024 * 1024).length === 0);
              missing(name + "/exe");
              onlyLeader();
            } finally { close(leader); close(tasks); }
          } else {
            canonical(exe); check(exe !== request.owned.executable.path && !/\/criterion-unofficial(?: \(deleted\))?$/.test(exe));
            bound(); const fd = fs.openSync(name + "/exe", fs.constants.O_RDONLY | fs.constants.O_NONBLOCK); handles.push(fd);
            try {
              const stat = fs.fstatSync(fd, { bigint: true }); check(stat.isFile()); exeDevice = device(stat); inode = stat.ino.toString();
              check(exeDevice !== request.owned.executable.device || inode !== request.owned.executable.inode);
              check(stable(stat, fs.fstatSync(fd, { bigint: true })) && fs.readlinkSync(name + "/exe") === exe);
            } finally { fs.closeSync(fd); handles.splice(handles.indexOf(fd), 1); }
          }
          check(identity(name, pid) === start); verify(process);
          result.push({ pid, start, exe, device: exeDevice, inode });
        } finally { close(process); }
      } } finally { directory.closeSync(); }
      verify(proc); missing(procPath + "/" + request.owned.pid);
      return result.sort((a, b) => a.pid - b.pid);
    }
    const first = scan(), last = scan(); check(JSON.stringify(first) === JSON.stringify(last));
    check(boot() === request.owned.bootId && fs.realpathSync(request.installedMain) === request.owned.executable.path); missing(procPath + "/" + request.owned.pid);
    for (const pin of pins) verify(pin); verify(proc); bound();
    return { schemaVersion: 1, appId: "com.mikestopcontinues.criterion.unofficial", ...request.owned };
  } finally { for (const fd of handles.reverse()) fs.closeSync(fd); }
}
