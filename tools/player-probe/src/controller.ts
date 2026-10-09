import type { ChildProcessWithoutNullStreams } from "node:child_process";
import { setInterval, clearInterval } from "node:timers";
import { Broker, ProbeError, type Cleanup } from "./broker.js";
import { MAX_CALLERS, NATIVE_ID, PLAYER_ID, UI_ID, VERSION, type BusMessage, type Failure, type Reply, type Snapshot } from "./protocol.js";

type Subscriber = { sender: string; message: BusMessage };

export class Controller {
  private broker: Broker | undefined;
  private readonly subscribers = new Map<string, Subscriber>();
  private heartbeat: ReturnType<typeof setInterval> | undefined;
  private disposed = false;
  private readonly actions = new Set<string>();

  constructor(private readonly spawn: () => ChildProcessWithoutNullStreams) {}

  async attach(message: BusMessage): Promise<void> {
    if (!this.valid(message, true)) return;
    const token = message.uniqueToken;
    const sender = message.sender;
    if (typeof token !== "string" || token.length < 1 || token.length > 128 || /[\u0000-\u001f\u007f]/.test(token) || typeof sender !== "string") {
      this.error(message, "invalidRequest"); return;
    }
    if (this.subscribers.size >= MAX_CALLERS || this.subscribers.has(token) || [...this.subscribers.values()].some((subscriber) => subscriber.sender === sender)) {
      this.error(message, "busy"); return;
    }
    if (this.broker && this.broker.state !== "running" && this.broker.state !== "starting" && !this.broker.cleanup.confirmed) {
      this.error(message, "cleanupUnconfirmed"); return;
    }
    const subscriber = { sender, message };
    this.subscribers.set(token, subscriber);
    let broker: Broker;
    try {
      if (!this.broker || this.broker.cleanup.confirmed) {
        broker = new Broker(this.spawn());
        this.broker = broker;
        void broker.closed.then(() => this.finished(broker));
      } else { broker = this.broker; }
      await broker.started();
      if (this.broker !== broker || broker.state !== "running" || this.subscribers.get(token) !== subscriber) return;
      this.publish();
      this.heartbeat ??= setInterval(() => {
        if (this.broker !== broker || broker.state !== "running") return;
        void broker.ping().then(() => { if (this.broker === broker) this.publish(); }).catch((error: unknown) => {
          if (!(error instanceof ProbeError && error.code === "busy")) void broker.close();
        });
      }, 1000);
    } catch {
      if (this.subscribers.get(token) !== subscriber) return;
      this.subscribers.delete(token);
      this.error(message, "unavailable");
    }
  }

  async ping(message: BusMessage): Promise<void> {
    const sender = this.admitAction(message);
    if (!sender) return;
    try {
      const broker = this.broker;
      const subscriber = [...this.subscribers.values()].find((entry) => entry.sender === message.sender);
      if (!broker || broker.state !== "running" || !subscriber) {
        this.error(message, "closed"); return;
      }
      try {
        await broker.ping();
        if (this.disposed || this.broker !== broker || broker.state !== "running" || ![...this.subscribers.values()].includes(subscriber)) { this.error(message, "closed"); return; }
        this.respond(message, this.snapshot(broker));
        this.publish();
      } catch (error: unknown) { this.error(message, error instanceof ProbeError ? error.code : "unavailable"); }
    } finally { this.actions.delete(sender); }
  }

  async close(message: BusMessage): Promise<void> {
    const sender = this.admitAction(message);
    if (!sender) return;
    try {
      const broker = this.broker;
      if (!broker) { this.error(message, "closed"); return; }
      const cleanup = await this.stopCurrent();
      if (!cleanup.confirmed) { this.error(message, "cleanupUnconfirmed"); return; }
      if (this.disposed || this.broker !== broker) { this.error(message, "closed"); return; }
      this.respond(message, this.snapshot(broker));
    } finally { this.actions.delete(sender); }
  }

