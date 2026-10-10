import { MAIN_APP_ID, type CpuFacts, type Epoch, type GetterClosure, type LibraryFact,
  type NativePrerequisiteFacts, type NativeRequirements, type PrerequisiteExecutor,
  type PrerequisiteRead, type StockNode16ReadProgram, type SystemInfo } from "./contract.js";

const graphics = ["libEGL.so.1", "libGLESv2.so.2"] as const;
function insist(value: unknown): asserts value { if (!value) throw new Error("native-prerequisites-refused"); }
function record(value: unknown): Record<string, unknown> {
  insist(value !== null && typeof value === "object" && !Array.isArray(value));
  return value as Record<string, unknown>;
}
function keys(value: Record<string, unknown>, names: readonly string[]): void {
  insist(Object.keys(value).sort().join(",") === [...names].sort().join(","));
}
function text(value: unknown, maximum: number, pattern?: RegExp): string {
  insist(typeof value === "string" && Buffer.byteLength(value) > 0 && Buffer.byteLength(value) <= maximum
    && !/[\x00-\x1f\x7f]/.test(value) && (!pattern || pattern.test(value)));
  return value;
}
function integer(value: unknown, minimum: number, maximum: number): number {
  insist(typeof value === "number" && Number.isSafeInteger(value) && value >= minimum && value <= maximum);
  return value;
}
function checkedRequirements(value: NativeRequirements): { requirements: NativeRequirements; sonames: string[] } {
  const input = record(value); keys(input, ["appId", "executableSha256", "neededSonames"]);
  insist(input.appId === MAIN_APP_ID);
  const executableSha256 = text(input.executableSha256, 64, /^[a-f0-9]{64}$/);
  insist(Array.isArray(input.neededSonames) && input.neededSonames.length > 0 && input.neededSonames.length <= 10);
  const neededSonames = input.neededSonames.map((name: unknown) => text(name, 128, /^[A-Za-z0-9_+.-]+\.so(?:\.[A-Za-z0-9_+.-]+)*$|^ld-linux\.so\.3$/));
  insist(neededSonames.every(name => name !== "." && name !== "..") && new Set(neededSonames).size === neededSonames.length);
  const sonames = [...new Set([...neededSonames, ...graphics])]; insist(sonames.length <= 12);
  return { requirements: { appId: MAIN_APP_ID, executableSha256, neededSonames }, sonames };
}

/** Inert. Call from compiled JS; the target text contains no TypeScript or host imports. */
export function emitPrerequisiteProgram(requirements: NativeRequirements, deadlineMs: number, nowMs: number): StockNode16ReadProgram {
  const checked = checkedRequirements(requirements);
  integer(deadlineMs, 1, Number.MAX_SAFE_INTEGER); integer(nowMs, 0, Number.MAX_SAFE_INTEGER);
  const timeoutMs = Math.min(10000, deadlineMs - nowMs); insist(timeoutMs > 0);
  const request = { requirements: checked.requirements, sonames: checked.sonames, budgetMs: Math.min(8000, timeoutMs) };
  const source = `'use strict';try {const facts=(${stockPrerequisites.toString()})(${JSON.stringify(request)});const output=JSON.stringify(facts);if(Buffer.byteLength(output)>65536)throw new Error('output-bound');process.stdout.write(output+'\\n');}catch {process.stderr.write('native-prerequisites-refused');process.exitCode=1;}\n`;
  insist(Buffer.byteLength(source) <= 128 * 1024);
  return Object.freeze({ source, stdin: "none", deadlineMs, timeoutMs, stdoutLimit: 65536, stderrLimit: 4096 });
}

