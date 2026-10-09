import { resolve } from "node:path";
import { buildSourceArchive } from "./src/archive.js";
import { loadPrepared, readSourceFile, writeCandidate } from "./src/files.js";
import { assembleSources, parseRequest, sha256 } from "./src/sources.js";
async function main(): Promise<void> {
  if (process.argv.length !== 2 || process.cwd() !== "/workspace") throw new Error("unexpectedExecutionContext");
  const input = resolve(".local/source-distribution/input");
  const requestBytes = await readSourceFile(resolve(input, "source-request.json"), 16384); const request = parseRequest(requestBytes);
  const project = await loadPrepared(resolve(".local/source-distribution/prepared", request.sourceCommit, "project.json"));
  const runtime = { channel: await readSourceFile(resolve(input, "rust-channel.toml"), 2 * 1024 * 1024), archive: await readSourceFile(resolve(input, "rust-src-nightly.tar.gz"), 32 * 1024 * 1024), copyright: await readSourceFile(resolve(input, "COPYRIGHT-library.html"), 8 * 1024 * 1024) };
  const candidate = await assembleSources(project, requestBytes, runtime, (name) => readSourceFile(resolve(input, "registry", name), 32 * 1024 * 1024));
  const bytes = await buildSourceArchive(candidate.files, "/workspace/.local/source-distribution/archive/candidate");
  const filename = `criterion-source-${project.revision}.tar.gz`; const output = resolve(".local/source-distribution/output", project.revision);
  await writeCandidate(output, filename, bytes);
  const seal = { schemaVersion: 1, status: "development-candidate", sourceCommit: project.revision, archive: { name: filename, size: bytes.length, sha256: sha256(bytes) }, manifestSha256: sha256(candidate.files.find((file) => file.name === "SOURCE-MANIFEST.json")?.bytes ?? Buffer.alloc(0)), files: candidate.files.length };
  await writeCandidate(output, "seal.json", Buffer.from(JSON.stringify(seal, null, 2) + "\n"));
  process.stdout.write(JSON.stringify(seal) + "\n");
}
void main().catch(() => { process.stderr.write("Source candidate refused; inspect fixed input completeness, checksums and output ownership.\n"); process.exitCode = 1; });