  async cancel(message: BusMessage): Promise<void> {
    if (typeof message.uniqueToken !== "string") return;
    const subscriber = this.subscribers.get(message.uniqueToken);
    if (!subscriber || subscriber.sender !== message.sender) return;
    this.subscribers.delete(message.uniqueToken);
    if (this.subscribers.size === 0) await this.stopCurrent();
    else this.publish();
  }

  async dispose(): Promise<Cleanup> {
    this.disposed = true;
    const subscribers = [...this.subscribers.values()];
    this.subscribers.clear();
    for (const subscriber of subscribers) {
      try { subscriber.message.cancel(); } catch { /* Retire without publishing after disposal. */ }
    }
    return this.stopCurrent();
  }

  private async stopCurrent(): Promise<Cleanup> {
    clearInterval(this.heartbeat);
    this.heartbeat = undefined;
    return this.broker ? this.broker.close() : { confirmed: true, exitCode: null };
  }

  private admitAction(message: BusMessage): string | undefined {
    if (!this.valid(message, false) || typeof message.sender !== "string") return undefined;
    if (this.actions.has(message.sender)) { this.error(message, "busy"); return undefined; }
    this.actions.add(message.sender);
    return message.sender;
  }

  private valid(message: BusMessage, subscription: boolean): boolean {
    if (this.disposed) { this.error(message, "closed"); return false; }
    if (message.sender !== UI_ID && message.sender !== PLAYER_ID && message.sender !== NATIVE_ID) { this.error(message, "unauthorized"); return false; }
    const payload = message.payload;
    if (!payload || typeof payload !== "object" || Array.isArray(payload)) { this.error(message, "invalidRequest"); return false; }
    const record = payload as Record<string, unknown>;
    if (Object.keys(record).some((key) => key !== "version" && !(subscription && key === "subscribe")) || message.isSubscription !== subscription || (subscription && record.subscribe !== true)) {
      this.error(message, "invalidRequest"); return false;
    }
    if (record.version !== VERSION) { this.error(message, "versionMismatch"); return false; }
    return true;
  }

  private snapshot(broker: Broker): Snapshot {
    return {
      returnValue: true, version: VERSION,
      state: broker.cleanup.confirmed ? "closed" : broker.state === "running" ? "running" : "failed",
      counter: broker.counter, pid: broker.pid, subscribers: this.subscribers.size,
      stopAcknowledged: broker.stopAcknowledged, cleanupConfirmed: broker.cleanup.confirmed,
      exitCode: broker.cleanup.exitCode,
    };
  }

  private publish(): void {
    const broker = this.broker;
    if (!broker || broker.state !== "running") return;
    for (const [token, subscriber] of [...this.subscribers]) {
      if (!this.respond(subscriber.message, this.snapshot(broker))) {
        this.subscribers.delete(token);
        try { subscriber.message.cancel(); } catch { /* Retire the unreachable subscription. */ }
      }
    }
    if (this.subscribers.size === 0) void this.stopCurrent();
  }

  private finished(broker: Broker): void {
    if (this.broker !== broker) return;
    clearInterval(this.heartbeat);
    this.heartbeat = undefined;
    const subscribers = [...this.subscribers.values()];
    this.subscribers.clear();
    for (const subscriber of subscribers) {
      this.respond(subscriber.message, broker.startedConfirmed ? this.snapshot(broker) : { returnValue: false, errorCode: "unavailable" });
      try { subscriber.message.cancel(); } catch { /* Remote cancellation is isolated. */ }
    }
  }

  private error(message: BusMessage, code: Failure): void {
    this.respond(message, { returnValue: false, errorCode: code });
    if (message.isSubscription === true) {
      try { message.cancel(); } catch { /* No remote callback can retain owned work. */ }
    }
  }

  private respond(message: BusMessage, reply: Reply): boolean {
    try { message.respond(reply); return true; } catch { return false; }
  }
}
