import { lstat } from "node:fs/promises";
import { join, resolve } from "node:path";
import { admitProject, safeName, type ProjectSnapshot, type SourceFile } from "../source-distribution/src/project.js";
import { summarizeOfflineSources, verifyOfflineSources, type OfflineSourceSummary } from "../source-distribution/src/offline.js";
import { readInput } from "./src/input.js";
import { ordinaryRoot, prepareMainOffline, verifyMaterialized } from "./src/main-source.js";

export type OfflineMainFiles = Readonly<{
  sourceRoot: string; outputDirectory: string; registryArchivesRoot: string;
  installedRuntimeRoot: string; registryRoot: string;
}>;
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalidOfflineMainFiles"); return value as Record<string, unknown>;
}
/** Executed explicitly in the pinned GNU toolkit; reuses the source-distribution archive owner. */
export async function prepareOfflineMainFromFiles(input: OfflineMainFiles, budget: Readonly<{ deadlineMs: number; now: () => number }>): Promise<OfflineSourceSummary> {
  let previous = -1;
  const check = () => {
    const current = budget.now();
    if (!Number.isSafeInteger(current) || current < 0 || current < previous || !Number.isSafeInteger(budget.deadlineMs) || current >= budget.deadlineMs) throw new Error("producerDeadline"); previous = current;
  };
  check();
  for (const path of Object.values(input)) { if (resolve(path) !== path) throw new Error("invalidOfflineMainFiles"); await ordinaryRoot(path); check(); }
  const descriptor = record(JSON.parse((await readInput(join(input.outputDirectory, "project.json"), 2 * 1024 * 1024)).toString("utf8")) as unknown); check();
  if (Object.keys(descriptor).sort().join() !== ["revision", "commit", "files"].sort().join() || typeof descriptor.revision !== "string"
    || typeof descriptor.commit !== "string" || descriptor.commit.length > 32768 || !Array.isArray(descriptor.files) || descriptor.files.length < 1 || descriptor.files.length > 4096) throw new Error("invalidOfflineMainFiles");
  const commit = Buffer.from(descriptor.commit, "base64"); if (commit.toString("base64") !== descriptor.commit) throw new Error("invalidOfflineMainFiles");
  const files: SourceFile[] = [];
  for (const entry of descriptor.files) {
    check(); const file = record(entry);
    if (Object.keys(file).sort().join() !== ["name", "mode"].sort().join() || typeof file.name !== "string" || !safeName(file.name) || file.mode !== 0o644 && file.mode !== 0o755) throw new Error("invalidOfflineMainFiles");
    const path = join(input.sourceRoot, file.name); const stat = await lstat(path); check();
    if (!stat.isFile() || stat.isSymbolicLink() || stat.nlink !== 1 || (stat.mode & 0o777) !== file.mode) throw new Error("invalidOfflineMainFiles");
    const bytes = await readInput(path, 32 * 1024 * 1024, 0); check(); files.push({ name: file.name, mode: file.mode, bytes });
  }
  const project: ProjectSnapshot = { revision: descriptor.revision, commit, files }; admitProject(project); check();
  const raw = join(input.outputDirectory, "offline-input");
  const requestBytes = await readInput(join(raw, "request.json"), 16384); check();
  const runtimeInputs = { channel: await readInput(join(raw, "channel.toml"), 2 * 1024 * 1024), archive: await readInput(join(raw, "rust-src.tar.gz"), 32 * 1024 * 1024), copyright: await readInput(join(raw, "COPYRIGHT-library.html"), 8 * 1024 * 1024) }; check();
  const offline = await verifyOfflineSources({ project, requestBytes, runtimeInputs,
    installedRuntimeRoot: input.installedRuntimeRoot, registryRoot: input.registryRoot, budget: { deadlineMs: budget.deadlineMs, now: () => { check(); return previous; } },
    readRegistryArchive: async name => { check(); if (!safeName(name) || name.includes("/")) throw new Error("invalidOfflineMainFiles"); const bytes = await readInput(join(input.registryArchivesRoot, name), 32 * 1024 * 1024); check(); return bytes; },
  }); check();
  await verifyMaterialized(input.sourceRoot, project.files, check);
  await prepareMainOffline(input.outputDirectory, offline, check); check();
  return summarizeOfflineSources(offline);
}
