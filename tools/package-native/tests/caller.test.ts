import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { CALLER_ID, CALLER_MAIN, CALLER_VERSION, CALLER_APPINFO, CALLER_PAYLOAD_NAMES, CALLER_IDENTITY, REQUIRED_CALLER_SOURCES, admitCallerReceipt, admitCallerManifest, callerArchiveFiles, auditCallerIpk, type CallerReceipt } from "../caller-build.js";
import { normalizeIpk, type PackageFiles } from "../../player-probe/src/normalize.js";
import { tarFixture, ipkFixture } from "../../player-probe/tests/fixture-tar.js";
import { admitBuildReceipt, sha256 } from "../src/receipt.js";
import { APP_ID } from "../src/admission.js";
import { metadataFixture } from "./admission.test.js";

function inputs() {
  const executable = metadataFixture(); const source = Buffer.from("frozen compile source\n");
  const receipt: CallerReceipt = {
    schemaVersion: 1, appId: CALLER_ID, version: CALLER_VERSION, target: "arm-unknown-linux-gnueabi", profile: "release", sourceCommit: "a".repeat(40),
    cargoLockSha256: sha256(source), executableSha256: sha256(executable), sourceSha256: Object.fromEntries(REQUIRED_CALLER_SOURCES.map((path) => [path, sha256(source)])),
  };
  return { executable, source, receipt, read: async (_path: string) => source };
}
test("caller receipt has its fixed identity and production admission stays separate", async () => {
  const input = inputs(); const bytes = Buffer.from(JSON.stringify(input.receipt));
  assert.equal((await admitCallerReceipt(bytes, input.executable, input.read)).appId, CALLER_ID);
  await assert.rejects(admitCallerReceipt(Buffer.from(JSON.stringify({ ...input.receipt, appId: APP_ID })), input.executable, input.read), /invalidCallerReceipt/);
  await assert.rejects(admitBuildReceipt(bytes, input.executable, input.read), /invalidReceipt/);
});
test("the fixed caller output supports strict archive normalization", async () => {
  const expected: PackageFiles = new Map(CALLER_PAYLOAD_NAMES.map((name) => [name, {
    bytes: name === CALLER_MAIN ? metadataFixture() : name === "appinfo.json" ? Buffer.from(JSON.stringify(CALLER_APPINFO)) : Buffer.from(name),
    mode: name === CALLER_MAIN ? 0o755 : 0o644,
  }]));
  const data = tarFixture([...callerArchiveFiles(expected)].map(([name, file]) => ({ name, ...file })));
  const control = Buffer.from(`Package: ${CALLER_ID}\nVersion: ${CALLER_VERSION}\nSection: misc\nPriority: optional\nArchitecture: arm\nInstalled-Size: 8192\nMaintainer: N/A <nobody@example.com>\nDescription: This is a webOS application.\nwebOS-Package-Format-Version: 2\nwebOS-Packager-Version: x.y.x\n`);
  const raw = ipkFixture(data, tarFixture([{ name: "control", bytes: control, mode: 0o644 }]));
  const normalized = await normalizeIpk(raw, callerArchiveFiles(expected), CALLER_IDENTITY, "/workspace/.local/native-caller-package/normalization");
  assert.equal(auditCallerIpk(normalized, expected).files[CALLER_MAIN]?.mode, 0o755);
  admitCallerManifest(Buffer.from(JSON.stringify(CALLER_APPINFO)));
});
for (const [name, change] of [
  ["unrelated identity", (r: CallerReceipt) => ({ ...r, appId: "com.other.native" })],
  ["prerelease version", (r: CallerReceipt) => ({ ...r, version: "0.1.0-development" })],
  ["host target", (r: CallerReceipt) => ({ ...r, target: "aarch64-apple-darwin" })],
  ["debug profile", (r: CallerReceipt) => ({ ...r, profile: "debug" })],
  ["unknown field", (r: CallerReceipt) => ({ ...r, privateSession: "unadmitted" })],
  ["missing required sources", (r: CallerReceipt) => ({ ...r, sourceSha256: {} })],
  ["traversal", (r: CallerReceipt) => ({ ...r, sourceSha256: { ...r.sourceSha256, "tools/native-caller-probe/../../private.rs": "a".repeat(64) } })],
  ["unrelated tool source", (r: CallerReceipt) => ({ ...r, sourceSha256: { ...r.sourceSha256, "tools/player-probe/broker/src/main.rs": "a".repeat(64) } })],
] as const) {
  test(`caller receipt rejects ${name} before source reads`, async () => {
    const input = inputs(); let reads = 0;
    await assert.rejects(admitCallerReceipt(Buffer.from(JSON.stringify(change(input.receipt))), input.executable, async () => { reads += 1; return input.source; }), /invalidCallerReceipt/);
    assert.equal(reads, 0);
  });
}
test("caller receipt binds current locked dependencies, exact executable and native ABI source", async () => {
  const input = inputs(); const bytes = Buffer.from(JSON.stringify(input.receipt));
  await assert.rejects(admitCallerReceipt(bytes, Buffer.from("changed ELF"), input.read), /callerExecutableMismatch/);
  for (const changed of ["Cargo.lock", "tools/native-caller-probe/abi.c"]) await assert.rejects(admitCallerReceipt(bytes, input.executable, async (path) => path === changed ? Buffer.from("changed source") : input.source), /callerSourceMismatch/);
});
test("caller receipt rejects malformed, oversized and invalid hash attestations", async () => {
  const input = inputs();
  for (const bytes of [Buffer.from("{"), Buffer.from("null"), Buffer.from("[]"), Buffer.alloc(256 * 1024 + 1), Buffer.from(JSON.stringify({ ...input.receipt, executableSha256: "invalid" }))]) {
    await assert.rejects(admitCallerReceipt(bytes, input.executable, input.read), /invalidCallerReceipt/);
  }
});
test("caller metadata is exact and never admits guessed privileges or another main", () => {
  admitCallerManifest(readFileSync("/workspace/tools/native-caller-probe/packaging/appinfo.json"));
  for (const record of [{ ...CALLER_APPINFO, main: "other-executable" }, { ...CALLER_APPINFO, id: APP_ID }, { ...CALLER_APPINFO, requiredPermissions: ["keymanager.operation"] }]) {
    assert.throws(() => admitCallerManifest(Buffer.from(JSON.stringify(record))), /invalidCallerManifest/);
  }
});
test("caller payload cannot weaken main mode or add hidden files", () => {
  const files = new Map<string, { bytes: Buffer; mode: 0o644 | 0o755 }>(CALLER_PAYLOAD_NAMES.map((name) => [name, { bytes: Buffer.from(name), mode: name === CALLER_MAIN ? 0o755 : 0o644 }]));
  files.set(CALLER_MAIN, { bytes: metadataFixture(), mode: 0o644 });
  assert.throws(() => callerArchiveFiles(files), /invalidCallerPayload/);
  files.set(CALLER_MAIN, { bytes: metadataFixture(), mode: 0o755 }); files.set("unexpected", { bytes: Buffer.alloc(0), mode: 0o644 });
  assert.throws(() => callerArchiveFiles(files), /invalidCallerPayload/);
});
