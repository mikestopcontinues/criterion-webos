// Test-only pipe peer. This file is excluded from both IPKs.
import { spawn } from "node:child_process";
const mode = process.argv[2];
if (mode === "hold") {
  setTimeout(() => process.exit(0), 3200);
} else if (mode === "inherited-pipe") {
  const child = spawn(process.execPath, [__filename, "hold"], { shell: false, detached: false, stdio: ["ignore", "inherit", "inherit"] });
  child.unref();
  process.stdout.write(`ready ${process.pid}\n`);
  process.stdin.once("data", () => process.exit(0));
} else {
  process.stdout.write(mode === "wrong-pid" ? "ready 9999999999\n" : `ready ${process.pid}\n`);
  if (mode === "stderr") process.stderr.write("x".repeat(4097));
  if (mode === "oversized") process.stdout.write("x".repeat(4097));
  if (mode === "ignore-stop") {
    process.on("SIGTERM", () => { /* Force escalation. */ });
    setInterval(() => { /* Keep this deliberately unresponsive peer alive after EOF. */ }, 1000);
  }
  if (mode === "no-pong") process.on("SIGTERM", () => process.exit(0));
  process.stdin.on("data", (bytes: Buffer) => {
    const line = bytes.toString();
    const ping = /^ping ([1-9][0-9]*)\n$/.exec(line);
    if (ping && mode === "duplicate") process.stdout.write(`pong ${ping[1]} ${ping[1]} ${process.pid}\npong ${ping[1]} ${ping[1]} ${process.pid}\n`);
    if (ping && mode === "slow-pong") setTimeout(() => process.stdout.write(`pong ${ping[1]} ${ping[1]} ${process.pid}\n`), 100);
    if (line.includes("stop\n") && mode !== "ignore-stop") process.exit(0);
  });
}
