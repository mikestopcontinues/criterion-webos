import { spawnSync } from "node:child_process";
import { chmod, mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { archiveToolVersions, inspectIpk, normalizeIpk, TOOL_ENVIRONMENT, type PackageFile, type PackageFiles } from "../player-probe/src/normalize.js";
import { MAX_EXECUTABLE_BYTES, MAX_IPK_BYTES, admitExecutable } from "./src/admission.js";
import { readInput } from "./src/input.js";
import { sha256 } from "./src/receipt.js";
export const CALLER_ID = "com.mikestopcontinues.criterion.probe.native";
export const CALLER_MAIN = "criterion-native-caller-probe";
export const CALLER_VERSION = "0.1.0";
export const CALLER_APPINFO = {
  id: CALLER_ID, version: CALLER_VERSION, type: "native", main: CALLER_MAIN, title: "Criterion Native Caller Probe",
  appDescription: "Disposable native caller probe; TV admission is unverified.", icon: "icon.png", nativeLifeCycleInterfaceVersion: 2, handlesRelaunch: false,
} as const;
export const CALLER_IDENTITY = { id: CALLER_ID, version: CALLER_VERSION, architecture: "arm" } as const;
export const CALLER_PAYLOAD_NAMES = ["appinfo.json", CALLER_MAIN, "icon.png", "LICENSE", "NOTICES.md", "NOTICE-caller.md", "NOTICE-platform.md"] as const;
export function admitCallerManifest(bytes: Buffer): void {
  let value: unknown;
  try { value = JSON.parse(bytes.toString("utf8")); } catch { throw new Error("invalidCallerManifest"); }
  if (bytes.length > 4096 || !value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalidCallerManifest");
  const record = value as Record<string, unknown>;
  if (Object.keys(record).length !== Object.keys(CALLER_APPINFO).length
    || Object.entries(CALLER_APPINFO).some(([key, expected]) => record[key] !== expected)) throw new Error("invalidCallerManifest");
}
export function callerArchiveFiles(expected: PackageFiles): PackageFiles {
  if (expected.size !== CALLER_PAYLOAD_NAMES.length || CALLER_PAYLOAD_NAMES.some((name) => !expected.has(name))
    || [...expected].some(([name, file]) => file.mode !== (name === CALLER_MAIN ? 0o755 : 0o644))) throw new Error("invalidCallerPayload");
  const manifest = expected.get("appinfo.json"); const executable = expected.get(CALLER_MAIN);
  if (!manifest || !executable) throw new Error("invalidCallerPayload");
  admitCallerManifest(manifest.bytes); admitExecutable(executable.bytes);
  const files = new Map([...expected].map(([name, file]) => [`usr/palm/applications/${CALLER_ID}/${name}`, file]));
  files.set(`usr/palm/packages/${CALLER_ID}/packageinfo.json`, {
    bytes: Buffer.from(JSON.stringify({ id: CALLER_ID, version: CALLER_VERSION, app: CALLER_ID }, null, 2) + "\n"), mode: 0o644,
  });
  return files;
}
export function auditCallerIpk(bytes: Buffer, expected: PackageFiles): { controlSha256: string; files: Record<string, { sha256: string; bytes: number; mode: number }> } {
  const control = inspectIpk(bytes, callerArchiveFiles(expected), CALLER_IDENTITY, MAX_IPK_BYTES);
  return { controlSha256: sha256(control), files: Object.fromEntries([...expected].map(([name, file]) => [name, { sha256: sha256(file.bytes), bytes: file.bytes.length, mode: file.mode }])) };
}
export const REQUIRED_CALLER_SOURCES = [
  "Cargo.toml", "Dockerfile", "rust-toolchain.toml", ".cargo/config.toml", "tools/native-caller-probe/Cargo.toml",
  "tools/native-caller-probe/src/main.rs", "tools/native-caller-probe/src/lib.rs", "tools/native-caller-probe/src/native/mod.rs",
  "tools/native-caller-probe/src/native/bus.rs", "tools/native-caller-probe/src/native/ffi.rs", "tools/native-caller-probe/abi.c",
] as const;
export type CallerReceipt = {
  schemaVersion: 1; appId: typeof CALLER_ID; version: typeof CALLER_VERSION;
  target: "arm-unknown-linux-gnueabi"; profile: "release"; sourceCommit: string;
  cargoLockSha256: string; executableSha256: string; sourceSha256: Record<string, string>;
};
export async function admitCallerReceipt(bytes: Buffer, executable: Buffer, readSource: (path: string) => Promise<Buffer>): Promise<CallerReceipt> {
  function invalid(): never { throw new Error("invalidCallerReceipt"); }
  if (bytes.length < 2 || bytes.length > 256 * 1024) invalid();
  let value: unknown;
  try { value = JSON.parse(bytes.toString("utf8")); } catch { invalid(); }
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid();
  const record = value as Record<string, unknown>;
  const keys = ["schemaVersion", "appId", "version", "target", "profile", "sourceCommit", "cargoLockSha256", "executableSha256", "sourceSha256"];
  if (Object.keys(record).length !== keys.length || Object.keys(record).some((key) => !keys.includes(key))
    || record.schemaVersion !== 1 || record.appId !== CALLER_ID || record.version !== CALLER_VERSION || record.target !== "arm-unknown-linux-gnueabi" || record.profile !== "release"
    || typeof record.sourceCommit !== "string" || !/^[a-f0-9]{40}$/.test(record.sourceCommit)) invalid();
  for (const key of ["cargoLockSha256", "executableSha256"]) if (typeof record[key] !== "string" || !/^[a-f0-9]{64}$/.test(record[key])) invalid();
  const sources = record.sourceSha256;
  if (!sources || typeof sources !== "object" || Array.isArray(sources)) invalid();
  const hashes = sources as Record<string, unknown>; const paths = Object.keys(hashes);
  if (paths.length > 1024 || REQUIRED_CALLER_SOURCES.some((path) => !Object.hasOwnProperty.call(hashes, path))) invalid();
  for (const path of paths) {
    const fixed = [...REQUIRED_CALLER_SOURCES, "Cargo.lock"].includes(path);
    const source = (path.startsWith("crates/") || path.startsWith("tools/native-caller-probe/")) && /\.(rs|c|h|toml|glsl|vert|frag|wgsl)$/.test(path);
    if (path.length > 255 || (!fixed && !source) || !path.split("/").every((part) => /^[A-Za-z0-9_.-]+$/.test(part) && part !== "." && part !== "..")
      || typeof hashes[path] !== "string" || !/^[a-f0-9]{64}$/.test(hashes[path])) invalid();
  }
  const receipt = record as CallerReceipt;
  if (sha256(executable) !== receipt.executableSha256) throw new Error("callerExecutableMismatch");
  const frozen: [string, string][] = [["Cargo.lock", receipt.cargoLockSha256], ...Object.entries(receipt.sourceSha256)];
  for (const [path, expected] of frozen) {
    const current = await readSource(path);
    if (current.length > MAX_EXECUTABLE_BYTES || sha256(current) !== expected) throw new Error("callerSourceMismatch");
  }
  return receipt;
}

let phase = "inputs";
async function main(): Promise<void> {
  if (process.argv.length !== 2) throw new Error("unexpectedArgument");
  const root = resolve(process.cwd()); const output = join(root, ".local/native-caller-package");
  const input = join(output, "input"); const staging = join(output, "staging"); const raw = join(output, "raw-cli"); const packages = join(output, "ipks");
  const fileName = `${CALLER_ID}_${CALLER_VERSION}_arm.ipk`;
  const executable = await readInput(join(input, CALLER_MAIN), MAX_EXECUTABLE_BYTES); admitExecutable(executable);
  const receiptBytes = await readInput(join(input, "build-receipt.json"), 256 * 1024);
  const readSource = (path: string) => readInput(join(root, path), MAX_EXECUTABLE_BYTES);
  const receipt = await admitCallerReceipt(receiptBytes, executable, readSource);
  const cliPackage: unknown = JSON.parse((await readInput(join(root, "tools/player-probe/node_modules/@webos-tools/cli/package.json"), 16384)).toString("utf8"));
  if (!cliPackage || typeof cliPackage !== "object" || (cliPackage as Record<string, unknown>).version !== "3.2.6") throw new Error("invalidToolchain");
  const sources: Record<string, string> = {
    "appinfo.json": "tools/native-caller-probe/packaging/appinfo.json", "icon.png": "tools/native-caller-probe/packaging/icon.png",
    LICENSE: "LICENSE", "NOTICES.md": "NOTICES.md", "NOTICE-caller.md": "tools/native-caller-probe/NOTICE.md", "NOTICE-platform.md": "crates/criterion-platform/NOTICE.md",
  };
  const expected = new Map<string, PackageFile>([[CALLER_MAIN, { bytes: executable, mode: 0o755 }]]);
  for (const [name, path] of Object.entries(sources)) expected.set(name, { bytes: await readInput(join(root, path), 1024 * 1024), mode: 0o644 });
  const files = callerArchiveFiles(expected);
  phase = "staging";
  for (const directory of [staging, raw, packages]) { await rm(directory, { recursive: true, force: true }); await mkdir(directory, { recursive: true, mode: 0o755 }); }
  for (const [name, file] of expected) { await writeFile(join(staging, name), file.bytes); await chmod(join(staging, name), file.mode); }
  const home = join(output, "cli-home"); await mkdir(home, { recursive: true });
  const cli = join(root, "tools/player-probe/node_modules/@webos-tools/cli/bin");
  phase = "officialCli";
  const command = (script: string, args: string[]) => {
    const result = spawnSync(process.execPath, [join(cli, script), ...args], { shell: false, cwd: root, env: { ...TOOL_ENVIRONMENT, HOME: home }, encoding: "utf8", timeout: 60000, maxBuffer: 16384 });
    if (result.error || result.status !== 0) throw new Error("packageCommandFailed");
    return result.stdout + result.stderr;
  };
  await writeFile(join(output, "official-cli.log"), command("ares-config.js", ["--profile", "tv"]) + command("ares-package.js", [staging, "--no-minify", "--outdir", raw]));
  if (JSON.stringify((await readdir(raw)).sort()) !== JSON.stringify([fileName])) throw new Error("unexpectedPackage");
  const original = await readInput(join(raw, fileName), MAX_IPK_BYTES);
  phase = "normalization";
  const normalized = await normalizeIpk(original, files, CALLER_IDENTITY, join(output, "normalization"), MAX_IPK_BYTES);
  phase = "finalAudit";
  const audit = auditCallerIpk(normalized, expected);
  await admitCallerReceipt(receiptBytes, executable, readSource);
  await writeFile(join(packages, fileName), normalized, { mode: 0o644 });
  const tooling: Record<string, string> = {};
  for (const path of ["tools/package-native/caller-build.ts", "tools/package-native/tsconfig.json", "tools/package-native/src/admission.ts", "tools/package-native/src/input.ts", "tools/package-native/src/receipt.ts", "tools/player-probe/src/package.ts", "tools/player-probe/src/normalize.ts", "tools/player-probe/Dockerfile", "tools/player-probe/package-lock.json", ...Object.values(sources)]) tooling[path] = sha256(await readInput(join(root, path), 2 * 1024 * 1024));
  await writeFile(join(output, "package-seal.json"), JSON.stringify({
    schemaVersion: 1, appId: CALLER_ID, version: CALLER_VERSION, status: "disposable-admission-probe", build: receipt, receiptSha256: sha256(receiptBytes), tooling,
    tools: { node: process.version, cli: "3.2.6", ...archiveToolVersions() }, originalCli: { sha256: sha256(original), bytes: original.length },
    ipk: { file: fileName, sha256: sha256(normalized), bytes: normalized.length }, audit,
  }, null, 2) + "\n");
  process.stdout.write(`Sealed disposable native caller IPK: ${fileName}\n`);
}
if (require.main === module) void main().catch(() => { process.stderr.write(`callerPackageFailed:${phase}\n`); process.exitCode = 1; });
