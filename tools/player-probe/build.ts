import { build } from "esbuild";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { chmod, copyFile, lstat, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { admitBroker, ipkData } from "./src/package.js";
import { PLAYER_ID, SERVICE_ID, UI_ID, VERSION } from "./src/protocol.js";

const root = resolve(__dirname, "..");
const output = resolve(root, "../../.local/player-probe");
const staging = join(output, "staging");
const packages = join(output, "ipks");
const binary = join(output, "criterion-broker-probe-arm");
const sha = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");

async function regular(path: string): Promise<Buffer> {
  const stat = await lstat(path);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 2 * 1024 * 1024) throw new Error("invalidInput");
  return readFile(path);
}

async function audit(directory: string, expected: string[]): Promise<Record<string, { sha256: string; bytes: number; mode: number }>> {
  const files: Record<string, { sha256: string; bytes: number; mode: number }> = {};
  const visit = async (prefix: string) => {
    for (const name of await readdir(join(directory, prefix))) {
      const relative = prefix ? `${prefix}/${name}` : name;
      const path = join(directory, relative);
      const stat = await lstat(path);
      if (stat.isSymbolicLink()) throw new Error("invalidStaging");
      if (stat.isDirectory()) await visit(relative);
      else {
        const bytes = await regular(path);
        files[relative] = { sha256: sha(bytes), bytes: bytes.length, mode: stat.mode & 0o777 };
      }
    }
  };
  await visit("");
  if (JSON.stringify(Object.keys(files).sort()) !== JSON.stringify(expected.sort())) throw new Error("unexpectedPackageContent");
  return files;
}

