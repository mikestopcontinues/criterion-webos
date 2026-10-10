export interface BridgeWitness { pid: number; counter: number; at: number }
export interface EmeEnvironment { player: boolean; secure: boolean; topLevel: boolean; visible: boolean }
export interface EmeDependencies {
  environment(): EmeEnvironment;
  bridge(): BridgeWitness | undefined;
  ping(): Promise<boolean>;
  request?: (keySystem: string, configurations: MediaKeySystemConfiguration[]) => Promise<unknown>;
  now(): number;
  schedule(delay: number, callback: () => void): () => void;
  publish(report: string): void;
}

const variants = ["hw-video", "sw-crypto"] as const;
type Variant = typeof variants[number];
type Outcome = "not-issued" | "pending" | "resolved" | "refused" | "threw" | "invalid" | "timeout" | "interrupted";
type Scheme = "cenc" | "absent" | "null";
type Category = "none" | "unsupported" | "security" | "invalid-request" | "resource" | "other";
interface Selection { audioScheme: Scheme; videoScheme: Scheme }
interface Row { variant: Variant; attempted: boolean; settled: boolean; outcome: Outcome; error: Category; selection?: Selection }

function record(value: unknown, keys: readonly string[]): Record<string, unknown> | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const actual = Object.keys(value);
  if (actual.length > keys.length || actual.some((key) => !keys.includes(key))) return undefined;
  return value as Record<string, unknown>;
}

function scheme(value: unknown, contentType: string, robustness: string): Scheme | undefined {
  if (!Array.isArray(value) || value.length !== 1) return undefined;
  const capability = record(value[0], ["contentType", "robustness", "encryptionScheme"]);
  if (!capability || capability.contentType !== contentType || capability.robustness !== robustness) return undefined;
  if (!Object.prototype.hasOwnProperty.call(capability, "encryptionScheme")) return "absent";
  if (capability.encryptionScheme === null) return "null";
  return capability.encryptionScheme === "cenc" ? "cenc" : undefined;
}

function project(value: unknown, variant: Variant): Selection | undefined {
  const selected = record(value, ["label", "initDataTypes", "sessionTypes", "distinctiveIdentifier", "persistentState", "audioCapabilities", "videoCapabilities"]);
  if (!selected || selected.label !== variant || selected.distinctiveIdentifier !== "not-allowed" || selected.persistentState !== "not-allowed"
    || !Array.isArray(selected.initDataTypes) || selected.initDataTypes.length !== 1 || selected.initDataTypes[0] !== "cenc"
    || !Array.isArray(selected.sessionTypes) || selected.sessionTypes.length !== 1 || selected.sessionTypes[0] !== "temporary") return undefined;
  const audioScheme = scheme(selected.audioCapabilities, 'audio/mp4; codecs="mp4a.40.2"', "SW_SECURE_CRYPTO");
  const videoScheme = scheme(selected.videoCapabilities, 'video/mp4; codecs="avc1.640028"', variant === "hw-video" ? "HW_SECURE_ALL" : "SW_SECURE_CRYPTO");
  return audioScheme && videoScheme ? { audioScheme, videoScheme } : undefined;
}

function category(error: unknown): Category {
  try {
    if (!error || typeof error !== "object" || !("name" in error)) return "other";
    switch (error.name) {
      case "NotSupportedError": return "unsupported";
      case "SecurityError": return "security";
      case "TypeError": return "invalid-request";
      case "QuotaExceededError": return "resource";
      default: return "other";
    }
  } catch { return "other"; }
}

function configuration(variant: Variant): MediaKeySystemConfiguration {
  return { label: variant, initDataTypes: ["cenc"], sessionTypes: ["temporary"],
    distinctiveIdentifier: "not-allowed", persistentState: "not-allowed",
    audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"', robustness: "SW_SECURE_CRYPTO", encryptionScheme: "cenc" }],
    videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.640028"', robustness: variant === "hw-video" ? "HW_SECURE_ALL" : "SW_SECURE_CRYPTO", encryptionScheme: "cenc" }],
  };
}

