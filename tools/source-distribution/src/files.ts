import { constants } from "node:fs";
import { lstat, open } from "node:fs/promises";
import { join, resolve, sep } from "node:path";
import { readInput } from "../../package-native/src/input.js";
import { ordinaryDirectory, MAX_TAR_BYTES } from "./archive.js";
import { admitProject, type ProjectSnapshot, type SourceFile } from "./project.js";
function invalid(): never { throw new Error("invalidSourceFile"); }
/** The shared bounded reader plus a single-link condition; hashes/tree seals admit content. */
export async function readSourceFile(path: string, max: number): Promise<Buffer> {
  const before = await lstat(path); if (before.nlink !== 1) invalid();
  const bytes = await readInput(path, max); const after = await lstat(path);
  if (before.dev !== after.dev || before.ino !== after.ino || after.nlink !== 1) invalid(); return bytes;
}
/** Exclusive ordinary output under this worktree's ignored source-distribution namespace. */
export async function writeCandidate(directory: string, name: string, bytes: Buffer): Promise<void> {
  const absolute = resolve(directory); const scope = resolve(".local/source-distribution") + sep;
  if (!absolute.startsWith(scope) || !/^[A-Za-z0-9_.-]{1,128}$/.test(name) || name === "." || name === ".." || bytes.length > MAX_TAR_BYTES) invalid();
  await ordinaryDirectory(absolute);
  const file = await open(join(absolute, name), constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o644);
  try {
    await file.writeFile(bytes); await file.chmod(0o644); await file.sync();
    const stat = await file.stat(); if (!stat.isFile() || stat.nlink !== 1 || stat.size !== bytes.length) invalid();
  } finally { await file.close(); }
}
export async function savePrepared(directory: string, project: ProjectSnapshot): Promise<void> {
  admitProject(project);
  const bytes = Buffer.from(JSON.stringify({ schemaVersion: 1, sourceCommit: project.revision, commit: project.commit.toString("base64"), files: project.files.map((file) => ({ name: file.name, mode: file.mode, base64: file.bytes.toString("base64") })) }) + "\n");
  if (bytes.length > 192 * 1024 * 1024) invalid(); await writeCandidate(directory, "project.json", bytes);
}
function record(value: unknown, keys: string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join("\0") !== keys.sort().join("\0")) invalid(); return value as Record<string, unknown>;
}
function decoded(value: unknown, limit: number): Buffer {
  if (typeof value !== "string" || value.length > Math.ceil(limit / 3) * 4) invalid();
  const bytes = Buffer.from(value, "base64"); if (bytes.length > limit || bytes.toString("base64") !== value) invalid(); return bytes;
}
export async function loadPrepared(path: string): Promise<ProjectSnapshot> {
  let raw: unknown; try { raw = JSON.parse((await readSourceFile(path, 192 * 1024 * 1024)).toString("utf8")); } catch { return invalid(); }
  const root = record(raw, ["schemaVersion", "sourceCommit", "commit", "files"]);
  if (root.schemaVersion !== 1 || typeof root.sourceCommit !== "string" || !Array.isArray(root.files) || root.files.length < 1 || root.files.length > 4096) invalid();
  const files: SourceFile[] = root.files.map((item: unknown) => {
    const file = record(item, ["name", "mode", "base64"]);
    if (typeof file.name !== "string" || (file.mode !== 0o644 && file.mode !== 0o755)) invalid();
    return { name: file.name, mode: file.mode, bytes: decoded(file.base64, 32 * 1024 * 1024) };
  });
  const project = { revision: root.sourceCommit, commit: decoded(root.commit, 16384), files }; admitProject(project); return project;
}
