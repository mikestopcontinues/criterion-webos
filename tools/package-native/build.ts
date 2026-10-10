import { chmod, mkdir, open, readdir, realpath } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { archiveToolVersions, normalizeIpk, TOOL_ENVIRONMENT, type PackageFile } from "../player-probe/src/normalize.js";
import { APP_ID, MAX_EXECUTABLE_BYTES, MAX_IPK_BYTES, VERSION, admitExecutable } from "./src/admission.js";
import { archiveFiles, auditIpk, IDENTITY } from "./src/archive.js";
import { PACKAGING_IMAGE, PackagingCommandError, type ClosedCommand, type NativeMainExport, type NativeMainInput, type PackageCommand, type PackagingExecution } from "./src/contract.js";
import { readInput } from "./src/input.js";
import { admitManifest, PAYLOAD_NAMES } from "./src/manifest.js";
import { admitBuildReceipt, sha256 } from "./src/receipt.js";

const fileName = `${APP_ID}_${VERSION}_arm.ipk`;
const sources: Readonly<Record<string, string>> = {
  "appinfo.json": "tools/package-native/packaging/appinfo.json", "icon.png": "tools/package-native/packaging/icon.png", LICENSE: "LICENSE", "NOTICES.md": "NOTICES.md",
  "NOTICE-platform.md": "crates/criterion-platform/NOTICE.md", "NOTICE-egui_glow.md": "crates/criterion-ui/vendor/egui_glow/NOTICE.md",
  "LICENSE-egui_glow-MIT": "crates/criterion-ui/vendor/egui_glow/LICENSE-MIT", "LICENSE-egui_glow-APACHE": "crates/criterion-ui/vendor/egui_glow/LICENSE-APACHE",
  "NOTICE-image-webp.md": "crates/criterion-artwork/vendor/image-webp/PROVENANCE.md", "LICENSE-image-webp-MIT": "crates/criterion-artwork/vendor/image-webp/LICENSE-MIT",
  "LICENSE-image-webp-APACHE": "crates/criterion-artwork/vendor/image-webp/LICENSE-APACHE",
};
function freeze<T>(value: T): T {
  if (value && typeof value === "object") { for (const child of Object.values(value)) freeze(child); Object.freeze(value); }
  return value;
}
function neededSonames(bytes: Buffer): string[] {
  if (bytes.length < 1 || bytes.length > 65536 || bytes.some((b) => b !== 9 && b !== 10 && b !== 13 && (b < 32 || b > 126))) throw new Error("invalidDynamicMetadata");
  const lines = bytes.toString("ascii").split("\n");
  if (!lines.some((line) => /^Dynamic section at offset 0x[0-9a-f]+ contains [1-9][0-9]* entries:$/.test(line))) throw new Error("invalidDynamicMetadata");
  const names: string[] = [];
  for (const line of lines) {
    if (!line.includes("NEEDED")) continue;
    const name = /^\s*0x0*1\s+\(NEEDED\)\s+Shared library: \[([^\]]+)\]\s*$/.exec(line)?.[1];
    if (!name || name.length > 128 || !/^[A-Za-z0-9_+.-]+\.so(?:\.[A-Za-z0-9_+.-]+)*$/.test(name) || names.includes(name) || names.length === 10) throw new Error("invalidDynamicMetadata");
    names.push(name);
  }
  if (names.length === 0) throw new Error("invalidDynamicMetadata");
  return names;
}
function closedResult(value: unknown, limit: number): ClosedCommand {
  if (!value || typeof value !== "object") throw new Error("unresolvedCommand");
  const r = value as Record<string, unknown>;
  if (r.closed !== true) throw new Error("unresolvedCommand");
  if (Object.keys(r).sort().join() !== ["closed", "exitCode", "signal", "stderr", "stdout", "timedOut"].sort().join()
    || (r.exitCode !== null && (!Number.isSafeInteger(r.exitCode) || (r.exitCode as number) < 0 || (r.exitCode as number) > 255))
    || (r.signal !== null && (typeof r.signal !== "string" || !/^SIG[A-Z0-9]{1,16}$/.test(r.signal)))
    || typeof r.timedOut !== "boolean" || !Buffer.isBuffer(r.stdout) || !Buffer.isBuffer(r.stderr)
    || r.stdout.length + r.stderr.length > limit) throw new Error("invalidCommandResult");
  return { closed: true, exitCode: r.exitCode as number | null, signal: r.signal as string | null, timedOut: r.timedOut,
    stdout: Buffer.from(r.stdout), stderr: Buffer.from(r.stderr) };
}

