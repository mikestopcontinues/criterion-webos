import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { APPINFO, admitManifest } from "../src/manifest.js";

test("only the reviewed native development descriptor can enter the package", () => {
  const bytes = readFileSync("/workspace/tools/package-native/packaging/appinfo.json");
  assert.doesNotThrow(() => admitManifest(bytes));
  assert.throws(() => admitManifest(Buffer.from(JSON.stringify({ ...APPINFO, main: "other-executable" }))), /invalidManifest/);
});
test("MAIN declares only the fixed DB8 operation permission", () => {
  const bytes = readFileSync("/workspace/tools/package-native/packaging/appinfo.json");
  const manifest: unknown = JSON.parse(bytes.toString("utf8"));
  assert.equal(typeof manifest, "object");
  assert.deepEqual((manifest as Record<string, unknown>).requiredPermissions, ["database.operation"]);
  for (const permission of [undefined, [], ["database.operation", "securitykey.operation"], ["database.operation", "database.operation"]]) {
    assert.throws(() => admitManifest(Buffer.from(JSON.stringify({ ...APPINFO, requiredPermissions: permission }))), /invalidManifest/);
  }
});
test("an unreviewed permission, app identity, or prerelease version is rejected", () => {
  for (const value of [{ ...APPINFO, requiredPermissions: ["keymanager.operation"] }, { ...APPINFO, id: "com.other.app" }, { ...APPINFO, version: "0.1.0-development" }, { ...APPINFO, type: "web" }]) {
    assert.throws(() => admitManifest(Buffer.from(JSON.stringify(value))), /invalidManifest/);
  }
});
