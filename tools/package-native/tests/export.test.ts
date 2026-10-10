import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { randomBytes } from "node:crypto";
import { copyFile, cp, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { packageNativeMain, PACKAGING_IMAGE, PackagingCommandError, type ClosedCommand, type CommandResult, type NativeMainInput, type PackageCommand } from "../index.js";
import { APP_ID, VERSION } from "../src/admission.js";
import { PAYLOAD_NAMES } from "../src/manifest.js";
import { REQUIRED_SOURCES, sha256, type BuildReceipt } from "../src/receipt.js";
import { metadataFixture } from "./admission.test.js";
import { ipkFixture, tarFixture } from "../../player-probe/tests/fixture-tar.js";

const complete = (stdout = Buffer.alloc(0)): ClosedCommand => ({ closed: true, exitCode: 0, signal: null, timedOut: false, stdout, stderr: Buffer.alloc(0) });
const dynamicFixture = Buffer.from("\nDynamic section at offset 0x100 contains 3 entries:\n  Tag        Type                         Name/Value\n 0x00000001 (NEEDED)                     Shared library: [libm.so.6]\n 0x00000001 (NEEDED)                     Shared library: [libc.so.6]\n 0x00000000 (NULL)                       0x0\n");
async function inputFixture(): Promise<NativeMainInput> {
  const executable = metadataFixture();
  const receipt: BuildReceipt = { schemaVersion: 1, appId: APP_ID, version: VERSION, target: "arm-unknown-linux-gnueabi", profile: "release",
    sourceCommit: "a".repeat(40), cargoLockSha256: sha256(await readFile("/workspace/Cargo.lock")), executableSha256: sha256(executable), sourceSha256: {} };
  for (const path of REQUIRED_SOURCES) receipt.sourceSha256[path] = sha256(await readFile(join("/workspace", path)));
  const suffix = [...randomBytes(12)].map((byte) => String.fromCharCode(97 + byte % 26)).join("");
  return { sourceRoot: "/workspace", outputDirectory: `/workspace/.local/native-package/exports/fixture${suffix}`, executable, buildReceipt: Buffer.from(JSON.stringify(receipt)) };
}
async function cliArchive(command: PackageCommand, extra = false): Promise<void> {
  const staging = command.args[1]; const raw = command.args[4];
  if (!staging || !raw || !command.args[0]?.endsWith("/ares-package.js")) throw new Error("invalidFixtureCommand");
  const entries = [];
  for (const name of PAYLOAD_NAMES) entries.push({ name: `usr/palm/applications/${APP_ID}/${name}`, bytes: await readFile(join(staging, name)), mode: name === "criterion-unofficial" ? 0o755 : 0o644 });
  entries.push({ name: `usr/palm/packages/${APP_ID}/packageinfo.json`, bytes: Buffer.from(JSON.stringify({ id: APP_ID, version: VERSION, app: APP_ID }, null, 2) + "\n"), mode: 0o644 });
  if (extra) entries.push({ name: `usr/palm/applications/${APP_ID}/unowned.js`, bytes: Buffer.from("unowned payload\n"), mode: 0o644 });
  const control = Buffer.from(`Package: ${APP_ID}\nVersion: ${VERSION}\nSection: misc\nPriority: optional\nArchitecture: arm\nInstalled-Size: 8192\nMaintainer: N/A <nobody@example.com>\nDescription: This is a webOS application.\nwebOS-Package-Format-Version: 2\nwebOS-Packager-Version: x.y.x\n`);
  await writeFile(join(raw, `${APP_ID}_${VERSION}_arm.ipk`), ipkFixture(tarFixture(entries), tarFixture([{ name: "control", bytes: control, mode: 0o644 }])), { flag: "wx" });
}
async function sourceFixture(input: NativeMainInput): Promise<NativeMainInput> {
  const root = join(input.outputDirectory, "../source" + input.outputDirectory.slice(input.outputDirectory.lastIndexOf("/") + 1));
  await mkdir(root, { mode: 0o700 });
  await cp("/workspace/tools/package-native", join(root, "tools/package-native"), { recursive: true, force: false, errorOnExist: true });
  for (const path of [...REQUIRED_SOURCES, "Cargo.lock", "LICENSE", "NOTICES.md", "tools/player-probe/Dockerfile", "tools/player-probe/package-lock.json", "tools/player-probe/src/package.ts", "tools/player-probe/src/normalize.ts",
    "tools/player-probe/node_modules/@webos-tools/cli/package.json", "tools/player-probe/node_modules/@webos-tools/cli/bin/ares-config.js", "tools/player-probe/node_modules/@webos-tools/cli/bin/ares-package.js",
    "crates/criterion-platform/NOTICE.md", "crates/criterion-ui/vendor/egui_glow/NOTICE.md", "crates/criterion-ui/vendor/egui_glow/LICENSE-MIT", "crates/criterion-ui/vendor/egui_glow/LICENSE-APACHE",
    "crates/criterion-artwork/vendor/image-webp/PROVENANCE.md", "crates/criterion-artwork/vendor/image-webp/LICENSE-MIT", "crates/criterion-artwork/vendor/image-webp/LICENSE-APACHE"]) {
    await mkdir(dirname(join(root, path)), { recursive: true });
    await copyFile(join("/workspace", path), join(root, path));
  }
  return { ...input, sourceRoot: root };
}

test("importing the public MAIN package module performs no work", () => {
  const entry = join(__dirname, "../index.js");
  const result = spawnSync(process.execPath, ["-e", `require(${JSON.stringify(entry)})`], {
    shell: false, env: { PATH: "/usr/bin:/bin" }, timeout: 3000, maxBuffer: 4096,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0);
  assert.equal(result.signal, null);
  assert.equal(result.stdout.length, 0);
  assert.equal(result.stderr.length, 0);
});

test("late acknowledged closure stops before another command and keeps its bounded private evidence", async () => {
  const input = await inputFixture(); let current = 100; let issued = 0;
  await assert.rejects(packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => current,
    execute: async () => { issued += 1; current = 10000; return complete(dynamicFixture); } }), (error: unknown) => {
      assert.ok(error instanceof PackagingCommandError);
      assert.equal(error.message, "packagingDeadline");
      assert.equal(error.outcome?.closed, true);
      assert.ok(error.outcome?.stdout.equals(dynamicFixture));
      assert.equal(error.command.deadlineMs, 10000);
      return true;
    });
  assert.equal(issued, 1);
  assert.ok(!(await readdir(input.outputDirectory)).includes("package-seal.json"));
});

