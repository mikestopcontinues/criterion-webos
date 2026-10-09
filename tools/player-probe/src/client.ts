import { MAX_SEQUENCE, SERVICE_ID, VERSION, type Snapshot } from "./protocol.js";

export interface Request { cancel(): void }
export interface Requests {
  request(service: string, options: {
    method: string; parameters: Record<string, unknown>; subscribe?: boolean;
    onSuccess(value: unknown): void; onFailure(value: unknown): void;
  }): Request;
}

function snapshot(value: unknown): Snapshot | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const record = value as Record<string, unknown>;
  const keys = ["returnValue", "version", "state", "counter", "pid", "subscribers", "stopAcknowledged", "cleanupConfirmed", "exitCode"];
  if (Object.keys(record).length !== keys.length || Object.keys(record).some((key) => !keys.includes(key))) return undefined;
  if (record.returnValue !== true || record.version !== VERSION || typeof record.state !== "string" || !["running", "closed", "failed"].includes(record.state)) return undefined;
  for (const [key, min, max] of [["counter", 0, MAX_SEQUENCE], ["pid", 1, 0xffffffff], ["subscribers", 0, 2]] as const) {
    if (typeof record[key] !== "number" || !Number.isInteger(record[key]) || record[key] < min || record[key] > max) return undefined;
  }
  if (typeof record.stopAcknowledged !== "boolean" || typeof record.cleanupConfirmed !== "boolean") return undefined;
  if (record.exitCode !== null && (typeof record.exitCode !== "number" || !Number.isInteger(record.exitCode) || record.exitCode < 0 || record.exitCode > 255)) return undefined;
  if (record.state === "running" && (record.cleanupConfirmed || record.exitCode !== null)) return undefined;
  if (record.state === "closed" && !record.cleanupConfirmed) return undefined;
  return record as Snapshot;
}

/** Owns only this WAM app's bus handles. External callbacks cannot publish after dispose. */
export class Client {
  private disposed = false;
  private subscription: Request | undefined;
  private subscriptionActive = false;
  private readonly requests = new Set<Request>();
  private readonly settlements = new Map<() => void, "ping" | "close">();
  private attachTimer: ReturnType<typeof setTimeout> | undefined;
  private started: Promise<boolean> | undefined;
  private pending: ((value: boolean) => void) | undefined;
  private last: Snapshot | undefined;

  constructor(private readonly bus: Requests, private readonly publish: (value: Snapshot | "unavailable") => void) {}

  attach(): Promise<boolean> {
    if (this.disposed) return Promise.resolve(false);
    if (this.started) return this.started;
    this.started = new Promise<boolean>((resolve) => { this.pending = resolve; });
    this.subscriptionActive = true;
    this.attachTimer = setTimeout(() => this.fail(), 3000);
    try {
      const handle = this.bus.request(`luna://${SERVICE_ID}`, {
        method: "attach", parameters: { version: VERSION }, subscribe: true,
        onSuccess: (value) => {
          if (this.disposed || !this.subscriptionActive) return;
          const accepted = this.accept(value);
          clearTimeout(this.attachTimer);
          this.pending?.(accepted?.state === "running"); this.pending = undefined;
        },
        onFailure: () => { if (!this.disposed && this.subscriptionActive) this.fail(); },
      });
      if (this.disposed || !this.subscriptionActive) this.cancel(handle); else this.subscription = handle;
    } catch { this.fail(); }
    return this.started;
  }

  async action(method: "ping" | "close"): Promise<boolean> {
    if (this.disposed || !(await this.attach()) || this.disposed || this.last?.state !== "running" || this.requests.size > 0) return false;
    return new Promise<boolean>((resolve) => {
      let handle: Request | undefined;
      let settled = false;
      let timer: ReturnType<typeof setTimeout> | undefined;
      const retire = () => finish(undefined);
      const finish = (value: unknown) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.settlements.delete(retire);
        if (handle) { this.requests.delete(handle); this.cancel(handle); }
        if (this.disposed) { resolve(false); return; }
        resolve(this.accept(value) !== undefined);
      };
      this.settlements.set(retire, method);
      timer = setTimeout(retire, 3000);
      try {
        handle = this.bus.request(`luna://${SERVICE_ID}`, {
          method, parameters: { version: VERSION }, onSuccess: finish, onFailure: () => finish(undefined),
        });
        if (settled || this.disposed) this.cancel(handle); else this.requests.add(handle);
      } catch { finish(undefined); }
    });
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.subscriptionActive = false;
    clearTimeout(this.attachTimer);
    this.pending?.(false); this.pending = undefined;
    if (this.subscription) this.cancel(this.subscription); this.subscription = undefined;
    for (const settle of [...this.settlements.keys()]) settle();
    for (const request of this.requests) this.cancel(request);
    this.requests.clear();
  }

  private accept(value: unknown): Snapshot | undefined {
    const accepted = snapshot(value);
    if (this.last?.state === "closed" && accepted?.state !== "closed") return undefined;
    if (!accepted || (this.last && (accepted.pid !== this.last.pid || accepted.counter < this.last.counter))) { this.fail(); return undefined; }
    this.last = accepted;
    this.publish(accepted);
    if (accepted.state === "closed") {
      this.subscriptionActive = false;
      if (this.subscription) this.cancel(this.subscription); this.subscription = undefined;
      for (const [settle, method] of [...this.settlements]) if (method === "ping") settle();
    }
    return accepted;
  }

  private fail(): void {
    if (this.disposed) return;
    this.pending?.(false); this.pending = undefined;
    this.publish("unavailable");
    this.dispose();
  }

  private cancel(request: Request): void { try { request.cancel(); } catch { /* Retirement owns all local admission. */ } }
}
