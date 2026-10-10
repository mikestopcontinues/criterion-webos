import type { CommandResult, PackageCommand } from "./contract.js";
import type { PackagingFiles } from "../package-phase.js";

export type PackageWorkerInput = PackagingFiles & Readonly<{ deadlineMs: number; modulePath: string; image: string }>;
/** This function is serialized only from checked TypeScript emission; importing it is inert. */
async function packageWorker(input: PackageWorkerInput): Promise<void> {
  const { spawn } = require("node:child_process") as typeof import("node:child_process");
  const execute = (command: PackageCommand): Promise<CommandResult> => new Promise(resolve => {
    if (Date.now() >= command.deadlineMs) { resolve({ closed: false }); return; }
    const child = spawn(command.executable, [...command.args], { cwd: command.cwd, env: { ...command.env }, shell: false, stdio: ["ignore", "pipe", "pipe"] });
    const stdout: Buffer[] = [], stderr: Buffer[] = []; let bytes = 0, timedOut = false, failed = false;
    const kill = () => { timedOut = true; child.kill("SIGKILL"); };
    const timer = setTimeout(kill, Math.max(1, Math.min(command.timeoutMs, command.deadlineMs - Date.now())));
    const collect = (parts: Buffer[], part: Buffer) => {
      const remaining = command.maxOutputBytes - bytes;
      if (remaining > 0) parts.push(Buffer.from(part.subarray(0, remaining)));
      bytes += part.length;
      if (bytes > command.maxOutputBytes) { failed = true; child.kill("SIGKILL"); }
    };
    child.stdout.on("data", (part: Buffer) => collect(stdout, part));
    child.stderr.on("data", (part: Buffer) => collect(stderr, part));
    child.on("error", () => { failed = true; });
    child.on("close", (exitCode, signal) => {
      clearTimeout(timer);
      if (exitCode === null && signal === null) { resolve({ closed: false }); return; }
      resolve({ closed: true, exitCode: failed && exitCode === 0 ? 1 : exitCode, signal, timedOut, stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr) });
    });
  });
  try {
    const phase = require(input.modulePath) as typeof import("../package-phase.js");
    const result = await phase.packageMainFromFiles(input, { image: input.image as import("./contract.js").PackagingExecution["image"], deadlineMs: input.deadlineMs, now: Date.now, execute });
    process.stdout.write(JSON.stringify(result) + "\n");
  } catch (error) {
    const value = error instanceof Error ? error : new Error("packageWorkerFailed");
    const evidence = value as Error & { command?: PackageCommand; outcome?: { stdout: Buffer; stderr: Buffer; closed: true; exitCode: number | null; signal: string | null; timedOut: boolean } };
    process.stderr.write(JSON.stringify({ error: value.message, command: evidence.command ?? null, outcome: evidence.outcome ? { ...evidence.outcome, stdout: evidence.outcome.stdout.toString("base64"), stderr: evidence.outcome.stderr.toString("base64") } : null }) + "\n");
    process.exitCode = 1;
  }
}
/** Fixed Node24 packaging-image driver; no SSH/device/CLI deployment operation is emitted. */
export function packageWorkerProgram(input: PackageWorkerInput): string {
  if (input.modulePath !== "/workspace/.local/native-package/compiled/tools/package-native/package-phase.js"
    || input.sourceRoot !== "/workspace" || input.outputDirectory !== "/workspace/.local/native-package/exports/main"
    || input.executablePath !== "/workspace/.local/native-package/input/criterion-unofficial"
    || input.receiptPath !== "/workspace/.local/native-package/input/build-receipt.json"
    || !Number.isSafeInteger(input.deadlineMs) || input.deadlineMs < 1
    || input.image !== "sha256:cebcb9a6d3df693d21710cb78ec9a1d0ed19c8e26ecdf705686c9d235d887916") throw new Error("invalidPackageWorker");
  return `"use strict";\n(${packageWorker.toString()})(${JSON.stringify(input)}).catch(error => { process.stderr.write(String(error) + "\\n"); process.exitCode = 1; });\n`;
}
/** Invoke the emitter from the freshly compiled captured source, never from a host cache/revision. */
export function packageWorkerLaunch(input: PackageWorkerInput): string {
  packageWorkerProgram(input); // Shares the exact fixed-path/input admission.
  return '"use strict";\nconst fs = require("node:fs");\nconst emitter = require("/workspace/.local/native-package/compiled/tools/package-native/src/package-worker.js");\n'
    + `fs.writeFileSync("/workspace/.local/native-package/package-worker.cjs", emitter.packageWorkerProgram(${JSON.stringify(input)}), {flag:"wx",mode:384});\n`
    + 'require("/workspace/.local/native-package/package-worker.cjs");\n';
}
