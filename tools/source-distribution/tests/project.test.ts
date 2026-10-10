import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { captureProject, projectDescriptors, safeName, type GitReader } from "../src/project.js";
type Fixture = { revision: string; commit: string; tree: string; blobs: Record<string, string> };
/** Frozen public fixture captured with stock host Git; no real account values. */
const fixture = JSON.parse(readFileSync("tools/source-distribution/tests/fixtures/project.json", "utf8")) as Fixture;
export function fixtureGit(changes: Partial<Record<Parameters<GitReader>[0], Buffer>> = {}): GitReader {
  return (operation, object) => {
    const changed = changes[operation]; if (changed) return changed;
    if (operation === "head") return Buffer.from(fixture.revision + "\n");
    if (operation === "status") return Buffer.alloc(0);
    if (operation === "commit") return Buffer.from(fixture.commit, "base64");
    if (operation === "tree") return Buffer.from(fixture.tree, "base64");
    const blob = object && fixture.blobs[object]; if (!blob) throw new Error("missingFixtureBlob"); return Buffer.from(blob, "base64");
  };
}
export const fixtureRevision = fixture.revision;
test("the host Git export preserves safe tracked project logs and exact source bytes", () => {
  const snapshot = captureProject(fixtureRevision, fixtureGit());
  assert.deepEqual(snapshot.files.map((file) => [file.name, file.bytes.toString(), file.mode]), [
    ["Cargo.lock", "version = 4\n", 0o644], ["logs/2026-10-09.md", "public engineering log\n", 0o644],
  ]);
});
test("a truncated Git file listing cannot seal the same committed revision", () => {
  const original = fixtureGit()("tree").toString();
  const shortened = Buffer.from(original.split("\0").slice(0, 1).join("\0") + "\0");
  assert.throws(() => captureProject(fixtureRevision, fixtureGit({ tree: shortened })), /invalidProjectSource/);
});
test("even a committed private path rejects the entire source export", () => {
  const privateFixture = JSON.parse(readFileSync("tools/source-distribution/tests/fixtures/private-project.json", "utf8")) as Fixture;
  const read: GitReader = (operation, object) => {
    if (operation === "head") return Buffer.from(privateFixture.revision + "\n");
    if (operation === "status") return Buffer.alloc(0);
    if (operation === "commit") return Buffer.from(privateFixture.commit, "base64");
    if (operation === "tree") return Buffer.from(privateFixture.tree, "base64");
    const blob = object && privateFixture.blobs[object]; if (!blob) throw new Error("missingFixtureBlob"); return Buffer.from(blob, "base64");
  };
  assert.throws(() => captureProject(privateFixture.revision, read), /invalidProjectSource/);
});
test("dirty, changed HEAD and symlink trees never admit a project snapshot", () => {
  assert.throws(() => captureProject(fixtureRevision, fixtureGit({ status: Buffer.from(" M Cargo.lock\n") })), /dirtyOrWrongRevision/);
  assert.throws(() => captureProject("0".repeat(40), fixtureGit()), /dirtyOrWrongRevision/);
  const tree = fixtureGit()("tree").toString().replace("100644", "120000");
  assert.throws(() => captureProject(fixtureRevision, fixtureGit({ tree: Buffer.from(tree) })), /invalidProjectSource/);
});
test("case-folded directory aliases refuse an otherwise valid complete Git tree",()=>{
 const data=JSON.parse(readFileSync("tools/source-distribution/tests/fixtures/case-project.json","utf8")) as Fixture;
 const read:GitReader=(operation,object)=>{
  if(operation==="head")return Buffer.from(data.revision+"\n");if(operation==="status")return Buffer.alloc(0);if(operation==="commit")return Buffer.from(data.commit,"base64");if(operation==="tree")return Buffer.from(data.tree,"base64");const bytes=object&&data.blobs[object];if(!bytes)throw new Error("missingFixtureBlob");return Buffer.from(bytes,"base64");
 };
 assert.throws(()=>captureProject(data.revision,read),/invalidProjectSource/);
});
test("literal parentheses preserve the exact committed project filename", () => {
  const name = "literal.repr(C).expected.rs", bytes = Buffer.from("pub const LITERAL: u8 = 7;\n");
  const hash = (kind: string, data: Buffer) => createHash("sha1").update(`${kind} ${data.length}\0`).update(data).digest("hex");
  const blob = hash("blob", bytes), tree = Buffer.concat([Buffer.from(`100644 ${name}\0`), Buffer.from(blob, "hex")]);
  const commit = Buffer.from(`tree ${hash("tree", tree)}\nauthor Public Fixture <fixture@example.invalid> 0 +0000\ncommitter Public Fixture <fixture@example.invalid> 0 +0000\n\nLiteral filename fixture\n`);
  const revision = hash("commit", commit);
  const read: GitReader = (operation) => operation === "head" ? Buffer.from(revision + "\n") : operation === "status" ? Buffer.alloc(0) : operation === "commit" ? commit : operation === "tree" ? Buffer.from(`100644 blob ${blob}\t${name}\0`) : bytes;
  const snapshot = captureProject(revision, read);
  assert.deepEqual(snapshot.files, [{ name, mode: 0o644, bytes }]);
  assert.equal(safeName("src/output_tests/expected/into_bytes_enum.repr(C).expected.rs"), true);
  assert.equal(safeName("src/output_tests/expected/literal.repr(align(2)).expected.rs"), true);
});
test("shared path policy refuses traversal, controls and shell syntax through actual Git descriptors", () => {
  const names = ["../outside", "/absolute", "a//empty", "a/./same", "a/../outside", "a\\outside", "$(command).rs", "`command`.rs", "a;command.rs", "a&command.rs", "a|command.rs", "a>output.rs", "a space.rs", "a\n", "a\r", "a\t", "a\0", "a\u001b", "a\u007f"];
  for (const name of names) {
    assert.equal(safeName(name), false, JSON.stringify(name));
    assert.throws(() => projectDescriptors(Buffer.from(`100644 blob ${"a".repeat(40)}\t${name}\0`)), /invalidProjectSource/);
  }
});
