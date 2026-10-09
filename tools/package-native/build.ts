import { spawnSync } from "node:child_process";
import { chmod, mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { archiveToolVersions, normalizeIpk, TOOL_ENVIRONMENT, type PackageFile } from "../player-probe/src/normalize.js";
import { APP_ID, MAX_EXECUTABLE_BYTES, MAX_IPK_BYTES, VERSION, admitExecutable } from "./src/admission.js";
import { archiveFiles, auditIpk, IDENTITY } from "./src/archive.js";
import { readInput } from "./src/input.js";
import { admitManifest, PAYLOAD_NAMES } from "./src/manifest.js";
import { admitBuildReceipt, sha256 } from "./src/receipt.js";

const root = resolve(process.cwd());
const output = join(root, ".local/native-package");
const input = join(output, "input");
const staging = join(output, "staging");
const raw = join(output, "raw-cli");
const packages = join(output, "ipks");
const fileName = `${APP_ID}_${VERSION}_arm.ipk`;
const sources: Record<string, string> = {
  "appinfo.json": "tools/package-native/packaging/appinfo.json", "icon.png": "tools/package-native/packaging/icon.png", LICENSE: "LICENSE", "NOTICES.md": "NOTICES.md",
  "NOTICE-platform.md": "crates/criterion-platform/NOTICE.md", "NOTICE-egui_glow.md": "crates/criterion-ui/vendor/egui_glow/NOTICE.md",
  "LICENSE-egui_glow-MIT": "crates/criterion-ui/vendor/egui_glow/LICENSE-MIT", "LICENSE-egui_glow-APACHE": "crates/criterion-ui/vendor/egui_glow/LICENSE-APACHE",
  "NOTICE-image-webp.md": "crates/criterion-artwork/vendor/image-webp/PROVENANCE.md", "LICENSE-image-webp-MIT": "crates/criterion-artwork/vendor/image-webp/LICENSE-MIT",
  "LICENSE-image-webp-APACHE": "crates/criterion-artwork/vendor/image-webp/LICENSE-APACHE",
};
let phase = "inputs";
async function main(): Promise<void> {
  if (process.argv.length !== 2) throw new Error("unexpectedArgument");
  const executable = await readInput(join(input, "criterion-unofficial"), MAX_EXECUTABLE_BYTES);
  admitExecutable(executable);
  const receiptBytes = await readInput(join(input, "build-receipt.json"), 256 * 1024);
  const readSource = (path: string) => readInput(join(root, path), MAX_EXECUTABLE_BYTES);
  const receipt = await admitBuildReceipt(receiptBytes, executable, readSource);
  const cliPackage: unknown = JSON.parse((await readInput(join(root, "tools/player-probe/node_modules/@webos-tools/cli/package.json"), 16384)).toString("utf8"));
  if (!cliPackage || typeof cliPackage !== "object" || (cliPackage as Record<string, unknown>).version !== "3.2.6") throw new Error("invalidToolchain");
  const expected = new Map<string, PackageFile>([["criterion-unofficial", { bytes: executable, mode: 0o755 }]]);
  for (const [name, path] of Object.entries(sources)) expected.set(name, { bytes: await readInput(join(root, path), 1024 * 1024), mode: 0o644 });
  const manifest = expected.get("appinfo.json"); if (!manifest) throw new Error("invalidManifest");
  admitManifest(manifest.bytes);
  if (expected.size !== PAYLOAD_NAMES.length) throw new Error("invalidStaging");
  phase = "staging";
  for (const directory of [staging, raw, packages]) {
    await rm(directory, { recursive: true, force: true });
    await mkdir(directory, { recursive: true, mode: 0o755 });
  }
  for (const [name, file] of expected) { await writeFile(join(staging, name), file.bytes); await chmod(join(staging, name), file.mode); }
  const home = join(output, "cli-home"); await mkdir(home, { recursive: true });
  const cli = join(root, "tools/player-probe/node_modules/@webos-tools/cli/bin");
  phase = "officialCli";
  const command = (script: string, args: string[]) => {
    const result = spawnSync(process.execPath, [join(cli, script), ...args], {
      shell: false, cwd: root, env: { ...TOOL_ENVIRONMENT, HOME: home }, encoding: "utf8", timeout: 60000, maxBuffer: 16384,
    });
    if (result.error || result.status !== 0) throw new Error("packageCommandFailed");
    return result.stdout + result.stderr;
  };
  const cliLog = command("ares-config.js", ["--profile", "tv"]) + command("ares-package.js", [staging, "--no-minify", "--outdir", raw]);
  await writeFile(join(output, "official-cli.log"), cliLog);
  if (JSON.stringify((await readdir(raw)).sort()) !== JSON.stringify([fileName])) throw new Error("unexpectedPackage");
  const original = await readInput(join(raw, fileName), MAX_IPK_BYTES);
  phase = "normalization";
  const normalized = await normalizeIpk(original, archiveFiles(expected), IDENTITY, join(output, "normalization"), MAX_IPK_BYTES);
  phase = "finalAudit";
  const audit = auditIpk(normalized, expected);
  await admitBuildReceipt(receiptBytes, executable, readSource);
  await writeFile(join(packages, fileName), normalized, { mode: 0o644 });
  const tooling: Record<string, string> = {};
  for (const path of ["tools/package-native/build.ts", "tools/package-native/tsconfig.json", "tools/player-probe/Dockerfile", "tools/player-probe/package-lock.json", "tools/player-probe/src/package.ts", "tools/player-probe/src/normalize.ts", ...Object.values(sources)]) tooling[path] = sha256(await readInput(join(root, path), 2 * 1024 * 1024));
  for (const name of await readdir(join(root, "tools/package-native/src"))) if (name.endsWith(".ts")) {
    const path = `tools/package-native/src/${name}`; tooling[path] = sha256(await readInput(join(root, path), 2 * 1024 * 1024));
  }
  await writeFile(join(output, "package-seal.json"), JSON.stringify({
    schemaVersion: 1, appId: APP_ID, version: VERSION, status: "development", build: receipt, receiptSha256: sha256(receiptBytes), tooling,
    tools: { node: process.version, cli: "3.2.6", ...archiveToolVersions() }, originalCli: { sha256: sha256(original), bytes: original.length },
    ipk: { file: fileName, sha256: sha256(normalized), bytes: normalized.length }, audit,
  }, null, 2) + "\n");
  process.stdout.write(`Sealed development native IPK: ${fileName}\n`);
}
void main().catch(() => { process.stderr.write(`nativePackageFailed:${phase}\n`); process.exitCode = 1; });