function command(executable: string, args: string[]): string {
  const result = spawnSync(executable, args, { shell: false, cwd: root, encoding: "utf8", timeout: 60000, maxBuffer: 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error("packageCommandFailed");
  return result.stdout;
}

async function main(): Promise<void> {
  if (process.argv.length !== 2) throw new Error("unexpectedArgument");
  const executable = await regular(binary);
  admitBroker(executable);
  await rm(staging, { recursive: true, force: true });
  await rm(packages, { recursive: true, force: true });
  await mkdir(packages, { recursive: true });
  const seals: Record<string, unknown> = {};
  const expectedApp = ["appinfo.json", "index.html", "icon.png", "webOSTV.js", "LICENSE-webOSTV.txt", "app.js"];
  for (const role of ["ui", "player"] as const) {
    const directory = join(staging, role);
    await mkdir(directory, { recursive: true });
    const manifest = await regular(join(root, "packaging", role, "appinfo.json"));
    const app: unknown = JSON.parse(manifest.toString());
    if (!app || typeof app !== "object" || (app as Record<string, unknown>).id !== (role === "ui" ? UI_ID : PLAYER_ID) || (app as Record<string, unknown>).version !== VERSION) throw new Error("invalidManifest");
    await writeFile(join(directory, "appinfo.json"), manifest);
    for (const [source, destination] of [["packaging/index.html", "index.html"], ["packaging/icon.png", "icon.png"], ["vendor/webOSTV.js", "webOSTV.js"], ["vendor/LICENSE-2.0.txt", "LICENSE-webOSTV.txt"]]) {
      if (!source || !destination) throw new Error("invalidManifest");
      await copyFile(join(root, source), join(directory, destination));
    }
    await build({ entryPoints: [join(root, "src/wam.ts")], outfile: join(directory, "app.js"), bundle: true, platform: "browser", target: "es2018", format: "iife", define: { PROBE_ROLE: JSON.stringify(role) }, logLevel: "silent" });
    seals[role] = await audit(directory, [...expectedApp]);
  }
  const service = join(staging, "service");
  await mkdir(join(service, "bin"), { recursive: true });
  for (const name of ["package.json", "services.json"]) await copyFile(join(root, "packaging/service", name), join(service, name));
  const servicePackage: unknown = JSON.parse((await regular(join(service, "package.json"))).toString());
  const services: unknown = JSON.parse((await regular(join(service, "services.json"))).toString());
  const expectedService = { id: SERVICE_ID, services: [{ name: SERVICE_ID, commands: ["attach", "ping", "close"].map((name) => ({ name, public: true })) }] };
  if (JSON.stringify(servicePackage) !== JSON.stringify({ name: SERVICE_ID, version: VERSION, main: "bridge.js" }) || JSON.stringify(services) !== JSON.stringify(expectedService)) throw new Error("invalidManifest");
  await writeFile(join(service, "bin/criterion-broker-probe"), executable);
  await chmod(join(service, "bin/criterion-broker-probe"), 0o755);
  await build({ entryPoints: [join(root, "src/bridge.ts")], outfile: join(service, "bridge.js"), bundle: true, platform: "node", target: "node16.20.2", format: "cjs", external: ["webos-service"], logLevel: "silent" });
  seals.service = await audit(service, ["package.json", "services.json", "bridge.js", "bin/criterion-broker-probe"]);
  const cli = join(root, "node_modules/@webos-tools/cli/bin/ares-package.js");
  command(process.execPath, [join(root, "node_modules/@webos-tools/cli/bin/ares-config.js"), "--profile", "tv"]);
  command(process.execPath, [cli, join(staging, "ui"), "--no-minify", "--outdir", packages]);
  command(process.execPath, [cli, join(staging, "player"), service, "--no-minify", "--outdir", packages]);
  const ipks = await audit(packages, [`${UI_ID}_${VERSION}_all.ipk`, `${PLAYER_ID}_${VERSION}_all.ipk`]);
  // Compare every regular archived payload file against its sealed staging bytes.
  for (const role of ["ui", "player"] as const) {
    const id = role === "ui" ? UI_ID : PLAYER_ID;
    const data = ipkData(await regular(join(packages, `${id}_${VERSION}_all.ipk`)));
    const tar = (args: string[]) => {
      const result = spawnSync("/usr/bin/tar", args, { shell: false, input: data, timeout: 5000, maxBuffer: 2 * 1024 * 1024 });
      if (result.error || result.status !== 0) throw new Error("invalidIpkPayload");
      return result.stdout;
    };
    const archived = tar(["-tzf", "-"]).toString().trim().split("\n");
    const expected = new Map<string, Buffer>();
    for (const name of expectedApp) expected.set(`usr/palm/applications/${id}/${name}`, await regular(join(staging, role, name)));
    if (role === "player") for (const name of ["package.json", "services.json", "bridge.js", "bin/criterion-broker-probe"]) expected.set(`usr/palm/services/${SERVICE_ID}/${name}`, await regular(join(service, name)));
    const packageInfo = `usr/palm/packages/${id}/packageinfo.json`;
    const files = archived.filter((name) => !name.endsWith("/"));
    if (JSON.stringify([...files].sort()) !== JSON.stringify([...expected.keys(), packageInfo].sort())) throw new Error("unexpectedIpkContent");
    const types = tar(["-tvzf", "-"]).toString().trim().split("\n");
    if (types.some((line) => !line.startsWith("d") && !line.startsWith("-"))) throw new Error("invalidIpkFileType");
    for (const [name, bytes] of expected) {
      if (!tar(["-xzOf", "-", name]).equals(bytes)) throw new Error("ipkContentChanged");
    }
    const metadata: unknown = JSON.parse(tar(["-xzOf", "-", packageInfo]).toString());
    const expectedMetadata = { id, version: VERSION, app: id, ...(role === "player" ? { services: [SERVICE_ID] } : {}) };
    if (JSON.stringify(metadata) !== JSON.stringify(expectedMetadata)) throw new Error("invalidIpkMetadata");
    if (role === "player" && !types.some((line) => line.startsWith("-rwxr-xr-x") && line.endsWith(`usr/palm/services/${SERVICE_ID}/bin/criterion-broker-probe`))) throw new Error("invalidBrokerMode");
  }
  const inputs: Record<string, string> = {};
  for (const directory of ["src", "broker/src", "packaging/ui", "packaging/player", "packaging/service", "vendor"]) {
    for (const name of await readdir(join(root, directory))) inputs[`${directory}/${name}`] = sha(await regular(join(root, directory, name)));
  }
  for (const name of ["build.ts", "package-lock.json", "broker/Cargo.toml", "packaging/index.html", "packaging/icon.png"]) inputs[name] = sha(await regular(join(root, name)));
  await writeFile(join(output, "package-seal.json"), JSON.stringify({ version: VERSION, inputs, binary: sha(executable), staging: seals, ipks }, null, 2) + "\n");
  process.stdout.write("Two sealed disposable IPKs built; no install or launch performed.\n");
}
void main().catch(() => { process.stderr.write("probePackageFailed\n"); process.exitCode = 1; });
