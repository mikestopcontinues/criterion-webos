import assert from "node:assert/strict";
import { cpSync, existsSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { chmod, link, lstat, mkdir, mkdtemp, readFile, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { prepareCliConfiguration } from "../src/cli-configuration.js";

const names = ["ares.json", "command-service.json", "config.json", "ipk.json", "novacom-devices.json", "query/query-app.json", "query/query-hosted.json", "query/query-package.json", "query/query-service.json", "sdk.json", "template.json", "webos_emul"] as const;
const helloSha256 = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
type Fixture = { dependencies: string; cli: string; source: string; destination: string };
async function withFixture(run: (fixture: Fixture) => Promise<void>): Promise<void> {
  const root = await realpath(await mkdtemp(join(tmpdir(), "criterion-cli-configuration-")));
  const dependencies = join(root, "dependencies"), cli = join(dependencies, "@webos-tools/cli"), source = join(cli, "files/conf");
  try {
    await mkdir(join(source, "query"), { recursive: true });
    await writeFile(join(cli, "package.json"), JSON.stringify({ name: "@webos-tools/cli", version: "3.2.6" }));
    for (const name of names) await writeFile(join(source, name), "hello");
    await run({ dependencies, cli, source, destination: join(root, "owned-conf") });
  } finally { await rm(root, { recursive: true, force: true }); }
}
function afterFirstCopy(fixture: Fixture, mutate: () => void): () => void {
  let changed = false;
  return () => {
    if (!changed && existsSync(join(fixture.destination, "ares.json"))) { changed = true; mutate(); }
  };
}

test("the locked CLI configuration receives a fresh immutable initial inventory and an independent writable copy", async () => {
  await withFixture(async fixture => {
    const inventory = await prepareCliConfiguration(fixture.dependencies, fixture.destination, () => {});
    assert.deepEqual(inventory, names.map(name => ({ name, bytes: 5, sha256: helloSha256 })));
    assert.ok(Object.isFrozen(inventory) && inventory.every(Object.isFrozen));
    assert.equal((await lstat(fixture.destination)).mode & 0o777, 0o700);
    assert.equal((await lstat(join(fixture.destination, "query"))).mode & 0o777, 0o700);
    for (const name of names) {
      assert.equal((await readFile(join(fixture.destination, name))).toString(), "hello");
      assert.equal((await lstat(join(fixture.destination, name))).mode & 0o777, 0o600);
    }
    await writeFile(join(fixture.destination, "config.json"), "private TV profile");
    await rm(join(fixture.destination, "query/query-app.json"));
    for (const name of names) assert.equal((await readFile(join(fixture.source, name))).toString(), "hello");
  });
});

test("a foreign or already prepared configuration directory is preserved", async () => {
  await withFixture(async fixture => {
    await mkdir(fixture.destination); await writeFile(join(fixture.destination, "captain-owned"), "preserve");
    await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, () => {}), /EEXIST/);
    assert.equal((await readFile(join(fixture.destination, "captain-owned"))).toString(), "preserve");
  });
});

test("configuration preparation requires the exact locked CLI package identity", async () => {
  for (const value of [{ name: "@webos-tools/cli", version: "3.2.7" }, { name: "foreign", version: "3.2.6" }, {}, []]) {
    await withFixture(async fixture => {
      await writeFile(join(fixture.cli, "package.json"), JSON.stringify(value));
      await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, () => {}), /invalidCliConfiguration/);
      assert.equal(existsSync(fixture.destination), false);
    });
  }
});

test("missing and extra configuration entries are refused before allocating a writable copy", async () => {
  for (const change of ["missing", "extra", "extra-query", "extra-directory"] as const) {
    await withFixture(async fixture => {
      if (change === "missing") await rm(join(fixture.source, "sdk.json"));
      if (change === "extra") await writeFile(join(fixture.source, "foreign.json"), "hello");
      if (change === "extra-query") await writeFile(join(fixture.source, "query/foreign.json"), "hello");
      if (change === "extra-directory") await mkdir(join(fixture.source, "foreign"));
      await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, () => {}), /invalidCliConfiguration/);
      assert.equal(existsSync(fixture.destination), false);
    });
  }
});

test("configuration files must be unlinked ordinary files without special modes", async () => {
  for (const change of ["symlink", "hardlink", "directory", "special-mode"] as const) {
    await withFixture(async fixture => {
      const path = join(fixture.source, "config.json");
      if (change === "special-mode") await chmod(path, 0o4644);
      else {
        await rm(path);
        if (change === "symlink") await symlink("ares.json", path);
        if (change === "hardlink") await link(join(fixture.source, "ares.json"), path);
        if (change === "directory") await mkdir(path);
      }
      await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, () => {}), /invalidCliConfiguration/);
      assert.equal(existsSync(fixture.destination), false);
    });
  }
});

