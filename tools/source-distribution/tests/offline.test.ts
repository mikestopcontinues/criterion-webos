import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, link, mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { gzipSync } from "node:zlib";
import { tarFixture, type TarEntry } from "../../player-probe/tests/fixture-tar.js";
import type { ProjectSnapshot, SourceFile } from "../src/project.js";
import { verifyOfflineSources, type OfflineSourceInput } from "../src/offline.js";

const sha = (bytes: Buffer): string => createHash("sha256").update(bytes).digest("hex");
const gitHash = (kind: string, bytes: Buffer): string => createHash("sha1").update(`${kind} ${bytes.length}\0`).update(bytes).digest("hex");
type GitTree = Map<string, SourceFile | GitTree>;
export function projectFixture(files: SourceFile[]): ProjectSnapshot {
  const root: GitTree = new Map();
  for (const file of files) {
    const parts = file.name.split("/"); let tree = root;
    for (const part of parts.slice(0, -1)) {
      const previous = tree.get(part); const child = previous instanceof Map ? previous : new Map<string, SourceFile | GitTree>();
      tree.set(part, child); tree = child;
    }
    tree.set(parts[parts.length - 1] ?? "", file);
  }
  const encode = (tree: GitTree): Buffer => Buffer.concat([...tree].sort(([a, av], [b, bv]) => Buffer.compare(Buffer.from(a + (av instanceof Map ? "/" : "")), Buffer.from(b + (bv instanceof Map ? "/" : "")))).flatMap(([name, entry]) => [Buffer.from(`${entry instanceof Map ? "40000" : entry.mode === 0o755 ? "100755" : "100644"} ${name}\0`), Buffer.from(gitHash(entry instanceof Map ? "tree" : "blob", entry instanceof Map ? encode(entry) : entry.bytes), "hex")]));
  const commit = Buffer.from(`tree ${gitHash("tree", encode(root))}\nauthor Public Fixture <fixture@example.invalid> 0 +0000\ncommitter Public Fixture <fixture@example.invalid> 0 +0000\n\nOffline source fixture\n`);
  return { revision: gitHash("commit", commit), commit, files };
}
const packageEntries = (name: string): TarEntry[] => [
  { name: `${name}/`, mode: 0o755, type: "5", bytes: Buffer.alloc(0) },
  { name: `${name}/src/`, mode: 0o755, type: "5", bytes: Buffer.alloc(0) },
  { name: `${name}/Cargo.toml`, mode: 0o644, bytes: Buffer.from("[package]\nname = \"literal\"\n") },
  { name: `${name}/src/lib.rs`, mode: 0o644, bytes: Buffer.from("pub const LITERAL: u8 = 7;\n") },
];
const crates = new Map([
  ["example-1.2.3.crate", gzipSync(tarFixture(packageEntries("example-1.2.3")))],
  ["runtime-only-1.0.0.crate", gzipSync(tarFixture(packageEntries("runtime-only-1.0.0")))],
]);
function lock(name: string, version: string): Buffer {
  const archive = crates.get(`${name}-${version}.crate`); assert.ok(archive);
  return Buffer.from(`version = 4\n\n[[package]]\nname = "${name}"\nversion = "${version}"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "${sha(archive)}"\n`);
}
const runtimeNames = ["library/Cargo.toml", "library/std/src/lib.rs", "library/core/src/lib.rs", "library/compiler-builtins/Cargo.toml", "library/compiler-builtins/LICENSE.txt", "library/stdarch/LICENSE-MIT", "library/stdarch/LICENSE-APACHE", "library/portable-simd/LICENSE-MIT", "library/portable-simd/LICENSE-APACHE", "src/llvm-project/libunwind/LICENSE.TXT"];
const runtimePrefix = "rust-src-nightly/rust-src/lib/rustlib/src/rust/";
const runtimeFiles = [...runtimeNames.map((name) => ({ name, bytes: Buffer.from("literal runtime source\n"), mode: 0o644 as const })), { name: "library/Cargo.lock", bytes: lock("runtime-only", "1.0.0"), mode: 0o644 as const }];
const runtimeEntries: TarEntry[] = [
  ...runtimeFiles.map((file) => ({ ...file, name: runtimePrefix + file.name })),
  ...["LICENSE-MIT", "LICENSE-APACHE", "COPYRIGHT", "git-commit-hash", "version"].map((name) => ({ name: `rust-src-nightly/${name}`, mode: 0o644, bytes: Buffer.from(name === "git-commit-hash" ? "4c9d2bfe4ad7a65669098754964aaebe0ec1ced2" : name === "version" ? "1.98.0-nightly (4c9d2bfe4 2026-07-01)" : "literal runtime license\n") })),
];
const archive = gzipSync(tarFixture(runtimeEntries));
const channel = Buffer.from(`manifest-version = "2"\ndate = "2026-07-02"\n[pkg.rust-src]\nversion = "1.98.0-nightly (4c9d2bfe4 2026-07-01)"\n[pkg.rust-src.target."*"]\navailable = true\nurl = "https://static.rust-lang.org/dist/2026-07-02/rust-src-nightly.tar.gz"\nhash = "${sha(archive)}"\n`);
const copyright = Buffer.from("literal runtime copyright\n");
const project = projectFixture([
  ...["Cargo.toml", "LICENSE", "NOTICES.md", ".cargo/config.toml", "dev", "crates/criterion-app/Cargo.toml", "crates/criterion-app/src/main.rs", "tools/native-caller-probe/Cargo.toml", "tools/player-probe/broker/Cargo.toml"].map((name) => ({ name, bytes: Buffer.from("public literal source\n"), mode: 0o644 as const })),
  { name: "Cargo.lock", bytes: lock("example", "1.2.3"), mode: 0o644 },
  { name: "Dockerfile", bytes: Buffer.from("https://github.com/webosbrew/native-toolchain/releases/download/webos-d7ed7ee.6/arm-webos-linux-gnueabi_sdk-buildroot_linux-aarch64.tar.bz2\n45a2d12ff557457d92cde4fddaa77a6f1090fca03adc43bb74397e5e0c379501\n"), mode: 0o644 },
  { name: "rust-toolchain.toml", bytes: Buffer.from('[toolchain]\nchannel = "nightly-2026-07-02"\n'), mode: 0o644 },
]);
const requestBytes = Buffer.from(JSON.stringify({ schemaVersion: 1, sourceCommit: project.revision, runtime: { channelManifestSha256: sha(channel), sourceArchiveSha256: sha(archive), copyrightSha256: sha(copyright) }, exclusions: [
  ...["stock-sdl2", "stock-egl", "stock-gles2", "stock-lunaservice2", "stock-glib2", "stock-libc", "stock-libm", "stock-libpthread", "stock-librt", "stock-libdl", "stock-libgcc-s"].map((component) => ({ component, basis: "system-library", reason: "Public fixture owner declaration only." })),
  ...["glibc-startup-nonshared", "gcc-crtstuff"].map((component) => ({ component, basis: "linked-file-permission", reason: "Public fixture owner declaration only." })),
] }));
async function write(root: string, name: string, bytes: Buffer): Promise<void> {
  const path = join(root, name); await mkdir(join(path, ".."), { recursive: true }); await writeFile(path, bytes, { mode: 0o644 });
}
export async function withFixture(run: (input: OfflineSourceInput) => Promise<void>): Promise<void> {
  const root = await mkdtemp(join(tmpdir(), "criterion-offline-source-"));
  const installedRuntimeRoot = join(root, "rust"); const registryRoot = join(root, "vendor");
  try {
    for (const file of runtimeFiles) await write(installedRuntimeRoot, file.name, file.bytes);
    for (const [name, raw] of crates) {
      const crateRoot = join(registryRoot, name.replace(/\.crate$/, ""));
      const files: Record<string, string> = {};
      for (const entry of packageEntries(name.replace(/\.crate$/, "")).filter((entry) => entry.type !== "5")) {
        const relative = entry.name.slice(name.length - ".crate".length + 1); files[relative] = sha(entry.bytes); await write(crateRoot, relative, entry.bytes);
      }
      await write(crateRoot, ".cargo-checksum.json", Buffer.from(JSON.stringify({ files, package: sha(raw) })));
    }
    await run({ project, requestBytes, runtimeInputs: { archive, channel, copyright }, readRegistryArchive: async (name) => { const bytes = crates.get(name); if (!bytes) throw new Error("missingLiteralArchive"); return bytes; }, installedRuntimeRoot, registryRoot, budget: { deadlineMs: 60000, now: () => 0 } });
  } finally { await rm(root, { recursive: true, force: true }); }
}
async function replaceExampleArchive(input: OfflineSourceInput, entries: TarEntry[]): Promise<OfflineSourceInput> {
  const replacement = gzipSync(tarFixture(entries)); const original = crates.get("example-1.2.3.crate"); assert.ok(original);
  const changedProject = projectFixture(input.project.files.map((file) => ({ name: file.name, mode: 0o644 as const, bytes: file.name === "Cargo.lock" ? Buffer.from(file.bytes.toString().replace(sha(original), sha(replacement))) : file.bytes })));
  const changedRequest = JSON.parse(input.requestBytes.toString()) as { sourceCommit: string }; changedRequest.sourceCommit = changedProject.revision;
  const files: Record<string, string> = {};
  for (const entry of packageEntries("example-1.2.3").filter((entry) => entry.type !== "5")) files[entry.name.slice("example-1.2.3/".length)] = sha(entry.bytes);
  await write(input.registryRoot, "example-1.2.3/.cargo-checksum.json", Buffer.from(JSON.stringify({ files, package: sha(replacement) })));
  return { ...input, project: changedProject, requestBytes: Buffer.from(JSON.stringify(changedRequest)), readRegistryArchive: async (name) => name === "example-1.2.3.crate" ? replacement : input.readRegistryArchive(name) };
}
test("authenticated runtime and complete lock-union vendor yield detached build source bytes", async () => {
  await withFixture(async (input) => {
    const result = await verifyOfflineSources(input);
    assert.deepEqual(result.registry.map((crate) => `${crate.name}-${crate.version}`), ["example-1.2.3", "runtime-only-1.0.0"]);
    assert.equal(result.runtimeFiles.find((file) => file.name === "library/core/src/lib.rs")?.bytes.toString(), "literal runtime source\n");
    assert.equal(result.runtimeFiles.find((file) => file.name === "src/llvm-project/libunwind/LICENSE.TXT")?.bytes.toString(), "literal runtime source\n");
    assert.equal(result.registry[0]?.files.find((file) => file.name === "src/lib.rs")?.bytes.toString(), "pub const LITERAL: u8 = 7;\n");
    assert.equal(result.runtime.commit, "4c9d2bfe4ad7a65669098754964aaebe0ec1ced2");
    assert.deepEqual(result.registry[0]?.directories, ["src"]);
    assert.deepEqual(result.registry[0]?.inventory.find((file) => file.name === "src/lib.rs"), { name: "src/lib.rs", mode: 0o644, size: 27, sha256: "4f5cc1e457ddd27d50c7bea9e13282f783061fed68d5b7514db58fca6b7362bb" });
    await write(input.installedRuntimeRoot, "library/core/src/lib.rs", Buffer.from("changed after admission\n"));
    assert.equal(result.runtimeFiles.find((file) => file.name === "library/core/src/lib.rs")?.bytes.toString(), "literal runtime source\n");
    await write(input.registryRoot, "example-1.2.3/src/lib.rs", Buffer.from("changed after admission\n"));
    assert.equal(result.registry[0]?.files.find((file) => file.name === "src/lib.rs")?.bytes.toString(), "pub const LITERAL: u8 = 7;\n");
  });
});
test("runtime bytes, omitted sources and extra source directories refuse offline binding", async () => {
  const mutations = [
    async (input: OfflineSourceInput): Promise<void> => write(input.installedRuntimeRoot, "library/core/src/lib.rs", Buffer.from("tampered runtime source\n")),
    async (input: OfflineSourceInput): Promise<void> => rm(join(input.installedRuntimeRoot, "library/Cargo.lock")),
    async (input: OfflineSourceInput): Promise<void> => mkdir(join(input.installedRuntimeRoot, "library/extra")),
  ];
  for (const mutate of mutations) await withFixture(async (input) => {
    await mutate(input); await assert.rejects(() => verifyOfflineSources(input), /invalidOfflineSource/);
  });
});
test("vendor byte tampering, forged Cargo checksums and missing runtime-only crates refuse offline binding", async () => {
  const mutations = [
    async (input: OfflineSourceInput): Promise<void> => write(input.registryRoot, "example-1.2.3/src/lib.rs", Buffer.from("pub const LITERAL: u8 = 9;\n")),
    async (input: OfflineSourceInput): Promise<void> => write(input.registryRoot, "example-1.2.3/.cargo-checksum.json", Buffer.from('{"files":{},"package":null}')),
    async (input: OfflineSourceInput): Promise<void> => rm(join(input.registryRoot, "runtime-only-1.0.0"), { recursive: true }),
    async (input: OfflineSourceInput): Promise<void> => write(input.registryRoot, "example-1.2.3/.cargo-ok", Buffer.from("v1")),
    async (input: OfflineSourceInput): Promise<void> => write(input.registryRoot, "unlocked-1.0.0/Cargo.toml", Buffer.from("extra crate\n")),
  ];
  for (const mutate of mutations) await withFixture(async (input) => {
    await mutate(input); await assert.rejects(() => verifyOfflineSources(input), /invalidOfflineSource/);
  });
});
test("source trees reject symlinks, hard links and changed executable modes", async () => {
  const mutations = [
    async (input: OfflineSourceInput): Promise<void> => { const path = join(input.installedRuntimeRoot, "library/core/src/lib.rs"); await rm(path); await symlink("../../../std/src/lib.rs", path); },
    async (input: OfflineSourceInput): Promise<void> => { const path = join(input.registryRoot, "example-1.2.3/src/lib.rs"); await link(path, join(input.registryRoot, "example-1.2.3/src/linked.rs")); },
    async (input: OfflineSourceInput): Promise<void> => chmod(join(input.registryRoot, "example-1.2.3/src/lib.rs"), 0o755),
    async (input: OfflineSourceInput): Promise<void> => { const path = input.installedRuntimeRoot; await symlink(path, path + "-alias"); Object.defineProperty(input, "installedRuntimeRoot", { value: path + "-alias" }); },
  ];
  for (const mutate of mutations) await withFixture(async (input) => {
    await mutate(input); await assert.rejects(() => verifyOfflineSources(input), /invalidOfflineSource/);
  });
});
test("wrong raw archive checksums refuse before trusting an otherwise matching vendor tree", async () => {
  await withFixture(async (input) => {
    await assert.rejects(() => verifyOfflineSources({ ...input, readRegistryArchive: async () => Buffer.from("opaque checksum-forged cache bytes\n") }), /invalidOfflineSource/);
    const changed = JSON.parse(input.requestBytes.toString("utf8")) as { sourceCommit: string };
    changed.sourceCommit = "0".repeat(40);
    await assert.rejects(() => verifyOfflineSources({ ...input, requestBytes: Buffer.from(JSON.stringify(changed)) }), /invalidSourceInput/);
  });
});
test("checksum-admitted raw crates still refuse linked, special, duplicate, aliased and traversing archive members", async () => {
  const additions: TarEntry[] = [
    { name: "example-1.2.3/src/linked.rs", mode: 0o644, type: "2", bytes: Buffer.alloc(0) },
    { name: "example-1.2.3/src/hard.rs", mode: 0o644, type: "1", bytes: Buffer.alloc(0) },
    { name: "example-1.2.3/src/fifo", mode: 0o644, type: "6", bytes: Buffer.alloc(0) },
    { name: "example-1.2.3/src/lib.rs", mode: 0o644, bytes: Buffer.from("pub const LITERAL: u8 = 7;\n") },
    { name: "example-1.2.3/SRC/", mode: 0o755, type: "5", bytes: Buffer.alloc(0) },
    { name: "example-1.2.3/../outside.rs", mode: 0o644, bytes: Buffer.from("escape\n") },
    { name: "another-1.2.3/src/lib.rs", mode: 0o644, bytes: Buffer.from("wrong archive prefix\n") },
  ];
  for (const addition of additions) await withFixture(async (input) => {
    const changed = await replaceExampleArchive(input, [...packageEntries("example-1.2.3"), addition]);
    await assert.rejects(() => verifyOfflineSources(changed), /invalidOfflineSource|invalidSourceInput/);
  });
});
test("empty source files, executable source files and explicit empty directories retain their archive identity", async () => {
  await withFixture(async (input) => {
    const script = Buffer.from("#!/bin/sh\nexit 0\n");
    const entries = [...packageEntries("example-1.2.3"),
      { name: "example-1.2.3/zero.txt", mode: 0o644, bytes: Buffer.alloc(0) },
      { name: "example-1.2.3/empty/", mode: 0o755, type: "5", bytes: Buffer.alloc(0) },
      { name: "example-1.2.3/scripts/generate.sh", mode: 0o755, bytes: script },
    ];
    const changed = await replaceExampleArchive(input, entries); const replacement = await changed.readRegistryArchive("example-1.2.3.crate");
    await write(input.registryRoot, "example-1.2.3/zero.txt", Buffer.alloc(0)); await mkdir(join(input.registryRoot, "example-1.2.3/empty"));
    await write(input.registryRoot, "example-1.2.3/scripts/generate.sh", script); await chmod(join(input.registryRoot, "example-1.2.3/scripts/generate.sh"), 0o755);
    const files: Record<string, string> = {};
    for (const entry of entries.filter((entry) => entry.type !== "5").sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name)))) files[entry.name.slice("example-1.2.3/".length)] = sha(entry.bytes);
    await write(input.registryRoot, "example-1.2.3/.cargo-checksum.json", Buffer.from(JSON.stringify({ files, package: sha(replacement) })));
    const result = await verifyOfflineSources(changed);
    assert.deepEqual(result.registry[0]?.directories, ["empty", "scripts", "src"]);
    assert.equal(result.registry[0]?.files.find((file) => file.name === "zero.txt")?.bytes.length, 0);
    assert.equal(result.registry[0]?.files.find((file) => file.name === "scripts/generate.sh")?.mode, 0o755);
    await rm(join(input.registryRoot, "example-1.2.3/empty"), { recursive: true }); await assert.rejects(() => verifyOfflineSources(changed), /invalidOfflineSource/);
  });
});
test("expired, nonfinite and regressed original clocks refuse source publication", async () => {
  await withFixture(async (input) => {
    for (const now of [() => 60000, () => Number.NaN, () => Number.POSITIVE_INFINITY, () => -1]) await assert.rejects(() => verifyOfflineSources({ ...input, budget: { deadlineMs: 60000, now } }), /sourceDeadline/);
    let reads = 0;
    await assert.rejects(() => verifyOfflineSources({ ...input, budget: { deadlineMs: 60000, now: () => ++reads < 3 ? 100 : 99 } }), /sourceDeadline/);
  });
});
test("original deadline expiry after GNU tar retains its actual closed command result", async () => {
  await withFixture(async (input) => {
    let reads = 0;
    await assert.rejects(() => verifyOfflineSources({ ...input, budget: { deadlineMs: 1000, now: () => ++reads < 3 ? 0 : 1000 } }), (error: unknown) => {
      assert.ok(error instanceof Error && error.message === "sourceDeadline");
      assert.ok("outcome" in error && typeof error.outcome === "object" && error.outcome !== null);
      const result = error.outcome as { status: number | null; stdout: Buffer; stderr: Buffer };
      assert.equal(result.status, 0); assert.ok(result.stdout.toString().includes("rust-src-nightly/git-commit-hash")); assert.equal(result.stderr.length, 0); return true;
    });
  });
});
