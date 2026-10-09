import { createHash } from "node:crypto";
export type SourceFile = { name: string; bytes: Buffer; mode: 0o644 | 0o755 };
export type GitReader = (operation: "head" | "status" | "commit" | "tree" | "blob", object?: string) => Buffer;
export type ProjectSnapshot = { revision: string; commit: Buffer; files: SourceFile[] };
const gitHash = (kind: "commit" | "tree" | "blob", bytes: Buffer): string => createHash("sha1").update(`${kind} ${bytes.length}\0`).update(bytes).digest("hex");
function invalid(): never { throw new Error("invalidProjectSource"); }
export function safeName(name: string): boolean {
  return name.length > 0 && name.length <= 240 && name.split("/").every((part) => /^[A-Za-z0-9_.+@-]+$/.test(part) && part !== "." && part !== "..");
}
/** Rejects directory aliases as well as file aliases on case-insensitive source/output filesystems. */
export function unambiguousNames(names: readonly string[]): boolean {
  const seen = new Map<string, string>();
  for (const name of names) {
    const parts = name.split("/");
    for (let count = 1; count <= parts.length; count += 1) {
      const path = parts.slice(0, count).join("/"); const key = path.toLowerCase(); const previous = seen.get(key);
      if (previous !== undefined && previous !== path) return false; seen.set(key, path);
    }
  }
  return true;
}
const privateParts = new Set([".git", ".local", ".cache", ".ssh", ".aws", ".gnupg", "node_modules", "target", "dist", "test-results", "playwright-report"]);
/** Structural private/output exclusions; committed source text is not secret-scanned. */
function publicProjectName(name: string): boolean {
  return safeName(name) && !name.split("/").some((part) => privateParts.has(part.toLowerCase()) || ((part.toLowerCase() === ".env" || part.toLowerCase().startsWith(".env.")) && part !== ".env.example"))
    && !/\.(apk|apkm|xapk|ipk)$/i.test(name);
}
type Tree = Map<string, Tree | SourceFile>;
/** Checks the complete source tree against the actual commit object, not a claimed revision. */
export function admitProject(snapshot: ProjectSnapshot): void {
  const { revision, commit, files } = snapshot;
  if (!/^[a-f0-9]{40}$/.test(revision) || commit.length > 16384 || gitHash("commit", commit) !== revision || files.length < 1 || files.length > 4096) invalid();
  if (!unambiguousNames(files.map((file) => file.name))) invalid();
  const tree: Tree = new Map(); const names = new Set<string>(); let total = 0;
  for (const file of files) {
    if (!publicProjectName(file.name) || names.has(file.name.toLowerCase()) || ![0o644, 0o755].includes(file.mode)) invalid();
    names.add(file.name.toLowerCase()); total += file.bytes.length;
    if (file.bytes.length > 32 * 1024 * 1024 || total > 128 * 1024 * 1024) invalid();
    const parts = file.name.split("/"); let parent = tree;
    for (const part of parts.slice(0, -1)) {
      const entry = parent.get(part);
      if (entry && !(entry instanceof Map)) invalid();
      const child = entry ?? new Map<string, Tree | SourceFile>(); parent.set(part, child); parent = child;
    }
    const name = parts[parts.length - 1]; if (!name || parent.has(name)) invalid(); parent.set(name, file);
  }
  const digest = (entries: Tree): string => {
    const sorted = [...entries].sort(([a, av], [b, bv]) => Buffer.compare(Buffer.from(a + (av instanceof Map ? "/" : "")), Buffer.from(b + (bv instanceof Map ? "/" : ""))));
    const bytes = Buffer.concat(sorted.flatMap(([name, entry]) => [Buffer.from(`${entry instanceof Map ? "40000" : entry.mode === 0o755 ? "100755" : "100644"} ${name}\0`), Buffer.from(entry instanceof Map ? digest(entry) : gitHash("blob", entry.bytes), "hex")]));
    return gitHash("tree", bytes);
  };
  const expected = /^tree ([a-f0-9]{40})\n/.exec(commit.toString("utf8"))?.[1];
  if (!expected || digest(tree) !== expected) invalid();
}
/** Host Git owns revision/clean-state admission; only committed blobs are exported. */
export function captureProject(revision: string, read: GitReader): ProjectSnapshot {
  if (!/^[a-f0-9]{40}$/.test(revision) || read("head").toString() !== revision + "\n" || read("status").length !== 0) throw new Error("dirtyOrWrongRevision");
  const commit = read("commit", revision);
  if (commit.length > 16384 || gitHash("commit", commit) !== revision) invalid();
  const records = read("tree", revision).toString("utf8").split("\0");
  if (records.pop() !== "" || records.length < 1 || records.length > 4096) invalid();
  const files: SourceFile[] = []; const names = new Set<string>(); let total = 0;
  for (const record of records) {
    const match = /^(100644|100755) blob ([a-f0-9]{40})\t(.+)$/.exec(record);
    if (!match || !match[2] || !match[3] || !publicProjectName(match[3]) || names.has(match[3])) invalid();
    const bytes = read("blob", match[2]);
    total += bytes.length;
    if (bytes.length > 32 * 1024 * 1024 || total > 128 * 1024 * 1024 || gitHash("blob", bytes) !== match[2]) invalid();
    names.add(match[3]); files.push({ name: match[3], bytes, mode: match[1] === "100755" ? 0o755 : 0o644 });
  }
  if (read("head").toString() !== revision + "\n" || read("status").length !== 0) throw new Error("dirtyOrWrongRevision");
  const snapshot = { revision, commit, files: files.sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name))) };
  admitProject(snapshot); return snapshot;
}
