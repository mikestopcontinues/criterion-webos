import { MAIN_APP_ID, type NativeRequirements, type PrerequisiteExecutor,
  type StockNode16ReadProgram, type ActualReadOutcome } from "./contract.js";

const installedMain = `/media/developer/apps/usr/palm/applications/${MAIN_APP_ID}/criterion-unofficial`;
const graphics = ["libEGL.so.1", "libGLESv2.so.2"];
interface Elf { readonly class: 32; readonly endian: "little"; readonly machine: 40; readonly type: 2 | 3; readonly flags: number }
export interface RuntimeFile {
  readonly path: string; readonly bytes: number; readonly sha256: string;
  readonly device: string; readonly inode: string; readonly elf: Elf;
}
/** Stable file/process observations; not foreground, rendering or ABI admission. */
export interface NativeRuntimeFacts {
  readonly schemaVersion: 1; readonly appId: typeof MAIN_APP_ID;
  readonly bootId: string; readonly pid: number; readonly startTicks: string;
  readonly executable: RuntimeFile;
  readonly libraries: readonly (RuntimeFile & { readonly soname: string })[];
}
export type NativeRuntimeRead = { readonly outcome: ActualReadOutcome | null; readonly response:
  { readonly kind: "accepted"; readonly facts: NativeRuntimeFacts } |
  { readonly kind: "unavailable"; readonly reason: "requirements" | "deadline" | "closure" | "execution" | "response" } };
