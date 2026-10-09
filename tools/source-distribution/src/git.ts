import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import type { GitReader } from "./project.js";
/** Host-owned built-in Git reads with no hooks, replacements, filters or inherited Git config. */
export function hostGit(root: string): GitReader {
  const absolute = resolve(root); if (realpathSync(absolute) !== absolute) throw new Error("invalidGitRoot");
  return (operation, object) => {
    if ((operation === "blob" || operation === "commit" || operation === "tree") && (!object || !/^[a-f0-9]{40}$/.test(object))) throw new Error("invalidGitObject");
    const args = operation === "head" ? ["rev-parse", "--verify", "HEAD"] : operation === "status" ? ["status", "--porcelain=v1", "--untracked-files=normal"] : operation === "tree" ? ["ls-tree", "-rz", "--full-tree", object ?? ""] : ["cat-file", operation, object ?? ""];
    const result = spawnSync("git", ["--no-replace-objects", "-c", "core.fsmonitor=false", "-c", "core.untrackedCache=false", "-c", "core.hooksPath=/dev/null", "-C", absolute, ...args], {
      shell: false, timeout: 60000, maxBuffer: operation === "blob" ? 32 * 1024 * 1024 + 1 : 2 * 1024 * 1024,
      env: { PATH: "/opt/homebrew/bin:/usr/bin:/bin", LANG: "C", LC_ALL: "C", TZ: "UTC", GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null", GIT_OPTIONAL_LOCKS: "0" },
    });
    if (result.error || result.status !== 0 || result.stderr.length !== 0) throw new Error("gitSourceReadFailed"); return result.stdout;
  };
}