/** Owns one explicitly invoked, access-only packaged-player measurement. */
export class EmeProbe {
  private consumed = false;
  private stopped = false;
  private interrupt: (() => void) | undefined;
  private startedAt = 0;
  private deadline = 0;
  private origin: BridgeWitness | undefined;
  private acknowledgedCounter = 0;
  private readonly rows: Row[] = variants.map((variant) => ({ variant, attempted: false, settled: false, outcome: "not-issued", error: "none" }));
  constructor(private readonly dependencies: EmeDependencies) {}
  async run(): Promise<void> {
    if (this.consumed || this.stopped) return;
    this.consumed = true;
    this.startedAt = this.dependencies.now(); this.deadline = this.startedAt + 12000;
    if (!this.guard()) { this.stop(); return; }
    this.origin = this.dependencies.bridge();
    const ping = await this.wait(this.dependencies.ping(), Math.min(this.deadline, this.dependencies.now() + 3000));
    const acknowledged = this.dependencies.bridge();
    if (ping.kind !== "complete" || ping.rejected || ping.value !== true || !this.guard() || !this.origin || !acknowledged || acknowledged.counter <= this.origin.counter) { this.stop(); return; }
    this.acknowledgedCounter = acknowledged.counter;
    for (const row of this.rows) {
      if (!this.guard() || !this.dependencies.request) { this.stop(); return; }
      row.attempted = true; row.outcome = "pending";
      let returned = false;
      try {
        const queryDeadline = Math.min(this.deadline, this.dependencies.now() + 6000);
        const operation = this.dependencies.request("com.widevine.alpha", [configuration(row.variant)]);
        returned = true;
        const pending = this.wait(operation, queryDeadline, () => { row.settled = true; });
        this.publish();
        const result = await pending;
        if (result.kind !== "complete") { row.outcome = result.kind; break; }
        if (!this.guard()) { this.stop(); row.outcome = "interrupted"; row.selection = undefined; return; }
        if (result.rejected) {
          row.outcome = "refused"; row.error = category(result.value);
          if (row.error === "unsupported") continue;
          break;
        }
        const access = result.value;
        if (!access || typeof access !== "object" || !("keySystem" in access) || access.keySystem !== "com.widevine.alpha" || !("getConfiguration" in access) || typeof access.getConfiguration !== "function") {
          row.outcome = "invalid"; break;
        }
        try { row.selection = project(access.getConfiguration(), row.variant); }
        catch { row.outcome = "invalid"; break; }
        if (!row.selection) { row.outcome = "invalid"; break; }
        if (!this.guard()) { this.stop(); row.outcome = "interrupted"; row.selection = undefined; return; }
        row.outcome = "resolved";
      } catch (error: unknown) {
        row.outcome = returned ? "invalid" : "threw";
        if (!returned) row.settled = true;
        row.error = category(error); break;
      }
    }
    this.publish();
  }
  stop(): void {
    this.stopped = true;
    for (const row of this.rows) if (row.outcome === "pending") row.outcome = "interrupted";
    this.interrupt?.();
  }
  report(): string {
    const output = this.rows.map((row) => {
      const selection = row.selection;
      const base = `${row.variant} attempted=${row.attempted} settled=${row.settled} ${row.outcome} error=${row.error}`;
      if (!selection) return base + " configuration=unconfirmed";
      return base + ` label=${row.variant} init=cenc session=temporary ID=not-allowed persistence=not-allowed audio=mp4a.40.2/SW_SECURE_CRYPTO video=avc1.640028/${row.variant === "hw-video" ? "HW_SECURE_ALL" : "SW_SECURE_CRYPTO"} audioScheme=${selection.audioScheme} videoScheme=${selection.videoScheme} cenc=${selection.audioScheme === "cenc" && selection.videoScheme === "cenc"}`;
    }).join("\n");
    return output.length <= 1024 ? output : "EME report unavailable";
  }

  private guard(): boolean {
    try {
      if (this.stopped || typeof this.dependencies.request !== "function") return false;
      const environment = this.dependencies.environment();
      if (!environment.player || !environment.secure || !environment.topLevel || !environment.visible) return false;
      const witness = this.dependencies.bridge();
      const now = this.dependencies.now();
      return Number.isFinite(now) && now >= this.startedAt && now < this.deadline && witness !== undefined
        && Number.isInteger(witness.pid) && witness.pid > 0 && Number.isInteger(witness.counter)
        && witness.counter >= this.acknowledgedCounter && now >= witness.at && now - witness.at <= 1000
        && (!this.origin || witness.pid === this.origin.pid);
    } catch { return false; }
  }

  private publish(): void {
    if (!this.guard()) return;
    try { this.dependencies.publish(this.report()); } catch { /* Observation cannot retain ordinary work. */ }
  }

  private wait(operation: Promise<unknown>, deadline: number, settled?: () => void): Promise<
    { kind: "complete"; value: unknown; rejected: boolean } | { kind: "timeout" | "interrupted" }
  > {
    return new Promise((resolve) => {
      let active = true;
      const finish = (kind: "timeout" | "interrupted") => {
        if (!active) return;
        active = false; cancel(); this.interrupt = undefined; resolve({ kind });
      };
      const cancel = this.dependencies.schedule(Math.max(0, deadline - this.dependencies.now()), () => finish("timeout"));
      this.interrupt = () => finish("interrupted");
      const observe = (value: unknown, rejected: boolean) => {
        settled?.();
        if (!active) return; // Late settlement closes only the existing witness.
        if (this.stopped) { finish("interrupted"); return; }
        if (this.dependencies.now() >= deadline) { finish("timeout"); return; }
        active = false; cancel(); this.interrupt = undefined; resolve({ kind: "complete", value, rejected });
      };
      void operation.then((value) => observe(value, false), (error: unknown) => observe(error, true));
      if (this.stopped) finish("interrupted");
    });
  }
}