function check(value: unknown): asserts value { if (!value) throw new Error("native-runtime-refused"); }
function record(value: unknown): Record<string, unknown> { check(value !== null && typeof value === "object" && !Array.isArray(value)); return value as Record<string, unknown>; }
function keys(value: Record<string, unknown>, names: readonly string[]): void { check(Object.keys(value).sort().join(",") === [...names].sort().join(",")); }
function text(value: unknown, maximum: number, pattern?: RegExp): string {
  check(typeof value === "string" && Buffer.byteLength(value) > 0 && Buffer.byteLength(value) <= maximum && !/[\x00-\x1f\x7f]/.test(value) && (!pattern || pattern.test(value))); return value;
}
function integer(value: unknown, minimum: number, maximum: number): number { check(typeof value === "number" && Number.isSafeInteger(value) && value >= minimum && value <= maximum); return value; }
function expected(value: NativeRequirements): { requirements: NativeRequirements; sonames: string[] } {
  const input = record(value); keys(input, ["appId", "executableSha256", "neededSonames"]); check(input.appId === MAIN_APP_ID);
  const executableSha256 = text(input.executableSha256, 64, /^[a-f0-9]{64}$/);
  check(Array.isArray(input.neededSonames) && input.neededSonames.length > 0 && input.neededSonames.length <= 10);
  const neededSonames = input.neededSonames.map((name: unknown) => text(name, 128, /^[A-Za-z0-9_+.-]+\.so(?:\.[A-Za-z0-9_+.-]+)*$/));
  check(new Set(neededSonames).size === neededSonames.length);
  return { requirements: { appId: MAIN_APP_ID, executableSha256, neededSonames }, sonames: [...new Set([...neededSonames, ...graphics])] };
}
function canonical(value: unknown): string {
  const path = text(value, 4096); check(path.startsWith("/") && !path.endsWith(" (deleted)") && path.slice(1).split("/").every(part => part !== "" && part !== "." && part !== "..")); return path;
}
/** Inert; invoke emission from checked compiled JS, never TypeScript function text. */
export function emitNativeRuntimeProgram(requirements: NativeRequirements, deadlineMs: number, nowMs: number): StockNode16ReadProgram {
  const input = expected(requirements); integer(deadlineMs, 1, Number.MAX_SAFE_INTEGER); integer(nowMs, 0, Number.MAX_SAFE_INTEGER);
  const timeoutMs = Math.min(10000, deadlineMs - nowMs); check(timeoutMs > 0);
  const request = { sha256: input.requirements.executableSha256, sonames: input.sonames, installedMain, budgetMs: Math.min(8000, timeoutMs) };
  const source = `'use strict';try {const facts=(${stockRuntime.toString()})(${JSON.stringify(request)});const output=JSON.stringify(facts);if(Buffer.byteLength(output)>65536)throw new Error('output-bound');process.stdout.write(output+'\\n');}catch {process.stderr.write('native-runtime-refused');process.exitCode=1;}\n`;
  check(Buffer.byteLength(source) <= 128 * 1024);
  return Object.freeze({ source, stdin: "none", deadlineMs, timeoutMs, stdoutLimit: 65536, stderrLimit: 4096 });
}
export async function readNativeRuntime(requirements: NativeRequirements, executor: PrerequisiteExecutor): Promise<NativeRuntimeRead> {
  const deadlineMs = executor.deadlineMs, before = executor.now();
  if (!Number.isSafeInteger(deadlineMs) || !Number.isSafeInteger(before) || before < 0 || before >= deadlineMs) return { outcome: null, response: { kind: "unavailable", reason: "deadline" } };
  let program: StockNode16ReadProgram; let captured: NativeRequirements;
  try { captured = expected(requirements).requirements; program = emitNativeRuntimeProgram(captured, deadlineMs, before); }
  catch { return { outcome: null, response: { kind: "unavailable", reason: "requirements" } }; }
  let outcome: ActualReadOutcome;
  try { outcome = await executor.execute(program); }
  catch { return { outcome: null, response: { kind: "unavailable", reason: "closure" } }; }
  if (outcome.kind === "unresolved") return { outcome, response: { kind: "unavailable", reason: "closure" } };
  const after = executor.now();
  if (!Number.isSafeInteger(after) || after < before || after >= deadlineMs) return { outcome, response: { kind: "unavailable", reason: "deadline" } };
  if (outcome.timedOut || outcome.exitCode !== 0 || outcome.signal !== null) return { outcome, response: { kind: "unavailable", reason: "execution" } };
  try {
    check(Buffer.isBuffer(outcome.stderr) && outcome.stderr.length <= program.stderrLimit);
    return { outcome, response: { kind: "accepted", facts: decodeNativeRuntimeFacts(outcome.stdout, captured) } };
  } catch { return { outcome, response: { kind: "unavailable", reason: "response" } }; }
}
export function decodeNativeRuntimeFacts(bytes: Uint8Array, requirements: NativeRequirements): NativeRuntimeFacts {
  const input = expected(requirements); check(bytes instanceof Uint8Array && bytes.byteLength > 0 && bytes.byteLength <= 65536);
  const encoded = Buffer.from(bytes), raw = encoded.toString("utf8"); check(Buffer.from(raw).equals(encoded));
  const value = record(JSON.parse(raw) as unknown); keys(value, ["schemaVersion", "appId", "bootId", "pid", "startTicks", "executable", "libraries"]);
  check(value.schemaVersion === 1 && value.appId === MAIN_APP_ID);
  const bootId = text(value.bootId, 36, /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/);
  const pid = integer(value.pid, 2, 0x7fffffff), startTicks = text(value.startTicks, 20, /^[1-9][0-9]*$/);
  let total = 0;
  function file(value: unknown, soname?: string): RuntimeFile {
    const f = record(value); keys(f, ["path", "bytes", "sha256", "device", "inode", "elf", ...(soname ? ["soname"] : [])]);
    if (soname) check(f.soname === soname);
    const path = canonical(f.path); if (soname) check(/^\/(?:lib|usr\/lib)\//.test(path));
    const size = integer(f.bytes, 52, 32 * 1024 * 1024); total += size; check(total <= 128 * 1024 * 1024);
    const sha256 = text(f.sha256, 64, /^[a-f0-9]{64}$/), device = text(f.device, 17, /^[a-f0-9]{1,8}:[a-f0-9]{1,8}$/), inode = text(f.inode, 20, /^[1-9][0-9]*$/);
    const e = record(f.elf); keys(e, ["class", "endian", "machine", "type", "flags"]);
    check(e.class === 32 && e.endian === "little" && e.machine === 40 && (e.type === 2 || e.type === 3));
    const flags = integer(e.flags, 0, 0xffffffff); check((flags & 0xff000000) === 0x05000000 && (flags & 0x400) === 0);
    return { path, bytes: size, sha256, device, inode, elf: { class: 32, endian: "little", machine: 40, type: e.type, flags } };
  }
  const executable = file(value.executable); check(executable.sha256 === input.requirements.executableSha256 && executable.elf.type === 3);
  check(Array.isArray(value.libraries) && value.libraries.length === input.sonames.length);
  const libraries = value.libraries.map((value: unknown, index: number) => { const soname = input.sonames[index]; check(soname !== undefined); return { ...file(value, soname), soname }; });
  return { schemaVersion: 1, appId: MAIN_APP_ID, bootId, pid, startTicks, executable, libraries };
}

/** Self-contained stock-Node16 read. Only fixed MAIN/proc and required library files. */
function stockRuntime(request: { sha256: string; sonames: string[]; installedMain: string; budgetMs: number }): NativeRuntimeFacts {
  const fs = require("node:fs") as typeof import("node:fs");
  const crypto = require("node:crypto") as typeof import("node:crypto");
  const path = require("node:path") as typeof import("node:path");
  const origin = process.hrtime.bigint();
  function check(value: unknown): asserts value { if (!value) throw new Error("read-refused"); }
  const bound = () => check(Number((process.hrtime.bigint() - origin) / 1000000n) < request.budgetMs);
  check(process.platform === "linux" && process.arch === "arm" && process.versions.node === "16.20.2" && process.geteuid?.() === 0);
  const stable = (a: import("node:fs").BigIntStats, b: import("node:fs").BigIntStats) =>
    (["dev", "ino", "mode", "uid", "gid", "nlink", "size", "mtimeNs", "ctimeNs"] as const).every(key => a[key] === b[key]);
  function readProc(name: string, maximum: number): Buffer {
    bound(); const before = fs.lstatSync(name, { bigint: true }); check(before.isFile() && (before.mode & 0o022n) === 0n);
    const fd = fs.openSync(name, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
    try {
      const initial = fs.fstatSync(fd, { bigint: true }); check(stable(before, initial));
      const bytes = Buffer.alloc(maximum + 1); let count = 0;
      while (count < bytes.length) { bound(); const size = fs.readSync(fd, bytes, count, bytes.length - count, null); if (!size) break; count += size; }
      check(count <= maximum && stable(initial, fs.fstatSync(fd, { bigint: true })) && stable(initial, fs.lstatSync(name, { bigint: true })));
      const result = bytes.subarray(0, count), text = result.toString("utf8"); check(Buffer.from(text).equals(result)); return result;
    } finally { fs.closeSync(fd); }
  }
  function boot(): string { const value = readProc("/proc/sys/kernel/random/boot_id", 128).toString().trim(); check(/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(value)); return value; }
  function canonical(value: string): string { check(Buffer.byteLength(value) <= 4096 && value.startsWith("/") && !/[\x00-\x1f\x7f]/.test(value) && !value.endsWith(" (deleted)") && value.slice(1).split("/").every(part => part !== "" && part !== "." && part !== "..")); return value; }
  const firstBoot = boot(); const mainPath = canonical(fs.realpathSync(request.installedMain));
  function identity(pid: number): { pid: number; startTicks: string } {
    const base = `/proc/${pid}`; check(fs.readlinkSync(base + "/exe") === mainPath);
    const stat = readProc(base + "/stat", 4096).toString(); check(stat.startsWith(pid + " (")); const end = stat.lastIndexOf(") "); check(end >= 0);
    const fields = stat.slice(end + 2).trim().split(/\s+/), startTicks = fields[19];
    check(fields[0] !== undefined && /^[RSDTtKWPI]$/.test(fields[0]) && startTicks !== undefined && /^[1-9][0-9]{0,19}$/.test(startTicks));
    check(fs.readlinkSync(base + "/exe") === mainPath); return { pid, startTicks };
  }
  function find(): { pid: number; startTicks: string } {
    bound(); const directory = fs.opendirSync("/proc", { bufferSize: 1 }); const found: { pid: number; startTicks: string }[] = []; let count = 0, pids = 0;
    try { for (;;) {
      bound(); const entry = directory.readSync(); if (!entry) break; check(++count <= 8192);
      if (!/^[1-9][0-9]*$/.test(entry.name)) continue; check(++pids <= 2048);
      const pid = Number(entry.name); check(Number.isSafeInteger(pid) && pid > 0 && pid <= 0x7fffffff);
      let exe: string; try { exe = fs.readlinkSync(`/proc/${pid}/exe`); } catch (error) {
        if (error !== null && typeof error === "object" && "code" in error && (error.code === "ENOENT" || error.code === "ESRCH")) continue; throw error;
      }
      if (exe !== mainPath) { check(!exe.endsWith("/criterion-unofficial") && !exe.endsWith("/criterion-unofficial (deleted)")); continue; }
      check(pid > 1); found.push(identity(pid)); check(found.length <= 1);
    } } finally { directory.closeSync(); }
    const first = found[0]; check(first !== undefined); return first;
  }
  const first = find();
  type Mapping = { path: string; device: string; inode: string; executable: boolean };
  function maps(): Mapping[] {
    const rows = readProc(`/proc/${first.pid}/maps`, 1024 * 1024).toString().trim().split("\n"); check(rows.length > 0 && rows.length <= 8192);
    return rows.flatMap(row => {
      const m = /^([a-f0-9]{1,16})-([a-f0-9]{1,16}) ([r-][w-][x-][ps]) ([a-f0-9]{1,16}) ([a-f0-9]{1,8}):([a-f0-9]{1,8}) (0|[1-9][0-9]{0,19})(?:\s+(.*))?$/.exec(row); check(m !== null);
      const low = m[1], high = m[2], permissions = m[3], major = m[5], minor = m[6], inode = m[7], name = m[8];
      check(low && high && permissions && major && minor && inode && BigInt("0x" + high) > BigInt("0x" + low));
      if (!name || !name.startsWith("/")) return [];
      return [{ path: name, device: BigInt("0x" + major).toString(16) + ":" + BigInt("0x" + minor).toString(16), inode, executable: permissions[2] === "x" }];
    });
  }
  const firstMaps = maps();
  const handles: number[] = [], pins: { fd: number; anchored: string; stat: import("node:fs").BigIntStats }[] = [];
  const links: { anchored: string; stat: import("node:fs").BigIntStats; target: string }[] = [];
  function pinDirectory(name: string): number {
    bound(); check(handles.length < 512); const before = fs.lstatSync(name, { bigint: true }); check(before.isDirectory() && before.uid === 0n && (before.mode & 0o022n) === 0n);
    const fd = fs.openSync(name, fs.constants.O_RDONLY | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW); handles.push(fd);
    const stat = fs.fstatSync(fd, { bigint: true }); check(stable(before, stat)); pins.push({ fd, anchored: name, stat }); return fd;
  }
  function resolve(name: string, library: boolean): { fd: number; canonical: string; stat: import("node:fs").BigIntStats } {
    let current = name, depth = 0;
    for (;;) {
      canonical(current); if (library) check(/^\/(?:lib|usr\/lib)\//.test(current));
      const parts = current.slice(1).split("/"); check(parts.length <= 32); let fd = pinDirectory("/"), actual = "", restarted = false;
      for (let index = 0; index < parts.length; index++) {
        bound(); const part = parts[index]; check(part !== undefined); const anchored = `/proc/self/fd/${fd}/${part}`, before = fs.lstatSync(anchored, { bigint: true }); check(before.uid === 0n);
        if (before.isSymbolicLink()) {
          check(++depth <= 16); const target = fs.readlinkSync(anchored); check(target.length > 0 && Buffer.byteLength(target) <= 4096 && !/[\x00-\x1f\x7f]/.test(target));
          check(stable(before, fs.lstatSync(anchored, { bigint: true })) && target === fs.readlinkSync(anchored)); links.push({ anchored, stat: before, target });
          current = path.posix.resolve(actual || "/", target, ...parts.slice(index + 1)); restarted = true; break;
        }
        actual += "/" + part;
        if (index + 1 < parts.length) { fd = pinDirectory(anchored); continue; }
        check(before.isFile() && (before.mode & 0o022n) === 0n && before.size >= 52n && before.size <= 32n * 1024n * 1024n);
        check(handles.length < 512); const file = fs.openSync(anchored, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK); handles.push(file);
        const stat = fs.fstatSync(file, { bigint: true }); check(stable(before, stat)); pins.push({ fd: file, anchored, stat }); return { fd: file, canonical: actual, stat };
      }
      check(restarted);
    }
  }
  function verify(): void {
    for (const pin of pins) { bound(); check(stable(pin.stat, fs.fstatSync(pin.fd, { bigint: true })) && stable(pin.stat, fs.lstatSync(pin.anchored, { bigint: true }))); }
    for (const link of links) { bound(); check(stable(link.stat, fs.lstatSync(link.anchored, { bigint: true })) && link.target === fs.readlinkSync(link.anchored)); }
  }
  let total = 0;
  function inspect(file: ReturnType<typeof resolve>, isMain: boolean): RuntimeFile {
    bound(); const size = Number(file.stat.size); total += size; check(total <= 128 * 1024 * 1024); if (isMain) check((file.stat.mode & 0o111n) !== 0n);
    const major = ((file.stat.dev >> 8n) & 0xfffn) | ((file.stat.dev >> 32n) & 0xfffff000n), minor = (file.stat.dev & 0xffn) | ((file.stat.dev >> 12n) & 0xffffff00n);
    const device = major.toString(16) + ":" + minor.toString(16), inode = file.stat.ino.toString();
    const matching = firstMaps.filter(row => row.path === file.canonical); check(matching.some(row => row.executable) && matching.every(row => row.device === device && row.inode === inode));
    const header = Buffer.alloc(52); check(fs.readSync(file.fd, header, 0, 52, 0) === 52);
    check(header.subarray(0, 7).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46, 1, 1, 1])) && header.readUInt16LE(18) === 40 && header.readUInt32LE(20) === 1 && header.readUInt16LE(40) === 52);
    const type = header.readUInt16LE(16), flags = header.readUInt32LE(36); check((type === 2 || type === 3) && (!isMain || type === 3) && (flags & 0xff000000) === 0x05000000 && (flags & 0x400) === 0);
    const hash = crypto.createHash("sha256"), chunk = Buffer.alloc(65536); let count = 0;
    while (count <= size) { bound(); const got = fs.readSync(file.fd, chunk, 0, Math.min(chunk.length, size - count + 1), count); if (!got) break; count += got; check(count <= size); hash.update(chunk.subarray(0, got)); }
    check(count === size); verify();
    return { path: file.canonical, bytes: size, sha256: hash.digest("hex"), device, inode, elf: { class: 32, endian: "little", machine: 40, type, flags } };
  }
  try {
    const main = resolve(request.installedMain, false); check(main.canonical === mainPath); const executable = inspect(main, true); check(executable.sha256 === request.sha256);
    const libraries = request.sonames.map(soname => {
      let file: ReturnType<typeof resolve> | undefined;
      for (const root of ["/lib", "/usr/lib"]) {
        try { file = resolve(root + "/" + soname, true); break; }
        catch (error) { if (error !== null && typeof error === "object" && "code" in error && error.code === "ENOENT") continue; throw error; }
      }
      check(file !== undefined); return { ...inspect(file, false), soname };
    });
    const lastMaps = maps();
    for (const file of [executable, ...libraries]) {
      const rows = (items: Mapping[]) => JSON.stringify(items.filter(row => row.path === file.path)); check(rows(firstMaps) === rows(lastMaps));
    }
    const last = find(); check(last.pid === first.pid && last.startTicks === first.startTicks && boot() === firstBoot && fs.realpathSync(request.installedMain) === mainPath); verify(); bound();
    return { schemaVersion: 1, appId: "com.mikestopcontinues.criterion.unofficial", bootId: firstBoot, pid: first.pid, startTicks: first.startTicks, executable, libraries };
  } finally { for (const fd of handles.reverse()) fs.closeSync(fd); }
}
