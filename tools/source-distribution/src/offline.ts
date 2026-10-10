import { constants } from "node:fs";
import { lstat, open, readdir, realpath } from "node:fs/promises";
import { join, resolve, sep } from "node:path";
import { gunzipSync } from "node:zlib";
import { admitProject, safeName, unambiguousNames, type ProjectSnapshot, type SourceFile } from "./project.js";
import { runtimeSourcePrefix, sha256, sourceRequirements, sourceTar, type ArchiveBudget, type RuntimeInputs } from "./sources.js";

export type OfflineSourceInput = Readonly<{
  project: ProjectSnapshot; requestBytes: Buffer; runtimeInputs: RuntimeInputs;
  readRegistryArchive: (name: string) => Promise<Buffer>;
  installedRuntimeRoot: string; registryRoot: string;
  budget: ArchiveBudget;
}>;
export type OfflineInventory = Readonly<{ name: string; mode: 0o644 | 0o755; size: number; sha256: string }>;
export type OfflineSources = Readonly<{
  sourceCommit: string;
  runtime: Readonly<{ version: string; commit: string; archiveSha256: string; lockSha256: string }>;
  runtimeFiles: readonly SourceFile[]; runtimeDirectories: readonly string[]; runtimeInventory: readonly OfflineInventory[];
  registry: readonly Readonly<{ name: string; version: string; archiveSha256: string; files: readonly SourceFile[]; directories: readonly string[]; inventory: readonly OfflineInventory[] }>[];
}>;
export type OfflineSourceSummary = Readonly<{
  sourceCommit: string; runtime: OfflineSources["runtime"];
  runtimeInventory: readonly OfflineInventory[]; runtimeDirectories: readonly string[];
  registry: readonly Readonly<{ name: string; version: string; archiveSha256: string; inventory: readonly OfflineInventory[]; directories: readonly string[] }>[];
}>;
/** The transport receipt contains source identity and exact shape/hashes, with no source buffers. */
export function summarizeOfflineSources(sources: OfflineSources): OfflineSourceSummary {
  return admitOfflineSummary({ sourceCommit: sources.sourceCommit, runtime: { version: sources.runtime.version, commit: sources.runtime.commit, archiveSha256: sources.runtime.archiveSha256, lockSha256: sources.runtime.lockSha256 }, runtimeInventory: sources.runtimeInventory, runtimeDirectories: sources.runtimeDirectories, registry: sources.registry.map((crate) => ({ name: crate.name, version: crate.version, archiveSha256: crate.archiveSha256, inventory: crate.inventory, directories: crate.directories })) });
}
/** Rechecks toolkit-written files through the same ordinary-tree authenticator used by source admission. */
export async function verifyOfflineMaterialized(rootRust: string, rootVendor: string, summary: OfflineSourceSummary, budget: ArchiveBudget): Promise<void> {
  const check = deadlineCheck(budget); check(); const admitted = admitOfflineSummary(summary); check();
  await compareTree(rootRust, { files: admitted.runtimeInventory, directories: admitted.runtimeDirectories }, check);
  await compareTree(rootVendor, vendorTree(admitted.registry), check); check();
}
type Tree = { files: SourceFile[]; directories: string[] };
type InventoryTree = { files: readonly OfflineInventory[]; directories: readonly string[] };
const MAX_BYTES = 512 * 1024 * 1024;
const MAX_FILE = 32 * 1024 * 1024;
const sorted = (names: Iterable<string>): string[] => [...names].sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
function invalid(): never { throw new Error("invalidOfflineSource"); }
function inventory(files: readonly SourceFile[]): OfflineInventory[] {
  return files.map((file) => ({ name: file.name, mode: file.mode, size: file.bytes.length, sha256: sha256(file.bytes) }));
}
function directoriesFor(files: readonly Pick<SourceFile, "name">[], declared: readonly string[]): string[] {
  const directories = new Set(declared); const names = new Set(files.map((file) => file.name));
  for (const file of files) {
    const parts = file.name.split("/");
    for (let count = 1; count < parts.length; count += 1) directories.add(parts.slice(0, count).join("/"));
  }
  if (directories.size > 32768 || [...directories].some((name) => names.has(name)) || !unambiguousNames([...names, ...directories])) invalid();
  return sorted(directories);
}
function record(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join("\0") !== [...keys].sort().join("\0")) invalid(); return value as Record<string, unknown>;
}
function hash(value: unknown): string { if (typeof value !== "string" || !/^[a-f0-9]{64}$/.test(value)) invalid(); return value; }
function revision(value: unknown): string { if (typeof value !== "string" || !/^[a-f0-9]{40}$/.test(value)) invalid(); return value; }
/** One inventory admission feeds both the metadata decoder and the filesystem authenticator. */
function admitInventoryTree(rawFiles: unknown, rawDirectories: unknown): InventoryTree {
  if (!Array.isArray(rawFiles) || !Array.isArray(rawDirectories) || rawFiles.length + rawDirectories.length > 49152 || rawDirectories.length > 32768) invalid();
  let total = 0;
  const files: OfflineInventory[] = rawFiles.map((raw: unknown) => {
    const file = record(raw, ["name", "mode", "size", "sha256"]);
    if (typeof file.name !== "string" || !safeName(file.name) || (file.mode !== 0o644 && file.mode !== 0o755) || typeof file.size !== "number" || !Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_FILE) invalid();
    total += file.size; if (total > MAX_BYTES) invalid();
    return { name: file.name, mode: file.mode, size: file.size, sha256: hash(file.sha256) };
  });
  const directories: string[] = rawDirectories.map((name: unknown) => { if (typeof name !== "string" || !safeName(name)) invalid(); return name; });
  if (new Set(files.map((file) => file.name)).size !== files.length || new Set(directories).size !== directories.length || !unambiguousNames([...files.map((file) => file.name), ...directories]) || directoriesFor(files, directories).length !== directories.length) invalid();
  return { files: files.sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name))), directories: sorted(directories) };
}
function vendorTree(registry: OfflineSourceSummary["registry"]): InventoryTree {
  const files = registry.flatMap((crate) => crate.inventory.map((file) => ({ ...file, name: `${crate.name}-${crate.version}/${file.name}` })));
  const directories = registry.flatMap((crate) => [`${crate.name}-${crate.version}`, ...crate.directories.map((name) => `${crate.name}-${crate.version}/${name}`)]);
  return admitInventoryTree(files, directories);
}
/** Decodes only the bounded metadata receipt; archive authentication remains the toolkit source phase. */
export function admitOfflineSummary(value: unknown): OfflineSourceSummary {
  const root = record(value, ["sourceCommit", "runtime", "runtimeInventory", "runtimeDirectories", "registry"]);
  const runtime = record(root.runtime, ["version", "commit", "archiveSha256", "lockSha256"]); const commit = revision(runtime.commit);
  if (typeof runtime.version !== "string" || runtime.version.length < 1 || runtime.version.length > 256 || !/^[\x20-\x7e]+$/.test(runtime.version) || !runtime.version.includes(`(${commit.slice(0, 9)} `) || !Array.isArray(root.registry) || root.registry.length > 2048) invalid();
  const runtimeTree = admitInventoryTree(root.runtimeInventory, root.runtimeDirectories); const seen = new Set<string>(); let total = runtimeTree.files.reduce((sum, file) => sum + file.size, 0); let vendorEntries = 0;
  const registry: OfflineSourceSummary["registry"][number][] = root.registry.map((raw: unknown) => {
    const crate = record(raw, ["name", "version", "archiveSha256", "inventory", "directories"]);
    if (typeof crate.name !== "string" || !/^[A-Za-z0-9_-]{1,80}$/.test(crate.name) || typeof crate.version !== "string" || !/^[0-9][A-Za-z0-9.+-]{0,127}$/.test(crate.version)) invalid();
    const key = `${crate.name}-${crate.version}`.toLowerCase(); if (seen.has(key)) invalid(); seen.add(key);
    if (!Array.isArray(crate.inventory) || !Array.isArray(crate.directories)) invalid(); vendorEntries += crate.inventory.length + crate.directories.length + 1; if (vendorEntries > 49152) invalid();
    const tree = admitInventoryTree(crate.inventory, crate.directories); total += tree.files.reduce((sum, file) => sum + file.size, 0); if (total > MAX_BYTES) invalid();
    return { name: crate.name, version: crate.version, archiveSha256: hash(crate.archiveSha256), inventory: tree.files, directories: tree.directories };
  });
  vendorTree(registry);
  return { sourceCommit: revision(root.sourceCommit), runtime: { version: runtime.version, commit, archiveSha256: hash(runtime.archiveSha256), lockSha256: hash(runtime.lockSha256) }, runtimeInventory: runtimeTree.files, runtimeDirectories: runtimeTree.directories, registry };
}
function deadlineCheck(budget: ArchiveBudget): () => number {
  let last = -1;
  return (): number => {
    const now = budget.now();
    if (!Number.isFinite(now) || now < 0 || now < last || !Number.isFinite(budget.deadlineMs) || now >= budget.deadlineMs) throw new Error("sourceDeadline"); last = now; return now;
  };
}
/** GNU tar only reads authenticated archive members to bounded stdout; it never writes an extracted tree. */
function archiveTree(archive: Buffer, root: string, budget: ArchiveBudget): Tree {
  if (archive.length < 18 || archive.length > MAX_FILE) invalid();
  try { gunzipSync(archive, { maxOutputLength: 128 * 1024 * 1024 }); } catch { return invalid(); }
  const descriptionBytes = sourceTar(["--ignore-zeros", "--numeric-owner", "--full-time", "-tvzf", "-"], archive, 8 * 1024 * 1024, budget);
  const descriptions = descriptionBytes.toString("utf8");
  if (!Buffer.from(descriptions).equals(descriptionBytes)) invalid();
  const records = descriptions.trimEnd().split("\n");
  if (records.length < 1 || records.length > 16384) invalid();
  const allNames: string[] = []; const files: { name: string; size: number; mode: 0o644 | 0o755 }[] = []; const directories: string[] = [];
  let total = 0;
  for (const record of records) {
    const match = /^([d-][rwx-]{9}) [0-9]+\/[0-9]+\s+([0-9]+) [0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)? (.+)$/.exec(record);
    if (!match || !match[1] || !match[2] || !match[3]) invalid();
    const name = match[3].replace(/\/$/, "");
    if (!safeName(name) || !(name === root || name.startsWith(root + "/"))) invalid();
    allNames.push(name);
    const size = Number(match[2]); if (!Number.isSafeInteger(size) || size < 0 || size > MAX_FILE) invalid();
    const relative = name.slice(root.length + 1);
    if (match[1].startsWith("d")) {
      if (size !== 0) invalid(); if (relative) directories.push(relative);
    } else {
      if (!relative || match[3].endsWith("/")) invalid();
      total += size; if (total > 128 * 1024 * 1024) invalid();
      files.push({ name: relative, size, mode: match[1].includes("x") ? 0o755 : 0o644 });
    }
  }
  if (files.length < 1 || new Set(allNames).size !== allNames.length || !unambiguousNames(allNames)) invalid();
  const bytes = sourceTar(["--ignore-zeros", "-xOzf", "-"], archive, 128 * 1024 * 1024, budget);
  if (bytes.length !== total) invalid();
  let offset = 0;
  const content = files.map((file): SourceFile => {
    const result = { name: file.name, mode: file.mode, bytes: Buffer.from(bytes.subarray(offset, offset + file.size)) }; offset += file.size; return result;
  }).sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name)));
  return { files: content, directories: directoriesFor(content, directories) };
}
async function ordinaryRoot(path: string): Promise<string> {
  const absolute = resolve(path); let parent: string = sep;
  for (const part of absolute.split(sep).filter(Boolean)) {
    parent = join(parent, part); const stat = await lstat(parent);
    if (!stat.isDirectory() || stat.isSymbolicLink() || await realpath(parent) !== parent) invalid();
  }
  return absolute;
}
async function compareTree(root: string, expected: InventoryTree, check: () => void): Promise<void> {
  const admitted = admitInventoryTree(expected.files, expected.directories);
  const absolute = await ordinaryRoot(root); const files = new Map(admitted.files.map((file) => [file.name, file])); const directories = new Set(admitted.directories);
  let visited = 0; const foundFiles = new Set<string>(); const foundDirectories = new Set<string>();
  async function visit(relative: string): Promise<void> {
    check(); const path = join(absolute, relative); const before = await lstat(path);
    if (!before.isDirectory() || before.isSymbolicLink() || (before.mode & 0o7000) !== 0 || await realpath(path) !== path) invalid();
    const entries = await readdir(path);
    for (const entry of entries) {
      check(); const name = relative ? relative + "/" + entry : entry;
      if (!safeName(name) || ++visited > 49152) invalid();
      const child = join(absolute, name); const stat = await lstat(child);
      if (stat.isDirectory() && !stat.isSymbolicLink()) {
        if (!directories.has(name)) invalid(); foundDirectories.add(name); await visit(name);
      } else {
        const file = files.get(name);
        if (!file || !stat.isFile() || stat.isSymbolicLink() || stat.nlink !== 1 || stat.size !== file.size || (stat.mode & 0o7777) !== file.mode || await realpath(child) !== child) invalid();
        const handle = await open(child, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
        try {
          const opened = await handle.stat();
          if (opened.dev !== stat.dev || opened.ino !== stat.ino || opened.nlink !== 1 || !opened.isFile() || opened.size !== stat.size) invalid();
          const bytes = Buffer.alloc(file.size + 1); let offset = 0;
          while (offset < bytes.length) {
            check(); const result = await handle.read(bytes, offset, bytes.length - offset, offset); if (result.bytesRead === 0) break; offset += result.bytesRead;
          }
          const after = await handle.stat(); const located = await lstat(child);
          if (offset !== file.size || sha256(bytes.subarray(0, offset)) !== file.sha256 || after.dev !== stat.dev || after.ino !== stat.ino || after.nlink !== 1 || after.size !== stat.size || after.mtimeMs !== stat.mtimeMs || after.ctimeMs !== stat.ctimeMs || located.dev !== stat.dev || located.ino !== stat.ino || located.nlink !== 1) invalid();
        } finally { await handle.close(); }
        foundFiles.add(name);
      }
    }
    const after = await lstat(path);
    if (before.dev !== after.dev || before.ino !== after.ino || before.mtimeMs !== after.mtimeMs || before.ctimeMs !== after.ctimeMs || (await readdir(path)).sort().join("\0") !== entries.sort().join("\0")) invalid();
  }
  await visit(""); check();
  if (foundFiles.size !== files.size || foundDirectories.size !== directories.size) invalid();
}
function checksumFile(files: readonly SourceFile[], archiveSha256: string): SourceFile {
  const hashes: Record<string, string> = {};
  for (const file of files) { if (file.name === ".cargo-checksum.json") invalid(); Object.defineProperty(hashes, file.name, { value: sha256(file.bytes), enumerable: true }); }
  return { name: ".cargo-checksum.json", mode: 0o644, bytes: Buffer.from(JSON.stringify({ files: hashes, package: archiveSha256 })) };
}
/** Binds actual prepared source trees to the admitted raw archives, then returns detached build buffers. */
export async function verifyOfflineSources(input: OfflineSourceInput): Promise<OfflineSources> {
  const check = deadlineCheck(input.budget);
  check();
  admitProject(input.project);
  if (input.requestBytes.length > 16384 || input.runtimeInputs.channel.length > 2 * 1024 * 1024 || input.runtimeInputs.archive.length > MAX_FILE || input.runtimeInputs.copyright.length > 8 * 1024 * 1024) invalid();
  const budget: ArchiveBudget = { deadlineMs: input.budget.deadlineMs, now: check };
  const project = { revision: input.project.revision, commit: Buffer.from(input.project.commit), files: input.project.files.map((file) => ({ ...file, bytes: Buffer.from(file.bytes) })) };
  const requestBytes = Buffer.from(input.requestBytes); const runtimeInputs = { channel: Buffer.from(input.runtimeInputs.channel), archive: Buffer.from(input.runtimeInputs.archive), copyright: Buffer.from(input.runtimeInputs.copyright) };
  const requirements = sourceRequirements(project, requestBytes, runtimeInputs, budget); check();
  // The component envelope is already admitted by sourceRequirements; select its actual source-root members.
  const component = archiveTree(runtimeInputs.archive, "rust-src-nightly", budget);
  const sourcePrefix = runtimeSourcePrefix.slice("rust-src-nightly/".length);
  const runtimeFiles = component.files.filter((file) => file.name.startsWith(sourcePrefix)).map((file) => ({ ...file, name: file.name.slice(sourcePrefix.length) }));
  if (!runtimeFiles.some((file) => file.name === "library/Cargo.lock")) invalid();
  const runtimeDirectories = directoriesFor(runtimeFiles, component.directories.filter((name) => name.startsWith(sourcePrefix)).map((name) => name.slice(sourcePrefix.length)).filter(Boolean));
  await compareTree(input.installedRuntimeRoot, { files: inventory(runtimeFiles), directories: runtimeDirectories }, check);
  const registry: OfflineSources["registry"][number][] = []; const allVendorFiles: SourceFile[] = []; const vendorDirectories: string[] = [];
  let total = runtimeFiles.reduce((sum, file) => sum + file.bytes.length, 0);
  for (const source of requirements.registry) {
    check(); const raw = await input.readRegistryArchive(source.file); check();
    total += raw.length; if (raw.length < 1 || raw.length > MAX_FILE || total > MAX_BYTES) invalid();
    const bytes = Buffer.from(raw); if (sha256(bytes) !== source.sha256) invalid();
    const root = `${source.name}-${source.version}`; const tree = archiveTree(bytes, root, budget);
    total += tree.files.reduce((sum, file) => sum + file.bytes.length, 0); if (total > MAX_BYTES) invalid();
    const checksum = checksumFile(tree.files, source.sha256); total += checksum.bytes.length; if (total > MAX_BYTES) invalid();
    const files = [...tree.files, checksum].sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name)));
    registry.push({ name: source.name, version: source.version, archiveSha256: source.sha256, files, directories: tree.directories, inventory: inventory(files) });
    allVendorFiles.push(...files.map((file) => ({ ...file, name: root + "/" + file.name }))); vendorDirectories.push(root, ...tree.directories.map((name) => root + "/" + name));
  }
  await compareTree(input.registryRoot, { files: inventory(allVendorFiles), directories: directoriesFor(allVendorFiles, vendorDirectories) }, check); check();
  return { sourceCommit: project.revision, runtime: { version: requirements.identity.version, commit: requirements.identity.commit, archiveSha256: requirements.request.runtime.sourceArchiveSha256, lockSha256: sha256(requirements.identity.lock) }, runtimeFiles, runtimeDirectories, runtimeInventory: inventory(runtimeFiles), registry };
}
