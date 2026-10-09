import { spawnSync } from "node:child_process";
import { chmod, lstat, mkdir, mkdtemp, realpath, rm, utimes, writeFile } from "node:fs/promises";
import { join, resolve, sep } from "node:path";
import { gunzipSync } from "node:zlib";
import { TOOL_ENVIRONMENT } from "../../player-probe/src/normalize.js";
import { safeName, unambiguousNames, type SourceFile } from "./project.js";
export const MAX_SOURCE_BYTES = 256 * 1024 * 1024;
export const MAX_TAR_BYTES = 320 * 1024 * 1024;
export function invalidArchive(): never { throw new Error("invalidSourceArchive"); }
function command(args: string[], input: Buffer | undefined, maxOutput: number, cwd?: string): Buffer {
  const result = spawnSync("/usr/bin/tar", args, { shell: false, env: TOOL_ENVIRONMENT, input, cwd, timeout: 60000, maxBuffer: maxOutput });
  if (result.error || result.status !== 0 || result.stderr.length !== 0) invalidArchive();
  return result.stdout;
}
/** Creates only ordinary directories; no ancestor link is followed. */
export async function ordinaryDirectory(path: string): Promise<void> {
  const absolute = resolve(path); let parent: string = sep;
  for (const part of absolute.split(sep).filter(Boolean)) {
    parent = join(parent, part);
    try { await mkdir(parent, { mode: 0o755 }); } catch (error: unknown) {
      if (!(error instanceof Error) || !("code" in error) || error.code !== "EEXIST") throw error;
    }
    const stat = await lstat(parent);
    if (!stat.isDirectory() || stat.isSymbolicLink() || await realpath(parent) !== parent) invalidArchive();
  }
}
function admitted(files: readonly SourceFile[]): SourceFile[] {
  if (files.length < 1 || files.length > 8192 || !unambiguousNames(files.map((file) => file.name))) invalidArchive();
  const names = new Set<string>(); let total = 0;
  const result = files.map((file) => {
    total += file.bytes.length;
    if (!safeName(file.name) || names.has(file.name.toLowerCase()) || ![0o644, 0o755].includes(file.mode) || file.bytes.length > 32 * 1024 * 1024 || total > MAX_SOURCE_BYTES) invalidArchive();
    names.add(file.name.toLowerCase());
    return { ...file, bytes: Buffer.from(file.bytes) };
  });
  return result.sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name)));
}
function directoryNames(files: readonly SourceFile[]): string[] {
  const names = new Set<string>(); const fileNames = new Set(files.map((file) => file.name));
  for (const file of files) {
    const parts = file.name.split("/");
    for (let count = 1; count < parts.length; count += 1) {
      const path = parts.slice(0, count).join("/"); if (fileNames.has(path)) invalidArchive(); names.add(path + "/");
    }
  }
  if (names.size > 32768) invalidArchive();
  return [...names].sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
}
async function withTree<T>(files: readonly SourceFile[], work: string, run: (root: string, directories: string[]) => T): Promise<T> {
  if (!/^\/workspace\/\.local\/source-distribution\/archive\/[a-z0-9-]{1,40}$/.test(work)) invalidArchive();
  await ordinaryDirectory(work);
  const root = await mkdtemp(join(work, "tree-"));
  try {
    const directories = directoryNames(files);
    for (const directory of directories) { await mkdir(join(root, directory), { recursive: true, mode: 0o755 }); await chmod(join(root, directory), 0o755); }
    for (const file of files) {
      await writeFile(join(root, file.name), file.bytes, { flag: "wx", mode: file.mode }); await chmod(join(root, file.name), file.mode); await utimes(join(root, file.name), 0, 0);
    }
    for (const directory of [...directories].reverse()) await utimes(join(root, directory), 0, 0);
    return run(root, directories);
  } finally { await rm(root, { recursive: true, force: true }); }
}
function audit(bytes: Buffer, files: readonly SourceFile[], root: string, directories: string[]): void {
  if (bytes.length > MAX_TAR_BYTES || bytes.length < 18 || bytes[0] !== 31 || bytes[1] !== 139 || bytes[2] !== 8 || bytes[3] !== 0 || bytes.readUInt32LE(4) !== 0) invalidArchive();
  let tar: Buffer;
  try { tar = gunzipSync(bytes, { maxOutputLength: MAX_TAR_BYTES }); } catch { return invalidArchive(); }
  const names = command(["--ignore-zeros", "--numeric-owner", "-tf", "-"], tar, 8 * 1024 * 1024).toString().trimEnd().split("\n");
  const descriptions = command(["--ignore-zeros", "--numeric-owner", "-tvf", "-"], tar, 8 * 1024 * 1024).toString().trimEnd().split("\n");
  const expected = new Map(files.map((file) => [file.name, file])); const dirs = new Set(directories);
  if (names.length !== files.length + directories.length || descriptions.length !== names.length || new Set(names).size !== names.length) invalidArchive();
  for (let index = 0; index < names.length; index += 1) {
    const name = names[index]; const fields = descriptions[index]?.trim().split(/\s+/);
    if (!name || !fields || fields.length !== 6 || fields[1] !== "0/0" || fields[3] !== "1970-01-01" || fields[4] !== "00:00" || fields[5] !== name) invalidArchive();
    const file = expected.get(name);
    if (file) {
      if (fields[0] !== (file.mode === 0o755 ? "-rwxr-xr-x" : "-rw-r--r--") || fields[2] !== String(file.bytes.length)) invalidArchive();
    } else if (!dirs.has(name) || fields[0] !== "drwxr-xr-x" || fields[2] !== "0") invalidArchive();
  }
  // One complete read-only comparison; never extract archive members to disk.
  command(["--ignore-zeros", "--compare", "--file=-"], tar, 16384, root);
}
function canonical(files: readonly SourceFile[], root: string, directories: string[]): Buffer {
  const names = [...directories, ...files.map((file) => file.name)].sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  return command(["--format=ustar", "--owner=0", "--group=0", "--numeric-owner", "--mtime=@0", "--no-recursion", "--null", "--verbatim-files-from", "-czf", "-", "-T", "-"], Buffer.from(names.join("\0") + "\0"), MAX_TAR_BYTES, root);
}
export async function auditSourceArchive(bytes: Buffer, files: readonly SourceFile[], work: string): Promise<void> {
  const frozen = admitted(files);
  await withTree(frozen, work, (root, dirs) => {
    if (!bytes.equals(canonical(frozen, root, dirs))) invalidArchive();
    audit(bytes, frozen, root, dirs);
  });
}
/** Fixed USTAR/gzip construction from frozen buffers, audited before returning bytes. */
export async function buildSourceArchive(files: readonly SourceFile[], work: string): Promise<Buffer> {
  const frozen = admitted(files);
  return withTree(frozen, work, (root, directories) => {
    const bytes = canonical(frozen, root, directories);
    audit(bytes, frozen, root, directories); return bytes;
  });
}
