import { resolve } from "node:path";
import { loadPrepared, readSourceFile } from "./src/files.js";
import { parseRequest, sourceRequirements } from "./src/sources.js";
async function main(): Promise<void> {
  if (process.argv.length !== 2 || process.cwd() !== "/workspace") throw new Error("unexpectedExecutionContext");
  const input = resolve(".local/source-distribution/input");
  const bytes = await readSourceFile(resolve(input, "source-request.json"), 16384); const request = parseRequest(bytes);
  const project = await loadPrepared(resolve(".local/source-distribution/prepared", request.sourceCommit, "project.json"));
  const runtime = { channel: await readSourceFile(resolve(input, "rust-channel.toml"), 2 * 1024 * 1024), archive: await readSourceFile(resolve(input, "rust-src-nightly.tar.gz"), 32 * 1024 * 1024), copyright: await readSourceFile(resolve(input, "COPYRIGHT-library.html"), 8 * 1024 * 1024) };
  const requirements = sourceRequirements(project, bytes, runtime);
  process.stdout.write(JSON.stringify({ schemaVersion: 1, status: "development-source-requirements", sourceCommit: project.revision, registryScope: "complete-project-and-runtime-lock-union-overincluded", registry: requirements.registry }, null, 2) + "\n");
}
void main().catch(() => { process.stderr.write("Source requirements refused; inspect revision and fixed runtime inputs.\n"); process.exitCode = 1; });
