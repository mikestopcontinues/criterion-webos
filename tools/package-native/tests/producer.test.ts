import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { packageMainFromFiles } from "../package-phase.js";
import { APP_ID, VERSION } from "../src/admission.js";
import { PACKAGING_IMAGE, type ClosedCommand, type PackageCommand } from "../src/contract.js";
import { PAYLOAD_NAMES } from "../src/manifest.js";
import { REQUIRED_SOURCES, sha256, type BuildReceipt } from "../src/receipt.js";
import { metadataFixture } from "./admission.test.js";
import { ipkFixture, tarFixture } from "../../player-probe/tests/fixture-tar.js";
import { projectFixture, withFixture } from "../../source-distribution/tests/offline.test.js";
import type { ProjectSnapshot, SourceFile } from "../../source-distribution/src/project.js";
import { createHash } from "node:crypto";
import { produceNativeMain } from "../producer.js";
import { MainProducerCommandError, type MainProducerInput, type MainProducerExecution, type ProducerCommand } from "../src/producer-contract.js";
import { packageWorkerProgram, packageWorkerLaunch } from "../src/package-worker.js";
import { spawnSync } from "node:child_process";
import { readInput } from "../src/input.js";
import { prepareOfflineMainFromFiles } from "../offline-phase.js";
import { offlineWorkerLaunch } from "../src/offline-worker.js";

const dynamic = Buffer.from("Dynamic section at offset 0x100 contains 2 entries:\n 0x00000001 (NEEDED) Shared library: [libc.so.6]\n 0x00000000 (NULL) 0x0\n");
const closed = (stdout: Buffer = Buffer.alloc(0)): ClosedCommand => ({ closed: true, exitCode: 0, signal: null, timedOut: false, stdout, stderr: Buffer.alloc(0) });

async function archive(command: PackageCommand): Promise<void> {
  const staging = command.args[1], output = command.args[4]; assert.ok(staging && output);
  const entries = [];
  for (const name of PAYLOAD_NAMES) entries.push({ name: `usr/palm/applications/${APP_ID}/${name}`, bytes: await readFile(join(staging, name)), mode: name === "criterion-unofficial" ? 0o755 : 0o644 });
  entries.push({ name: `usr/palm/packages/${APP_ID}/packageinfo.json`, bytes: Buffer.from(JSON.stringify({ id: APP_ID, version: VERSION, app: APP_ID }, null, 2) + "\n"), mode: 0o644 });
  const control = Buffer.from(`Package: ${APP_ID}\nVersion: ${VERSION}\nSection: misc\nPriority: optional\nArchitecture: arm\nInstalled-Size: 8192\nMaintainer: N/A <nobody@example.com>\nDescription: This is a webOS application.\nwebOS-Package-Format-Version: 2\nwebOS-Packager-Version: x.y.x\n`);
  await writeFile(join(output, `${APP_ID}_${VERSION}_arm.ipk`), ipkFixture(tarFixture(entries), tarFixture([{ name: "control", bytes: control, mode: 0o644 }])), { flag: "wx" });
}

test("the explicit packaging image phase consumes earned files and returns a real normalized MAIN audit", async () => {
  const name = "producer" + [...randomBytes(8)].map(byte => String.fromCharCode(97 + byte % 26)).join("");
  const input = `/workspace/.local/native-package/${name}`; await mkdir(input, { mode: 0o700 });
  const executable = metadataFixture(); const receipt: BuildReceipt = { schemaVersion: 1, appId: APP_ID, version: VERSION,
    target: "arm-unknown-linux-gnueabi", profile: "release", sourceCommit: "a".repeat(40),
    cargoLockSha256: sha256(await readFile("/workspace/Cargo.lock")), executableSha256: sha256(executable), sourceSha256: {} };
  for (const path of REQUIRED_SOURCES) receipt.sourceSha256[path] = sha256(await readFile(join("/workspace", path)));
  await writeFile(join(input, "criterion-unofficial"), executable, { flag: "wx" });
  await writeFile(join(input, "build-receipt.json"), JSON.stringify(receipt), { flag: "wx" });
  const requests: PackageCommand[] = [];
  const result = await packageMainFromFiles({ sourceRoot: "/workspace", outputDirectory: `/workspace/.local/native-package/exports/${name}`,
    executablePath: join(input, "criterion-unofficial"), receiptPath: join(input, "build-receipt.json") }, {
    image: PACKAGING_IMAGE, deadlineMs: 10000, now: () => 100,
    execute: async command => { requests.push(command); if (command.args[0] === "-d") return closed(dynamic);
      if (command.args[0]?.endsWith("/ares-package.js")) await archive(command); return closed(); },
  });
  assert.equal(result.ipk.sha256, sha256(await readFile(result.ipk.path)));
  assert.deepEqual(result.executableRequirements.neededSonames, ["libc.so.6"]);
  assert.equal(result.audit.files["criterion-unofficial"]?.sha256, sha256(executable));
  assert.equal(requests.length, 3);
  assert.ok(requests.every(request => request.deadlineMs === 10000));
});

