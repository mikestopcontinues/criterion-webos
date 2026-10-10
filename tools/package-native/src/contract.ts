import type { auditIpk } from "./archive.js";
import type { BuildReceipt } from "./receipt.js";
import type { APP_ID } from "./admission.js";

export const PACKAGING_IMAGE = "sha256:cebcb9a6d3df693d21710cb78ec9a1d0ed19c8e26ecdf705686c9d235d887916" as const;
export type PackageCommand = Readonly<{
  executable: string; args: readonly string[]; cwd: string; env: Readonly<Record<string, string>>;
  deadlineMs: number; timeoutMs: number; maxOutputBytes: number;
}>;
export type ClosedCommand = {
  closed: true; exitCode: number | null; signal: string | null; timedOut: boolean; stdout: Buffer; stderr: Buffer;
};
export type CommandResult = ClosedCommand | { closed: false };
/** Private bounded command evidence survives a late clock/guard refusal; no success is implied. */
export class PackagingCommandError extends Error {
  constructor(message: "unresolvedCommand" | "invalidCommandResult" | "packageCommandFailed" | "packagingDeadline", readonly command: PackageCommand, readonly outcome: ClosedCommand | null) {
    super(message); this.name = "PackagingCommandError";
  }
}
/**
 * Run this API inside the fixed packaging image in a caller-bounded process/container.
 * The caller must enforce the ORIGINAL deadline and join that entire lifetime, including
 * synchronous GNU normalization children. Cooperative checks cannot interrupt them.
 * execute bounds both streams and returns only after the issued process closes;
 * an unknown outcome is {closed:false}, never a successful timeout acknowledgement.
 */
export type PackagingExecution = Readonly<{
  image: typeof PACKAGING_IMAGE; deadlineMs: number; now: () => number;
  execute: (command: PackageCommand) => Promise<CommandResult>;
}>;
export type NativeMainInput = Readonly<{
  sourceRoot: string; outputDirectory: string; executable: Buffer; buildReceipt: Buffer;
}>;
export type NativeMainExport = Readonly<{
  schemaVersion: 1; status: "development"; identityScope: "build-receipt-content-only";
  build: BuildReceipt; receiptSha256: string;
  ipk: Readonly<{ path: string; sha256: string; bytes: number }>;
  packageSeal: Readonly<{ path: string; sha256: string; bytes: number }>;
  executableRequirements: Readonly<{ appId: typeof APP_ID; executableSha256: string; neededSonames: readonly string[] }>;
  audit: ReturnType<typeof auditIpk>;
}>;
