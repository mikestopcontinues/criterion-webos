import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { createHash } from "node:crypto";
import { gzipSync, gunzipSync } from "node:zlib";
import { tarFixture } from "../../player-probe/tests/fixture-tar.js";
import { captureProject, type GitReader } from "../src/project.js";
import { assembleSources, type PublicRequest } from "../src/sources.js";
const sha = (bytes: Buffer): string => createHash("sha256").update(bytes).digest("hex");
const fixture = JSON.parse(readFileSync("tools/source-distribution/tests/fixtures/source-project.json", "utf8")) as {revision:string;commit:string;tree:string;blobs:Record<string,string>};
const read: GitReader = (operation, object) => {
  if (operation === "head") return Buffer.from(fixture.revision + "\n");
  if (operation === "status") return Buffer.alloc(0);
  if (operation === "commit") return Buffer.from(fixture.commit,"base64");
  if (operation === "tree") return Buffer.from(fixture.tree,"base64");
  const blob = object && fixture.blobs[object]; if (!blob) throw new Error("missingFixtureBlob"); return Buffer.from(blob,"base64");
};
export const sourceProject = captureProject(fixture.revision, read);
const runtimeLock = Buffer.from('version = 4\n\n[[package]]\nname = "runtime-only"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "b8f848aafb52a8e1b353f996636c9a52442bf48ef692a188e6ed6fe2a322b843"\n');
const prefix = "rust-src-nightly/rust-src/lib/rustlib/src/rust/";
const required = ["library/Cargo.toml","library/std/src/lib.rs","library/core/src/lib.rs","library/compiler-builtins/Cargo.toml","library/compiler-builtins/LICENSE.txt","library/stdarch/LICENSE-MIT","library/stdarch/LICENSE-APACHE","library/portable-simd/LICENSE-MIT","library/portable-simd/LICENSE-APACHE","src/llvm-project/libunwind/LICENSE.TXT"];
export const runtimeArchive = gzipSync(tarFixture([
  ...required.map((name)=>({name:prefix+name,mode:0o644,bytes:Buffer.from("public synthetic runtime fixture\n")})),
  {name:prefix+"library/Cargo.lock",mode:0o644,bytes:runtimeLock},
  ...["LICENSE-MIT","LICENSE-APACHE","COPYRIGHT","git-commit-hash","version"].map((name)=>({name:"rust-src-nightly/"+name,mode:0o644,bytes:Buffer.from(name === "git-commit-hash" ? "4c9d2bfe4ad7a65669098754964aaebe0ec1ced2" : name === "version" ? "1.98.0-nightly (4c9d2bfe4 2026-07-01)" : "public synthetic runtime fixture\n")})),
]));
export const runtimeChannel = Buffer.from(`manifest-version = "2"\ndate = "2026-07-02"\n[pkg.rust-src]\nversion = "1.98.0-nightly (4c9d2bfe4 2026-07-01)"\n[pkg.rust-src.target."*"]\navailable = true\nurl = "https://static.rust-lang.org/dist/2026-07-02/rust-src-nightly.tar.gz"\nhash = "${sha(runtimeArchive)}"\n`);
export const runtimeCopyright = Buffer.from("public synthetic runtime copyright fixture\n");
export const request: PublicRequest = {
 schemaVersion:1, sourceCommit:fixture.revision,
 runtime:{channelManifestSha256:sha(runtimeChannel),sourceArchiveSha256:sha(runtimeArchive),copyrightSha256:sha(runtimeCopyright)},
 exclusions:["stock-sdl2","stock-egl","stock-gles2","stock-lunaservice2","stock-glib2","stock-libc","stock-libm","stock-libpthread","stock-librt","stock-libdl","stock-libgcc-s"].map(component=>({component,basis:"system-library",reason:"Owner-declared fixture exclusion; final eligibility unproved."})),
};
request.exclusions.push({component:"glibc-startup-nonshared",basis:"linked-file-permission",reason:"Owner-declared retained file permission; actual maps remain required."},{component:"gcc-crtstuff",basis:"linked-file-permission",reason:"Owner-declared retained GCC exception; conditions remain required."});
export const rawRequest = ():Buffer => Buffer.from(JSON.stringify(request));
export const runtimeInputs = {channel:runtimeChannel,archive:runtimeArchive,copyright:runtimeCopyright};
export const registry = new Map([["example-1.2.3.crate",Buffer.from("sample-registry-source\n")],["runtime-only-1.0.0.crate",Buffer.from("sample-runtime-source\n")]]);
test("complete source admission includes public project/config/resources and runtime-only locked source",async()=>{
 const candidate = await assembleSources(sourceProject,rawRequest(),runtimeInputs,async(name)=>{const bytes=registry.get(name);if(!bytes)throw new Error("missingSource");return bytes;});
 assert.deepEqual(candidate.files.filter(file=>file.name.startsWith("registry/")).map(file=>file.name),["registry/example-1.2.3.crate","registry/runtime-only-1.0.0.crate"]);
 assert.ok(candidate.files.some(file=>file.name==="project/logs/2026-10-09.md"));
 assert.ok(candidate.files.some(file=>file.name==="project/.cargo/config.toml"));
 assert.ok(candidate.files.some(file=>file.name==="runtime/rust-src-nightly.tar.gz" && file.bytes.equals(runtimeArchive)));
 assert.equal(candidate.manifest.status,"development-candidate");
 assert.equal(candidate.manifest.registryScope,"complete-project-and-runtime-lock-union-overincluded");
 assert.equal(candidate.manifest.sourceCommit,sourceProject.revision);
});
test("omitted runtime-only archive and checksum mismatches refuse source admission",async()=>{
 const reader=async(name:string):Promise<Buffer>=>{const bytes=registry.get(name);if(!bytes)throw new Error("missingSource");return bytes;};
 await assert.rejects(()=>assembleSources(sourceProject,rawRequest(),runtimeInputs,async(name)=>{if(name.startsWith("runtime-only"))throw new Error("missingSource");return reader(name);}),/missingSource/);
 await assert.rejects(()=>assembleSources(sourceProject,rawRequest(),runtimeInputs,async()=>Buffer.from("changed source")),/invalidSourceInput/);
 await assert.rejects(()=>assembleSources(sourceProject,rawRequest(),{...runtimeInputs,archive:Buffer.from("changed source")},reader),/invalidSourceInput/);
});
test("source manifest seals modified vendors and embedded resources rather than excluding them",async()=>{
 const candidate=await assembleSources(sourceProject,rawRequest(),runtimeInputs,async(name)=>{const value=registry.get(name);if(!value)throw new Error("missingSource");return value;});
 assert.ok(candidate.files.some(file=>file.name==="project/vendor/modified/src/lib.rs"));
 assert.ok(candidate.files.some(file=>file.name==="project/assets/fonts/embedded.ttf"));
 for(const file of candidate.files.filter(file=>file.name!=="SOURCE-MANIFEST.json")) {
  const seal=candidate.manifest.files.find(entry=>entry.name===file.name);
  assert.equal(seal?.sha256,sha(file.bytes)); assert.equal(seal?.size,file.bytes.length); assert.equal(seal?.mode,file.mode);
 }
});
test("request admission refuses blanket SDK exclusions, extra/private fields and wrong revisions",async()=>{
 const read=async(name:string):Promise<Buffer>=>{const value=registry.get(name);if(!value)throw new Error("missingSource");return value;};
 for(const changed of [{...request,sourceCommit:"0".repeat(40)},{...request,privateReceipt:".local/account.json"},{...request,exclusions:[{component:"SDK",basis:"system-library",reason:"Blanket SDK exclusion rejected."}]},{...request,exclusions:[]}])
  await assert.rejects(()=>assembleSources(sourceProject,Buffer.from(JSON.stringify(changed)),runtimeInputs,read),/invalidSourceInput/);
});
test("linked or traversing runtime archive members are refused before source admission",async()=>{
 const read=async(name:string):Promise<Buffer>=>{const value=registry.get(name);if(!value)throw new Error("missingSource");return value;};
 for(const entry of [{name:"rust-src-nightly/linked",type:"2"},{name:"rust-src-nightly/../private",type:"0"}]) {
  const archive=gzipSync(Buffer.concat([tarFixture([{...entry,mode:0o644,bytes:Buffer.alloc(0)}]).subarray(0,512),gunzipSync(runtimeArchive)]));
  const changed={...request,runtime:{...request.runtime,sourceArchiveSha256:sha(archive)}};
  const channel=Buffer.from(runtimeChannel.toString().replace(sha(runtimeArchive),sha(archive))); changed.runtime.channelManifestSha256=sha(channel);
  await assert.rejects(()=>assembleSources(sourceProject,Buffer.from(JSON.stringify(changed)),{...runtimeInputs,channel,archive},read),/invalidSourceInput/);
 }
});
