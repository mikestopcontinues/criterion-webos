import { resolve } from "node:path";
import { readSourceFile, savePrepared } from "./src/files.js";
import { hostGit } from "./src/git.js";
import { captureProject } from "./src/project.js";
import { parseRequest } from "./src/sources.js";
async function main(): Promise<void> {
  if (process.argv.length !== 2) throw new Error("unexpectedArguments");
  const root = process.cwd(); const request = parseRequest(await readSourceFile(resolve(".local/source-distribution/input/source-request.json"), 16384));
  const snapshot = captureProject(request.sourceCommit, hostGit(root));
  await savePrepared(resolve(".local/source-distribution/prepared", snapshot.revision), snapshot);
  process.stdout.write(JSON.stringify({ status: "prepared-development-source", sourceCommit: snapshot.revision, files: snapshot.files.length }) + "\n");
}
void main().catch(() => { process.stderr.write("Source preparation refused; inspect clean revision and fixed public inputs.\n"); process.exitCode = 1; });
