import * as fs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import type { ActualReadOutcome, NativeRequirements, StockNode16ReadProgram } from "../contract.js";

export const executable = "/media/developer/apps/usr/palm/applications/com.mikestopcontinues.criterion.unofficial/criterion-unofficial";
export class RuntimeFixture {
  readonly root = fs.mkdtempSync(join(tmpdir(), "criterion-runtime-"));
  readonly paths: string[] = [];
  readonly requirements: NativeRequirements;
  beforeFs: ((operation: string, path: string) => void) | undefined;
  afterRead: ((fd: number) => void) | undefined;
  constructor() {
    for (const path of ["/proc/sys/kernel/random", "/proc/42", "/lib", "/usr/lib", executable.slice(0, executable.lastIndexOf("/"))]) fs.mkdirSync(this.local(path), { recursive: true, mode: 0o755 });
    this.write("/proc/sys/kernel/random/boot_id", "12345678-1234-1234-1234-123456789abc\n");
    this.write("/proc/42/stat", "42 (criterion-unofficial) S " + Array(18).fill("0").join(" ") + " 456 0\n");
    const elf = Buffer.alloc(56); elf.set([0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
    elf.writeUInt16LE(3, 16); elf.writeUInt16LE(40, 18); elf.writeUInt32LE(1, 20); elf.writeUInt32LE(0x05000200, 36); elf.writeUInt16LE(52, 40); elf.write("film", 52);
    this.write(executable, elf); fs.chmodSync(this.local(executable), 0o755);
    fs.symlinkSync(executable, this.local("/proc/42/exe"));
    this.write("/usr/lib/libfixture.so.1.2", elf);
    for (const name of ["libSDL2-2.0.so.0", "libc.so.6", "libEGL.so.1", "libGLESv2.so.2"]) fs.symlinkSync("../usr/lib/libfixture.so.1.2", this.local("/lib/" + name));
    this.requirements = { appId: "com.mikestopcontinues.criterion.unofficial", executableSha256: createHash("sha256").update(elf).digest("hex"), neededSonames: ["libSDL2-2.0.so.0", "libc.so.6"] };
    this.maps();
  }
  local(path: string): string { return this.root + path; }
  write(path: string, value: string | Buffer): void { fs.writeFileSync(this.local(path), value); }
  maps(): void {
    const row = (path: string, low: string, high: string) => {
      const stat = fs.statSync(this.local(path), { bigint: true });
      const major = ((stat.dev >> 8n) & 0xfffn) | ((stat.dev >> 32n) & 0xfffff000n);
      const minor = (stat.dev & 0xffn) | ((stat.dev >> 12n) & 0xffffff00n);
      return `${low}-${high} r-xp 00000000 ${major.toString(16)}:${minor.toString(16)} ${stat.ino} ${path}\n`;
    };
    this.write("/proc/42/maps", row(executable, "1000", "2000") + row("/usr/lib/libfixture.so.1.2", "3000", "4000"));
  }
  dispose(): void { fs.rmSync(this.root, { recursive: true, force: true }); }
  execute = async (program: StockNode16ReadProgram): Promise<ActualReadOutcome> => {
    let stdout = "", stderr = "", exitCode = 0;
    const filesystem = new Proxy(fs, { get: (target, key) => {
      const value: unknown = Reflect.get(target, key); if (typeof value !== "function") return value;
      if (key === "readSync") return (...args: unknown[]) => {
        const result: unknown = Reflect.apply(value, target, args); if (typeof args[0] === "number") this.afterRead?.(args[0]); return result;
      };
      if (!["openSync", "lstatSync", "readlinkSync", "opendirSync", "realpathSync"].includes(String(key))) return value;
      return (...args: unknown[]) => {
        if (typeof args[0] === "string") {
          const path = args[0]; this.paths.push(path); this.beforeFs?.(String(key), path);
          assert.ok(path.startsWith("/") && !path.includes("/../"), path);
          if (!/^\/proc\/self\/fd\/\d+(?:\/|$)/.test(path)) args[0] = this.local(path === "/" ? "" : path);
        }
        const result: unknown = Reflect.apply(value, target, args);
        return key === "realpathSync" && typeof result === "string" ? result.slice(this.root.length) : result;
      };
    } });
    runInNewContext(program.source, { Buffer,
      require: (name: string): unknown => {
        if (name === "node:fs") return filesystem;
        if (name === "node:path" || name === "node:crypto") return require(name) as unknown;
        throw new Error("unexpected require: " + name);
      }, process: { platform: "linux", arch: "arm", versions: { node: "16.20.2" }, geteuid: () => 0, hrtime: process.hrtime,
        stdout: { write: (text: string) => { stdout += text; } }, stderr: { write: (text: string) => { stderr += text; } },
        get exitCode() { return exitCode; }, set exitCode(value: number) { exitCode = value; } },
    }, { timeout: Math.min(10000, program.timeoutMs) });
    return { kind: "closed", exitCode, signal: null, timedOut: false, stdout: Buffer.from(stdout), stderr: Buffer.from(stderr) };
  };
}
