import type { GitCommand, GitCommandResult } from "../../source-distribution/src/git.js";
import type { RuntimeInputs } from "../../source-distribution/src/sources.js";
import type { NativeMainExport, CommandResult } from "./contract.js";

/** Explicit development input; the image digest is not a source/license/reproducibility claim. */
export const NATIVE_COMPILER_IMAGE = "sha256:0e1305bede70dd737e724f9e14eb2a76c43358f86175f37719e992350f4495a4" as const;
export type MainProducerInput = Readonly<{
  sourceRoot: string; sourceCommit: string; outputDirectory: string; dependenciesRoot: string;
  offline: Readonly<{
    requestBytes: Buffer; runtimeInputs: RuntimeInputs; installedRuntimeRoot: string; registryRoot: string;
    registryArchivesRoot: string;
  }>;
}>;
export type ProducerContainerCommand = Readonly<{
  kind: "container"; image: string; platform: "linux/arm64";
  network: "none"; cpus: 2; readOnly: true; pull: "never"; tmpfs: readonly ["/tmp"];
  executable: string; args: readonly string[]; cwd: string; env: Readonly<Record<string, string>>;
  mounts: readonly Readonly<{ source: string; target: string; readOnly: boolean }>[];
  deadlineMs: number; timeoutMs: number; maxOutputBytes: number;
}>;
export type ProducerCommand = ProducerContainerCommand | (GitCommand & Readonly<{ kind: "git" }>);
/** closed means the entire owned lifetime, including container/GNU children, was joined. */
export type MainProducerExecution = Readonly<{
  /** The caller bounds the entire invocation and joins each owned container/GNU lifetime. */
  deadlineMs: number; now: () => number;
  execute: (command: ProducerCommand) => Promise<CommandResult | GitCommandResult>;
}>;
export class MainProducerCommandError extends Error {
  constructor(message: string, readonly command: ProducerCommand, readonly received: unknown, readonly cause?: unknown) {
    super(message); this.name = "MainProducerCommandError";
  }
}
export type NativeMainProduct = Readonly<{
  schemaVersion: 1; status: "development"; sourceCommit: string;
  buildReceipt: Readonly<{ path: string; sha256: string; bytes: number }>;
  executable: Readonly<{ path: string; sha256: string; bytes: number }>;
  package: NativeMainExport;
  producerSeal: Readonly<{ path: string; sha256: string; bytes: number }>;
}>;