/** Does not resolve a device or issue anything except the one injected read. */
export async function readNativePrerequisites(requirements: NativeRequirements, executor: PrerequisiteExecutor): Promise<PrerequisiteRead> {
  const deadlineMs = executor.deadlineMs;
  let program: StockNode16ReadProgram;
  const before = executor.now();
  if (!Number.isSafeInteger(deadlineMs) || !Number.isSafeInteger(before) || before < 0 || before >= deadlineMs) {
    return { outcome: null, response: { kind: "unavailable", reason: "deadline" } };
  }
  try { program = emitPrerequisiteProgram(requirements, deadlineMs, before); }
  catch { return { outcome: null, response: { kind: "unavailable", reason: "requirements" } }; }
  // Retain a snapshot: the caller's requirements may change while awaiting transport.
  const expected = checkedRequirements(requirements).requirements;
  const outcome = await executor.execute(program);
  if (outcome.kind === "unresolved") return { outcome, response: { kind: "unavailable", reason: "closure" } };
  const after = executor.now();
  if (!Number.isSafeInteger(after) || after < before || after >= deadlineMs) {
    return { outcome, response: { kind: "unavailable", reason: "deadline" } };
  }
  if (outcome.timedOut || outcome.exitCode !== 0 || outcome.signal !== null) {
    return { outcome, response: { kind: "unavailable", reason: "execution" } };
  }
  try {
    insist(Buffer.isBuffer(outcome.stderr) && outcome.stderr.length <= program.stderrLimit);
    return { outcome, response: { kind: "accepted", facts: decodePrerequisiteFacts(outcome.stdout, expected) } };
  } catch { return { outcome, response: { kind: "unavailable", reason: "response" } }; }
}

