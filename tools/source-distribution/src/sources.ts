import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { gunzipSync } from "node:zlib";
import { TOOL_ENVIRONMENT } from "../../player-probe/src/normalize.js";
import { admitProject, safeName, unambiguousNames, type ProjectSnapshot, type SourceFile } from "./project.js";
import { lockedSources } from "./locks.js";
export type PublicRequest = {
  schemaVersion: 1;
  sourceCommit: string;
  runtime: { channelManifestSha256: string; sourceArchiveSha256: string; copyrightSha256: string };
  exclusions: { component: string; basis: "system-library" | "linked-file-permission"; reason: string }[];
};
export type RuntimeInputs = { channel: Buffer; archive: Buffer; copyright: Buffer };
export const sha256 = (bytes: Buffer): string => createHash("sha256").update(bytes).digest("hex");
function invalid(): never { throw new Error("invalidSourceInput"); }
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid();
  return value as Record<string, unknown>;
}
function exact(value: Record<string, unknown>, keys: string[]): void {
  if (Object.keys(value).sort().join("\0") !== keys.sort().join("\0")) invalid();
}
function hash(value: unknown): string { if (typeof value !== "string" || !/^[a-f0-9]{64}$/.test(value)) invalid(); return value; }
const shared = ["stock-sdl2", "stock-egl", "stock-gles2", "stock-lunaservice2", "stock-glib2", "stock-libc", "stock-libm", "stock-libpthread", "stock-librt", "stock-libdl", "stock-libgcc-s"];
const linked = ["glibc-startup-nonshared", "gcc-crtstuff"];
/** Structural declarations only: final artifact/legal eligibility is a release-owner decision. */
export function parseRequest(bytes: Buffer): PublicRequest {
  if (bytes.length < 1 || bytes.length > 16384 || !Buffer.from(bytes.toString("utf8")).equals(bytes)) invalid();
  let raw: unknown; try { raw = JSON.parse(bytes.toString("utf8")); } catch { return invalid(); }
  const root = object(raw); exact(root, ["schemaVersion", "sourceCommit", "runtime", "exclusions"]);
  if (root.schemaVersion !== 1 || typeof root.sourceCommit !== "string" || !/^[a-f0-9]{40}$/.test(root.sourceCommit)) invalid();
  const runtime = object(root.runtime); exact(runtime, ["channelManifestSha256", "sourceArchiveSha256", "copyrightSha256"]);
  if (!Array.isArray(root.exclusions) || root.exclusions.length !== shared.length + linked.length) invalid();
  const seen = new Set<string>();
  const exclusions = root.exclusions.map((entry: unknown) => {
    const item = object(entry); exact(item, ["component", "basis", "reason"]);
    if (typeof item.component !== "string" || seen.has(item.component) || typeof item.reason !== "string" || item.reason.length < 20 || item.reason.length > 512 || !/^[\x20-\x7e]+$/.test(item.reason)) invalid();
    const basis: "system-library" | "linked-file-permission" = shared.includes(item.component) ? "system-library" : linked.includes(item.component) ? "linked-file-permission" : invalid();
    if (item.basis !== basis) invalid(); seen.add(item.component);
    return { component: item.component, basis, reason: item.reason };
  }).sort((a, b) => Buffer.compare(Buffer.from(a.component), Buffer.from(b.component)));
  return { schemaVersion: 1, sourceCommit: root.sourceCommit, runtime: { channelManifestSha256: hash(runtime.channelManifestSha256), sourceArchiveSha256: hash(runtime.sourceArchiveSha256), copyrightSha256: hash(runtime.copyrightSha256) }, exclusions };
}
export type ArchiveBudget = Readonly<{ deadlineMs: number; now: () => number }>;
export type SourceArchiveOutcome = Readonly<{ stdout: Buffer; stderr: Buffer; status: number | null; signal: NodeJS.Signals | null; error?: Error }>;
/** Retains the actual bounded child result when a deadline or command refuses publication. */
export class SourceArchiveCommandError extends Error {
  constructor(message: "sourceDeadline" | "invalidSourceInput", readonly args: readonly string[], readonly outcome: SourceArchiveOutcome | null) {
    super(message); this.name = "SourceArchiveCommandError";
  }
}
export function sourceTar(args: string[], bytes: Buffer, max: number, budget?: ArchiveBudget): Buffer {
  const started = budget?.now();
  if (budget && (started === undefined || !Number.isFinite(started) || started < 0 || !Number.isFinite(budget.deadlineMs) || budget.deadlineMs <= started)) throw new SourceArchiveCommandError("sourceDeadline", [...args], null);
  const timeout = budget && started !== undefined ? Math.max(1, Math.min(60000, Math.floor(budget.deadlineMs - started))) : 60000;
  const result = spawnSync("/usr/bin/tar", args, { input: bytes, shell: false, env: TOOL_ENVIRONMENT, timeout, maxBuffer: max });
  if (budget && started !== undefined) {
    let finished: number; try { finished = budget.now(); } catch { throw new SourceArchiveCommandError("sourceDeadline", [...args], result); }
    if (!Number.isFinite(finished) || finished < started || finished >= budget.deadlineMs) throw new SourceArchiveCommandError("sourceDeadline", [...args], result);
    if (result.error || result.status !== 0 || result.stderr.length !== 0) throw new SourceArchiveCommandError("invalidSourceInput", [...args], result);
  }
  if (result.error || result.status !== 0 || result.stderr.length !== 0) invalid(); return result.stdout;
}
/** Inspects source archives without extracting files or executing their scripts. */
export function runtimeMembers(archive: Buffer, budget?: ArchiveBudget): Set<string> {
  if (archive.length < 18 || archive.length > 32 * 1024 * 1024) invalid();
  try { gunzipSync(archive, { maxOutputLength: 128 * 1024 * 1024 }); } catch { return invalid(); }
  const names = sourceTar(["--ignore-zeros", "-tzf", "-"], archive, 8 * 1024 * 1024, budget).toString("utf8").trimEnd().split("\n");
  const descriptions = sourceTar(["--ignore-zeros", "--numeric-owner", "-tvzf", "-"], archive, 8 * 1024 * 1024, budget).toString("utf8").trimEnd().split("\n");
  if (names.length < 1 || names.length > 16384 || descriptions.length !== names.length || new Set(names.map((name) => name.replace(/\/$/, ""))).size !== names.length || !unambiguousNames(names.map((name) => name.replace(/\/$/, "")))) invalid();
  const regular = new Set<string>();
  for (let index = 0; index < names.length; index += 1) {
    const name = names[index]; const description = descriptions[index];
    if (!name || !safeName(name.replace(/\/$/, "")) || !(name === "rust-src-nightly" || name.startsWith("rust-src-nightly/")) || !description || !/^(d|-)r[-rwx]{8} /.test(description)) invalid();
    if (description.startsWith("-")) regular.add(name);
  }
  return regular;
}
export function runtimeFile(archive: Buffer, names: Set<string>, name: string, limit: number, budget?: ArchiveBudget): Buffer {
  if (!names.has(name) || names.has(name + "/")) invalid();
  const bytes = sourceTar(["-xOzf", "-", "--", name], archive, limit, budget); if (bytes.length < 1) invalid(); return bytes;
}
export const runtimeSourcePrefix = "rust-src-nightly/rust-src/lib/rustlib/src/rust/";
const prefix = runtimeSourcePrefix;
const required = ["library/Cargo.toml", "library/std/src/lib.rs", "library/core/src/lib.rs", "library/compiler-builtins/Cargo.toml", "library/compiler-builtins/LICENSE.txt", "library/stdarch/LICENSE-MIT", "library/stdarch/LICENSE-APACHE", "library/portable-simd/LICENSE-MIT", "library/portable-simd/LICENSE-APACHE", "src/llvm-project/libunwind/LICENSE.TXT"];
function runtimeIdentity(project: ProjectSnapshot, inputs: RuntimeInputs, request: PublicRequest, budget?: ArchiveBudget): { lock: Buffer; version: string; commit: string; url: string } {
  if (inputs.channel.length > 2 * 1024 * 1024 || inputs.copyright.length < 1 || inputs.copyright.length > 8 * 1024 * 1024 || sha256(inputs.channel) !== request.runtime.channelManifestSha256 || sha256(inputs.archive) !== request.runtime.sourceArchiveSha256 || sha256(inputs.copyright) !== request.runtime.copyrightSha256) invalid();
  const toolchain = project.files.find((file) => file.name === "rust-toolchain.toml")?.bytes.toString("utf8");
  const date = /^channel = "nightly-(\d{4}-\d{2}-\d{2})"$/m.exec(toolchain ?? "")?.[1]; if (!date) invalid();
  const channel = inputs.channel.toString("utf8");
  if (!Buffer.from(channel).equals(inputs.channel) || !channel.startsWith('manifest-version = "2"\n') || !channel.includes(`\ndate = "${date}"\n`)) invalid();
  const sections = channel.split(/^\[/m);
  const sources = sections.filter((section) => section.startsWith("pkg.rust-src]\n"));
  const targets = sections.filter((section) => section.startsWith('pkg.rust-src.target."*"]\n'));
  if (sources.length !== 1 || targets.length !== 1) invalid();
  const source = sources[0]; const target = targets[0];
  const version = /^version = "([^"\n]+)"$/m.exec(source ?? "")?.[1];
  const url = `https://static.rust-lang.org/dist/${date}/rust-src-nightly.tar.gz`;
  if (!version || !target || !target.includes("\navailable = true\n") || !target.includes(`\nurl = "${url}"\n`) || !target.includes(`\nhash = "${request.runtime.sourceArchiveSha256}"\n`)) invalid();
  const names = runtimeMembers(inputs.archive, budget);
  for (const name of [...required.map((name) => prefix + name), "rust-src-nightly/LICENSE-MIT", "rust-src-nightly/LICENSE-APACHE", "rust-src-nightly/COPYRIGHT"]) if (!names.has(name)) invalid();
  const commit = runtimeFile(inputs.archive, names, "rust-src-nightly/git-commit-hash", 128, budget).toString().trim();
  if (!/^[a-f0-9]{40}$/.test(commit) || !version.includes(`(${commit.slice(0, 9)} `) || runtimeFile(inputs.archive, names, "rust-src-nightly/version", 256, budget).toString().trim() !== version) invalid();
  return { lock: runtimeFile(inputs.archive, names, prefix + "library/Cargo.lock", 2 * 1024 * 1024, budget), version, commit, url };
}
const sdk = {
  release: "webos-d7ed7ee.6",
  archiveUrl: "https://github.com/webosbrew/native-toolchain/releases/download/webos-d7ed7ee.6/arm-webos-linux-gnueabi_sdk-buildroot_linux-aarch64.tar.bz2",
  archiveSha256: "45a2d12ff557457d92cde4fddaa77a6f1090fca03adc43bb74397e5e0c379501",
  nativeToolchainCommit: "a2787d6c9607f70a5946454fcd06d99909a7a604",
  buildrootCommit: "79af0e655b4ae9252a93ec55a2cd786b335c4a6c",
  glibcCommit: "ddbe4400ebed0b7fb34e989a3ac7a73a71669bf9",
  gccCommit: "2ee5e4300186a92ad73f1a1a64cb918dc76c8d67",
  importedBuildConfig: "unavailable; public source pins do not establish bit-for-bit imported SDK reconstruction",
};
export type SourceManifest = {
  schemaVersion: 1; status: "development-candidate"; sourceCommit: string;
  projectScope: "complete-committed-public-git-tree";
  registryScope: "complete-project-and-runtime-lock-union-overincluded";
  runtime: { version: string; commit: string; sourceUrl: string; sourceSha256: string; channelSha256: string; copyrightSha256: string; lockSha256: string };
  sdk: typeof sdk; requestedBuild: { packages: string[]; features: string[]; target: string; profile: string; buildStd: string[]; buildStdFeatures: string[] };
  declaredExclusions: PublicRequest["exclusions"];
  files: { name: string; size: number; mode: number; sha256: string }[];
};
/** Lists every required raw archive before dependency preparation; never performs network work. */
export function sourceRequirements(project: ProjectSnapshot, requestBytes: Buffer, runtime: RuntimeInputs, budget?: ArchiveBudget) {
  admitProject(project); const request = parseRequest(requestBytes); if (request.sourceCommit !== project.revision) invalid();
  for (const name of ["Cargo.toml", "Cargo.lock", "LICENSE", "NOTICES.md", "Dockerfile", "rust-toolchain.toml", ".cargo/config.toml", "dev", "crates/criterion-app/Cargo.toml", "crates/criterion-app/src/main.rs", "tools/native-caller-probe/Cargo.toml", "tools/player-probe/broker/Cargo.toml"]) if (!project.files.some((file) => file.name === name && file.bytes.length > 0)) invalid();
  const dockerfile = project.files.find((file) => file.name === "Dockerfile")?.bytes.toString("utf8");
  if (!dockerfile?.includes(sdk.archiveUrl) || !dockerfile.includes(sdk.archiveSha256)) invalid();
  const identity = runtimeIdentity(project, runtime, request, budget);
  const projectLock = project.files.find((file) => file.name === "Cargo.lock")?.bytes; if (!projectLock) invalid();
  const registry = lockedSources(projectLock, identity.lock).map((source) => ({ ...source, file: `${source.name}-${source.version}.crate`, url: `https://static.crates.io/crates/${source.name}/${source.name}-${source.version}.crate` }));
  return { request, identity, registry };
}
/** Full public tree and conservative locked source closure; never claims artifact/legal admission. */
export async function assembleSources(project: ProjectSnapshot, requestBytes: Buffer, runtime: RuntimeInputs, readRegistry: (name: string) => Promise<Buffer>): Promise<{ files: SourceFile[]; manifest: SourceManifest }> {
  const { request, identity, registry } = sourceRequirements(project, requestBytes, runtime);
  const files: SourceFile[] = project.files.map((file) => ({ ...file, name: "project/" + file.name, bytes: Buffer.from(file.bytes) }));
  files.push({ name: "source-identity/project.commit", bytes: Buffer.from(project.commit), mode: 0o644 },
    { name: "runtime/rust-src-nightly.tar.gz", bytes: Buffer.from(runtime.archive), mode: 0o644 },
    { name: "runtime/channel.toml", bytes: Buffer.from(runtime.channel), mode: 0o644 },
    { name: "runtime/COPYRIGHT-library.html", bytes: Buffer.from(runtime.copyright), mode: 0o644 },
    { name: "runtime/Cargo.lock", bytes: identity.lock, mode: 0o644 });
  let total = files.reduce((sum, file) => sum + file.bytes.length, 0);
  for (const source of registry) {
    const name = source.file; const bytes = await readRegistry(name);
    total += bytes.length;
    if (bytes.length < 1 || bytes.length > 32 * 1024 * 1024 || total > 256 * 1024 * 1024 || sha256(bytes) !== source.sha256) invalid();
    files.push({ name: "registry/" + name, bytes: Buffer.from(bytes), mode: 0o644 });
  }
  files.sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name)));
  const manifest: SourceManifest = {
    schemaVersion: 1, status: "development-candidate", sourceCommit: project.revision, projectScope: "complete-committed-public-git-tree", registryScope: "complete-project-and-runtime-lock-union-overincluded",
    runtime: { version: identity.version, commit: identity.commit, sourceUrl: identity.url, sourceSha256: request.runtime.sourceArchiveSha256, channelSha256: request.runtime.channelManifestSha256, copyrightSha256: request.runtime.copyrightSha256, lockSha256: sha256(identity.lock) }, sdk,
    requestedBuild: { packages: ["criterion-app", "criterion-broker-probe", "criterion-native-caller-probe"], features: ["criterion-app/webos", "criterion-broker-probe/webos", "criterion-native-caller-probe/webos"], target: "arm-unknown-linux-gnueabi", profile: "release", buildStd: ["std", "panic_abort"], buildStdFeatures: ["compiler-builtins-mem"] }, declaredExclusions: request.exclusions,
    files: files.map((file) => ({ name: file.name, size: file.bytes.length, mode: file.mode, sha256: sha256(file.bytes) })),
  };
  files.push({ name: "SOURCE-MANIFEST.json", bytes: Buffer.from(JSON.stringify(manifest, null, 2) + "\n"), mode: 0o644 });
  return { files, manifest };
}