const gitHash = (kind: string, bytes: Buffer): string => createHash("sha1").update(`${kind} ${bytes.length}\0`).update(bytes).digest("hex");
function gitFixture(project: ProjectSnapshot, command: ProducerCommand): ClosedCommand {
  const args = command.args;
  if (args.includes("rev-parse")) return closed(Buffer.from(project.revision + "\n"));
  if (args.includes("status")) return closed();
  if (args.includes("ls-tree")) return closed(Buffer.from(project.files.map(file => `${file.mode === 0o755 ? "100755" : "100644"} blob ${gitHash("blob", file.bytes)}\t${file.name}\0`).join("")));
  if (args.includes("commit")) return closed(project.commit);
  const file = project.files.find(file => gitHash("blob", file.bytes) === args[args.length - 1]); assert.ok(file); return closed(file.bytes);
}
async function sourceFixture(initial: SourceFile[]): Promise<ProjectSnapshot> {
  const files = new Map(initial.map(file => [file.name, file]));
  async function visit(name: string): Promise<void> {
    for (const entry of await readdir(join("/workspace", name), { withFileTypes: true })) {
      if (entry.name === "node_modules") continue;
      const path = name + "/" + entry.name;
      if (entry.isDirectory()) await visit(path);
      else if (entry.isFile()) files.set(path, { name: path, bytes: await readFile(join("/workspace", path)), mode: 0o644 });
    }
  }
  for (const path of ["tools/package-native", "tools/source-distribution", "tools/player-probe", "crates"]) await visit(path);
  for (const name of ["Cargo.toml", "Dockerfile", "rust-toolchain.toml", ".cargo/config.toml", "LICENSE", "NOTICES.md"]) files.set(name, { name, bytes: await readFile(join("/workspace", name)), mode: 0o644 });
  files.set("crates/criterion-app/src/literal-empty.rs", { name: "crates/criterion-app/src/literal-empty.rs", bytes: Buffer.alloc(0), mode: 0o644 });
  return projectFixture([...files.values()].sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name))));
}
type ProducerFixture = { input: MainProducerInput; execution: MainProducerExecution; commands: ProducerCommand[]; project: ProjectSnapshot };
let preparedProject: ProjectSnapshot | undefined;
async function withProducer(run: (fixture: ProducerFixture) => Promise<void>): Promise<void> {
  await withFixture(async input => {
    const project = preparedProject ?? await sourceFixture(input.project.files); preparedProject = project;
    const request = JSON.parse(input.requestBytes.toString("utf8")) as { sourceCommit: string }; request.sourceCommit = project.revision;
    const name = "main" + [...randomBytes(8)].map(byte => String.fromCharCode(97 + byte % 26)).join("");
    const outputDirectory = `/workspace/.local/native-package/producers/${name}`;
    const commands: ProducerCommand[] = [];
    const registryArchivesRoot = join(input.registryRoot, "..", "archives"); await mkdir(registryArchivesRoot);
    for (const file of ["example-1.2.3.crate", "runtime-only-1.0.0.crate"]) await writeFile(join(registryArchivesRoot, file), await input.readRegistryArchive(file), { flag: "wx" });
    const producerInput: MainProducerInput = { sourceRoot: "/workspace", sourceCommit: project.revision, outputDirectory,
      dependenciesRoot: "/workspace/tools/player-probe/node_modules",
      offline: { installedRuntimeRoot: input.installedRuntimeRoot, registryRoot: input.registryRoot, registryArchivesRoot, requestBytes: Buffer.from(JSON.stringify(request)), runtimeInputs: { channel: Buffer.from(input.runtimeInputs.channel), archive: Buffer.from(input.runtimeInputs.archive), copyright: Buffer.from(input.runtimeInputs.copyright) } } };
    const execution: MainProducerExecution = {
      deadlineMs: 60000, now: () => 100,
      execute: async command => {
        commands.push(command);
        if (command.kind === "git") return gitFixture(project, command);
        if (command.args[0] === "/workspace/.local/native-package/offline-launcher.cjs") return closed(Buffer.from(JSON.stringify(await prepareOfflineMainFromFiles({ sourceRoot: join(outputDirectory, "source"), outputDirectory, registryArchivesRoot, installedRuntimeRoot: input.installedRuntimeRoot, registryRoot: input.registryRoot }, { deadlineMs: 60000, now: () => 100 }))));
        if (command.executable === "/bin/sh") return closed(Buffer.from("aarch64\nrustc 1.98.0-nightly (4c9d2bfe4 2026-07-01)\nbinary: rustc\ncommit-hash: 4c9d2bfe4ad7a65669098754964aaebe0ec1ced2\ncommit-date: 2026-07-01\nhost: aarch64-unknown-linux-gnu\nrelease: 1.98.0-nightly\nLLVM version: 22.1.0\ncargo 1.98.0-nightly (123456789 2026-07-01)\n/usr/local/rustup/toolchains/nightly-2026-07-02-aarch64-unknown-linux-gnu\n" + ["/usr/local/rustup/toolchains/nightly-2026-07-02-aarch64-unknown-linux-gnu/bin/rustc", "/usr/local/rustup/toolchains/nightly-2026-07-02-aarch64-unknown-linux-gnu/bin/cargo", "/opt/webos-sdk/bin/arm-webos-linux-gnueabi-gcc.br_real", "/usr/bin/readelf"].map(path => "a".repeat(64) + "  " + path + "\n").join("") + "arm-webos-linux-gnueabi-gcc.br_real (Buildroot) 12.3.0\nGNU readelf (GNU Binutils) 2.40\n"));
        if (command.executable.endsWith("/cargo")) {
          const path = join(outputDirectory, "target/arm-unknown-linux-gnueabi/release"); await mkdir(path, { recursive: true });
          await writeFile(join(path, "criterion-unofficial"), metadataFixture(), { mode: 0o755 }); return closed();
        }
        if (command.executable === "/usr/bin/readelf") return closed(dynamic);
        if (command.args.includes("tools/package-native/tsconfig.json")) return closed();
        await writeFile(join(outputDirectory, "package/package-worker.cjs"), packageWorkerProgram({ sourceRoot: "/workspace", outputDirectory: "/workspace/.local/native-package/exports/main", executablePath: "/workspace/.local/native-package/input/criterion-unofficial", receiptPath: "/workspace/.local/native-package/input/build-receipt.json", modulePath: "/workspace/.local/native-package/compiled/tools/package-native/package-phase.js", image: PACKAGING_IMAGE, deadlineMs: 60000 }), { flag: "wx" });
        const phaseInput = `/workspace/.local/native-package/${name}`; await mkdir(phaseInput);
        await writeFile(join(phaseInput, "criterion-unofficial"), await readFile(join(outputDirectory, "package/input/criterion-unofficial")));
        await writeFile(join(phaseInput, "build-receipt.json"), await readFile(join(outputDirectory, "package/input/build-receipt.json")));
        const cliRoot = join(outputDirectory, "source/tools/player-probe/node_modules/@webos-tools/cli"); await mkdir(join(cliRoot, "bin"), { recursive: true });
        for (const path of ["package.json", "bin/ares-config.js", "bin/ares-package.js"]) await writeFile(join(cliRoot, path), await readFile(join("/workspace/tools/player-probe/node_modules/@webos-tools/cli", path)));
        const result = await packageMainFromFiles({ sourceRoot: join(outputDirectory, "source"),
          outputDirectory: `/workspace/.local/native-package/exports/${name}`,
          executablePath: join(phaseInput, "criterion-unofficial"), receiptPath: join(phaseInput, "build-receipt.json") }, {
          image: PACKAGING_IMAGE, deadlineMs: 60000, now: () => 100,
          execute: async request => { if (request.args[0] === "-d") return closed(dynamic);
            if (request.args[0]?.endsWith("/ares-package.js")) await archive(request); return closed(); },
        });
        // Controlled container mount translation; real package bytes/audit come from the maintained owner above.
        const ipkPath = join(outputDirectory, `package/exports/main/ipks/${APP_ID}_${VERSION}_arm.ipk`);
        await mkdir(join(ipkPath, ".."), { recursive: true }); await writeFile(ipkPath, await readFile(result.ipk.path));
        const ipk = { ...result.ipk, path: `/workspace/.local/native-package/exports/main/ipks/${APP_ID}_${VERSION}_arm.ipk` };
        const seal = JSON.parse((await readFile(result.packageSeal.path)).toString("utf8")) as Record<string, unknown>; seal.ipk = ipk;
        const sealBytes = Buffer.from(JSON.stringify(seal));
        await writeFile(join(outputDirectory, "package/exports/main/package-seal.json"), sealBytes);
        return closed(Buffer.from(JSON.stringify({ ...result, ipk, packageSeal: { path: "/workspace/.local/native-package/exports/main/package-seal.json", sha256: sha256(sealBytes), bytes: sealBytes.length } })));
      },
    };
    await run({ input: producerInput, execution, commands, project });
  });
}
test("produceNativeMain connects admitted Git/offline sources to the earned MAIN package and seals last", async () => {
  await withProducer(async ({ input, execution, commands, project }) => {
    const product = await produceNativeMain(input, execution);
    assert.equal(product.sourceCommit, project.revision);
    assert.equal(product.executable.sha256, sha256(metadataFixture()));
    assert.equal(product.package.audit.files["criterion-unofficial"]?.sha256, product.executable.sha256);
    assert.equal(product.package.ipk.sha256, sha256(await readFile(product.package.ipk.path)));
    assert.equal((await readFile(join(input.outputDirectory, "source/crates/criterion-app/src/literal-empty.rs"))).length, 0);
    const seal = JSON.parse((await readFile(product.producerSeal.path)).toString("utf8")) as { sourceCommit: string; status: string };
    assert.equal(seal.sourceCommit, project.revision); assert.equal(seal.status, "development");
    assert.ok(commands.every(command => command.deadlineMs === 60000));
    const cargo = commands.find(command => command.executable.endsWith("/cargo")); assert.ok(cargo && cargo.kind === "container");
    assert.deepEqual(cargo.args, ["build", "-p", "criterion-app", "--features", "criterion-app/webos", "--target", "arm-unknown-linux-gnueabi", "-Z", "build-std=std,panic_abort", "-Z", "build-std-features=compiler-builtins-mem", "--release", "--locked", "--offline", "--jobs", "2"]);
    assert.equal(cargo.network, "none"); assert.equal(cargo.cpus, 2);
  });
});

