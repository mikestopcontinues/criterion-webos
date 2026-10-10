/** Fixed inert launcher; the phase implementation is loaded from freshly compiled captured source. */
export function offlineWorkerLaunch(deadlineMs: number): string {
  if (!Number.isSafeInteger(deadlineMs) || deadlineMs < 1) throw new Error("invalidOfflineWorker");
  const input = { sourceRoot: "/workspace", outputDirectory: "/workspace/.local/native-package", registryArchivesRoot: "/offline/registry", installedRuntimeRoot: "/offline/runtime", registryRoot: "/offline/vendor" };
  return '"use strict";\nconst phase = require("/workspace/.local/native-package/compiled/tools/package-native/offline-phase.js");\n'
    + `phase.prepareOfflineMainFromFiles(${JSON.stringify(input)}, {deadlineMs:${deadlineMs},now:Date.now}).then(result => { process.stdout.write(JSON.stringify(result) + "\\n"); }).catch(error => {\n`
    + 'const outcome = error && error.outcome; process.stderr.write(JSON.stringify({error:error instanceof Error ? error.message : "offlinePhaseFailed",args:error && error.args || null,outcome:outcome ? {...outcome,stdout:outcome.stdout.toString("base64"),stderr:outcome.stderr.toString("base64")} : null}) + "\\n"); process.exitCode = 1;\n});\n';
}
