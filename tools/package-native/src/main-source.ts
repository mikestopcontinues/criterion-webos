import { constants } from "node:fs";
import { lstat, mkdir, open, realpath } from "node:fs/promises";
import { dirname, join, resolve, sep } from "node:path";
import { admitProject, safeName, type ProjectSnapshot, type SourceFile } from "../../source-distribution/src/project.js";
import type { OfflineSources } from "../../source-distribution/src/offline.js";
import { readInput } from "./input.js";
import { REQUIRED_SOURCES, sha256, type BuildReceipt } from "./receipt.js";
import { APP_ID, VERSION } from "./admission.js";

export async function ordinaryRoot(path: string): Promise<void> {
  if (resolve(path) !== path) throw new Error("invalidProducerPath");
  let parent: string = sep;
  for (const part of path.split(sep).filter(Boolean)) {
    parent = join(parent, part); const stat = await lstat(parent);
    if (!stat.isDirectory() || stat.isSymbolicLink() || await realpath(parent) !== parent) throw new Error("invalidProducerPath");
  }
}
export async function writeOwned(path: string, bytes: Buffer, mode: 0o644 | 0o755 | 0o600 = 0o600): Promise<void> {
  const file = await open(path, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, mode);
  try { await file.writeFile(bytes); await file.chmod(mode); await file.sync(); }
  finally { await file.close(); }
}
async function materialize(root: string, files: readonly SourceFile[], directories: readonly string[], check: () => void): Promise<void> {
  await mkdir(root, { mode: 0o700 }); check();
  for (const name of directories) {
    if (!safeName(name)) throw new Error("invalidProducerSource");
    check(); await mkdir(join(root, name), { recursive: true, mode: 0o755 }); check();
  }
  for (const file of files) {
    check(); if (!safeName(file.name)) throw new Error("invalidProducerSource");
    await mkdir(dirname(join(root, file.name)), { recursive: true, mode: 0o755 });
    await writeOwned(join(root, file.name), file.bytes, file.mode); check();
  }
}
export async function prepareMainProject(root: string, project: ProjectSnapshot, check: () => void): Promise<void> {
  admitProject(project); check();
  await materialize(join(root, "source"), project.files, [], check);
  // Empty mount points never enter the committed input inventory.
  await mkdir(join(root, "source/tools/player-probe/node_modules"), { recursive: true, mode: 0o755 });
  await mkdir(join(root, "source/.local/native-package"), { recursive: true, mode: 0o755 }); check();
}
export async function prepareMainOffline(root: string, offline: OfflineSources, check: () => void): Promise<void> {
  await materialize(join(root, "rust"), offline.runtimeFiles, offline.runtimeDirectories, check);
  const vendorFiles: SourceFile[] = []; const vendorDirectories: string[] = [];
  for (const crate of offline.registry) {
    const name = `${crate.name}-${crate.version}`;
    vendorDirectories.push(name, ...crate.directories.map(path => `${name}/${path}`));
    vendorFiles.push(...crate.files.map(file => ({ ...file, name: `${name}/${file.name}` })));
  }
  await materialize(join(root, "vendor"), vendorFiles, vendorDirectories, check);
  for (const path of ["target", "cargo", "package"]) { check(); await mkdir(join(root, path), { mode: 0o700 }); check(); }
  await writeOwned(join(root, "cargo/config.toml"), Buffer.from('[source.crates-io]\nreplace-with = "verified-vendor"\n[source.verified-vendor]\ndirectory = "/vendor"\n[net]\noffline = true\n'));
  await mkdir(join(root, "package/compiled"), { mode: 0o755 }); check();
}
export function receiptForMain(project: ProjectSnapshot, executable: Buffer): BuildReceipt {
  const files = new Map(project.files.map(file => [file.name, file.bytes]));
  const sourceSha256: Record<string, string> = {};
  for (const file of project.files) {
    if ([...REQUIRED_SOURCES, "Cargo.lock"].includes(file.name) || file.name.startsWith("crates/") && /\.(rs|c|h|toml|glsl|vert|frag|wgsl)$/.test(file.name)) sourceSha256[file.name] = sha256(file.bytes);
  }
  const lock = files.get("Cargo.lock");
  if (!lock || REQUIRED_SOURCES.some(path => !files.has(path))) throw new Error("invalidProducerSource");
  return { schemaVersion: 1, appId: APP_ID, version: VERSION, target: "arm-unknown-linux-gnueabi", profile: "release",
    sourceCommit: project.revision, cargoLockSha256: sha256(lock), executableSha256: sha256(executable), sourceSha256 };
}
export async function verifyMaterialized(root: string, files: readonly SourceFile[], check: () => void): Promise<void> {
  for (const file of files) {
    check(); const path = join(root, file.name); const stat = await lstat(path);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.nlink !== 1 || (stat.mode & 0o777) !== file.mode || !(await readInput(path, 32 * 1024 * 1024, 0)).equals(file.bytes)) throw new Error("producerSourceChanged"); check();
  }
}
