import { APP_ID, VERSION } from "./admission.js";

export const APPINFO = {
  id: APP_ID, version: VERSION, type: "native", main: "criterion-unofficial", title: "Criterion Unofficial",
  appDescription: "Development build; TV behavior and playback are unverified.", icon: "icon.png",
  nativeLifeCycleInterfaceVersion: 2, handlesRelaunch: false,
} as const;
export function admitManifest(bytes: Buffer): void {
  let value: unknown;
  try { value = JSON.parse(bytes.toString("utf8")); } catch { throw new Error("invalidManifest"); }
  if (bytes.length > 4096 || !value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalidManifest");
  const record = value as Record<string, unknown>;
  if (Object.keys(record).length !== Object.keys(APPINFO).length
    || Object.entries(APPINFO).some(([key, expected]) => record[key] !== expected)) throw new Error("invalidManifest");
}

export const PAYLOAD_NAMES = ["appinfo.json", "criterion-unofficial", "icon.png", "LICENSE", "NOTICES.md", "NOTICE-platform.md", "NOTICE-egui_glow.md", "LICENSE-egui_glow-MIT", "LICENSE-egui_glow-APACHE", "NOTICE-image-webp.md", "LICENSE-image-webp-MIT", "LICENSE-image-webp-APACHE"] as const;
