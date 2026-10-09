import assert from "node:assert/strict";
import { mkdir, readFile, rm, symlink, unlink, writeFile } from "node:fs/promises";
import { test } from "node:test";
import { writeCandidate } from "../src/files.js";
const root="/workspace/.local/source-distribution/io-tests";
test("candidate writes refuse existing outputs instead of replacing their bytes",async()=>{
 await mkdir(root,{recursive:true}); const name="candidate.tar.gz"; await writeFile(root+"/"+name,"previous-public-candidate\n");
 await assert.rejects(()=>writeCandidate(root,name,Buffer.from("new bytes")));
 assert.equal((await readFile(root+"/"+name)).toString(),"previous-public-candidate\n");
 await rm(root+"/"+name);
});
test("candidate output cannot escape by a linked ancestor or traversal name",async()=>{
 await mkdir(root+"/outside",{recursive:true}); await symlink(root+"/outside",root+"/linked");
 try {
  await assert.rejects(()=>writeCandidate(root+"/linked","candidate.tar.gz",Buffer.from("new bytes")));
  await assert.rejects(()=>writeCandidate(root,"../escaped.tar.gz",Buffer.from("new bytes")));
 } finally {await unlink(root+"/linked"); await rm(root+"/outside",{recursive:true}); await rm("/workspace/.local/source-distribution/escaped.tar.gz",{force:true});}
});
test("prepared source seals round trip and refuse omitted committed files",async()=>{
 const {savePrepared,loadPrepared}=await import("../src/files.js");const {sourceProject}=await import("./source.test.js");
 const directory=root+"/prepared";await rm(directory,{recursive:true,force:true});
 await savePrepared(directory,sourceProject);const restored=await loadPrepared(directory+"/project.json");
 assert.equal(restored.revision,sourceProject.revision);assert.deepEqual(restored.files,sourceProject.files);
 const raw=JSON.parse((await readFile(directory+"/project.json")).toString()) as {files:unknown[]}; raw.files.pop();await writeFile(directory+"/project.json",JSON.stringify(raw));
 await assert.rejects(()=>loadPrepared(directory+"/project.json"),/invalidProjectSource/);await rm(directory,{recursive:true});
});
test("hard-linked input files are not admitted as ordinary public inputs",async()=>{
 const {link,unlink}=await import("node:fs/promises");const {readSourceFile}=await import("../src/files.js");
 const name=root+"/input";const alias=root+"/input-alias";await writeFile(name,"public bytes");await link(name,alias);
 try {await assert.rejects(()=>readSourceFile(name,1024),/invalidSourceFile/);}finally{await unlink(alias);await unlink(name);}
});
