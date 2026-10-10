import { mkdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { captureHostProject } from "../source-distribution/src/git.js";
import { verifyOfflineSources } from "../source-distribution/src/offline.js";
import { APP_ID, MAX_EXECUTABLE_BYTES, MAX_IPK_BYTES, VERSION, admitExecutable } from "./src/admission.js";
import { PACKAGING_IMAGE, type ClosedCommand, type NativeMainExport } from "./src/contract.js";
import { readInput } from "./src/input.js";
import { ordinaryRoot, prepareMainSource, receiptForMain, verifyMaterialized, writeOwned } from "./src/main-source.js";
import { packageWorkerLaunch } from "./src/package-worker.js";
import { PAYLOAD_NAMES } from "./src/manifest.js";
import { admitBuildReceipt, sha256 } from "./src/receipt.js";
import { MainProducerCommandError, NATIVE_COMPILER_IMAGE, type MainProducerExecution, type MainProducerInput, type NativeMainProduct, type ProducerCommand, type ProducerContainerCommand } from "./src/producer-contract.js";

const sdkMetadata = 'set -eu\nuname -m\nrustc -vV\ncargo -V\nrustc --print sysroot\nsha256sum "$(rustup which rustc)" "$(rustup which cargo)" /opt/webos-sdk/bin/arm-webos-linux-gnueabi-gcc.br_real /usr/bin/readelf\n/opt/webos-sdk/bin/arm-webos-linux-gnueabi-gcc.br_real --version\n/usr/bin/readelf --version\n';
const mainArgs = ["build", "-p", "criterion-app", "--features", "criterion-app/webos", "--target", "arm-unknown-linux-gnueabi", "-Z", "build-std=std,panic_abort", "-Z", "build-std-features=compiler-builtins-mem", "--release", "--locked", "--offline", "--jobs", "2"] as const;
const environment = { PATH: "/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin", LANG: "C", LC_ALL: "C", TZ: "UTC", HOME: "/tmp", RUSTUP_HOME: "/usr/local/rustup", CARGO_HOME: "/cargo", CARGO_TARGET_DIR: "/target",
  WEBOS_SDK: "/opt/webos-sdk", WEBOS_SYSROOT: "/opt/webos-sdk/arm-webos-linux-gnueabi/sysroot", CC_arm_unknown_linux_gnueabi: "/opt/webos-sdk/bin/arm-webos-linux-gnueabi-gcc.br_real", AR_arm_unknown_linux_gnueabi: "/opt/webos-sdk/bin/arm-webos-linux-gnueabi-ar", CFLAGS_arm_unknown_linux_gnueabi: "--sysroot=/opt/webos-sdk/arm-webos-linux-gnueabi/sysroot" };
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalidProducerResult"); return value as Record<string, unknown>;
}
function closed(value: unknown, limit: number): ClosedCommand {
  const item = record(value);
  if (Reflect.ownKeys(item).length !== 6 || Object.keys(item).sort().join() !== ["closed", "exitCode", "signal", "timedOut", "stdout", "stderr"].sort().join()
    || item.closed !== true || typeof item.timedOut !== "boolean" || !Buffer.isBuffer(item.stdout) || !Buffer.isBuffer(item.stderr)
    || item.stdout.length + item.stderr.length > limit
    || item.exitCode !== null && (!Number.isSafeInteger(item.exitCode) || (item.exitCode as number) < 0 || (item.exitCode as number) > 255)
    || item.signal !== null && (typeof item.signal !== "string" || !/^SIG[A-Z0-9]{1,16}$/.test(item.signal))
    || (item.exitCode === null) === (item.signal === null)) throw new Error("invalidProducerResult");
  return { closed: true, exitCode: item.exitCode as number | null, signal: item.signal as string | null, timedOut: item.timedOut, stdout: Buffer.from(item.stdout), stderr: Buffer.from(item.stderr) };
}
function parseMetadata(bytes: Buffer, version: string, commit: string, channel: string): { sysroot: string; toolHashes: Record<string, string> } {
  if (bytes.length > 65536 || !Buffer.from(bytes.toString("utf8")).equals(bytes)) throw new Error("invalidCompilerMetadata");
  const text = bytes.toString("utf8");
  const sysroot = `/usr/local/rustup/toolchains/${channel}-aarch64-unknown-linux-gnu`;
  const release = /^release: (.+)$/m.exec(text)?.[1];
  if (!text.startsWith("aarch64\n") || /^commit-hash: ([a-f0-9]{40})$/m.exec(text)?.[1] !== commit
    || /^host: (.+)$/m.exec(text)?.[1] !== "aarch64-unknown-linux-gnu" || release !== version.split(" ")[0]
    || !text.split("\n").includes(sysroot) || !/^cargo [0-9]+\.[0-9]+\.[0-9]+-nightly \(/m.test(text)
    || !text.includes("GNU readelf") || !text.includes("gcc")) throw new Error("invalidCompilerMetadata");
  const hashes: Record<string, string> = {};
  const paths = [`${sysroot}/bin/rustc`, `${sysroot}/bin/cargo`, "/opt/webos-sdk/bin/arm-webos-linux-gnueabi-gcc.br_real", "/usr/bin/readelf"];
  for (const line of text.split("\n")) {
    const match = /^([a-f0-9]{64})  (.+)$/.exec(line);
    if (!match) continue;
    const path = match[2], hash = match[1];
    if (!path || !hash || !paths.includes(path) || hashes[path]) throw new Error("invalidCompilerMetadata"); hashes[path] = hash;
  }
  if (Object.keys(hashes).length !== 4) throw new Error("invalidCompilerMetadata"); return { sysroot, toolHashes: hashes };
}

/** Captures committed source, authenticates offline sources, builds MAIN, then invokes the admitted package owner. */
export async function produceNativeMain(input: MainProducerInput, execution: MainProducerExecution): Promise<NativeMainProduct> {
  const { deadlineMs, now, execute } = execution; let previous = -1;
  const check = (): number => {
    const current = now();
    if (!Number.isSafeInteger(deadlineMs) || !Number.isSafeInteger(current) || current < 0 || current < previous || current >= deadlineMs) throw new Error("producerDeadline");
    previous = current; return deadlineMs - current;
  };
  check();
  const sourceRoot = input.sourceRoot, output = input.outputDirectory, dependenciesRoot = input.dependenciesRoot, sourceCommit = input.sourceCommit;
  if (resolve(sourceRoot) !== sourceRoot || resolve(dependenciesRoot) !== dependenciesRoot || !/^[a-f0-9]{40}$/.test(sourceCommit)
    || !output.startsWith(sourceRoot + "/.local/native-package/producers/") || !/^[a-z]{1,64}$/.test(output.slice((sourceRoot + "/.local/native-package/producers/").length))) throw new Error("invalidProducerInput");
  const offlineInput = { ...input.offline, requestBytes: Buffer.from(input.offline.requestBytes), runtimeInputs: { channel: Buffer.from(input.offline.runtimeInputs.channel), archive: Buffer.from(input.offline.runtimeInputs.archive), copyright: Buffer.from(input.offline.runtimeInputs.copyright) } };
  await ordinaryRoot(sourceRoot); check(); await ordinaryRoot(dependenciesRoot); check();
  await ordinaryRoot(join(sourceRoot, ".local/native-package")); check();
  await mkdir(join(sourceRoot, ".local/native-package/producers"), { recursive: true, mode: 0o700 }); await ordinaryRoot(join(sourceRoot, ".local/native-package/producers")); check();
  await mkdir(output, { mode: 0o700 }); check(); // Exclusive; failed/partial products are retained, never reset.
  await writeOwned(join(output, "attempt.json"), Buffer.from(JSON.stringify({ schemaVersion: 1, sourceCommit, deadlineMs, status: "started" }) + "\n")); check();
  const commands: { name: string; command: ProducerCommand; stdoutSha256: string; stderrSha256: string; exitCode: number | null; signal: string | null; timedOut: boolean }[] = [];
  const run = async (name: string, command: ProducerCommand): Promise<ClosedCommand> => {
    check(); let received: unknown;
    try { received = await execute(command); } catch (error) { throw new MainProducerCommandError("unresolvedProducerCommand", command, error); }
    let result: ClosedCommand;
    try { result = closed(received, command.maxOutputBytes); } catch { throw new MainProducerCommandError("invalidProducerResult", command, received); }
    // Earned closed output remains available even when the original clock expires after await.
    await writeOwned(join(output, name + ".stdout"), result.stdout); await writeOwned(join(output, name + ".stderr"), result.stderr);
    commands.push({ name, command, stdoutSha256: sha256(result.stdout), stderrSha256: sha256(result.stderr), exitCode: result.exitCode, signal: result.signal, timedOut: result.timedOut });
    await writeOwned(join(output, name + ".command.json"), Buffer.from(JSON.stringify(commands[commands.length - 1]) + "\n"));
    if (command.kind === "container") {
      try { check(); } catch { throw new MainProducerCommandError("producerDeadline", command, result); }
      if (result.exitCode !== 0 || result.signal !== null || result.timedOut) throw new MainProducerCommandError("producerCommandFailed", command, result);
    } // Git's shared owner admits its exact result and clock, retaining the actual closed outcome.
    return result;
  };
  let gitSequence = 0;
  const project = await captureHostProject(sourceRoot, sourceCommit, { deadlineMs, now: () => { check(); return previous; }, execute: command => run(`git-${String(++gitSequence).padStart(4, "0")}`, { ...command, kind: "git" }) }); check();
  const offline = await verifyOfflineSources({ ...offlineInput, project, budget: { deadlineMs, now: () => { check(); return previous; } } }); check();
  await prepareMainSource(output, project, offline, () => { check(); }); check();
  const channelFile = project.files.find(file => file.name === "rust-toolchain.toml");
  const channel = channelFile && /^channel = "(nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})"$/m.exec(channelFile.bytes.toString("utf8"))?.[1];
  if (!channel) throw new Error("invalidProducerToolchain");
  const sourceMount = { source: join(output, "source"), target: "/workspace", readOnly: true };
  const container = (image: string, executable: string, args: readonly string[], mounts: ProducerContainerCommand["mounts"], limit: number, timeout: number): ProducerContainerCommand => Object.freeze({
    kind: "container", image, platform: "linux/arm64", network: "none", cpus: 2, readOnly: true, pull: "never", tmpfs: Object.freeze(["/tmp"] as const), executable,
    args: Object.freeze([...args]), cwd: "/workspace", env: Object.freeze({ ...environment, RUSTUP_TOOLCHAIN: channel }),
    mounts: Object.freeze(mounts.map(mount => Object.freeze({ ...mount }))), deadlineMs, timeoutMs: Math.min(timeout, check()), maxOutputBytes: limit,
  });
  const metadata = await run("sdk-metadata", container(NATIVE_COMPILER_IMAGE, "/bin/sh", ["-ec", sdkMetadata], [sourceMount], 65536, 30000));
  if (metadata.stderr.length) throw new Error("invalidCompilerMetadata");
  const compiler = parseMetadata(metadata.stdout, offline.runtime.version, offline.runtime.commit, channel);
  const sdkMounts = [sourceMount, { source: join(output, "vendor"), target: "/vendor", readOnly: true }, { source: join(output, "rust"), target: compiler.sysroot + "/lib/rustlib/src/rust", readOnly: true }, { source: join(output, "cargo"), target: "/cargo", readOnly: true }, { source: join(output, "target"), target: "/target", readOnly: false }];
  await run("cargo-main", container(NATIVE_COMPILER_IMAGE, "/usr/local/cargo/bin/cargo", mainArgs, sdkMounts, 8 * 1024 * 1024, 20 * 60 * 1000));
  const executablePath = join(output, "target/arm-unknown-linux-gnueabi/release/criterion-unofficial");
  check(); const executable = await readInput(executablePath, MAX_EXECUTABLE_BYTES); check(); admitExecutable(executable);
  const receipt = receiptForMain(project, executable); const receiptBytes = Buffer.from(JSON.stringify(receipt, null, 2) + "\n");
  const snapshot = join(output, "source");
  await admitBuildReceipt(receiptBytes, executable, path => readInput(join(snapshot, path), MAX_EXECUTABLE_BYTES, 0)); check();
  const readelfMounts = [sourceMount, { source: join(output, "target"), target: "/target", readOnly: true }];
  for (const [name, args] of [["readelf-headers", ["-h", "-l", "-A"]], ["readelf-dynamic", ["-d"]], ["readelf-versions", ["-V"]]] as const) {
    await run(name, container(NATIVE_COMPILER_IMAGE, "/usr/bin/readelf", [...args, "/target/arm-unknown-linux-gnueabi/release/criterion-unofficial"], readelfMounts, 256 * 1024, 30000));
    if (sha256(await readInput(executablePath, MAX_EXECUTABLE_BYTES)) !== receipt.executableSha256) throw new Error("producerExecutableChanged"); check();
  }
  await verifyMaterialized(snapshot, project.files, () => { check(); });
  const packageRoot = join(output, "package"); await mkdir(join(packageRoot, "input"), { mode: 0o700 });
  await writeOwned(join(packageRoot, "input/criterion-unofficial"), executable, 0o755);
  await writeOwned(join(packageRoot, "input/build-receipt.json"), receiptBytes, 0o644); check();
  const packageMounts = [sourceMount, { source: dependenciesRoot, target: "/workspace/tools/player-probe/node_modules", readOnly: true }, { source: packageRoot, target: "/workspace/.local/native-package", readOnly: false }];
  await run("package-compile", container(PACKAGING_IMAGE, "/usr/local/bin/node", ["tools/player-probe/node_modules/typescript/bin/tsc", "-p", "tools/package-native/tsconfig.json"], packageMounts, 1024 * 1024, 60000));
  const program = packageWorkerLaunch({ sourceRoot: "/workspace", outputDirectory: "/workspace/.local/native-package/exports/main", executablePath: "/workspace/.local/native-package/input/criterion-unofficial", receiptPath: "/workspace/.local/native-package/input/build-receipt.json", modulePath: "/workspace/.local/native-package/compiled/tools/package-native/package-phase.js", image: PACKAGING_IMAGE, deadlineMs });
  await writeOwned(join(packageRoot, "package-launcher.cjs"), Buffer.from(program), 0o644); check();
  const packaged = await run("package-main", container(PACKAGING_IMAGE, "/usr/local/bin/node", ["/workspace/.local/native-package/package-launcher.cjs"], packageMounts, 512 * 1024, 180000));
  if (packaged.stderr.length !== 0) throw new Error("invalidProducerPackage");
  const result = await admitPackageResult(packaged.stdout, packageRoot, receiptBytes, receipt); check();
  const workerBytes = await readInput(join(packageRoot, "package-worker.cjs"), 32768); check();
  await verifyMaterialized(snapshot, project.files, () => { check(); });
  if (sha256(await readInput(executablePath, MAX_EXECUTABLE_BYTES)) !== receipt.executableSha256) throw new Error("producerExecutableChanged"); check();
  const artifact = (path: string, bytes: Buffer) => ({ path, sha256: sha256(bytes), bytes: bytes.length });
  const buildReceipt = artifact(join(packageRoot, "input/build-receipt.json"), receiptBytes);
  const executableArtifact = artifact(executablePath, executable);
  const sealBytes = Buffer.from(JSON.stringify({ schemaVersion: 1, status: "development", sourceCommit, originalDeadlineMs: deadlineMs,
    source: { commitSha256: sha256(project.commit), files: project.files.map(file => ({ name: file.name, mode: file.mode, bytes: file.bytes.length, sha256: sha256(file.bytes) })) },
    compiler: { requiredImage: NATIVE_COMPILER_IMAGE, ...compiler }, offline: { requestSha256: sha256(offlineInput.requestBytes), channelManifestSha256: sha256(offlineInput.runtimeInputs.channel), copyrightSha256: sha256(offlineInput.runtimeInputs.copyright), runtime: offline.runtime, runtimeInventory: offline.runtimeInventory, registry: offline.registry.map(crate => ({ name: crate.name, version: crate.version, archiveSha256: crate.archiveSha256, inventory: crate.inventory })) },
    commands, packagingLauncherSha256: sha256(Buffer.from(program)), generatedPackagingWorkerSha256: sha256(workerBytes), buildReceipt, executable: executableArtifact, package: result,
    limits: ["Development source/input attribution only; no reproducibility or source-license admission.", "ELF/readelf facts are not runtime, CPU instruction, symbol resolution or playback admission."],
  }, null, 2) + "\n");
  check(); const sealPath = join(output, "producer-seal.json"); await writeOwned(sealPath, sealBytes, 0o644); check();
  return { schemaVersion: 1, status: "development", sourceCommit, buildReceipt, executable: executableArtifact, package: result, producerSeal: artifact(sealPath, sealBytes) };
}

async function admitPackageResult(bytes: Buffer, root: string, receiptBytes: Buffer, receipt: NativeMainExport["build"]): Promise<NativeMainExport> {
  let value: unknown; try { value = JSON.parse(bytes.toString("utf8")); } catch { throw new Error("invalidProducerPackage"); }
  const item = record(value);
  if (Object.keys(item).sort().join() !== ["schemaVersion", "status", "identityScope", "build", "receiptSha256", "ipk", "packageSeal", "executableRequirements", "audit"].sort().join()
    || item.schemaVersion !== 1 || item.status !== "development" || item.identityScope !== "build-receipt-content-only" || JSON.stringify(item.build) !== JSON.stringify(receipt)
    || item.receiptSha256 !== sha256(receiptBytes)) throw new Error("invalidProducerPackage");
  const map = async (data: unknown, expected: string, limit: number) => {
    const object = record(data);
    if (Object.keys(object).sort().join() !== ["path", "sha256", "bytes"].sort().join() || object.path !== "/workspace/.local/native-package/" + expected || typeof object.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(object.sha256)
      || !Number.isSafeInteger(object.bytes) || (object.bytes as number) < 1 || (object.bytes as number) > limit) throw new Error("invalidProducerPackage");
    const path = join(root, expected); const content = await readInput(path, limit);
    if (content.length !== object.bytes || sha256(content) !== object.sha256) throw new Error("invalidProducerPackage");
    return { mapped: { path, sha256: object.sha256, bytes: content.length }, content };
  };
  const ipk = await map(item.ipk, `exports/main/ipks/${APP_ID}_${VERSION}_arm.ipk`, MAX_IPK_BYTES);
  const seal = await map(item.packageSeal, "exports/main/package-seal.json", 2 * 1024 * 1024);
  const sealValue = record(JSON.parse(seal.content.toString("utf8")) as unknown);
  if (sealValue.schemaVersion !== 1 || sealValue.status !== "development" || sealValue.identityScope !== "build-receipt-content-only" || sealValue.appId !== APP_ID || sealValue.version !== VERSION || sealValue.requiredPackagingImage !== PACKAGING_IMAGE) throw new Error("invalidProducerPackage");
  for (const key of ["build", "receiptSha256", "ipk", "executableRequirements", "audit"]) if (JSON.stringify(item[key]) !== JSON.stringify(sealValue[key])) throw new Error("invalidProducerPackage");
  const requirements = record(item.executableRequirements);
  if (Object.keys(requirements).sort().join() !== ["appId", "executableSha256", "neededSonames"].sort().join() || requirements.appId !== APP_ID || requirements.executableSha256 !== receipt.executableSha256
    || !Array.isArray(requirements.neededSonames) || requirements.neededSonames.length < 1 || requirements.neededSonames.length > 10
    || requirements.neededSonames.some(name => typeof name !== "string" || name.length > 128 || !/^[A-Za-z0-9_+.-]+\.so(?:\.[A-Za-z0-9_+.-]+)*$/.test(name))
    || new Set(requirements.neededSonames).size !== requirements.neededSonames.length) throw new Error("invalidProducerPackage");
  const audit = record(item.audit), auditFiles = record(audit.files);
  if (Object.keys(audit).sort().join() !== ["appId", "controlSha256", "files"].sort().join()
    || Object.keys(auditFiles).sort().join() !== [...PAYLOAD_NAMES].sort().join()
    || audit.appId !== APP_ID || typeof audit.controlSha256 !== "string" || !/^[a-f0-9]{64}$/.test(audit.controlSha256)
    || record(auditFiles["criterion-unofficial"]).sha256 !== receipt.executableSha256) throw new Error("invalidProducerPackage");
  const files: NativeMainExport["audit"]["files"] = {};
  for (const name of PAYLOAD_NAMES) {
    const file = record(auditFiles[name]); const mode = name === "criterion-unofficial" ? 0o755 : 0o644;
    if (Object.keys(file).sort().join() !== ["sha256", "bytes", "mode"].sort().join() || typeof file.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(file.sha256)
      || !Number.isSafeInteger(file.bytes) || (file.bytes as number) < 1 || (file.bytes as number) > MAX_EXECUTABLE_BYTES || file.mode !== (name === "criterion-unofficial" ? 0o755 : 0o644)) throw new Error("invalidProducerPackage");
    files[name] = { sha256: file.sha256, bytes: file.bytes as number, mode };
  }
  return { schemaVersion: 1, status: "development", identityScope: "build-receipt-content-only", build: receipt, receiptSha256: sha256(receiptBytes),
    ipk: ipk.mapped, packageSeal: seal.mapped,
    executableRequirements: { appId: APP_ID, executableSha256: receipt.executableSha256, neededSonames: requirements.neededSonames as string[] }, audit: { appId: APP_ID, controlSha256: audit.controlSha256, files } };
}
