import * as fs from "node:fs";
import * as cp from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import assert from "node:assert/strict";
import type { ActualReadOutcome, StockNode16ReadProgram } from "../index.js";

export class DeviceFixture {
  readonly root = fs.mkdtempSync(join(tmpdir(), "criterion-prerequisites-"));
  readonly paths: string[] = [];
  getterCalls = 0;
  getterStatus: number | null = 0;
  getterSignal: string | null = null;
  getterTimedOut = false;
  getterPid = 55;
  getterResponse: unknown = {
    returnValue: true, modelName: "fixture-C4", firmwareVersion: "33.31.69",
    sdkVersion: "10.3.1", boardType: "fixture-board",
  };
  beforeFs: ((operation: string, path: string) => void) | undefined;
  afterRead: ((fd: number) => void) | undefined;

  constructor() {
    for (const path of ["/lib", "/usr/lib", "/proc/sys/kernel/random", "/proc/42"]) {
      fs.mkdirSync(this.local(path), { recursive: true, mode: 0o755 });
    }
    this.write("/proc/sys/kernel/random/boot_id", "12345678-1234-1234-1234-123456789abc\n");
    this.write("/proc/42/stat", "42 (surface-manager) S " + Array(18).fill("0").join(" ") + " 456 0\n");
    fs.symlinkSync("/usr/bin/surface-manager", this.local("/proc/42/exe"));
    this.write("/proc/cpuinfo", "processor : 0\nCPU architecture : 7\nFeatures : vfp neon vfpv3\nSerial : private-do-not-emit\n\nprocessor : 1\nCPU architecture : 7\nFeatures : vfp vfpv3\n\n");
    const elf = Buffer.alloc(56);
    elf.set([0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
    elf.writeUInt16LE(3, 16); elf.writeUInt16LE(40, 18);
    elf.writeUInt32LE(1, 20); elf.writeUInt32LE(0x05000200, 36); elf.writeUInt16LE(52, 40);
    elf.write("film", 52);
    this.write("/usr/lib/libfixture.so.1.2", elf);
    for (const name of ["libSDL2-2.0.so.0", "libc.so.6", "libEGL.so.1", "libGLESv2.so.2"]) {
      fs.symlinkSync("../usr/lib/libfixture.so.1.2", this.local("/lib/" + name));
    }
  }

  local(path: string): string { return this.root + path; }
  write(path: string, value: string | Buffer): void { fs.writeFileSync(this.local(path), value); }
  dispose(): void { fs.rmSync(this.root, { recursive: true, force: true }); }

  execute = async (program: StockNode16ReadProgram): Promise<ActualReadOutcome> => {
    let stdout = "", stderr = "", exitCode = 0;
    const mapPath = (operation: string, path: string): string => {
      this.paths.push(path); this.beforeFs?.(operation, path);
      if (/^\/proc\/self\/fd\/\d+(?:\/|$)/.test(path)) return path;
      assert.ok(path === "/" || path === "/usr" || path === "/lib" || path.startsWith("/lib/")
        || path === "/usr/lib" || path.startsWith("/usr/lib/") || path === "/proc" || path.startsWith("/proc/"), path);
      return this.local(path === "/" ? "" : path);
    };
    const filesystem = new Proxy(fs, {
      get: (target, key) => {
        const value: unknown = Reflect.get(target, key);
        if (typeof value !== "function") return value;
        if (key === "readSync") return (...args: unknown[]) => {
          const result: unknown = Reflect.apply(value, target, args);
          if (typeof args[0] === "number") this.afterRead?.(args[0]);
          return result;
        };
        if (!["openSync", "lstatSync", "readlinkSync", "opendirSync"].includes(String(key))) return value;
        return (...args: unknown[]) => {
          if (typeof args[0] === "string") args[0] = mapPath(String(key), args[0]);
          return Reflect.apply(value, target, args);
        };
      },
    });
    const childProcess = {
      spawnSync: (file: string, args: readonly string[], options: cp.SpawnSyncOptions) => {
        this.getterCalls++;
        assert.equal(file, "/usr/bin/luna-send");
        assert.deepEqual(Array.from(args).slice(0, 5), ["-n", "1", "-w", "2000", "luna://com.webos.service.tv.systemproperty/getSystemInfo"]);
        assert.deepEqual(JSON.parse(args[5] ?? ""), { keys: ["modelName", "firmwareVersion", "sdkVersion", "boardType"], subscribe: false });
        assert.ok(options.timeout !== undefined && options.timeout > 0 && options.timeout <= 2500);
        assert.equal(options.killSignal, "SIGKILL");
        return { pid: this.getterPid, status: this.getterStatus, signal: this.getterSignal,
          stdout: JSON.stringify(this.getterResponse), stderr: "",
          error: this.getterTimedOut ? Object.assign(new Error("timeout"), { code: "ETIMEDOUT" }) : undefined };
      },
    };
    runInNewContext(program.source, {
      Buffer,
      require: (name: string): unknown => {
        if (name === "node:fs") return filesystem;
        if (name === "node:child_process") return childProcess;
        if (name === "node:path" || name === "node:crypto") return require(name) as unknown;
        throw new Error("unexpected require: " + name);
      },
      process: {
        platform: "linux", arch: "arm", versions: { node: "16.20.2" },
        geteuid: () => 0, hrtime: process.hrtime,
        stdout: { write: (text: string) => { stdout += text; } },
        stderr: { write: (text: string) => { stderr += text; } },
        get exitCode() { return exitCode; }, set exitCode(value: number) { exitCode = value; },
      },
    }, { timeout: Math.min(10000, program.timeoutMs) });
    return { kind: "closed", exitCode, signal: null, timedOut: false,
      stdout: Buffer.from(stdout), stderr: Buffer.from(stderr) };
  };
}
