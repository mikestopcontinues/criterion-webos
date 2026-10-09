export const VERSION = "0.1.0";
export const UI_ID = "com.mikestopcontinues.criterion.probe.ui";
export const PLAYER_ID = "com.mikestopcontinues.criterion.probe.player";
export const SERVICE_ID = `${PLAYER_ID}.bridge`;
export const BROKER_ARGS = ["--lease-ms", "10000", "--max-ms", "120000"] as const;
export const MAX_SEQUENCE = 1_000_000;

export type Failure = "unauthorized" | "invalidRequest" | "versionMismatch" | "busy" | "unavailable" | "closed" | "cleanupUnconfirmed";
export type Snapshot = {
  returnValue: true;
  version: typeof VERSION;
  state: "running" | "closed" | "failed";
  counter: number;
  pid: number;
  subscribers: number;
  stopAcknowledged: boolean;
  cleanupConfirmed: boolean;
  exitCode: number | null;
};
export type Reply = Snapshot | { returnValue: false; errorCode: Failure };

export interface BusMessage {
  sender: unknown;
  uniqueToken: unknown;
  payload: unknown;
  isSubscription: unknown;
  respond(reply: Reply): void;
  cancel(): void;
}

export interface BusService {
  register(method: string, request: (message: BusMessage) => void, cancel?: (message: BusMessage) => void): unknown;
}
