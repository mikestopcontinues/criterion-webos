import type { ChildProcessWithoutNullStreams } from "node:child_process";
import { setTimeout, clearTimeout } from "node:timers";
import { MAX_SEQUENCE, type Failure } from "./protocol.js";

export class ProbeError extends Error {
  constructor(readonly code: Failure) { super(code); }
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: ProbeError) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

export type Cleanup = { confirmed: boolean; exitCode: number | null };

async function within<T>(promise: Promise<T>, milliseconds: number): Promise<T | undefined> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<undefined>((resolve) => { timer = setTimeout(() => resolve(undefined), milliseconds); });
  const result = await Promise.race([promise, timeout]);
  clearTimeout(timer);
  return result;
}

/** Owns one fixed child and all pipe callbacks until actual child close/reap. */
export class Broker {
  state: "starting" | "running" | "closing" | "closed" | "failed" = "starting";
  counter = 0;
  readonly pid: number;
  stopAcknowledged = false;
  startedConfirmed = false;
  cleanup: Cleanup = { confirmed: false, exitCode: null };
  private readonly ready = deferred<void>();
  private readonly completion = deferred<Cleanup>();
  readonly closed = this.completion.promise;
  private pending: ReturnType<typeof deferred<number>> | undefined;
  private sequence = 0;
  private buffer = "";
  private outputBytes = 0;
  private errorBytes = 0;
  private shutdown: Promise<Cleanup> | undefined;
  private readonly startupTimer: ReturnType<typeof setTimeout>;

  constructor(private readonly child: ChildProcessWithoutNullStreams) {
    this.pid = child.pid ?? 0;
    this.startupTimer = setTimeout(() => this.fail(), 1000);
    child.stdout.on("data", this.onData);
    child.stderr.on("data", this.onErrorData);
    child.stdin.on("error", () => this.fail());
    child.on("error", () => this.fail());
    child.once("close", (exitCode) => {
      clearTimeout(this.startupTimer);
      child.stdout.removeListener("data", this.onData);
      child.stderr.removeListener("data", this.onErrorData);
      this.state = "closed";
      this.ready.reject(new ProbeError("unavailable"));
      this.pending?.reject(new ProbeError("closed"));
      this.pending = undefined;
      this.cleanup = { confirmed: true, exitCode };
      this.completion.resolve(this.cleanup);
    });
  }

  async started(): Promise<void> { await this.ready.promise; }

  async ping(): Promise<number> {
    await this.started();
    if (this.state !== "running") throw new ProbeError("closed");
    if (this.pending) throw new ProbeError("busy");
    if (this.sequence >= MAX_SEQUENCE) throw new ProbeError("unavailable");
    this.sequence += 1;
    const pending = deferred<number>();
    this.pending = pending;
    const timer = setTimeout(() => this.fail(), 1000);
    try {
      try { this.child.stdin.write(`ping ${this.sequence}\n`); } catch { this.fail(); }
      return await pending.promise;
    } finally {
      clearTimeout(timer);
      if (this.pending === pending) this.pending = undefined;
    }
  }

  close(): Promise<Cleanup> {
    this.shutdown ??= this.stop();
    return this.shutdown;
  }

  private async stop(): Promise<Cleanup> {
    if (this.cleanup.confirmed) return this.cleanup;
    this.state = "closing";
    clearTimeout(this.startupTimer);
    this.ready.reject(new ProbeError("closed"));
    this.pending?.reject(new ProbeError("closed"));
    this.pending = undefined;
    try { this.child.stdin.end("stop\n"); } catch { /* Close still owns retirement. */ }
    let completed = await within(this.closed, 1000);
    if (completed) return completed;
    try { this.child.kill("SIGTERM"); } catch { /* Signal acceptance is not cleanup proof. */ }
    completed = await within(this.closed, 500);
    if (completed) return completed;
    try { this.child.kill("SIGKILL"); } catch { /* Retain the child if close stays absent. */ }
    completed = await within(this.closed, 1000);
    if (completed) return completed;
    this.state = "failed";
    return this.cleanup;
  }

  private fail(): void {
    if (this.state === "closed") return;
    this.ready.reject(new ProbeError("unavailable"));
    this.pending?.reject(new ProbeError("unavailable"));
    void this.close();
  }

  private readonly onErrorData = (bytes: Buffer): void => {
    this.errorBytes += bytes.length;
    // Drain without retaining or logging child stderr.
    if (this.errorBytes > 4096) this.fail();
  };

  private readonly onData = (bytes: Buffer): void => {
    this.outputBytes += bytes.length;
    if (bytes.length > 4096 || this.outputBytes > 32768) { this.fail(); return; }
    this.buffer += bytes.toString("utf8");
    let newline = this.buffer.indexOf("\n");
    while (newline >= 0) {
      const line = this.buffer.slice(0, newline);
      this.buffer = this.buffer.slice(newline + 1);
      if (line.length > 48 || !this.frame(line)) { this.fail(); return; }
      newline = this.buffer.indexOf("\n");
    }
    if (this.buffer.length > 48) this.fail();
  };

  private frame(line: string): boolean {
    let match = /^ready ([1-9][0-9]{0,9})$/.exec(line);
    if (match) {
      if (this.startedConfirmed || (this.state !== "starting" && this.state !== "closing") || Number(match[1]) !== this.pid || this.pid <= 0) return false;
      this.startedConfirmed = true;
      clearTimeout(this.startupTimer);
      if (this.state === "starting") {
        this.state = "running";
        this.ready.resolve();
      }
      return true;
    }
    match = /^pong ([1-9][0-9]{0,6}) ([1-9][0-9]{0,6}) ([1-9][0-9]{0,9})$/.exec(line);
    if (match) {
      const sequence = Number(match[1]);
      const counter = Number(match[2]);
      if (!this.startedConfirmed || sequence !== this.sequence || counter !== sequence || counter !== this.counter + 1 || sequence > MAX_SEQUENCE || Number(match[3]) !== this.pid) return false;
      if (this.state !== "running" && this.state !== "closing") return false;
      this.counter = counter;
      if (this.state === "running") {
        if (!this.pending) return false;
        this.pending.resolve(counter);
      }
      return true;
    }
    match = /^stopped (0|[1-9][0-9]{0,6}) ([1-9][0-9]{0,9})$/.exec(line);
    if (match) {
      const counter = Number(match[1]);
      if (!this.startedConfirmed || this.stopAcknowledged || this.state !== "closing" || counter !== this.sequence || Number(match[2]) !== this.pid) return false;
      this.counter = counter;
      this.stopAcknowledged = true;
      return true;
    }
    return false;
  }
}
