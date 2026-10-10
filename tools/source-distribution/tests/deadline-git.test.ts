import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { captureHostProject, GitCommandError, type ClosedGitCommand, type GitCommand, type GitCommandResult, type HostGitExecution } from "../src/git.js";
type Fixture = { revision: string; commit: string; tree: string; blobs: Record<string, string> };
const fixture = JSON.parse(readFileSync("tools/source-distribution/tests/fixtures/project.json", "utf8")) as Fixture;
function closed(stdout: Buffer): ClosedGitCommand {
  return { closed: true, exitCode: 0, signal: null, timedOut: false, stdout, stderr: Buffer.alloc(0) };
}
function fixtureOutput(command: GitCommand, data: Fixture = fixture): Buffer {
  const operation = command.args.slice(command.args.indexOf("-C") + 2);
  if (operation[0] === "rev-parse") return Buffer.from(data.revision + "\n");
  if (operation[0] === "status") return Buffer.alloc(0);
  if (operation[0] === "ls-tree") return Buffer.from(data.tree, "base64");
  if (operation[0] === "cat-file" && operation[1] === "commit") return Buffer.from(data.commit, "base64");
  const bytes = operation[2] && data.blobs[operation[2]];
  if (operation[0] !== "cat-file" || operation[1] !== "blob" || !bytes) throw new Error("missingFixtureBlob");
  return Buffer.from(bytes, "base64");
}
async function inRoot(body: (root: string) => Promise<void>): Promise<void> {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "deadline-git-")));
  try { await body(root); } finally { rmSync(root, { recursive: true, force: true }); }
}
function execution(execute: (command: GitCommand) => Promise<GitCommandResult>, now: () => number = () => 0): HostGitExecution {
  return { deadlineMs: 100, now, execute };
}
test("deadline-owned host Git capture preserves the admitted committed public source", async () => inRoot(async (root) => {
  const snapshot = await captureHostProject(root, fixture.revision, execution(async (command) => closed(fixtureOutput(command))));
  assert.equal(snapshot.revision, "7ea0d769c83f067ab5a294c7ac082c88c1be990c");
  assert.deepEqual(snapshot.files.map((file) => [file.name, file.bytes.toString(), file.mode]), [
    ["Cargo.lock", "version = 4\n", 0o644], ["logs/2026-10-09.md", "public engineering log\n", 0o644],
  ]);
}));
test("an expired original Git deadline issues no host command", async () => inRoot(async (root) => {
  let calls = 0;
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
    calls += 1; return closed(fixtureOutput(command));
  }, () => 100)), /gitDeadline/);
  assert.equal(calls, 0);
}));
test("a late closed host Git command retains its actual outcome and stops capture", async () => inRoot(async (root) => {
  let clock = 0; let calls = 0; let issued: GitCommand | null = null; let actual: ClosedGitCommand | null = null;
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
    calls += 1; issued = command; actual = closed(fixtureOutput(command)); clock = 100; return actual;
  }, () => clock)), (error: unknown) => {
    assert.ok(error instanceof GitCommandError); assert.equal(error.message, "gitDeadline");
    assert.equal(error.command, issued); assert.equal(error.outcome, actual); return true;
  });
  assert.equal(calls, 1);
}));
test("a known closed Git result requires an actual exit code or signal", async () => inRoot(async (root) => {
  const actual = { ...closed(Buffer.alloc(0)), exitCode: null };
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async () => actual)), (error: unknown) => {
    assert.ok(error instanceof GitCommandError); assert.equal(error.message, "invalidCommandResult");
    assert.equal(error.received, actual); return true;
  });
}));
test("unexpected own Git result fields cannot acknowledge process closure", async () => inRoot(async (root) => {
  const actual = { ...closed(Buffer.from(fixture.revision + "\n")), extra: true };
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async () => actual)), (error: unknown) => {
    assert.ok(error instanceof GitCommandError); assert.equal(error.message, "invalidCommandResult");
    assert.equal(error.received, actual); return true;
  });
}));
test("the host Git output bound includes stdout and stderr together", async () => inRoot(async (root) => {
  const actual = { ...closed(Buffer.alloc(1024 * 1024 + 1)), stderr: Buffer.alloc(1024 * 1024) };
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async () => actual)), (error: unknown) => {
    assert.ok(error instanceof GitCommandError); assert.equal(error.message, "invalidCommandResult");
    assert.equal(error.outcome, actual); return true;
  });
}));
test("the shared host executor accepts only safe integer clocks and deadlines", async () => inRoot(async (root) => {
  const clocks: readonly (readonly [number, number])[] = [[100.5, 0], [100, 0.5], [Number.MAX_SAFE_INTEGER + 1, 0], [NaN, 0], [Infinity, 0], [100, NaN], [100, Infinity], [100, -1]];
  for (const [deadlineMs, clock] of clocks) {
    let calls = 0;
    await assert.rejects(captureHostProject(root, fixture.revision, {
      deadlineMs, now: () => clock,
      execute: async (command) => { calls += 1; return closed(fixtureOutput(command)); },
    }), /gitDeadline/);
    assert.equal(calls, 0);
  }
}));
test("all sanitized host Git reads consume the same shrinking original budget", async () => inRoot(async (root) => {
  let clock = 0; const commands: GitCommand[] = [];
  await captureHostProject(root, fixture.revision, execution(async (command) => {
    commands.push(command); clock += 10;
    assert.equal(command.executable, "git"); assert.equal(command.cwd, root); assert.equal(command.deadlineMs, 100);
    assert.deepEqual(command.args.slice(0, 9), ["--no-replace-objects", "-c", "core.fsmonitor=false", "-c", "core.untrackedCache=false", "-c", "core.hooksPath=/dev/null", "-C", root]);
    assert.deepEqual(command.env, { PATH: "/opt/homebrew/bin:/usr/bin:/bin", LANG: "C", LC_ALL: "C", TZ: "UTC", GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null", GIT_OPTIONAL_LOCKS: "0" });
    assert.ok(Object.isFrozen(command) && Object.isFrozen(command.args) && Object.isFrozen(command.env));
    assert.equal(command.maxOutputBytes, command.args[10] === "blob" ? 33554433 : 2097152);
    return closed(fixtureOutput(command));
  }, () => clock));
  assert.deepEqual(commands.map((command) => command.timeoutMs), [100, 90, 80, 70, 60, 50, 40, 30]);
  assert.deepEqual(commands.map((command) => command.args.slice(9, 11)), [
    ["rev-parse", "--verify"], ["status", "--porcelain=v1"], ["cat-file", "commit"], ["ls-tree", "-rz"],
    ["cat-file", "blob"], ["cat-file", "blob"], ["rev-parse", "--verify"], ["status", "--porcelain=v1"],
  ]);
}));
test("the per-command host Git limit cannot expand an original longer budget", async () => inRoot(async (root) => {
  await captureHostProject(root, fixture.revision, {
    deadlineMs: 120000, now: () => 0,
    execute: async (command) => { assert.equal(command.timeoutMs, 60000); assert.equal(command.deadlineMs, 120000); return closed(fixtureOutput(command)); },
  });
}));
test("caller mutation cannot renew the host Git deadline or replace its executor", async () => inRoot(async (root) => {
  let clock = 0; let calls = 0;
  const context = {
    deadlineMs: 100, now: () => clock,
    execute: async (command: GitCommand): Promise<GitCommandResult> => {
      calls += 1; context.deadlineMs = 10000; context.now = () => 0;
      context.execute = async () => { throw new Error("replacementExecutor"); };
      clock = 100; return closed(fixtureOutput(command));
    },
  };
  await assert.rejects(captureHostProject(root, fixture.revision, context), /gitDeadline/); assert.equal(calls, 1);
}));
for (const behavior of ["backward", "nonfinite", "throwing"] as const) {
  test(`${behavior} clock after actual host Git closure preserves custody`, async () => inRoot(async (root) => {
    let settled = false; let calls = 0; let actual: ClosedGitCommand | null = null;
    const now = (): number => {
      if (!settled) return 10;
      if (behavior === "throwing") throw new Error("clockUnavailable");
      return behavior === "backward" ? 9 : NaN;
    };
    await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
      calls += 1; actual = closed(fixtureOutput(command)); settled = true; return actual;
    }, now)), (error: unknown) => {
      assert.ok(error instanceof GitCommandError); assert.equal(error.message, "gitDeadline"); assert.equal(error.outcome, actual); return true;
    });
    assert.equal(calls, 1);
  }));
}
test("the final host status closure cannot settle after the original deadline", async () => inRoot(async (root) => {
  let statusReads = 0; let clock = 0; let final: ClosedGitCommand | null = null;
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
    const actual = closed(fixtureOutput(command));
    if (command.args[9] === "status" && ++statusReads === 2) { clock = 100; final = actual; }
    return actual;
  }, () => clock)), (error: unknown) => {
    assert.ok(error instanceof GitCommandError); assert.equal(error.message, "gitDeadline"); assert.equal(error.outcome, final);
    assert.deepEqual(error.command.args.slice(9), ["status", "--porcelain=v1", "--untracked-files=normal"]); return true;
  });
}));
test("the original deadline includes complete snapshot admission after the final status", async () => inRoot(async (root) => {
  let statusReads = 0; let finalClockReads = 0; let final: ClosedGitCommand | null = null;
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
    const actual = closed(fixtureOutput(command));
    if (command.args[9] === "status" && ++statusReads === 2) final = actual;
    return actual;
  }, () => statusReads === 2 ? (++finalClockReads === 1 ? 99 : 100) : 0)), (error: unknown) => {
    assert.ok(error instanceof GitCommandError); assert.equal(error.message, "gitDeadline"); assert.equal(error.outcome, final); return true;
  });
}));
test("an unresolved host Git child stops capture and retains the unresolved receipt", async () => inRoot(async (root) => {
  let calls = 0; const actual = { closed: false } as const;
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async () => { calls += 1; return actual; })), (error: unknown) => {
    assert.ok(error instanceof GitCommandError); assert.equal(error.message, "unresolvedCommand"); assert.equal(error.outcome, null); assert.equal(error.received, actual); return true;
  });
  assert.equal(calls, 1);
}));
test("an executor rejection retains its error without inferring host closure", async () => inRoot(async (root) => {
  const actual = new Error("unknownChildClosure");
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async () => { throw actual; })), (error: unknown) => {
    assert.ok(error instanceof GitCommandError); assert.equal(error.message, "unresolvedCommand"); assert.equal(error.outcome, null); assert.equal(error.received, actual); return true;
  });
}));
for (const actual of [
  { ...closed(Buffer.alloc(0)), exitCode: 1 },
  { ...closed(Buffer.alloc(0)), exitCode: null, signal: "SIGTERM" },
  { ...closed(Buffer.alloc(0)), timedOut: true },
  { ...closed(Buffer.alloc(0)), stderr: Buffer.from("Git failure\n") },
]) {
  test(`failed host Git outcome ${actual.exitCode}/${actual.signal}/${actual.timedOut}/${actual.stderr.length} cannot admit source`, async () => inRoot(async (root) => {
    let calls = 0;
    await assert.rejects(captureHostProject(root, fixture.revision, execution(async () => { calls += 1; return actual; })), (error: unknown) => {
      assert.ok(error instanceof GitCommandError); assert.equal(error.message, "gitSourceReadFailed"); assert.equal(error.outcome, actual); return true;
    });
    assert.equal(calls, 1);
  }));
}
test("dirty initial host state refuses before committed source reads", async () => inRoot(async (root) => {
  let calls = 0;
  await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
    calls += 1; return closed(command.args[9] === "status" ? Buffer.from(" M Cargo.lock\n") : fixtureOutput(command));
  })), /dirtyOrWrongRevision/);
  assert.equal(calls, 2);
}));
for (const changed of ["head", "status"] as const) {
  test(`changed final host ${changed} refuses the otherwise complete snapshot`, async () => inRoot(async (root) => {
    let reads = 0;
    await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
      const isSelected = command.args[9] === (changed === "head" ? "rev-parse" : "status");
      const bytes = isSelected && ++reads === 2 ? Buffer.from(changed === "head" ? "0".repeat(40) + "\n" : "?? new-source.rs\n") : fixtureOutput(command);
      return closed(bytes);
    })), /dirtyOrWrongRevision/);
  }));
}
test("wrong or malformed expected revision issues no committed-object command", async () => inRoot(async (root) => {
  for (const revision of ["0".repeat(40), "HEAD", "A".repeat(40)]) {
    const commands: GitCommand[] = [];
    await assert.rejects(captureHostProject(root, revision, execution(async (command) => {
      commands.push(command); return closed(fixtureOutput(command));
    })), /dirtyOrWrongRevision/);
    assert.ok(commands.every((command) => command.args[9] === "rev-parse"));
  }
}));
test("host Git capture refuses an aliased root before issuing any command", async () => inRoot(async (root) => {
  const alias = join(root, "alias"); symlinkSync(root, alias); let calls = 0;
  await assert.rejects(captureHostProject(alias, fixture.revision, execution(async (command) => { calls += 1; return closed(fixtureOutput(command)); })), /invalidGitRoot/);
  assert.equal(calls, 0);
}));
for (const file of ["private-project.json", "case-project.json"]) {
  test(`async host capture reuses whole-project exclusion for ${file}`, async () => inRoot(async (root) => {
    const data = JSON.parse(readFileSync(`tools/source-distribution/tests/fixtures/${file}`, "utf8")) as Fixture;
    await assert.rejects(captureHostProject(root, data.revision, execution(async (command) => closed(fixtureOutput(command, data)))), /invalidProjectSource/);
  }));
}
for (const [label, transform] of [
  ["symlink", (listing: string) => listing.replace("100644 blob", "120000 blob")],
  ["submodule", (listing: string) => listing.replace("100644 blob", "160000 commit")],
  ["truncated tree", (listing: string) => listing.split("\0").slice(0, 1).join("\0") + "\0"],
  ["missing tree terminator", (listing: string) => listing.slice(0, -1)],
  ["too many tree objects", (listing: string) => (listing.split("\0")[0] + "\0").repeat(4097)],
] as const) {
  test(`async host capture reuses committed tree admission for ${label}`, async () => inRoot(async (root) => {
    await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
      const bytes = fixtureOutput(command);
      return closed(command.args[9] === "ls-tree" ? Buffer.from(transform(bytes.toString())) : bytes);
    })), /invalidProjectSource/);
  }));
}
for (const object of ["commit", "blob"] as const) {
  test(`async host capture rejects altered ${object} bytes against actual Git identity`, async () => inRoot(async (root) => {
    await assert.rejects(captureHostProject(root, fixture.revision, execution(async (command) => {
      const bytes = fixtureOutput(command);
      return closed(command.args[9] === "cat-file" && command.args[10] === object ? Buffer.concat([bytes, Buffer.from("altered")]) : bytes);
    })), /invalidProjectSource/);
  }));
}
