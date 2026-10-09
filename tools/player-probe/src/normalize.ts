import { spawnSync } from "node:child_process";
import { chmod, mkdir, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { gunzipSync } from "node:zlib";
import { ipkMembers } from "./package.js";

export type PackageFile = { bytes: Buffer; mode: 0o644 | 0o755 };
export type PackageFiles = ReadonlyMap<string, PackageFile>;
export type PackageIdentity = { id: string; version: string; architecture: "arm" | "all" };
export const TOOL_ENVIRONMENT = { PATH: "/usr/bin:/bin", LANG: "C", LC_ALL: "C", TZ: "UTC" };
function invalid(): never { throw new Error("invalidIpkPayload"); }
function command(executable: "/usr/bin/tar" | "/usr/bin/ar", args: string[], input: Buffer | undefined, maxOutput: number, cwd?: string): Buffer {
  const result = spawnSync(executable, args, { shell: false, env: TOOL_ENVIRONMENT, input, cwd, timeout: 10000, maxBuffer: maxOutput });
  if (result.error || result.status !== 0 || result.stderr.length !== 0) invalid();
  return result.stdout;
}
function inspect(tar: Buffer, args: string[], maxOutput: number): Buffer {
  return command("/usr/bin/tar", ["--warning=no-unknown-keyword", "--ignore-zeros", ...args], tar, maxOutput);
}
function unpack(bytes: Buffer, maxOutput: number): Buffer {
  try { return gunzipSync(bytes, { maxOutputLength: maxOutput }); } catch { return invalid(); }
}
function safePath(name: string): boolean {
  return name.length <= 240 && name.split("/").every((part) => /^[A-Za-z0-9_.-]+$/.test(part) && part !== "." && part !== "..");
}
function directories(files: PackageFiles): Set<string> {
  const result = new Set<string>();
  for (const name of files.keys()) {
    if (!safePath(name)) invalid();
    const parts = name.split("/");
    for (let length = 1; length < parts.length; length += 1) result.add(parts.slice(0, length).join("/") + "/");
  }
  return result;
}
function inventory(tar: Buffer, files: PackageFiles, maxBytes: number, cliModes: boolean): void {
  const dirs = directories(files);
  const names = inspect(tar, ["-tf", "-"], 32768).toString("utf8").trimEnd().split("\n");
  const descriptions = inspect(tar, ["-tvf", "-"], 32768).toString("utf8").trimEnd().split("\n");
  if (names.length > files.size + dirs.size || names.length !== descriptions.length || new Set(names).size !== names.length) invalid();
  const seen = new Set<string>();
  for (let index = 0; index < names.length; index += 1) {
    const name = names[index]; const description = descriptions[index];
    if (!name || !description || !description.endsWith(" " + name)) invalid();
    const file = files.get(name);
    if (file) {
      const mode = file.mode === 0o755 ? "-rwxr-xr-x" : "-rw-r--r--";
      const generated = name === "control" || /^usr\/palm\/packages\/[a-z0-9.]+\/packageinfo\.json$/.test(name);
      if ((!description.startsWith(mode + " ") && !(cliModes && generated && description.startsWith("-rw-rw-rw- ")))
        || !inspect(tar, ["-xOf", "-", "--", name], maxBytes).equals(file.bytes)) invalid();
      seen.add(name);
    } else if (!dirs.has(name) || (!description.startsWith("drwxr-xr-x ") && !(cliModes && description.startsWith("drwxrwxrwx ")))) invalid();
  }
  if (seen.size !== files.size) invalid();
}
function controlMetadata(tar: Buffer, identity: PackageIdentity, maxBytes: number, cliModes: boolean): Buffer {
  const bytes = inspect(tar, ["-xOf", "-", "--", "control"], 4096);
  if (bytes.some((byte) => byte > 0x7f)) invalid();
  const lines = bytes.toString("ascii").split("\n");
  if (lines.pop() !== "" || lines.length !== 10) invalid();
  const fields = new Map<string, string>();
  for (const line of lines) {
    const split = line.indexOf(": ");
    if (split < 1 || fields.has(line.slice(0, split))) invalid();
    fields.set(line.slice(0, split), line.slice(split + 2));
  }
  const expected = {
    Package: identity.id, Version: identity.version, Section: "misc", Priority: "optional", Architecture: identity.architecture,
    Maintainer: "N/A <nobody@example.com>", Description: "This is a webOS application.",
    "webOS-Package-Format-Version": "2", "webOS-Packager-Version": "x.y.x",
  };
  const size = fields.get("Installed-Size");
  if (!size || !/^[1-9][0-9]{0,8}$/.test(size) || Number(size) > maxBytes || Object.entries(expected).some(([key, value]) => fields.get(key) !== value)) invalid();
  inventory(tar, new Map([["control", { bytes, mode: 0o644 }]]), 16384, cliModes);
  return bytes;
}
/** Bounded read-only GNU tar inspection; no archive member is extracted to disk. */
export function inspectIpk(bytes: Buffer, expected: PackageFiles, identity: PackageIdentity, maxBytes = 2 * 1024 * 1024, cliModes = false): Buffer {
  if (expected.size < 1 || expected.size > 64 || !/^[a-z0-9.]{1,80}$/.test(identity.id) || !/^[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}$/.test(identity.version)
    || !["arm", "all"].includes(identity.architecture) || [...expected.values()].some((file) => file.mode !== 0o644 && file.mode !== 0o755)
    || [...expected.values()].reduce((sum, file) => sum + file.bytes.length, 0) > maxBytes) invalid();
  const members = ipkMembers(bytes, maxBytes);
  const data = members.get("data.tar.gz"); const control = members.get("control.tar.gz");
  if (!data || !control) invalid();
  const controlBytes = controlMetadata(unpack(control, 16384), identity, maxBytes, cliModes);
  inventory(unpack(data, maxBytes), expected, maxBytes, cliModes);
  return controlBytes;
}
/** Reconstruct an unsigned archive from admitted sealed buffers using fixed GNU tools. */
export async function normalizeIpk(bytes: Buffer, expected: PackageFiles, identity: PackageIdentity, work: string, maxBytes = 2 * 1024 * 1024): Promise<Buffer> {
  const control = inspectIpk(bytes, expected, identity, maxBytes, true);
  const root = resolve(work);
  if (!/^\/workspace\/\.local\/(native-package|player-probe)\/normalization(?:\/[a-z]+)?$/.test(root)) invalid();
  await mkdir(dirname(root), { recursive: true });
  if (await realpath(dirname(root)) !== dirname(root)) invalid();
  await rm(root, { recursive: true, force: true });
  for (const kind of ["data", "control"]) await mkdir(join(root, kind), { recursive: true, mode: 0o755 });
  for (const directory of directories(expected)) {
    await mkdir(join(root, "data", directory), { recursive: true, mode: 0o755 });
    await chmod(join(root, "data", directory), 0o755);
  }
  for (const [name, file] of expected) {
    await writeFile(join(root, "data", name), file.bytes, { mode: file.mode });
    await chmod(join(root, "data", name), file.mode);
  }
  await writeFile(join(root, "control/control"), control, { mode: 0o644 });
  await chmod(join(root, "control/control"), 0o644);
  await writeFile(join(root, "debian-binary"), "2.0\n", { mode: 0o644 });
  for (const kind of ["data", "control"]) {
    const names = kind === "data" ? [...new Set([...expected.keys()].map((name) => name.split("/")[0] ?? invalid()))].sort() : ["control"];
    command("/usr/bin/tar", ["--format=ustar", "--sort=name", "--owner=0", "--group=0", "--numeric-owner", "--mtime=@0", "-czf", join(root, `${kind}.tar.gz`), "--", ...names], undefined, 16384, join(root, kind));
  }
  command("/usr/bin/ar", ["rcD", "normalized.ipk", "debian-binary", "control.tar.gz", "data.tar.gz"], undefined, 16384, root);
  const result = await readFile(join(root, "normalized.ipk"));
  inspectIpk(result, expected, identity, maxBytes);
  return result;
}

export function archiveToolVersions(): { ar: string; tar: string } {
  return {
    ar: command("/usr/bin/ar", ["--version"], undefined, 4096).toString("utf8").split("\n")[0] ?? invalid(),
    tar: command("/usr/bin/tar", ["--version"], undefined, 4096).toString("utf8").split("\n")[0] ?? invalid(),
  };
}
