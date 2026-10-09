import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { ipkFixture, tarFixture, type TarEntry as Entry } from "../../player-probe/tests/fixture-tar.js";
import { ipkMembers } from "../../player-probe/src/package.js";
import { APP_ID, MAX_IPK_BYTES, VERSION } from "../src/admission.js";
import { auditIpk, type Payload } from "../src/archive.js";
import { APPINFO, PAYLOAD_NAMES } from "../src/manifest.js";
import { metadataFixture } from "./admission.test.js";
import { readInput } from "../src/input.js";
import { sha256 } from "../src/receipt.js";

const appPrefix = `usr/palm/applications/${APP_ID}/`;
const packageInfo = `usr/palm/packages/${APP_ID}/packageinfo.json`;
export function archiveInputs() {
  const expected: Payload = new Map(PAYLOAD_NAMES.map((name: string) => [name, {
    bytes: name === "criterion-unofficial" ? metadataFixture() : name === "appinfo.json" ? Buffer.from(JSON.stringify(APPINFO)) : name === "NOTICES.md" ? readFileSync("/workspace/NOTICES.md") : Buffer.from(`${name}\n`),
    mode: name === "criterion-unofficial" ? 0o755 : 0o644,
  }]));
  const entries: Entry[] = [...expected].map(([name, file]) => ({ name: appPrefix + name, ...file }));
  entries.push({ name: packageInfo, bytes: Buffer.from(JSON.stringify({ id: APP_ID, version: VERSION, app: APP_ID }, null, 2) + "\n"), mode: 0o644 });
  const control = Buffer.from(`Package: ${APP_ID}\nVersion: ${VERSION}\nSection: misc\nPriority: optional\nArchitecture: arm\nInstalled-Size: 8192\nMaintainer: N/A <nobody@example.com>\nDescription: This is a webOS application.\nwebOS-Package-Format-Version: 2\nwebOS-Packager-Version: x.y.x\n`);
  const archive = (dataEntries = entries, controlBytes = control) => ipkFixture(tarFixture(dataEntries), tarFixture([{ name: "control", bytes: controlBytes, mode: 0o644 }]));
  return { expected, entries, control, archive };
}
test("the native archive seals the full notice compendium as exact nonexecutable bytes", async () => {
  const notices = await readInput("/workspace/NOTICES.md", 1024 * 1024);
  const input = archiveInputs();
  assert.deepEqual(auditIpk(input.archive(), input.expected).files["NOTICES.md"], { sha256: sha256(notices), bytes: notices.length, mode: 0o644 });
  const name = appPrefix + "NOTICES.md";
  const entry = input.entries.find((file) => file.name === name);
  assert.ok(entry);
  for (const entries of [
    input.entries.filter((file) => file.name !== name),
    input.entries.map((file) => file.name === name ? { ...file, bytes: Buffer.from("incomplete notices") } : file),
    [...input.entries, entry],
    input.entries.map((file) => file.name === name ? { ...file, mode: 0o666 } : file),
  ]) assert.throws(() => auditIpk(input.archive(entries), input.expected), /invalidIpkPayload/);
});
test("an added payload cannot pass the native archive seal", () => {
  const input = archiveInputs();
  assert.equal(auditIpk(input.archive(), input.expected).appId, APP_ID);
  const added = [...input.entries, { name: appPrefix + "unreviewed.js", bytes: Buffer.from("added"), mode: 0o644 }];
  assert.throws(() => auditIpk(input.archive(added), input.expected), /invalidIpkPayload/);
});
test("high-bit control bytes cannot masquerade as the reviewed ASCII metadata", () => {
  const input = archiveInputs(); const control = Buffer.from(input.control);
  control[0] = 0xd0; // lossy ASCII decoding would turn this into 'P'.
  assert.throws(() => auditIpk(input.archive(input.entries, control), input.expected), /invalidIpkPayload/);
});
test("a caller cannot weaken the native executable mode in the expected seal", () => {
  const input = archiveInputs(); const expected = new Map(input.expected);
  const main = expected.get("criterion-unofficial"); if (!main) throw new Error("invalidTestFixture");
  expected.set("criterion-unofficial", { bytes: main.bytes, mode: 0o644 });
  const entries = input.entries.map((e) => e.name.endsWith("/criterion-unofficial") ? { ...e, mode: 0o644 } : e);
  assert.throws(() => auditIpk(input.archive(entries), expected), /invalidIpkPayload/);
});
for (const [name, transform] of [
  ["changed content", (entries: Entry[]) => entries.map((e, i) => i === 0 ? { ...e, bytes: Buffer.from("changed descriptor") } : e)],
  ["missing payload", (entries: Entry[]) => entries.slice(1)],
  ["duplicate payload", (entries: Entry[]) => [...entries, entries[0] ?? { name: "bad", bytes: Buffer.alloc(0), mode: 0o644 }]],
  ["nonexecutable main", (entries: Entry[]) => entries.map((e) => e.name.endsWith("/criterion-unofficial") ? { ...e, mode: 0o644 } : e)],
  ["writable asset", (entries: Entry[]) => entries.map((e) => e.name.endsWith("/icon.png") ? { ...e, mode: 0o666 } : e)],
  ["setuid main", (entries: Entry[]) => entries.map((e) => e.name.endsWith("/criterion-unofficial") ? { ...e, mode: 0o4755 } : e)],
  ["traversal name", (entries: Entry[]) => entries.map((e, i) => i === 0 ? { ...e, name: appPrefix + "../appinfo.json" } : e)],
  ["absolute name", (entries: Entry[]) => entries.map((e, i) => i === 0 ? { ...e, name: "/" + e.name } : e)],
  ["unexpected directory", (entries: Entry[]) => [...entries, { name: "unexpected/", bytes: Buffer.alloc(0), mode: 0o755, type: "5" }]],
  ["writable directory", (entries: Entry[]) => [...entries, { name: "usr/", bytes: Buffer.alloc(0), mode: 0o777, type: "5" }]],
  ["symbolic link", (entries: Entry[]) => entries.map((e, i) => i === 0 ? { ...e, type: "2" } : e)],
  ["hard link", (entries: Entry[]) => entries.map((e, i) => i === 0 ? { ...e, type: "1" } : e)],
  ["FIFO", (entries: Entry[]) => entries.map((e, i) => i === 0 ? { ...e, type: "6" } : e)],
] as const) {
  test(`archive rejects ${name}`, () => {
    const input = archiveInputs();
    assert.throws(() => auditIpk(input.archive(transform(input.entries)), input.expected), /invalidIpkPayload/);
  });
}
test("wrong architecture, duplicate fields, unknown control fields and installer hooks are refused", () => {
  const input = archiveInputs();
  for (const control of [input.control.toString().replace("Architecture: arm", "Architecture: all"), input.control.toString().replace("Priority: optional", `Package: ${APP_ID}`), input.control.toString() + "Unexpected: field\n"]) {
    assert.throws(() => auditIpk(input.archive(input.entries, Buffer.from(control)), input.expected), /invalidIpkPayload/);
  }
  const controls = tarFixture([{ name: "control", bytes: input.control, mode: 0o644 }, { name: "postinst", bytes: Buffer.from("unreviewed"), mode: 0o755 }]);
  assert.throws(() => auditIpk(ipkFixture(tarFixture(input.entries), controls), input.expected), /invalidIpkPayload/);
});
test("corrupted and over-limit compressed data cannot enter the package", () => {
  const input = archiveInputs(); const corrupted = input.archive(); const data = ipkMembers(corrupted).get("data.tar.gz");
  if (!data) throw new Error("invalidTestFixture");
  data[data.length - 8] = (data[data.length - 8] ?? 0) ^ 0xff;
  assert.throws(() => auditIpk(corrupted, input.expected), /invalidIpkPayload/);
  const control = tarFixture([{ name: "control", bytes: input.control, mode: 0o644 }]);
  const valid = tarFixture(input.entries);
  // Valid files plus padding would pass tar inspection if the decompression cap vanished.
  const overLimit = Buffer.concat([valid, Buffer.alloc(MAX_IPK_BYTES + 1 - valid.length)]);
  assert.throws(() => auditIpk(ipkFixture(overLimit, control), input.expected), /invalidIpkPayload/);
});
test("content after tar end markers is still inspected", () => {
  const input = archiveInputs(); const added = tarFixture([{ name: appPrefix + "hidden", bytes: Buffer.from("added"), mode: 0o644 }]);
  const control = tarFixture([{ name: "control", bytes: input.control, mode: 0o644 }]);
  assert.throws(() => auditIpk(ipkFixture(Buffer.concat([tarFixture(input.entries), added]), control), input.expected), /invalidIpkPayload/);
});
test("normal PAX metadata is accepted and inherited tar options are excluded", () => {
  const input = archiveInputs(); const line = "SCHILY.dev=123\n";
  const pax = { name: "PaxHeader", bytes: Buffer.from(`${line.length + 3} ${line}`), mode: 0o644, type: "x" };
  const original = process.env.TAR_OPTIONS; process.env.TAR_OPTIONS = "--exclude=*";
  try { assert.doesNotThrow(() => auditIpk(input.archive([pax, ...input.entries]), input.expected)); }
  finally { if (original === undefined) delete process.env.TAR_OPTIONS; else process.env.TAR_OPTIONS = original; }
});
