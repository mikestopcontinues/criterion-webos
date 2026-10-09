import { spawn } from "node:child_process";
import { join } from "node:path";
import { Controller } from "./controller.js";
import { BROKER_ARGS, SERVICE_ID, type BusService } from "./protocol.js";

// The TV supplies this documented system module; it is never bundled from npm.
const implementation: unknown = require("webos-service");
if (typeof implementation !== "function") throw new Error("serviceUnavailable");
const Service = implementation as new (name: string) => BusService;
const service = new Service(SERVICE_ID);
const controller = new Controller(() => spawn(join(__dirname, "bin", "criterion-broker-probe"), [...BROKER_ARGS], {
  shell: false, detached: false, stdio: ["pipe", "pipe", "pipe"],
}));
service.register("attach", (message) => { void controller.attach(message); }, (message) => { void controller.cancel(message); });
service.register("ping", (message) => { void controller.ping(message); });
service.register("close", (message) => { void controller.close(message); });
let terminating = false;
function terminate(status: number): void {
  if (terminating) return;
  terminating = true;
  void controller.dispose().then((cleanup) => process.exit(cleanup.confirmed ? status : 2));
}
process.once("SIGTERM", () => terminate(0));
process.once("SIGINT", () => terminate(0));
process.once("uncaughtException", () => terminate(2));
process.once("unhandledRejection", () => terminate(2));
