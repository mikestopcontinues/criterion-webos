export type RegistrySource = { name: string; version: string; sha256: string };
const registry = "registry+https://github.com/rust-lang/crates.io-index";
function invalid(): never { throw new Error("invalidSourceLock"); }
function parse(bytes: Buffer): RegistrySource[] {
  if (bytes.length < 1 || bytes.length > 2 * 1024 * 1024) invalid();
  const text = bytes.toString("utf8");
  if (!Buffer.from(text).equals(bytes) || !/^version = 4$/m.test(text)) invalid();
  const result: RegistrySource[] = [];
  const sections = text.split(/^\[\[package\]\]$/m);
  const prefix = sections[0];
  if (!prefix || prefix.split("\n").some((line) => line !== "" && line !== "version = 4" && !line.startsWith("#")) || prefix.split("\n").filter((line) => line === "version = 4").length !== 1 || /^\s*\[(?:\[|[A-Za-z_"])/m.test(sections.slice(1).join("\n"))) invalid();
  const parts = sections.slice(1);
  if (parts.length < 1 || parts.length > 1024 || [...text.matchAll(/^\s*\[\[package\]\]/gm)].length !== parts.length) invalid();
  for (const part of parts) {
    let dependencies = false; let seenDependencies = false;
    for (const line of part.split("\n")) {
      if (line === "" || line.startsWith("#")) continue;
      if (dependencies) {
        if (line === "]") dependencies = false;
        else if (!/^ "[^"\\]+",$/.test(line)) invalid();
      } else if (line === "dependencies = [") {
        if (seenDependencies) invalid(); seenDependencies = true; dependencies = true;
      } else if (!/^(name|version|source|checksum) = "[^"\\]+"$/.test(line)) invalid();
    }
    if (dependencies) invalid();
    const value = (key: string): string | undefined => {
      const matches = [...part.matchAll(new RegExp(`^${key} = "([^"\\n]+)"$`, "gm"))];
      if (matches.length > 1 || [...part.matchAll(new RegExp(`^\\s*${key}\\s*=`, "gm"))].length !== matches.length) invalid();
      return matches[0]?.[1];
    };
    const name = value("name"); const version = value("version"); const source = value("source"); const sha256 = value("checksum");
    if (!name || !/^[A-Za-z0-9_-]{1,80}$/.test(name) || !version || !/^[0-9][A-Za-z0-9.+-]{0,127}$/.test(version)) invalid();
    if (source === undefined) { if (sha256 !== undefined) invalid(); continue; }
    if (source !== registry || !sha256 || !/^[a-f0-9]{64}$/.test(sha256)) invalid();
    result.push({ name, version, sha256 });
  }
  return result;
}
/** Includes every registry source in both locks, not an incorporated-code graph. */
export function lockedSources(project: Buffer, runtime: Buffer): RegistrySource[] {
  const sources = new Map<string, RegistrySource>();
  for (const source of [...parse(project), ...parse(runtime)]) {
    const key = `${source.name}-${source.version}`;
    const previous = sources.get(key);
    if (previous && previous.sha256 !== source.sha256) invalid();
    sources.set(key, source);
  }
  return [...sources].sort(([a], [b]) => Buffer.compare(Buffer.from(a), Buffer.from(b))).map(([, value]) => value);
}
