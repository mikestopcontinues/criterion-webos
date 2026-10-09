import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { gunzipSync } from "node:zlib";
import { buildSourceArchive } from "../src/archive.js";
import type { SourceFile } from "../src/project.js";
const files: SourceFile[] = [
  { name: "project/dev", bytes: Buffer.from("#!/bin/sh\ntrue\n"), mode: 0o755 },
  { name: "project/Cargo.lock", bytes: Buffer.from("version = 4\n"), mode: 0o644 },
];
test("the source archive is deterministic with fixed ownership, time, modes and exact members", async () => {
  const first = await buildSourceArchive(files, "/workspace/.local/source-distribution/archive/first");
  const second = await buildSourceArchive([...files].reverse(), "/workspace/.local/source-distribution/archive/second");
  assert.deepEqual(first, second);
  assert.equal(first.readUInt32LE(4), 0);
  const tar = gunzipSync(first);
  assert.equal(tar.subarray(0, 8).toString(), "project/");
  assert.deepEqual(tar.subarray(108, 124), Buffer.concat([Buffer.from("0000000"), Buffer.from([0]), Buffer.from("0000000"), Buffer.from([0])]));
  const listed = spawnSync("/usr/bin/tar", ["-tzf", "-"], { input: first, env: { PATH: "/usr/bin:/bin", LC_ALL: "C", TZ: "UTC" }, maxBuffer: 4096, timeout: 10000 });
  assert.equal(listed.status, 0); assert.equal(listed.stderr.length, 0);
  assert.equal(listed.stdout.toString(), "project/\nproject/Cargo.lock\nproject/dev\n");
});
test("unsealed PAX metadata is refused even when logical file content matches", async()=>{
 const { gzipSync } = await import("node:zlib");
 const { tarFixture } = await import("../../player-probe/tests/fixture-tar.js");
 const plain = tarFixture([{name:"project/",mode:0o755,type:"5",bytes:Buffer.alloc(0)},{name:"metadata",mode:0o644,type:"x",bytes:Buffer.from("19 comment=secret!\n")},...files.map(file=>({name:file.name,mode:file.mode,bytes:file.bytes}))]);
 const { auditSourceArchive } = await import("../src/archive.js");
 await assert.rejects(()=>auditSourceArchive(gzipSync(plain),files,"/workspace/.local/source-distribution/archive/metadata"),/invalidSourceArchive/);
});
test("candidate archive refuses extra, duplicate, linked, traversal and changed-mode/content members",async()=>{
 const { gzipSync } = await import("node:zlib");const {tarFixture}=await import("../../player-probe/tests/fixture-tar.js");const {auditSourceArchive}=await import("../src/archive.js");
 const directory={name:"project/",mode:0o755,type:"5",bytes:Buffer.alloc(0)};
 const entries=files.map(file=>({name:file.name,mode:file.mode,bytes:file.bytes}));
 const wrong=[
  [...entries,{name:"extra",mode:0o644,bytes:Buffer.from("private bytes")}],
  [...entries,entries[0]!],
  entries.map((entry,index)=>index===0?{...entry,type:"2"}:entry),
  entries.map((entry,index)=>index===0?{...entry,name:"../escape"}:entry),
  entries.map((entry,index)=>index===0?{...entry,mode:0o777}:entry),
  entries.map((entry,index)=>index===0?{...entry,bytes:Buffer.from("different source bytes")}:entry),
 ];
 for(const entries of wrong)await assert.rejects(()=>auditSourceArchive(gzipSync(tarFixture([directory,...entries])),files,"/workspace/.local/source-distribution/archive/refusal"),/invalidSourceArchive/);
});
test("locked version metadata and runtime embedded resource names remain literal safe paths",async()=>{
 const name="registry/wasip2-1.0.3+wasi-0.2.9.crate";
 const bytes=await buildSourceArchive([{name,bytes:Buffer.from("public fixture source\n"),mode:0o644},{name:"runtime/wasi-cli@0.2.0.wasm",bytes:Buffer.from("public fixture resource\n"),mode:0o644}],"/workspace/.local/source-distribution/archive/version-metadata");
 const listed=spawnSync("/usr/bin/tar",["-tzf","-"],{input:bytes,maxBuffer:4096});assert.equal(listed.status,0);assert.equal(listed.stdout.toString(),"registry/\n"+name+"\nruntime/\nruntime/wasi-cli@0.2.0.wasm\n");
});
