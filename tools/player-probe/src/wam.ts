import { Client, type Requests } from "./client.js";
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
const bus = window.webOS?.service;
if (status && bus) {
  let cleanupConfirmed = false;
  const client = new Client(bus, (value) => {
    if (value !== "unavailable" && value.state === "closed" && value.cleanupConfirmed) cleanupConfirmed = true;
    status.textContent = value === "unavailable" ? "Bridge unavailable" : `${value.state} · PID ${value.pid} · counter ${value.counter} · subscriptions ${value.subscribers} · stop ${value.stopAcknowledged} · cleanup ${value.cleanupConfirmed} · exit ${value.exitCode ?? "pending"}`;
  });
  // Presence only. No key-system, media, manifest or license requests.
  if (capabilities) capabilities.textContent = `Secure context: ${window.isSecureContext} · MSE: ${typeof MediaSource !== "undefined"} · EME: ${typeof navigator.requestMediaKeySystemAccess === "function"}`;
  let alive = true;
  let launching = false;
  let launchHandle: ReturnType<Requests["request"]> | undefined;
  const activate = () => { void client.attach().then(() => { if (alive) window.webOSSystem?.activate(); }); };
  const transfer = () => {
    if (launching || !alive) return;
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
  const close = () => { void client.action("close"); };
  const dispose = () => {
    if (!alive) return;
    alive = false;
    launchHandle?.cancel(); client.dispose();
    window.removeEventListener("webOSLaunch", activate);
    window.removeEventListener("webOSRelaunch", activate);
    window.removeEventListener("keydown", key);
    document.getElementById("transfer")?.removeEventListener("click", transfer);
    document.getElementById("ping")?.removeEventListener("click", ping);
    document.getElementById("close")?.removeEventListener("click", close);
    window.removeEventListener("pagehide", dispose);
  };
  window.addEventListener("webOSLaunch", activate);
  window.addEventListener("webOSRelaunch", activate);
  window.addEventListener("keydown", key);
  window.addEventListener("pagehide", dispose);
  document.getElementById("transfer")?.addEventListener("click", transfer);
  document.getElementById("ping")?.addEventListener("click", ping);
  document.getElementById("close")?.addEventListener("click", close);
  // Listener admission precedes activation, including already-launched cold startup.
  activate();
  document.getElementById("transfer")?.focus();
} else if (status) { status.textContent = "webOS bridge unavailable"; }