/** Inert MAIN packaging; PackagingExecution requires a caller-bounded, joined outer lifetime. */
export async function packageNativeMain(input: NativeMainInput, execution: PackagingExecution): Promise<NativeMainExport> {
  const root = input.sourceRoot; const output = input.outputDirectory;
  const now = execution.now; const execute = execution.execute; const deadline = execution.deadlineMs;
  let previous = -Infinity;
  const guard = () => {
    const current = now();
    if (!Number.isSafeInteger(current) || current < 0 || current < previous || !Number.isSafeInteger(deadline) || current >= deadline) throw new Error("packagingDeadline");
    previous = current; return deadline - current;
  };
  const step = async <T>(operation: () => Promise<T>): Promise<T> => { guard(); const result = await operation(); guard(); return result; };
  guard();
  if (execution.image !== PACKAGING_IMAGE || process.version !== "v24.20.0" || resolve(root) !== root
    || !/^\/workspace\/\.local\/native-package\/exports\/[a-z]{1,64}$/.test(output)
    || !Buffer.isBuffer(input.executable) || input.executable.length > MAX_EXECUTABLE_BYTES
    || !Buffer.isBuffer(input.buildReceipt) || input.buildReceipt.length > 256 * 1024) throw new Error("invalidPackagingInput");
  const executable = Buffer.from(input.executable); const receiptBytes = Buffer.from(input.buildReceipt);
  if (await step(() => realpath(root)) !== root) throw new Error("invalidSourceRoot");
  admitExecutable(executable);
  const readSource = (path: string) => step(() => readInput(join(root, path), MAX_EXECUTABLE_BYTES, 0));
  const receipt = await step(() => admitBuildReceipt(receiptBytes, executable, readSource));
  const cliRoot = join(root, "tools/player-probe/node_modules/@webos-tools/cli");
  const cliPackage: unknown = JSON.parse((await step(() => readInput(join(cliRoot, "package.json"), 16384))).toString("utf8"));
  if (!cliPackage || typeof cliPackage !== "object" || (cliPackage as Record<string, unknown>).version !== "3.2.6") throw new Error("invalidToolchain");
  const expected = new Map<string, PackageFile>([["criterion-unofficial", { bytes: executable, mode: 0o755 }]]);
  for (const [name, path] of Object.entries(sources)) expected.set(name, { bytes: await step(() => readInput(join(root, path), 1024 * 1024)), mode: 0o644 });
  const manifest = expected.get("appinfo.json"); if (!manifest) throw new Error("invalidManifest");
  admitManifest(manifest.bytes);
  if (expected.size !== PAYLOAD_NAMES.length) throw new Error("invalidStaging");
  const toolingPaths = ["tools/package-native/build.ts", "tools/package-native/index.ts", "tools/package-native/tsconfig.json", "tools/player-probe/Dockerfile", "tools/player-probe/package-lock.json", "tools/player-probe/src/package.ts", "tools/player-probe/src/normalize.ts", ...Object.values(sources)];
  for (const name of await step(() => readdir(join(root, "tools/package-native/src")))) if (name.endsWith(".ts")) toolingPaths.push(`tools/package-native/src/${name}`);
  const tooling: Record<string, string> = {};
  for (const path of toolingPaths.sort()) tooling[path] = sha256(await step(() => readInput(join(root, path), 2 * 1024 * 1024)));
  const readelf = await step(() => realpath("/usr/bin/readelf"));
  if (!/^\/usr\/bin\/[a-z0-9-]*readelf$/.test(readelf)) throw new Error("invalidToolchain");
  const toolPaths = [readelf, "/usr/local/bin/node", join(cliRoot, "package.json"), join(cliRoot, "bin/ares-config.js"), join(cliRoot, "bin/ares-package.js")];
  const toolHashes: Record<string, string> = {};
  for (const path of toolPaths) toolHashes[path] = sha256(await step(() => readInput(path, 128 * 1024 * 1024)));
  if (await step(() => realpath("/workspace/.local/native-package")) !== "/workspace/.local/native-package") throw new Error("invalidOutputDirectory");
  await step(() => mkdir(dirname(output), { recursive: true, mode: 0o700 }));
  if (await step(() => realpath(dirname(output))) !== dirname(output)) throw new Error("invalidOutputDirectory");
  await step(() => mkdir(output, { mode: 0o700 })); // Exclusive ownership; existing outputs are never reset.
  const writeNew = async (path: string, bytes: Buffer, mode: number) => step(async () => {
    const file = await open(path, "wx", mode);
    try { await file.writeFile(bytes); await file.sync(); } finally { await file.close(); }
  });
  const staging = join(output, "staging"); const raw = join(output, "raw-cli"); const packages = join(output, "ipks"); const home = join(output, "cli-home");
  for (const path of [staging, raw, packages, home]) await step(() => mkdir(path, { mode: 0o700 }));
  for (const [name, file] of expected) { await writeNew(join(staging, name), file.bytes, file.mode); await step(() => chmod(join(staging, name), file.mode)); }
  const stagedExecutable = join(staging, "criterion-unofficial");
  const command = async (name: string, binary: string, args: readonly string[], limit: number) => {
    const request: PackageCommand = freeze({ executable: binary, args: [...args], cwd: root, env: { ...TOOL_ENVIRONMENT, HOME: home }, deadlineMs: deadline, timeoutMs: Math.min(60000, guard()), maxOutputBytes: limit });
    let rawResult: unknown;
    try { rawResult = await execute(request); } catch { throw new PackagingCommandError("unresolvedCommand", request, null); }
    let result: ClosedCommand;
    try { result = closedResult(rawResult, limit); } catch (error) {
      throw new PackagingCommandError(error instanceof Error && error.message === "invalidCommandResult" ? "invalidCommandResult" : "unresolvedCommand", request, null);
    }
    try {
      guard();
      await writeNew(join(output, `${name}.stdout`), result.stdout, 0o600);
      await writeNew(join(output, `${name}.stderr`), result.stderr, 0o600);
    } catch (error) {
      if (error instanceof Error && error.message === "packagingDeadline") throw new PackagingCommandError("packagingDeadline", request, result);
      throw error;
    }
    if (result.exitCode !== 0 || result.signal !== null || result.timedOut) throw new PackagingCommandError("packageCommandFailed", request, result);
    return result;
  };
  const checkExecutable = async () => {
    if (sha256(await step(() => readInput(stagedExecutable, MAX_EXECUTABLE_BYTES))) !== receipt.executableSha256) throw new Error("executableMismatch");
  };
  await checkExecutable();
  const dynamic = await command("readelf-dynamic", readelf, ["-d", stagedExecutable], 65536);
  if (dynamic.stderr.length !== 0) throw new Error("invalidDynamicMetadata");
  const needed = neededSonames(dynamic.stdout); await checkExecutable();
  await command("official-config", "/usr/local/bin/node", [join(cliRoot, "bin/ares-config.js"), "--profile", "tv"], 16384);
  await command("official-package", "/usr/local/bin/node", [join(cliRoot, "bin/ares-package.js"), staging, "--no-minify", "--outdir", raw], 16384);
  if (JSON.stringify((await step(() => readdir(raw))).sort()) !== JSON.stringify([fileName])) throw new Error("unexpectedPackage");
  const original = await step(() => readInput(join(raw, fileName), MAX_IPK_BYTES));
  const normalization = join("/workspace/.local/native-package/normalization", output.slice(output.lastIndexOf("/") + 1));
  await step(() => mkdir(dirname(normalization), { recursive: true, mode: 0o700 }));
  if (await step(() => realpath(dirname(normalization))) !== dirname(normalization)) throw new Error("invalidOutputDirectory");
  await step(() => mkdir(normalization, { mode: 0o700 })); // Unchanged normalizer resets ONLY this newly earned empty directory.
  const normalized = await step(() => normalizeIpk(original, archiveFiles(expected), IDENTITY, normalization, MAX_IPK_BYTES));
  guard(); const audit = auditIpk(normalized, expected); guard();
  await step(() => admitBuildReceipt(receiptBytes, executable, readSource));
  for (const [path, hash] of Object.entries(tooling)) if (sha256(await step(() => readInput(join(root, path), 2 * 1024 * 1024))) !== hash) throw new Error("toolingChanged");
  for (const [path, hash] of Object.entries(toolHashes)) if (sha256(await step(() => readInput(path, 128 * 1024 * 1024))) !== hash) throw new Error("toolingChanged");
  if (await step(() => realpath("/usr/bin/readelf")) !== readelf) throw new Error("toolingChanged");
  const ipkPath = join(packages, fileName); await writeNew(ipkPath, normalized, 0o644);
  const published = await step(() => readInput(ipkPath, MAX_IPK_BYTES));
  if (!published.equals(normalized)) throw new Error("packageChanged");
  guard(); auditIpk(published, expected); guard();
  const requirements: NativeMainExport["executableRequirements"] = freeze({ appId: APP_ID, executableSha256: receipt.executableSha256, neededSonames: needed });
  const ipk = freeze({ path: ipkPath, sha256: sha256(published), bytes: published.length });
  const sealBytes = Buffer.from(JSON.stringify({
    schemaVersion: 1, appId: APP_ID, version: VERSION, status: "development", identityScope: "build-receipt-content-only", build: receipt, receiptSha256: sha256(receiptBytes), tooling, toolHashes,
    tools: { node: process.version, cli: "3.2.6", ...archiveToolVersions() }, requiredPackagingImage: PACKAGING_IMAGE,
    originalCli: { sha256: sha256(original), bytes: original.length }, ipk,
    dynamicMetadata: { command: [readelf, "-d", stagedExecutable], stdoutSha256: sha256(dynamic.stdout), bytes: dynamic.stdout.length, executableSha256: receipt.executableSha256 }, executableRequirements: requirements, audit,
  }, null, 2) + "\n");
  const sealPath = join(output, "package-seal.json");
  await writeNew(sealPath, sealBytes, 0o644); // Seal last, after the exact final package audit.
  return freeze({ schemaVersion: 1, status: "development", identityScope: "build-receipt-content-only", build: receipt, receiptSha256: sha256(receiptBytes),
    ipk, packageSeal: { path: sealPath, sha256: sha256(sealBytes), bytes: sealBytes.length }, executableRequirements: requirements, audit });
}