for (const [label, expiresOn] of [["first", 2], ["second", 4]] as const) {
  test(`deadline before ${label} log custody retains the acknowledged command outcome`, async () => {
    const input = await inputFixture(); let returned = false; let afterReturnReads = 0; let issued = 0;
    await assert.rejects(packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000,
      now: () => returned && ++afterReturnReads >= expiresOn ? 10000 : 100,
      execute: async () => { issued += 1; returned = true; return complete(dynamicFixture); } }), (error: unknown) => {
        assert.ok(error instanceof PackagingCommandError);
        assert.equal(error.message, "packagingDeadline");
        assert.equal(error.outcome?.closed, true);
        assert.ok(error.outcome?.stdout.equals(dynamicFixture));
        assert.equal(error.outcome?.stderr.length, 0);
        assert.equal(error.command.deadlineMs, 10000);
        return true;
      });
    assert.equal(issued, 1);
    const names = await readdir(input.outputDirectory);
    assert.equal(names.includes("readelf-dynamic.stdout"), label === "second");
    assert.ok(!names.includes("readelf-dynamic.stderr") && !names.includes("package-seal.json"));
  });
}

for (const [name, outcome] of [
  ["nonzero", { ...complete(Buffer.from("bounded failure")), exitCode: 1 }],
  ["signal", { ...complete(), exitCode: null, signal: "SIGTERM" }],
  ["closed timeout", { ...complete(), exitCode: null, signal: "SIGKILL", timedOut: true }],
  ["unresolved", { closed: false }],
] satisfies [string, CommandResult][]) {
  test(`a ${name} command cannot run the CLI or publish a seal`, async () => {
    const input = await inputFixture(); let issued = 0;
    await assert.rejects(packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => 100,
      execute: async () => { issued += 1; return outcome; } }), (error: unknown) => {
        assert.ok(error instanceof PackagingCommandError);
        assert.equal(error.outcome?.closed ?? false, outcome.closed);
        return true;
      });
    assert.equal(issued, 1);
    assert.ok(!(await readdir(input.outputDirectory)).includes("package-seal.json"));
  });
}

