import { createHash } from "node:crypto";
import { APP_ID, VERSION } from "./admission.js";

export const REQUIRED_SOURCES = ["Cargo.toml", "Dockerfile", "rust-toolchain.toml", ".cargo/config.toml", "crates/criterion-app/Cargo.toml", "crates/criterion-app/src/main.rs"] as const;
export type BuildReceipt = {
  schemaVersion: 1; appId: typeof APP_ID; version: typeof VERSION;
  target: "arm-unknown-linux-gnueabi"; profile: "release"; sourceCommit: string;
  cargoLockSha256: string; executableSha256: string; sourceSha256: Record<string, string>;
};
export const sha256 = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");
export async function admitBuildReceipt(bytes: Buffer, executable: Buffer, readSource: (path: string) => Promise<Buffer>): Promise<BuildReceipt> {
  const invalid = () => { throw new Error("invalidReceipt"); };
  if (bytes.length < 2 || bytes.length > 256 * 1024) invalid();
  let value: unknown;
  try { value = JSON.parse(bytes.toString("utf8")); } catch { invalid(); }
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid();
  const record = value as Record<string, unknown>;
  const keys = ["schemaVersion", "appId", "version", "target", "profile", "sourceCommit", "cargoLockSha256", "executableSha256", "sourceSha256"];
  if (Object.keys(record).length !== keys.length || Object.keys(record).some((key) => !keys.includes(key))
    || record.schemaVersion !== 1 || record.appId !== APP_ID || record.version !== VERSION || record.target !== "arm-unknown-linux-gnueabi" || record.profile !== "release"
    || typeof record.sourceCommit !== "string" || !/^[a-f0-9]{40}$/.test(record.sourceCommit)) invalid();
  for (const key of ["cargoLockSha256", "executableSha256"]) if (typeof record[key] !== "string" || !/^[a-f0-9]{64}$/.test(record[key])) invalid();
  const sources = record.sourceSha256;
  if (!sources || typeof sources !== "object" || Array.isArray(sources)) invalid();
  const hashes = sources as Record<string, unknown>;
  const paths = Object.keys(hashes);
  if (paths.length > 1024 || REQUIRED_SOURCES.some((path) => !Object.hasOwnProperty.call(hashes, path))) invalid();
  for (const path of paths) {
    const fixed = [...REQUIRED_SOURCES, "Cargo.lock"].includes(path);
    const source = path.startsWith("crates/") && /\.(rs|c|h|toml|glsl|vert|frag|wgsl)$/.test(path);
    if (path.length > 255 || (!fixed && !source) || !path.split("/").every((part) => /^[A-Za-z0-9_.-]+$/.test(part) && part !== "." && part !== "..")
      || typeof hashes[path] !== "string" || !/^[a-f0-9]{64}$/.test(hashes[path])) invalid();
  }
  const receipt = record as BuildReceipt;
  if (sha256(executable) !== receipt.executableSha256) throw new Error("executableMismatch");
  const frozen: [string, string][] = [["Cargo.lock", receipt.cargoLockSha256], ...Object.entries(receipt.sourceSha256)];
  for (const [path, expected] of frozen) {
    const current = await readSource(path);
    if (current.length > 32 * 1024 * 1024 || sha256(current) !== expected) throw new Error("sourceMismatch");
  }
  return receipt;
}
