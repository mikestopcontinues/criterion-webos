import { Client, type Requests } from "./client.js";
import { EmeProbe, type BridgeWitness } from "./eme.js";
import { PLAYER_ID, UI_ID } from "./protocol.js";

declare const PROBE_ROLE: "ui" | "player";
declare global {
  interface Window {
    webOS?: { service: Requests; platformBack(): void };
    webOSSystem?: { activate(): void };
  }
}
const role = PROBE_ROLE;
const status = document.getElementById("status");
const capabilities = document.getElementById("capabilities");
const emeButton = document.querySelector<HTMLButtonElement>("#eme");
const emeResults = document.getElementById("eme-results");
const bus = window.webOS?.service;
if (status && bus) {
  let cleanupConfirmed = false;
  let alive = true;
  let consumed = false;
  let witness: BridgeWitness | undefined;
  let eme: EmeProbe | undefined;
  const client = new Client(bus, (value) => {
    if (value !== "unavailable" && value.state === "closed" && value.cleanupConfirmed) cleanupConfirmed = true;
    status.textContent = value === "unavailable" ? "Bridge unavailable" : `${value.state} · PID ${value.pid} · counter ${value.counter} · subscriptions ${value.subscribers} · stop ${value.stopAcknowledged} · cleanup ${value.cleanupConfirmed} · exit ${value.exitCode ?? "pending"}`;
    witness = value !== "unavailable" && value.state === "running"
      ? { pid: value.pid, counter: value.counter, at: performance.now() } : undefined;
    if (!witness) eme?.stop();
    if (emeButton) emeButton.disabled = role !== "player" || !alive || consumed || !witness;
  });
  // These indicators report presence; the explicit PLAYER action owns access queries.
  if (capabilities) capabilities.textContent = `Secure context: ${window.isSecureContext} · MSE: ${typeof MediaSource !== "undefined"} · EME: ${typeof navigator.requestMediaKeySystemAccess === "function"}`;
  if (emeButton && emeResults) {
    emeButton.hidden = role !== "player";
    emeResults.hidden = role !== "player";
    eme = new EmeProbe({
      environment: () => ({ player: role === "player" && alive, secure: window.isSecureContext,
        topLevel: window.top === window, visible: document.visibilityState === "visible" }),
      bridge: () => witness,
      ping: () => client.action("ping"),
      request: typeof navigator.requestMediaKeySystemAccess === "function"
        ? (keySystem, configurations) => navigator.requestMediaKeySystemAccess(keySystem, configurations) : undefined,
      now: () => performance.now(),
      schedule: (delay, callback) => { const timer = setTimeout(callback, delay); return () => clearTimeout(timer); },
      publish: (report) => { if (alive) emeResults.textContent = report; },
    });
  }
  const measure = () => {
    if (!alive || consumed || !eme || role !== "player") return;
    consumed = true;
    if (emeButton) emeButton.disabled = true;
    void eme.run().catch(() => eme?.stop());
  };
  const visibility = () => { if (document.visibilityState !== "visible") eme?.stop(); };
  let launching = false;
  let launchHandle: ReturnType<Requests["request"]> | undefined;
  const activate = () => { void client.attach().then(() => { if (alive) window.webOSSystem?.activate(); }); };
  const transfer = () => {
    if (launching || !alive) return;
    eme?.stop();
    launching = true;
    void client.attach().then((ready) => {
      if (!alive || !ready) { launching = false; return; }
      try {
        launchHandle = bus.request("luna://com.webos.applicationManager", {
          method: "launch", parameters: { id: role === "ui" ? PLAYER_ID : UI_ID },
          onSuccess: () => { if (alive) launching = false; },
          onFailure: () => { if (alive) { launching = false; status.textContent = "Paired app launch failed"; } },
        });
      } catch { launching = false; status.textContent = "Paired app launch failed"; }
    });
  };
  const key = (event: KeyboardEvent) => {
    if (event.keyCode !== 461) return;
    event.preventDefault();
    if (role === "player") transfer();
    else if (cleanupConfirmed) window.webOS?.platformBack();
    else void client.action("close").then((closed) => { if (alive && (closed || cleanupConfirmed)) window.webOS?.platformBack(); });
  };
  const ping = () => { void client.action("ping"); };
  const close = () => { eme?.stop(); void client.action("close"); };
  const dispose = () => {
    if (!alive) return;
    alive = false;
    eme?.stop();
    launchHandle?.cancel(); client.dispose();
    window.removeEventListener("webOSLaunch", activate);
    window.removeEventListener("webOSRelaunch", activate);
    window.removeEventListener("keydown", key);
    document.getElementById("transfer")?.removeEventListener("click", transfer);
    document.getElementById("ping")?.removeEventListener("click", ping);
    document.getElementById("close")?.removeEventListener("click", close);
    emeButton?.removeEventListener("click", measure);
    document.removeEventListener("visibilitychange", visibility);
    window.removeEventListener("pagehide", dispose);
  };
  window.addEventListener("webOSLaunch", activate);
  window.addEventListener("webOSRelaunch", activate);
  window.addEventListener("keydown", key);
  window.addEventListener("pagehide", dispose);
  document.getElementById("transfer")?.addEventListener("click", transfer);
  document.getElementById("ping")?.addEventListener("click", ping);
  document.getElementById("close")?.addEventListener("click", close);
  emeButton?.addEventListener("click", measure);
  document.addEventListener("visibilitychange", visibility);
  // Listener admission precedes activation, including already-launched cold startup.
  activate();
  document.getElementById("transfer")?.focus();
} else if (status) { status.textContent = "webOS bridge unavailable"; }