test("a foreign output is retained and no command is issued", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    await mkdir(input.outputDirectory, { recursive: true }); await writeFile(join(input.outputDirectory, "foreign"), "captain-owned\n");
    await assert.rejects(() => produceNativeMain(input, execution), /EEXIST/);
    assert.equal(commands.length, 0); assert.equal((await readFile(join(input.outputDirectory, "foreign"))).toString(), "captain-owned\n");
  });
});
test("an unresolved Git outcome stops before source preparation and preserves unknown custody", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => { commands.push(command); return { closed: false }; } }), (error: unknown) => {
      assert.ok(error instanceof Error && "received" in error); return true;
    });
    assert.equal(commands.length, 1);
    await assert.rejects(() => readFile(join(input.outputDirectory, "producer-seal.json")), /ENOENT/);
  });
});
test("late known Git closure keeps actual bytes and refuses the next command", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    let time = 100;
    await assert.rejects(() => produceNativeMain(input, { ...execution, now: () => time, execute: async command => {
      const result = await execution.execute(command); time = 60000; return result;
    } }), (error: unknown) => {
      assert.ok(error instanceof Error && "received" in error);
      const received = error.received as { closed: boolean; stdout: Buffer }; assert.equal(received.closed, true); assert.ok(received.stdout.equals(Buffer.from(input.sourceCommit + "\n"))); return true;
    });
    assert.equal(commands.length, 1);
    assert.equal((await readFile(join(input.outputDirectory, "git-0001.stdout"))).toString(), input.sourceCommit + "\n");
    await assert.rejects(() => readFile(join(input.outputDirectory, "producer-seal.json")), /ENOENT/);
  });
});
test("post-command custody refusal retains the exact known closure and issues nothing further", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    const outcome = closed(Buffer.from(input.sourceCommit + "\n"));
    await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => {
      commands.push(command); await mkdir(join(input.outputDirectory, "git-0001.stdout")); return outcome;
    } }), (error: unknown) => {
      assert.ok(error instanceof MainProducerCommandError);
      assert.equal(error.message, "producerCustodyFailed"); assert.deepEqual(error.received, outcome);
      assert.ok(error.cause instanceof Error); assert.equal((error.cause as NodeJS.ErrnoException).code, "EEXIST"); return true;
    });
    assert.equal(commands.length, 1);
    await assert.rejects(() => readFile(join(input.outputDirectory, "producer-seal.json")), /ENOENT/);
  });
});
test("offline archive validation is an explicit settled GNU toolkit phase before the SDK", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    await produceNativeMain(input, execution);
    const offline = commands.findIndex(command => command.args.includes("/workspace/.local/native-package/offline-launcher.cjs"));
    const sdk = commands.findIndex(command => command.executable === "/bin/sh");
    assert.ok(offline >= 0 && sdk > offline);
    const command = commands[offline]; assert.ok(command?.kind === "container");
    assert.equal(command.image, PACKAGING_IMAGE); assert.equal(command.network, "none");
  });
});
test("wrong compiler commit refuses before the MAIN build", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => {
      const result = await execution.execute(command);
      return command.kind === "container" && command.executable === "/bin/sh" && result.closed ? { ...result, stdout: Buffer.from(result.stdout.toString().replace("commit-hash: 4c9d2bfe4ad7a65669098754964aaebe0ec1ced2", "commit-hash: " + "0".repeat(40))) } : result;
    } }), /invalidCompilerMetadata/);
    assert.equal(commands.filter(command => command.executable.endsWith("/cargo")).length, 0);
  });
});
test("a closed failed build retains original diagnostics and creates no receipt or package", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => {
      if (command.executable.endsWith("/cargo")) { commands.push(command); return { ...closed(), exitCode: 1, stderr: Buffer.from("literal compiler diagnostic\n") }; }
      return execution.execute(command);
    } }), (error: unknown) => { assert.ok(error instanceof MainProducerCommandError); assert.equal(error.message, "producerCommandFailed"); return true; });
    assert.equal((await readFile(join(input.outputDirectory, "cargo-main.stderr"))).toString(), "literal compiler diagnostic\n");
    await assert.rejects(() => readFile(join(input.outputDirectory, "package/input/build-receipt.json")), /ENOENT/);
    await assert.rejects(() => readFile(join(input.outputDirectory, "producer-seal.json")), /ENOENT/);
  });
});
test("an ELF changed during static inspection cannot reach packaging", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => {
      const result = await execution.execute(command);
      if (command.executable === "/usr/bin/readelf") await writeFile(join(input.outputDirectory, "target/arm-unknown-linux-gnueabi/release/criterion-unofficial"), Buffer.from("changed ELF\n"));
      return result;
    } }), /producerExecutableChanged/);
    assert.equal(commands.filter(command => command.args.includes("/workspace/.local/native-package/package-launcher.cjs")).length, 0);
  });
});
test("exact package bytes must still match the earned export before the producer seal", async () => {
  await withProducer(async ({ input, execution }) => {
    await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => {
      const result = await execution.execute(command);
      if (command.kind === "container" && command.args[0] === "/workspace/.local/native-package/package-launcher.cjs") await writeFile(join(input.outputDirectory, `package/exports/main/ipks/${APP_ID}_${VERSION}_arm.ipk`), "tampered output\n");
      return result;
    } }), /invalidProducerPackage/);
    await assert.rejects(() => readFile(join(input.outputDirectory, "producer-seal.json")), /ENOENT/);
  });
});
test("source request/runtime buffers are detached before the first awaited command", async () => {
  await withProducer(async ({ input, execution }) => {
    let changed = false;
    const product = await produceNativeMain(input, { ...execution, execute: async command => {
      if (!changed) { changed = true; input.offline.requestBytes.fill(0); input.offline.runtimeInputs.archive.fill(0); input.offline.runtimeInputs.channel.fill(0); input.offline.runtimeInputs.copyright.fill(0); }
      return execution.execute(command);
    } });
    assert.equal(product.sourceCommit, input.sourceCommit);
    assert.equal(product.package.ipk.sha256, sha256(await readFile(product.package.ipk.path)));
  });
});
test("empty source admission is explicit and all ordinary build inputs still require bytes", async () => {
  const path = "/workspace/.local/native-package/literal-empty-input"; await writeFile(path, Buffer.alloc(0), { flag: "wx" });
  await assert.rejects(() => readInput(path, 128), /invalidInput/);
  assert.equal((await readInput(path, 128, 0)).length, 0);
});
test("the fixed packaging wrapper emits valid Node24 code and really closes an invalid metadata-only package", async () => {
  const deadlineMs = Date.now() + 30000;
  await mkdir("/workspace/.local/native-package/input", { mode: 0o700 });
  const executable = metadataFixture(); const receipt: BuildReceipt = { schemaVersion: 1, appId: APP_ID, version: VERSION, target: "arm-unknown-linux-gnueabi", profile: "release", sourceCommit: "a".repeat(40), cargoLockSha256: sha256(await readFile("/workspace/Cargo.lock")), executableSha256: sha256(executable), sourceSha256: {} };
  for (const path of REQUIRED_SOURCES) receipt.sourceSha256[path] = sha256(await readFile(join("/workspace", path)));
  await writeFile("/workspace/.local/native-package/input/criterion-unofficial", executable, { flag: "wx" });
  await writeFile("/workspace/.local/native-package/input/build-receipt.json", JSON.stringify(receipt), { flag: "wx" });
  const program = packageWorkerProgram({ sourceRoot: "/workspace", outputDirectory: "/workspace/.local/native-package/exports/main", executablePath: "/workspace/.local/native-package/input/criterion-unofficial", receiptPath: "/workspace/.local/native-package/input/build-receipt.json", modulePath: "/workspace/.local/native-package/compiled/tools/package-native/package-phase.js", deadlineMs, image: PACKAGING_IMAGE });
  const syntax = spawnSync(process.execPath, ["--check"], { input: program, timeout: 30000, maxBuffer: 65536 });
  assert.equal(syntax.status, 0); assert.equal(syntax.signal, null); assert.equal(syntax.stderr.length, 0);
  const launch = packageWorkerLaunch({ sourceRoot: "/workspace", outputDirectory: "/workspace/.local/native-package/exports/main", executablePath: "/workspace/.local/native-package/input/criterion-unofficial", receiptPath: "/workspace/.local/native-package/input/build-receipt.json", modulePath: "/workspace/.local/native-package/compiled/tools/package-native/package-phase.js", deadlineMs, image: PACKAGING_IMAGE });
  const result = spawnSync(process.execPath, ["-e", launch], { timeout: 30000, maxBuffer: 65536 });
  assert.equal(result.status, 1); assert.equal(result.signal, null); assert.equal(result.stdout.length, 0);
  assert.match(result.stderr.toString(), /invalidDynamicMetadata/);
  // Only readelf read this literal ELF; no ELF execution, official CLI package or SDK occurred.
});
test("the fixed offline launcher is inert checked source and has no host tar or mutable deadline", () => {
  const program = offlineWorkerLaunch(123456789);
  const syntax = spawnSync(process.execPath, ["--check"], { input: program, timeout: 30000, maxBuffer: 65536 });
  assert.equal(syntax.status, 0); assert.equal(syntax.signal, null); assert.equal(syntax.stderr.length, 0);
  assert.match(program, /deadlineMs:123456789,now:Date.now/); assert.doesNotMatch(program, /spawn|\/usr\/bin\/tar/);
  assert.throws(() => offlineWorkerLaunch(Number.NaN), /invalidOfflineWorker/);
});
test("failed offline closure preserves raw outcome and cannot start SDK metadata", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => {
      if (command.args.includes("/workspace/.local/native-package/offline-launcher.cjs")) { commands.push(command); return { ...closed(), exitCode: 1, stderr: Buffer.from("literal GNU phase failure\n") }; }
      return execution.execute(command);
    } }), (error: unknown) => { assert.ok(error instanceof MainProducerCommandError); assert.equal(error.message, "producerCommandFailed"); return true; });
    assert.equal((await readFile(join(input.outputDirectory, "offline-sources.stderr"))).toString(), "literal GNU phase failure\n");
    assert.equal(commands.filter(command => command.executable === "/bin/sh").length, 0);
  });
});
test("changed authenticated materialization refuses the SDK before consuming changed Rust", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => {
      const result = await execution.execute(command);
      if (command.args.includes("/workspace/.local/native-package/offline-launcher.cjs")) await writeFile(join(input.outputDirectory, "rust/library/core/src/lib.rs"), "changed Rust source\n");
      return result;
    } }), /invalidOfflineSource/);
    assert.equal(commands.filter(command => command.executable === "/bin/sh").length, 0);
    await assert.rejects(() => readFile(join(input.outputDirectory, "producer-seal.json")), /ENOENT/);
  });
});
test("offline summary cannot claim a different raw runtime or source commit", async () => {
  for (const field of ["sourceCommit", "runtimeArchive"] as const) {
    await withProducer(async ({ input, execution, commands }) => {
      await assert.rejects(() => produceNativeMain(input, { ...execution, execute: async command => {
        const result = await execution.execute(command);
        if (command.args.includes("/workspace/.local/native-package/offline-launcher.cjs") && result.closed) {
          const summary = JSON.parse(result.stdout.toString("utf8")) as { sourceCommit: string; runtime: { archiveSha256: string } };
          if (field === "sourceCommit") summary.sourceCommit = "0".repeat(40); else summary.runtime.archiveSha256 = "0".repeat(64);
          return { ...result, stdout: Buffer.from(JSON.stringify(summary)) };
        }
        return result;
      } }), /invalidProducerOffline/);
      assert.equal(commands.filter(command => command.executable === "/bin/sh").length, 0);
    });
  }
});
test("post-command throwing clock retains both actual buffers and stops immediately", async () => {
  await withProducer(async ({ input, execution, commands }) => {
    let returned = false;
    const outcome = { ...closed(Buffer.from("literal joined stdout\n")), stderr: Buffer.from("literal joined stderr\n") };
    await assert.rejects(() => produceNativeMain(input, { ...execution, now: () => { if (returned) throw new Error("literal clock refusal"); return 100; }, execute: async command => { commands.push(command); returned = true; return outcome; } }), (error: unknown) => {
      assert.ok(error instanceof MainProducerCommandError); assert.equal(error.message, "producerCustodyFailed"); assert.deepEqual(error.received, outcome);
      assert.ok(error.cause instanceof Error); assert.equal(error.cause.message, "literal clock refusal"); return true;
    });
    assert.equal(commands.length, 1); await assert.rejects(() => readFile(join(input.outputDirectory, "producer-seal.json")), /ENOENT/);
  });
});
