export const MAIN_APP_ID = "com.mikestopcontinues.criterion.unofficial" as const;

/** Supplied by checked MAIN package/ELF metadata, not device discovery. */
export interface NativeRequirements {
  readonly appId: typeof MAIN_APP_ID;
  readonly executableSha256: string;
  readonly neededSonames: readonly string[];
}

/** Acknowledged closure is independent of exit/timeout and response validity. */
export type ActualReadOutcome =
  | { readonly kind: "closed"; readonly exitCode: number | null;
      readonly signal: string | null; readonly timedOut: boolean;
      readonly stdout: Buffer; readonly stderr: Buffer }
  | { readonly kind: "unresolved"; readonly reason: string };

export interface StockNode16ReadProgram {
  readonly source: string;
  readonly stdin: "none";
  readonly deadlineMs: number;
  readonly timeoutMs: number;
  readonly stdoutLimit: 65536;
  readonly stderrLimit: 4096;
}

export interface PrerequisiteExecutor {
  /** Original absolute deadline in the same clock as now; never renew after await. */
  readonly deadlineMs: number;
  readonly now: () => number;
  /** Enforce the supplied deadline/bounds; return actual closure or missing acknowledgement. */
  readonly execute: (program: StockNode16ReadProgram) => Promise<ActualReadOutcome>;
}

export interface Epoch {
  readonly bootId: string;
  readonly compositor: { readonly pid: number; readonly startTicks: string };
}

export type GetterClosure =
  | { readonly kind: "closed"; readonly exitCode: number | null;
      readonly signal: string | null; readonly timedOut: boolean }
  | { readonly kind: "not-issued" }
  | { readonly kind: "unresolved" };

export type SystemInfo =
  | { readonly kind: "available"; readonly modelName: string;
      readonly firmwareVersion: string; readonly sdkVersion: string; readonly boardType: string }
  | { readonly kind: "unavailable"; readonly reason: "getter-failed" | "getter-response" };

export type CpuFacts =
  | { readonly kind: "available"; readonly processorCount: number;
      readonly architecture: string; readonly features: readonly string[]; readonly sha256: string }
  | { readonly kind: "unavailable"; readonly reason: "cpu-response" };

export type LibraryFact =
  | { readonly soname: string; readonly kind: "available"; readonly path: string;
      readonly bytes: number; readonly sha256: string;
      readonly elf: { readonly class: 32; readonly endian: "little"; readonly machine: 40;
        readonly type: 2 | 3; readonly flags: number } }
  | { readonly soname: string; readonly kind: "unavailable";
      readonly reason: "missing" | "unsafe-path" | "file-bound" | "unsupported-elf" };

/** File/metadata observations only; this does not admit runtime ABI or playback. */
export interface NativePrerequisiteFacts {
  readonly schemaVersion: 1;
  readonly appId: typeof MAIN_APP_ID;
  readonly executableSha256: string;
  readonly epoch: Epoch;
  readonly runtime: { readonly node: string; readonly platform: "linux";
    readonly arch: string; readonly euid: 0 };
  readonly getter: GetterClosure;
  readonly systemInfo: SystemInfo;
  readonly cpu: CpuFacts;
  readonly libraries: readonly LibraryFact[];
}

export type PrerequisiteResponse =
  | { readonly kind: "accepted"; readonly facts: NativePrerequisiteFacts }
  | { readonly kind: "unavailable"; readonly reason:
      "deadline" | "requirements" | "closure" | "execution" | "response" };

export interface PrerequisiteRead {
  readonly outcome: ActualReadOutcome | null;
  readonly response: PrerequisiteResponse;
}
