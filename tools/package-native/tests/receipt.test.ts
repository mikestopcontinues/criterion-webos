import assert from "node:assert/strict";
import { test } from "node:test";
import { APP_ID, VERSION } from "../src/admission.js";
import { admitBuildReceipt, REQUIRED_SOURCES, sha256, type BuildReceipt } from "../src/receipt.js";
import { metadataFixture } from "./admission.test.js";

function inputs() {
  const executable = metadataFixture();
  const source = Buffer.from("frozen source bytes\n");
  const receipt: BuildReceipt = {
    schemaVersion: 1, appId: APP_ID, version: VERSION,
    target: "arm-unknown-linux-gnueabi", profile: "release", sourceCommit: "a".repeat(40),
    cargoLockSha256: sha256(source), executableSha256: sha256(executable),
    sourceSha256: Object.fromEntries(REQUIRED_SOURCES.map((path) => [path, sha256(source)])),
  };
  return { executable, source, receipt, read: async (_path: string) => source };
}

test("a source or lock mismatch prevents packaging a root-attested build", async () => {
  const input = inputs();
  const bytes = Buffer.from(JSON.stringify(input.receipt));
  assert.equal((await admitBuildReceipt(bytes, input.executable, input.read)).sourceCommit, "a".repeat(40));
  await assert.rejects(admitBuildReceipt(bytes, input.executable, async (path) => path === "Cargo.lock" ? Buffer.from("changed lock") : input.source), /sourceMismatch/);
});

for (const [name, change] of [
  ["wrong version", (r: BuildReceipt) => ({ ...r, version: "0.1.0-development" })],
  ["wrong app identity", (r: BuildReceipt) => ({ ...r, appId: "com.other.app" })],
  ["host target", (r: BuildReceipt) => ({ ...r, target: "aarch64-apple-darwin" })],
  ["unknown field", (r: BuildReceipt) => ({ ...r, privateSession: "unexpected" })],
  ["missing commit", (r: BuildReceipt) => ({ ...r, sourceCommit: "" })],
  ["invalid digest", (r: BuildReceipt) => ({ ...r, cargoLockSha256: "broken" })],
  ["missing frozen inputs", (r: BuildReceipt) => ({ ...r, sourceSha256: {} })],
  ["traversal input", (r: BuildReceipt) => ({ ...r, sourceSha256: { ...r.sourceSha256, "crates/../../private.rs": "a".repeat(64) } })],
  ["absolute input", (r: BuildReceipt) => ({ ...r, sourceSha256: { ...r.sourceSha256, "/tmp/input.rs": "a".repeat(64) } })],
  ["credential-like input", (r: BuildReceipt) => ({ ...r, sourceSha256: { ...r.sourceSha256, ".env": "a".repeat(64) } })],
] as const) {
  test(`receipt rejects ${name} before reading any source`, async () => {
    const input = inputs(); let reads = 0;
    await assert.rejects(admitBuildReceipt(Buffer.from(JSON.stringify(change(input.receipt))), input.executable, async () => { reads += 1; return input.source; }), /invalidReceipt/);
    assert.equal(reads, 0);
  });
}
test("changed executable and changed source are distinct admission failures", async () => {
  const input = inputs(); const bytes = Buffer.from(JSON.stringify(input.receipt));
  await assert.rejects(admitBuildReceipt(bytes, Buffer.from("another executable"), input.read), /executableMismatch/);
  await assert.rejects(admitBuildReceipt(bytes, input.executable, async (path) => path === "crates/criterion-app/src/main.rs" ? Buffer.from("modified code") : input.source), /sourceMismatch/);
});
test("malformed and oversized root receipts are rejected", async () => {
  const input = inputs();
  for (const bytes of [Buffer.from("{"), Buffer.from("[]"), Buffer.alloc(256 * 1024 + 1)]) await assert.rejects(admitBuildReceipt(bytes, input.executable, input.read), /invalidReceipt/);
});