/** Exact bounded response validation; accepted means observations, not ABI admission. */
export function decodePrerequisiteFacts(bytes: Uint8Array, requirements: NativeRequirements): NativePrerequisiteFacts {
  const expected = checkedRequirements(requirements);
  insist(bytes instanceof Uint8Array && bytes.byteLength > 0 && bytes.byteLength <= 65536);
  const encoded = Buffer.from(bytes); const raw = encoded.toString("utf8"); insist(Buffer.from(raw).equals(encoded));
  const input = record(JSON.parse(raw) as unknown);
  keys(input, ["schemaVersion", "appId", "executableSha256", "epoch", "runtime", "getter", "systemInfo", "cpu", "libraries"]);
  insist(input.schemaVersion === 1 && input.appId === MAIN_APP_ID && input.executableSha256 === expected.requirements.executableSha256);
  const e = record(input.epoch), compositor = record(e.compositor); keys(e, ["bootId", "compositor"]); keys(compositor, ["pid", "startTicks"]);
  const epoch: Epoch = { bootId: text(e.bootId, 36, /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/),
    compositor: { pid: integer(compositor.pid, 2, 0x7fffffff), startTicks: text(compositor.startTicks, 20, /^[1-9][0-9]*$/) } };
  const r = record(input.runtime); keys(r, ["node", "platform", "arch", "euid"]);
  insist(r.platform === "linux" && r.euid === 0);
  const runtime = { node: text(r.node, 32, /^16\.\d+\.\d+$/), platform: "linux" as const,
    arch: text(r.arch, 16, /^[a-z0-9_]+$/), euid: 0 as const };
  const g = record(input.getter); let getter: GetterClosure;
  if (g.kind === "closed") {
    keys(g, ["kind", "exitCode", "signal", "timedOut"]);
    insist(typeof g.timedOut === "boolean");
    const exitCode = g.exitCode === null ? null : integer(g.exitCode, 0, 255);
    const signal = g.signal === null ? null : text(g.signal, 32, /^SIG[A-Z0-9]+$/);
    insist((exitCode === null) !== (signal === null));
    getter = { kind: "closed", exitCode, signal, timedOut: g.timedOut };
  } else { keys(g, ["kind"]); insist(g.kind === "not-issued" || g.kind === "unresolved"); getter = { kind: g.kind }; }
  const s = record(input.systemInfo); let systemInfo: SystemInfo;
  if (s.kind === "available") {
    keys(s, ["kind", "modelName", "firmwareVersion", "sdkVersion", "boardType"]);
    insist(getter.kind === "closed" && getter.exitCode === 0 && getter.signal === null && !getter.timedOut);
    systemInfo = { kind: "available", modelName: text(s.modelName, 1024), firmwareVersion: text(s.firmwareVersion, 1024),
      sdkVersion: text(s.sdkVersion, 1024), boardType: text(s.boardType, 1024) };
  } else { keys(s, ["kind", "reason"]); insist(s.kind === "unavailable" && (s.reason === "getter-failed" || s.reason === "getter-response"));
    const successful = getter.kind === "closed" && getter.exitCode === 0 && getter.signal === null && !getter.timedOut;
    insist(s.reason === (successful ? "getter-response" : "getter-failed"));
    systemInfo = { kind: "unavailable", reason: s.reason }; }
  const c = record(input.cpu); let cpu: CpuFacts;
  if (c.kind === "available") {
    keys(c, ["kind", "processorCount", "architecture", "features", "sha256"]);
    insist(Array.isArray(c.features) && c.features.length <= 128);
    const features = c.features.map((feature: unknown) => text(feature, 64, /^[a-z0-9_]+$/));
    insist(new Set(features).size === features.length && features.join(",") === [...features].sort().join(","));
    const processorCount = integer(c.processorCount, 1, 256), architecture = text(c.architecture, 32, /^[A-Za-z0-9_.+-]+$/);
    const sha256 = text(c.sha256, 64, /^[a-f0-9]{64}$/);
    const crypto = require("node:crypto") as typeof import("node:crypto");
    insist(sha256 === crypto.createHash("sha256").update(JSON.stringify({ processorCount, architecture, features })).digest("hex"));
    cpu = { kind: "available", processorCount, architecture, features, sha256 };
  } else { keys(c, ["kind", "reason"]); insist(c.kind === "unavailable" && c.reason === "cpu-response"); cpu = { kind: "unavailable", reason: "cpu-response" }; }
  insist(Array.isArray(input.libraries) && input.libraries.length === expected.sonames.length);
  let total = 0;
  const libraries: LibraryFact[] = input.libraries.map((value: unknown, index: number) => {
    const l = record(value); insist(l.soname === expected.sonames[index]); const soname = expected.sonames[index]; insist(soname !== undefined);
    if (l.kind === "unavailable") {
      keys(l, ["soname", "kind", "reason"]);
      insist(l.reason === "missing" || l.reason === "unsafe-path" || l.reason === "file-bound" || l.reason === "unsupported-elf");
      return { soname, kind: "unavailable", reason: l.reason };
    }
    keys(l, ["soname", "kind", "path", "bytes", "sha256", "elf"]); insist(l.kind === "available");
    const path = text(l.path, 4096); insist(/^\/(?:lib|usr\/lib)\//.test(path) && path.split("/").slice(1).every(part => part !== "" && part !== "." && part !== ".."));
    const length = integer(l.bytes, 52, 32 * 1024 * 1024); total += length; insist(total <= 128 * 1024 * 1024);
    const elf = record(l.elf); keys(elf, ["class", "endian", "machine", "type", "flags"]);
    insist(elf.class === 32 && elf.endian === "little" && elf.machine === 40 && (elf.type === 2 || elf.type === 3));
    return { soname, kind: "available", path, bytes: length, sha256: text(l.sha256, 64, /^[a-f0-9]{64}$/),
      elf: { class: 32, endian: "little", machine: 40, type: elf.type, flags: integer(elf.flags, 0, 0xffffffff) } };
  });
  return { schemaVersion: 1, appId: MAIN_APP_ID, executableSha256: expected.requirements.executableSha256,
    epoch, runtime, getter, systemInfo, cpu, libraries };
}

type TargetRequest = { readonly requirements: NativeRequirements; readonly sonames: readonly string[]; readonly budgetMs: number };

/** Kept self-contained so strict compilation emits the only target implementation. */
function stockPrerequisites(request: TargetRequest): NativePrerequisiteFacts {
  const fs = require("node:fs") as typeof import("node:fs");
  const cp = require("node:child_process") as typeof import("node:child_process");
  const crypto = require("node:crypto") as typeof import("node:crypto");
  const path = require("node:path") as typeof import("node:path");
  const start = process.hrtime.bigint();
  const elapsed = () => Number((process.hrtime.bigint() - start) / 1000000n);
  const bounded = () => { if (elapsed() >= request.budgetMs) throw new Error("deadline"); };
  function check(value: unknown): asserts value { if (!value) throw new Error("read-refused"); }
  check(process.platform === "linux" && /^16\.\d+\.\d+$/.test(process.versions.node) && process.geteuid?.() === 0);
  const stable = (a: import("node:fs").BigIntStats, b: import("node:fs").BigIntStats) =>
    (["dev", "ino", "mode", "uid", "gid", "nlink", "size", "mtimeNs", "ctimeNs"] as const).every(key => a[key] === b[key]);
  const missing = (error: unknown) => error !== null && typeof error === "object" && "code" in error && (error.code === "ENOENT" || error.code === "ESRCH");
  function readProc(name: string, maximum: number): Buffer {
    bounded(); const before = fs.lstatSync(name, { bigint: true });
    check(before.isFile() && before.uid === 0n);
    const fd = fs.openSync(name, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
    try {
      const initial = fs.fstatSync(fd, { bigint: true }); check(stable(before, initial));
      const bytes = Buffer.alloc(maximum + 1); let count = 0;
      while (count < bytes.length) { bounded(); const size = fs.readSync(fd, bytes, count, bytes.length - count, null); if (!size) break; count += size; }
      check(count <= maximum && stable(initial, fs.fstatSync(fd, { bigint: true })) && stable(initial, fs.lstatSync(name, { bigint: true })));
      return bytes.subarray(0, count);
    } finally { fs.closeSync(fd); }
  }
  function epoch(): Epoch {
    const bootId = readProc("/proc/sys/kernel/random/boot_id", 128).toString().trim();
    check(/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(bootId));
    const directory = fs.opendirSync("/proc", { bufferSize: 1 });
    const compositors: { pid: number; startTicks: string }[] = []; let count = 0, pidCount = 0;
    try {
      for (;;) {
        bounded(); const entry = directory.readSync(); if (!entry) break; check(++count <= 8192);
        if (!/^[1-9][0-9]*$/.test(entry.name)) continue;
        check(++pidCount <= 2048);
        try {
          const base = "/proc/" + entry.name; if (fs.readlinkSync(base + "/exe") !== "/usr/bin/surface-manager") continue;
          const value = readProc(base + "/stat", 4096).toString(); const end = value.lastIndexOf(") "); check(end >= 0);
          const fields = value.slice(end + 2).trim().split(/\s+/); const startTicks = fields[19];
          check(fields[0] !== "Z" && startTicks !== undefined && /^[1-9][0-9]{0,19}$/.test(startTicks));
          check(fs.readlinkSync(base + "/exe") === "/usr/bin/surface-manager");
          const pid = Number(entry.name); check(Number.isSafeInteger(pid) && pid > 1 && pid <= 0x7fffffff);
          compositors.push({ pid, startTicks });
        } catch (error) { if (!missing(error)) throw error; }
      }
    } finally { directory.closeSync(); }
    check(compositors.length === 1); const compositor = compositors[0]; check(compositor !== undefined);
    return { bootId, compositor };
  }
  const firstEpoch = epoch();
  bounded();
  const getterResult = cp.spawnSync("/usr/bin/luna-send", ["-n", "1", "-w", "2000",
    "luna://com.webos.service.tv.systemproperty/getSystemInfo",
    JSON.stringify({ keys: ["modelName", "firmwareVersion", "sdkVersion", "boardType"], subscribe: false })],
  { encoding: "utf8", timeout: Math.min(2500, request.budgetMs - elapsed()), killSignal: "SIGKILL", maxBuffer: 16384,
    stdio: ["ignore", "pipe", "pipe"], env: { PATH: "/usr/bin:/bin", LANG: "C", LC_ALL: "C" } });
  const timedOut = getterResult.error !== undefined && "code" in getterResult.error && getterResult.error.code === "ETIMEDOUT";
  const getter: GetterClosure = getterResult.status !== null || getterResult.signal !== null
    ? { kind: "closed", exitCode: getterResult.status, signal: getterResult.signal, timedOut }
    : getterResult.pid === 0 ? { kind: "not-issued" } : { kind: "unresolved" };
  let systemInfo: SystemInfo = { kind: "unavailable", reason: "getter-failed" };
  if (getter.kind === "closed" && getter.exitCode === 0 && getter.signal === null && !getter.timedOut) {
    try {
      check(typeof getterResult.stdout === "string" && Buffer.byteLength(getterResult.stdout) <= 16384);
      const raw: unknown = JSON.parse(getterResult.stdout); check(raw !== null && typeof raw === "object" && !Array.isArray(raw));
      const info = raw as Record<string, unknown>; check(info.returnValue === true);
      function infoString(value: unknown): string { check(typeof value === "string" && value.length > 0 && Buffer.byteLength(value) <= 1024 && !/[\x00-\x1f\x7f]/.test(value)); return value; }
      systemInfo = { kind: "available", modelName: infoString(info.modelName), firmwareVersion: infoString(info.firmwareVersion),
        sdkVersion: infoString(info.sdkVersion), boardType: infoString(info.boardType) };
    } catch { systemInfo = { kind: "unavailable", reason: "getter-response" }; }
  }
  let cpu: CpuFacts = { kind: "unavailable", reason: "cpu-response" };
  const cpuBytes = readProc("/proc/cpuinfo", 65536);
  try {
    const groups = cpuBytes.toString().trim().split(/\n\s*\n/); check(groups.length > 0 && groups.length <= 257);
    const cores = groups.map(group => {
      const values = new Map<string, string>();
      for (const line of group.split("\n")) { const split = line.indexOf(":"); if (split >= 0) { const key = line.slice(0, split).trim(); check(!values.has(key)); values.set(key, line.slice(split + 1).trim()); } }
      const processor = values.get("processor"); if (processor === undefined) return null;
      const architecture = values.get("CPU architecture"), tokens = values.get("Features");
      check(/^(?:0|[1-9][0-9]{0,2})$/.test(processor) && architecture !== undefined && /^[A-Za-z0-9_.+-]{1,32}$/.test(architecture) && tokens !== undefined);
      const features = tokens.split(/\s+/).filter(Boolean); check(features.length <= 128 && features.every(feature => /^[a-z0-9_]{1,64}$/.test(feature)) && new Set(features).size === features.length);
      return { processor, architecture, features };
    }).filter((core): core is NonNullable<typeof core> => core !== null);
    const first = cores[0]; check(first !== undefined && cores.length <= 256 && new Set(cores.map(core => core.processor)).size === cores.length && cores.every(core => core.architecture === first.architecture));
    const normalized = { processorCount: cores.length, architecture: first.architecture,
      features: first.features.filter(feature => cores.every(core => core.features.includes(feature))).sort() };
    cpu = { kind: "available", ...normalized, sha256: crypto.createHash("sha256").update(JSON.stringify(normalized)).digest("hex") };
  } catch { /* Report only extracted facts; never return raw CPU text or Serial. */ }

  const handles: number[] = [];
  const directories: { fd: number; anchored: string; initial: import("node:fs").BigIntStats }[] = [];
  const links: { anchored: string; initial: import("node:fs").BigIntStats; target: string }[] = [];
  const files: { fd: number; anchored: string; initial: import("node:fs").BigIntStats }[] = [];
  const inRoots = (name: string) => name === "/lib" || name.startsWith("/lib/") || name === "/usr/lib" || name.startsWith("/usr/lib/");
  function openDirectory(anchored: string): number {
    bounded(); check(handles.length < 512); const before = fs.lstatSync(anchored, { bigint: true });
    if (!before.isDirectory() || before.uid !== 0n || (before.mode & 0o022n) !== 0n) throw new Error("unsafe-path");
    const fd = fs.openSync(anchored, fs.constants.O_RDONLY | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW); handles.push(fd);
    const initial = fs.fstatSync(fd, { bigint: true }); check(stable(before, initial));
    directories.push({ fd, anchored, initial }); return fd;
  }
  function resolve(name: string): { fd: number; anchored: string; canonical: string; initial: import("node:fs").BigIntStats } {
    let current = name, depth = 0;
    for (;;) {
      check(inRoots(current) && current.length <= 4096);
      const parts = current.slice(1).split("/"); if (parts.length > 16) throw new Error("unsafe-path");
      let fd = openDirectory("/"); let canonical = ""; let restarted = false;
      for (let index = 0; index < parts.length; index++) {
        bounded(); const part = parts[index]; check(part !== undefined && part !== "" && part !== "." && part !== "..");
        const anchored = `/proc/self/fd/${fd}/${part}`; const before = fs.lstatSync(anchored, { bigint: true });
        if (before.uid !== 0n) throw new Error("unsafe-path");
        if (before.isSymbolicLink()) {
          if (++depth > 8) throw new Error("unsafe-path"); const target = fs.readlinkSync(anchored);
          if (target.length === 0 || Buffer.byteLength(target) > 4096 || /[\x00-\x1f\x7f]/.test(target)) throw new Error("unsafe-path");
          check(stable(before, fs.lstatSync(anchored, { bigint: true })) && fs.readlinkSync(anchored) === target);
          links.push({ anchored, initial: before, target });
          current = path.posix.resolve(canonical || "/", target, ...parts.slice(index + 1));
          if (!inRoots(current)) throw new Error("unsafe-path"); restarted = true; break;
        }
        canonical += "/" + part;
        if (index + 1 < parts.length) { fd = openDirectory(anchored); continue; }
        if (!before.isFile() || (before.mode & 0o022n) !== 0n) throw new Error("unsafe-path");
        check(handles.length < 512);
        const file = fs.openSync(anchored, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK); handles.push(file);
        const initial = fs.fstatSync(file, { bigint: true }); check(stable(before, initial));
        files.push({ fd: file, anchored, initial }); return { fd: file, anchored, canonical, initial };
      }
      check(restarted);
    }
  }
  function verifyPaths(): void {
    for (const item of directories) { bounded(); check(stable(item.initial, fs.fstatSync(item.fd, { bigint: true })) && stable(item.initial, fs.lstatSync(item.anchored, { bigint: true }))); }
    for (const item of links) { bounded(); check(stable(item.initial, fs.lstatSync(item.anchored, { bigint: true })) && item.target === fs.readlinkSync(item.anchored)); }
    for (const item of files) { bounded(); check(stable(item.initial, fs.fstatSync(item.fd, { bigint: true })) && stable(item.initial, fs.lstatSync(item.anchored, { bigint: true }))); }
  }
  let total = 0;
  try {
    const libraries: LibraryFact[] = request.sonames.map(soname => {
      let file: ReturnType<typeof resolve> | undefined;
      for (const root of ["/lib", "/usr/lib"]) {
        try { file = resolve(root + "/" + soname); break; }
        catch (error) {
          if (missing(error)) continue;
          if (error instanceof Error && error.message === "unsafe-path") return { soname, kind: "unavailable", reason: "unsafe-path" };
          throw error;
        }
      }
      if (!file) return { soname, kind: "unavailable", reason: "missing" };
      const length = Number(file.initial.size);
      if (!Number.isSafeInteger(length) || length < 52 || length > 32 * 1024 * 1024 || total + length > 128 * 1024 * 1024) return { soname, kind: "unavailable", reason: "file-bound" };
      total += length; const buffer = Buffer.alloc(65536), header = Buffer.alloc(52), hash = crypto.createHash("sha256"); let read = 0;
      while (read < length) {
        bounded(); const count = fs.readSync(file.fd, buffer, 0, Math.min(buffer.length, length - read), read); check(count > 0);
        if (read < header.length) buffer.copy(header, read, 0, Math.min(count, header.length - read));
        hash.update(buffer.subarray(0, count)); read += count;
      }
      check(fs.readSync(file.fd, buffer, 0, 1, read) === 0 && stable(file.initial, fs.fstatSync(file.fd, { bigint: true })) && stable(file.initial, fs.lstatSync(file.anchored, { bigint: true })));
      const type = header.readUInt16LE(16);
      if (!header.subarray(0, 7).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46, 1, 1, 1])) || header.readUInt16LE(18) !== 40
        || header.readUInt32LE(20) !== 1 || header.readUInt16LE(40) !== 52 || (type !== 2 && type !== 3)) return { soname, kind: "unavailable", reason: "unsupported-elf" };
      return { soname, kind: "available", path: file.canonical, bytes: length, sha256: hash.digest("hex"),
        elf: { class: 32, endian: "little", machine: 40, type, flags: header.readUInt32LE(36) } };
    });
    verifyPaths(); const lastEpoch = epoch(); check(JSON.stringify(firstEpoch) === JSON.stringify(lastEpoch)); verifyPaths(); bounded();
    return { schemaVersion: 1, appId: request.requirements.appId, executableSha256: request.requirements.executableSha256,
      epoch: firstEpoch, runtime: { node: process.versions.node, platform: "linux", arch: process.arch, euid: 0 }, getter, systemInfo, cpu, libraries };
  } finally { for (const fd of handles.reverse()) fs.closeSync(fd); }
}