test("the inert API returns the exact audited MAIN package and seal through a settled fixture executor", async () => {
  const input = await inputFixture(); const commands: PackageCommand[] = []; const expectedExecutable = sha256(input.executable); let current = 100;
  const result = await packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => current,
    execute: async (command) => {
      commands.push(command);
      if (command.args[0] === "-d") { input.executable.fill(0); input.buildReceipt.fill(0); current = 8000; return complete(dynamicFixture); } // Metadata fixture; no real ELF/readelf acceptance.
      current = 9000;
      if (command.args[0]?.endsWith("/ares-package.js")) await cliArchive(command);
      return complete();
    } });
  assert.equal(commands.length, 3);
  assert.deepEqual(commands.map((c) => c.deadlineMs), [10000, 10000, 10000]);
  assert.deepEqual(commands.map((c) => c.timeoutMs), [9900, 2000, 1000]);
  assert.deepEqual(commands[0]?.args, ["-d", join(input.outputDirectory, "staging/criterion-unofficial")]);
  assert.equal(result.status, "development");
  assert.equal(result.identityScope, "build-receipt-content-only");
  assert.equal(result.ipk.sha256, sha256(await readFile(result.ipk.path)));
  assert.equal(result.packageSeal.sha256, sha256(await readFile(result.packageSeal.path)));
  assert.deepEqual(result.executableRequirements, { appId: APP_ID, executableSha256: expectedExecutable, neededSonames: ["libm.so.6", "libc.so.6"] });
  assert.equal(Object.keys(result.audit.files).length, 12);
  assert.equal(result.audit.files["criterion-unofficial"]?.sha256, expectedExecutable);
  assert.equal(result.audit.files["NOTICES.md"]?.sha256, sha256(await readFile("/workspace/NOTICES.md")));
  assert.ok(Object.isFrozen(result) && Object.isFrozen(result.executableRequirements.neededSonames) && Object.isFrozen(result.audit.files));
  const repeatedInput = { ...await inputFixture(), outputDirectory: input.outputDirectory }; let repeats = 0;
  await assert.rejects(packageNativeMain(repeatedInput, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => 100, execute: async () => { repeats += 1; return complete(); } }), /EEXIST/);
  assert.equal(repeats, 0);
  assert.equal(result.packageSeal.sha256, sha256(await readFile(result.packageSeal.path)));
  const names = await readdir(input.outputDirectory);
  assert.ok(names.includes("package-seal.json") && names.includes("readelf-dynamic.stdout"));
});

test("a preexisting normalization directory is retained without resetting it", async () => {
  const input = await inputFixture(); let issued = 0;
  const work = join("/workspace/.local/native-package/normalization", input.outputDirectory.slice(input.outputDirectory.lastIndexOf("/") + 1));
  await mkdir(dirname(work), { recursive: true }); await mkdir(work);
  await writeFile(join(work, "sentinel"), "existing work\n", { flag: "wx" });
  await assert.rejects(packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => 100,
    execute: async (command) => {
      issued += 1;
      if (command.args[0] === "-d") return complete(dynamicFixture);
      if (command.args[0]?.endsWith("/ares-package.js")) await cliArchive(command);
      return complete();
    } }), /EEXIST/);
  assert.equal(issued, 3);
  assert.equal((await readFile(join(work, "sentinel"))).toString(), "existing work\n");
  assert.deepEqual(await readdir(work), ["sentinel"]);
  assert.ok(!(await readdir(input.outputDirectory)).includes("package-seal.json"));
});

