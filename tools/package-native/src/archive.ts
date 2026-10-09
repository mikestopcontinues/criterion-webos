import { inspectIpk, type PackageFiles } from "../../player-probe/src/normalize.js";
import { APP_ID, MAX_IPK_BYTES, VERSION, admitExecutable } from "./admission.js";
import { PAYLOAD_NAMES, admitManifest } from "./manifest.js";
import { sha256 } from "./receipt.js";

export type Payload = PackageFiles;
export type FileSeal = { sha256: string; bytes: number; mode: number };
export const IDENTITY = { id: APP_ID, version: VERSION, architecture: "arm" } as const;
export function archiveFiles(expected: Payload): PackageFiles {
  if (expected.size !== PAYLOAD_NAMES.length || PAYLOAD_NAMES.some((name) => !expected.has(name))) throw new Error("invalidIpkPayload");
  if ([...expected].some(([name, file]) => file.mode !== (name === "criterion-unofficial" ? 0o755 : 0o644))) throw new Error("invalidIpkPayload");
  const manifest = expected.get("appinfo.json"); const executable = expected.get("criterion-unofficial");
  if (!manifest || !executable) throw new Error("invalidIpkPayload");
  admitManifest(manifest.bytes); admitExecutable(executable.bytes);
  const payload = new Map([...expected].map(([name, file]) => [`usr/palm/applications/${APP_ID}/${name}`, file]));
  payload.set(`usr/palm/packages/${APP_ID}/packageinfo.json`, {
    bytes: Buffer.from(JSON.stringify({ id: APP_ID, version: VERSION, app: APP_ID }, null, 2) + "\n"), mode: 0o644,
  });
  return payload;
}
export function auditIpk(bytes: Buffer, expected: Payload): { files: Record<string, FileSeal>; appId: typeof APP_ID; controlSha256: string } {
  const control = inspectIpk(bytes, archiveFiles(expected), IDENTITY, MAX_IPK_BYTES);
  return { appId: APP_ID, controlSha256: sha256(control), files: Object.fromEntries([...expected].map(([name, file]) => [name, { sha256: sha256(file.bytes), bytes: file.bytes.length, mode: file.mode }])) };
}