test("a linked configuration directory is refused", async () => {
  await withFixture(async fixture => {
    await rm(join(fixture.source, "query"), { recursive: true });
    await symlink(fixture.source, join(fixture.source, "query"));
    await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, () => {}), /invalidProducerPath|invalidCliConfiguration/);
    assert.equal(existsSync(fixture.destination), false);
  });
});

test("empty and overbound files and an overbound aggregate are refused", async () => {
  for (const change of ["empty", "file", "aggregate", "manifest"] as const) {
    await withFixture(async fixture => {
      if (change === "empty") await writeFile(join(fixture.source, "ares.json"), Buffer.alloc(0));
      if (change === "file") await writeFile(join(fixture.source, "ares.json"), Buffer.alloc(65537));
      if (change === "aggregate") for (const name of names) await writeFile(join(fixture.source, name), Buffer.alloc(45056));
      if (change === "manifest") await writeFile(join(fixture.cli, "package.json"), Buffer.alloc(65537));
      await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, () => {}), /invalidCliConfiguration/);
      assert.equal(existsSync(fixture.destination), false);
    });
  }
});

test("per-file and aggregate bounds remain inclusive", async () => {
  await withFixture(async fixture => {
    for (let index = 0; index < names.length; index++) {
      const name = names[index]; assert.ok(name);
      await writeFile(join(fixture.source, name), Buffer.alloc(index < 7 ? 65536 : index === 7 ? 65532 : 1, 97));
    }
    const inventory = await prepareCliConfiguration(fixture.dependencies, fixture.destination, () => {});
    assert.equal(inventory.reduce((total, file) => total + file.bytes, 0), 524288);
    assert.equal(inventory[0]?.bytes, 65536);
  });
});

test("changed source bytes and equal-byte source inode replacements invalidate the copy", async () => {
  for (const change of ["bytes", "inode", "directory", "extra", "manifest"] as const) {
    await withFixture(async fixture => {
      const check = afterFirstCopy(fixture, () => {
        const path = join(fixture.source, "config.json");
        if (change === "bytes") writeFileSync(path, "HELLO");
        if (change === "inode") { renameSync(path, path + ".old"); writeFileSync(path, "hello"); unlinkSync(path + ".old"); }
        if (change === "directory") { renameSync(fixture.source, fixture.source + "-old"); cpSync(fixture.source + "-old", fixture.source, { recursive: true }); }
        if (change === "extra") writeFileSync(join(fixture.source, "foreign.json"), "hello");
        if (change === "manifest") writeFileSync(join(fixture.cli, "package.json"), JSON.stringify({ name: "@webos-tools/cli", version: "3.2.7" }));
      });
      await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, check), /invalidCliConfiguration/);
      assert.equal(existsSync(fixture.destination), true);
    });
  }
});

test("replacing an owned directory or file and adding foreign output prevents a successful inventory", async () => {
  for (const change of ["directory", "file", "extra"] as const) {
    await withFixture(async fixture => {
      const check = afterFirstCopy(fixture, () => {
        if (change === "directory") { renameSync(fixture.destination, fixture.destination + "-old"); cpSync(fixture.destination + "-old", fixture.destination, { recursive: true }); }
        if (change === "file") { const path = join(fixture.destination, "ares.json"); renameSync(path, path + ".old"); writeFileSync(path, "hello", { mode: 0o600 }); unlinkSync(path + ".old"); }
        if (change === "extra") writeFileSync(join(fixture.destination, "foreign"), "preserve");
      });
      await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, check), /invalidCliConfiguration/);
      assert.equal(existsSync(fixture.destination), true);
    });
  }
});

test("the original deadline check can refuse preparation and preserve a partial copy", async () => {
  await withFixture(async fixture => {
    await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, () => { throw new Error("producerDeadline"); }), /producerDeadline/);
    assert.equal(existsSync(fixture.destination), false);
    const check = afterFirstCopy(fixture, () => { throw new Error("producerDeadline"); });
    await assert.rejects(() => prepareCliConfiguration(fixture.dependencies, fixture.destination, check), /producerDeadline/);
    assert.equal((await readFile(join(fixture.destination, "ares.json"))).toString(), "hello");
  });
});
