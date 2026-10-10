import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import { admitProject, admitProjectBlob, admitProjectCommit, projectDescriptors, type GitReader, type ProjectSnapshot, type SourceFile } from "./project.js";
export type GitCommand = Readonly<{
  executable: string; args: readonly string[]; cwd: string; env: Readonly<Record<string, string>>;
  deadlineMs: number; timeoutMs: number; maxOutputBytes: number;
}>;
export type ClosedGitCommand = {
  closed: true; exitCode: number | null; signal: string | null; timedOut: boolean; stdout: Buffer; stderr: Buffer;
};
export type GitCommandResult = ClosedGitCommand | { closed: false };
/** execute owns output limits and process closure under the original deadline; no detached/raced child is admitted. */
export type HostGitExecution = Readonly<{
  deadlineMs: number; now: () => number; execute: (command: GitCommand) => Promise<GitCommandResult>;
}>;
export class GitCommandError extends Error {
  constructor(message: "unresolvedCommand" | "invalidCommandResult" | "gitSourceReadFailed" | "gitDeadline", readonly command: GitCommand, readonly outcome: ClosedGitCommand | null, readonly received: unknown = outcome) {
    super(message); this.name = "GitCommandError";
  }
}
const environment = Object.freeze({ PATH: "/opt/homebrew/bin:/usr/bin:/bin", LANG: "C", LC_ALL: "C", TZ: "UTC", GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null", GIT_OPTIONAL_LOCKS: "0" });
function gitRoot(root: string): string {
  const absolute = resolve(root); if (realpathSync(absolute) !== absolute) throw new Error("invalidGitRoot"); return absolute;
}
function gitCommand(root: string, operation: Parameters<GitReader>[0], object: string | undefined, deadlineMs: number, timeoutMs: number): GitCommand {
  if ((operation === "blob" || operation === "commit" || operation === "tree") && (!object || !/^[a-f0-9]{40}$/.test(object))) throw new Error("invalidGitObject");
  const args = operation === "head" ? ["rev-parse", "--verify", "HEAD"] : operation === "status" ? ["status", "--porcelain=v1", "--untracked-files=normal"] : operation === "tree" ? ["ls-tree", "-rz", "--full-tree", object ?? ""] : ["cat-file", operation, object ?? ""];
  return Object.freeze({ executable: "git", args: Object.freeze(["--no-replace-objects", "-c", "core.fsmonitor=false", "-c", "core.untrackedCache=false", "-c", "core.hooksPath=/dev/null", "-C", root, ...args]), cwd: root, env: environment, deadlineMs, timeoutMs, maxOutputBytes: operation === "blob" ? 32 * 1024 * 1024 + 1 : 2 * 1024 * 1024 });
}
function closedResult(result: unknown): result is ClosedGitCommand {
  if (typeof result !== "object" || result === null) return false;
  const keys = Reflect.ownKeys(result);
  if (keys.length !== 6 || !keys.every((key) => typeof key === "string" && ["closed", "exitCode", "signal", "timedOut", "stdout", "stderr"].includes(key))) return false;
  const value = result as Partial<ClosedGitCommand>;
  return value.closed === true && (value.exitCode === null || (typeof value.exitCode === "number" && Number.isInteger(value.exitCode) && value.exitCode >= 0 && value.exitCode <= 255))
    && (value.signal === null || (typeof value.signal === "string" && /^SIG[A-Z0-9]+$/.test(value.signal))) && typeof value.timedOut === "boolean"
    && Buffer.isBuffer(value.stdout) && Buffer.isBuffer(value.stderr) && ((value.exitCode === null) !== (value.signal === null));
}
export async function captureHostProject(root: string, revision: string, execution: HostGitExecution): Promise<ProjectSnapshot> {
  const { deadlineMs, now, execute } = execution;
  let previous = -Infinity; let lastCommand: GitCommand | null = null; let lastOutcome: ClosedGitCommand | null = null;
  const remaining = (): number => {
    let current: number;
    try { current = now(); } catch {
      if (lastCommand) throw new GitCommandError("gitDeadline", lastCommand, lastOutcome);
      throw new Error("gitDeadline");
    }
    if (!Number.isSafeInteger(deadlineMs) || !Number.isSafeInteger(current) || current < 0 || current < previous || deadlineMs - current < 1) {
      if (lastCommand) throw new GitCommandError("gitDeadline", lastCommand, lastOutcome);
      throw new Error("gitDeadline");
    }
    previous = current; return deadlineMs - current;
  };
  remaining();
  const absolute = gitRoot(root);
  const read = async (operation: Parameters<GitReader>[0], object?: string): Promise<Buffer> => {
    const command = gitCommand(absolute, operation, object, deadlineMs, Math.min(60000, Math.floor(remaining())));
    lastCommand = command; lastOutcome = null;
    let result: GitCommandResult;
    try { result = await execute(command); } catch (error) { throw new GitCommandError("unresolvedCommand", command, null, error); }
    if (result?.closed === false && Reflect.ownKeys(result).length === 1) throw new GitCommandError("unresolvedCommand", command, null, result);
    if (!closedResult(result)) throw new GitCommandError("invalidCommandResult", command, null, result);
    lastOutcome = result;
    if (result.stdout.length + result.stderr.length > command.maxOutputBytes) throw new GitCommandError("invalidCommandResult", command, result);
    remaining();
    if (result.exitCode !== 0 || result.signal !== null || result.timedOut || result.stderr.length !== 0) throw new GitCommandError("gitSourceReadFailed", command, result);
    return result.stdout;
  };
  if (!/^[a-f0-9]{40}$/.test(revision) || (await read("head")).toString() !== revision + "\n" || (await read("status")).length !== 0) throw new Error("dirtyOrWrongRevision");
  const commit = await read("commit", revision);
  admitProjectCommit(revision, commit);
  const descriptors = projectDescriptors(await read("tree", revision));
  const files: SourceFile[] = []; let total = 0;
  for (const descriptor of descriptors) {
    const bytes = await read("blob", descriptor.object); total = admitProjectBlob(descriptor.object, bytes, total);
    files.push({ name: descriptor.name, bytes, mode: descriptor.mode });
  }
  if ((await read("head")).toString() !== revision + "\n" || (await read("status")).length !== 0) throw new Error("dirtyOrWrongRevision");
  const snapshot = { revision, commit, files: files.sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name))) };
  admitProject(snapshot); remaining(); return snapshot;
}
/** Host-owned built-in Git reads with no hooks, replacements, filters or inherited Git config. */
export function hostGit(root: string): GitReader {
  const absolute = gitRoot(root);
  return (operation, object) => {
    const command = gitCommand(absolute, operation, object, 0, 60000);
    const result = spawnSync(command.executable, command.args, {
      shell: false, timeout: command.timeoutMs, maxBuffer: command.maxOutputBytes,
      env: command.env,
    });
    if (result.error || result.status !== 0 || result.stderr.length !== 0) throw new Error("gitSourceReadFailed"); return result.stdout;
  };
}
