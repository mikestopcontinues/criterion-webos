import assert from "node:assert/strict";
import { readNativePrerequisites, type NativeRequirements } from "../index.js";
import { DeviceFixture } from "./fixture.js";

const requirements: NativeRequirements = {
  appId: "com.mikestopcontinues.criterion.unofficial",
  executableSha256: "1".repeat(64),
  neededSonames: ["libSDL2-2.0.so.0", "libc.so.6"],
};

async function run(): Promise<void> {
  let time = 100;
  let calls = 0;
  const result = await readNativePrerequisites(requirements, {
    deadlineMs: 1000,
    now: () => time,
    execute: async (request) => {
      calls++;
      assert.equal(request.deadlineMs, 1000);
      assert.equal(request.stdin, "none");
      assert.ok(request.timeoutMs <= 900);
      time = 1001;
      return {
        kind: "closed", exitCode: 0, signal: null, timedOut: false,
        stdout: Buffer.from("{}"), stderr: Buffer.alloc(0),
      };
    },
  });
  assert.equal(calls, 1);
  assert.equal(result.outcome?.kind, "closed");
  assert.deepEqual(result.response, { kind: "unavailable", reason: "deadline" });
  console.log("PASS original deadline survives the awaited executor");
  const device = new DeviceFixture();
  try {
    const facts = await readNativePrerequisites(requirements, { deadlineMs: 10000, now: () => 0, execute: device.execute });
    assert.equal(facts.response.kind, "accepted");
    if (facts.response.kind !== "accepted") throw new Error("fixture facts unavailable");
    assert.deepEqual(facts.response.facts.systemInfo, { kind: "available", modelName: "fixture-C4",
      firmwareVersion: "33.31.69", sdkVersion: "10.3.1", boardType: "fixture-board" });
    assert.equal(device.getterCalls, 1);
    assert.equal(facts.response.facts.libraries.length, 4);
    const first = facts.response.facts.libraries[0];
    assert.ok(first?.kind === "available");
    assert.equal(first.path, "/usr/lib/libfixture.so.1.2");
    assert.deepEqual(first.elf, { class: 32, endian: "little", machine: 40, type: 3, flags: 0x05000200 });
    assert.ok(facts.response.facts.cpu.kind === "available");
    assert.deepEqual(facts.response.facts.cpu.features, ["vfp", "vfpv3"]);
    assert.ok(!JSON.stringify(facts.response.facts).includes("private-do-not-emit"));
    console.log("PASS compiled stock program reports stable library and system/CPU facts");
    device.write("/proc/cpuinfo", "processor : 0\nCPU architecture : 7\nFeatures : vfp neon\n\nHardware : fixture\nRevision : private-revision\nSerial : private-serial\n\n");
    const footer = await readNativePrerequisites(requirements, { deadlineMs: 10000, now: () => 0, execute: device.execute });
    assert.ok(footer.response.kind === "accepted" && footer.response.facts.cpu.kind === "available");
    assert.equal(footer.response.facts.cpu.processorCount, 1);
    assert.deepEqual(footer.response.facts.cpu.features, ["neon", "vfp"]);
    assert.ok(!JSON.stringify(footer.response.facts).includes("private-"));
    console.log("PASS ARM CPU footer is excluded from core facts");
  } finally { device.dispose(); }
}

void run().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
