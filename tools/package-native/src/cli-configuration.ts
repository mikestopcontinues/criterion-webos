import type { Stats } from "node:fs";
import { lstat, mkdir, opendir } from "node:fs/promises";
import { dirname, join, resolve, sep } from "node:path";
import { readInput } from "./input.js";
import { ordinaryRoot, writeOwned } from "./main-source.js";
import { sha256 } from "./receipt.js";

const names = ["ares.json", "command-service.json", "config.json", "ipk.json", "novacom-devices.json", "query/query-app.json", "query/query-hosted.json", "query/query-package.json", "query/query-service.json", "sdk.json", "template.json", "webos_emul"] as const;
const rootNames = ["ares.json", "command-service.json", "config.json", "ipk.json", "novacom-devices.json", "query", "sdk.json", "template.json", "webos_emul"] as const;
const queryNames = ["query-app.json", "query-hosted.json", "query-package.json", "query-service.json"] as const;
const maxFileBytes = 64 * 1024, maxConfigurationBytes = 512 * 1024;
type InitialFile = Readonly<{ name: string; bytes: number; sha256: string }>;
type CapturedFile = Readonly<{ path: string; stat: Stats; bytes: Buffer }>;
function invalid(): never { throw new Error("invalidCliConfiguration"); }
function sameIdentity(before: Stats, after: Stats): boolean {
  return before.dev === after.dev && before.ino === after.ino && before.mode === after.mode;
}
function unchanged(before: Stats, after: Stats): boolean {
  return sameIdentity(before, after) && before.nlink === after.nlink && before.size === after.size && before.mtimeMs === after.mtimeMs && before.ctimeMs === after.ctimeMs;
}
function regular(stat: Stats): boolean {
  return stat.isFile() && !stat.isSymbolicLink() && stat.nlink === 1 && (stat.mode & 0o7000) === 0 && stat.size >= 1 && stat.size <= maxFileBytes;
}

/** Owns only a fresh copy of the locked CLI's mutable files/conf directory. Never executes the CLI. */
export async function prepareCliConfiguration(dependenciesRoot: string, destination: string, check: () => void): Promise<readonly InitialFile[]> {
  check();
  if (resolve(dependenciesRoot) !== dependenciesRoot || resolve(destination) !== destination
    || destination === dependenciesRoot || destination.startsWith(dependenciesRoot + sep)) invalid();
  const cliRoot = join(dependenciesRoot, "@webos-tools/cli"), source = join(cliRoot, "files/conf"), query = join(source, "query");
  await ordinaryRoot(source); check(); await ordinaryRoot(query); check(); await ordinaryRoot(dirname(destination)); check();
  // Remember every ancestor used by the fixed source and owned destination paths.
  const directories = new Map<string, Stats>();
  for (const root of [query, dirname(destination)]) {
    let path = root;
    while (!directories.has(path)) {
      check(); const stat = await lstat(path); check();
      if (!stat.isDirectory() || stat.isSymbolicLink()) invalid();
      directories.set(path, stat);
      const parent = dirname(path); if (parent === path) break; path = parent;
    }
  }
  const sourceStat = directories.get(source), queryStat = directories.get(query);
  if (!sourceStat || !queryStat) invalid();
  const ownedDirectories = new Map<string, Stats>();
  const verifyDirectories = async (): Promise<void> => {
    for (const [path, before] of [...directories, ...ownedDirectories]) {
      check(); const after = await lstat(path); check();
      if (!after.isDirectory() || after.isSymbolicLink() || !sameIdentity(before, after)) invalid();
    }
  };
  const exactEntries = async (path: string, expected: readonly string[]): Promise<void> => {
    check(); const directory = await opendir(path);
    try {
      check(); const found = new Set<string>();
      for (;;) {
        check(); const entry = await directory.read(); check();
        if (!entry) break;
        if (!expected.includes(entry.name) || found.has(entry.name) || found.size >= expected.length) invalid();
        found.add(entry.name);
      }
      if (found.size !== expected.length) invalid();
    } finally { await directory.close(); }
    check();
  };
  const capture = async (path: string): Promise<CapturedFile> => {
    check(); const before = await lstat(path); check(); if (!regular(before)) invalid();
    const bytes = await readInput(path, maxFileBytes); check();
    const after = await lstat(path); check(); if (!regular(after) || !unchanged(before, after)) invalid();
    return { path, stat: before, bytes };
  };
  const manifest = await capture(join(cliRoot, "package.json"));
  let packageData: unknown;
  try { packageData = JSON.parse(manifest.bytes.toString("utf8")) as unknown; } catch { invalid(); }
  if (!packageData || typeof packageData !== "object" || Array.isArray(packageData)
    || !("name" in packageData) || packageData.name !== "@webos-tools/cli"
    || !("version" in packageData) || packageData.version !== "3.2.6") invalid();
  await exactEntries(source, rootNames); await exactEntries(query, queryNames);
  const captured: CapturedFile[] = []; let total = 0;
  for (const name of names) {
    const file = await capture(join(source, name)); total += file.bytes.length;
    if (total > maxConfigurationBytes) invalid(); captured.push(file);
  }
  const verifySource = async (): Promise<void> => {
    await verifyDirectories();
    await exactEntries(source, rootNames); await exactEntries(query, queryNames);
    for (const [path, before] of [[source, sourceStat], [query, queryStat]] as const) {
      check(); const after = await lstat(path); check(); if (!unchanged(before, after)) invalid();
    }
    for (const file of [manifest, ...captured]) {
      const current = await capture(file.path);
      if (!unchanged(file.stat, current.stat) || !file.bytes.equals(current.bytes)) invalid();
    }
  };
  await verifySource();
  check(); await mkdir(destination, { mode: 0o700 });
  ownedDirectories.set(destination, await lstat(destination)); check();
  await verifyDirectories(); await mkdir(join(destination, "query"), { mode: 0o700 });
  ownedDirectories.set(join(destination, "query"), await lstat(join(destination, "query"))); check();
  const ownedFiles: CapturedFile[] = [];
  for (let index = 0; index < names.length; index++) {
    const name = names[index], file = captured[index]; if (!name || !file) invalid();
    await verifyDirectories(); check();
    const path = join(destination, name); await writeOwned(path, file.bytes);
    const stat = await lstat(path); check();
    if (!regular(stat) || (stat.mode & 0o777) !== 0o600) invalid();
    ownedFiles.push({ path, stat, bytes: file.bytes });
  }
  await verifySource();
  await exactEntries(destination, rootNames); await exactEntries(join(destination, "query"), queryNames);
  for (const file of ownedFiles) {
    const current = await capture(file.path);
    if (!unchanged(file.stat, current.stat) || !file.bytes.equals(current.bytes)) invalid();
  }
  await verifyDirectories(); check();
  return Object.freeze(captured.map((file, index) => Object.freeze({ name: names[index] ?? invalid(), bytes: file.bytes.length, sha256: sha256(file.bytes) })));
}