test("an extra CLI payload remains failure evidence and cannot become a sealed package", async () => {
  const input = await inputFixture(); let issued = 0;
  await assert.rejects(packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => 100,
    execute: async (command) => {
      issued += 1;
      if (command.args[0] === "-d") return complete(dynamicFixture);
      if (command.args[0]?.endsWith("/ares-package.js")) await cliArchive(command, true);
      return complete();
    } }), /invalidIpkPayload/);
  assert.equal(issued, 3);
  assert.ok(!(await readdir(input.outputDirectory)).includes("package-seal.json"));
  assert.deepEqual(await readdir(join(input.outputDirectory, "ipks")), []);
  assert.deepEqual(await readdir(join(input.outputDirectory, "raw-cli")), [`${APP_ID}_${VERSION}_arm.ipk`]);
});

test("a backward command clock keeps acknowledged closure but cannot advance packaging", async () => {
  const input = await inputFixture(); let current = 100; let issued = 0;
  await assert.rejects(packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => current,
    execute: async () => { issued += 1; current = 99; return complete(dynamicFixture); } }), (error: unknown) => {
      assert.ok(error instanceof PackagingCommandError);
      assert.equal(error.message, "packagingDeadline"); assert.equal(error.outcome?.closed, true); return true;
    });
  assert.equal(issued, 1);
  assert.ok(!(await readdir(input.outputDirectory)).includes("package-seal.json"));
});

test("source changes after the CLI prevent final package admission", async () => {
  const input = await sourceFixture(await inputFixture()); let issued = 0;
  await assert.rejects(packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => 100,
    execute: async (command) => {
      issued += 1;
      if (command.args[0] === "-d") return complete(dynamicFixture);
      if (command.args[0]?.endsWith("/ares-package.js")) { await cliArchive(command); await writeFile(join(input.sourceRoot, "crates/criterion-app/src/main.rs"), "changed source\n"); }
      return complete();
    } }), /sourceMismatch/);
  assert.equal(issued, 3);
  assert.ok(!(await readdir(input.outputDirectory)).includes("package-seal.json"));
  assert.deepEqual(await readdir(join(input.outputDirectory, "ipks")), []);
  assert.deepEqual(await readdir(join(input.outputDirectory, "raw-cli")), [`${APP_ID}_${VERSION}_arm.ipk`]);
});

for (const [name, bytes] of [
  ["empty", Buffer.alloc(0)], ["binary", Buffer.from([128])], ["overbound", Buffer.alloc(65537, 32)],
  ["missing dynamic header", Buffer.from(" 0x00000001 (NEEDED) Shared library: [libc.so.6]\n")],
  ["duplicate", Buffer.from(dynamicFixture.toString().replace("libm.so.6", "libc.so.6"))],
  ["path", Buffer.from(dynamicFixture.toString().replace("libm.so.6", "../libm.so.6"))],
  ["wrong tag", Buffer.from(dynamicFixture.toString().replace("0x00000001", "0x00000002"))],
  ["no dependencies", Buffer.from("Dynamic section at offset 0x100 contains 1 entries:\n 0x00000000 (NULL) 0x0\n")],
  ["eleven dependencies", Buffer.from("Dynamic section at offset 0x100 contains 12 entries:\n" + Array.from({ length: 11 }, (_, i) => ` 0x00000001 (NEEDED) Shared library: [lib${i}.so.1]\n`).join(""))],
] as const) {
  test(`unsafe ${name} readelf metadata cannot reach the official CLI`, async () => {
    const input = await inputFixture(); let issued = 0;
    await assert.rejects(packageNativeMain(input, { image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => 100,
      execute: async () => { issued += 1; return complete(bytes); } }), /invalidDynamicMetadata|invalidCommandResult/);
    assert.equal(issued, 1);
    assert.ok(!(await readdir(input.outputDirectory)).includes("package-seal.json"));
  });
}
