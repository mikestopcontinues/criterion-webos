import assert from "node:assert/strict";
import { test } from "node:test";
import { inspectIpk, normalizeIpk, type PackageFiles } from "../src/normalize.js";
import { UI_ID, VERSION } from "../src/protocol.js";
import { ipkFixture, tarFixture } from "./fixture-tar.js";

test("CLI writable generated metadata and directories become strictly sealed modes", async () => {
  const app = `usr/palm/applications/${UI_ID}/index.html`;
  const info = `usr/palm/packages/${UI_ID}/packageinfo.json`;
  const expected: PackageFiles = new Map([[app, { bytes: Buffer.from("fixture app\n"), mode: 0o644 }], [info, { bytes: Buffer.from("fixture package info\n"), mode: 0o644 }]]);
  const dirs = ["usr/", "usr/palm/", "usr/palm/applications/", `usr/palm/applications/${UI_ID}/`, "usr/palm/packages/", `usr/palm/packages/${UI_ID}/`];
  const data = tarFixture([
    ...dirs.map((name) => ({ name, bytes: Buffer.alloc(0), mode: 0o777, type: "5" })),
    ...[...expected].map(([name, file]) => ({ name, bytes: file.bytes, mode: name === info ? 0o666 : file.mode })),
  ]);
  const control = Buffer.from(`Package: ${UI_ID}\nVersion: ${VERSION}\nSection: misc\nPriority: optional\nArchitecture: all\nInstalled-Size: 8192\nMaintainer: N/A <nobody@example.com>\nDescription: This is a webOS application.\nwebOS-Package-Format-Version: 2\nwebOS-Packager-Version: x.y.x\n`);
  const raw = ipkFixture(data, tarFixture([{ name: "control", bytes: control, mode: 0o666 }]));
  const identity = { id: UI_ID, version: VERSION, architecture: "all" } as const;
  assert.throws(() => inspectIpk(raw, expected, identity), /invalidIpkPayload/);
  const normalized = await normalizeIpk(raw, expected, identity, "/workspace/.local/player-probe/normalization/test");
  assert.doesNotThrow(() => inspectIpk(normalized, expected, identity));
  assert.deepEqual(await normalizeIpk(raw, expected, identity, "/workspace/.local/player-probe/normalization/test"), normalized);
});
