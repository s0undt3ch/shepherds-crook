// Unit tests for mod/lib.js. Run with `node --test tests/mod`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import * as lib from "../../mod/lib.js";
import { validate } from "./schema.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SCHEMA = join(root, "schema", "status.schema.json");

const status = (flocks, extra = {}) =>
  JSON.stringify({ version: 1, crook_version: "0.1.0", flocks, ...extra });
const flock = (slug, over = {}) => ({
  slug,
  folder: `/home/me/.local/share/crook/flocks/2026-11-27-${slug}`,
  state: "active",
  space: `crook-${slug}`,
  ...over,
});

test("the shim script is the D22 expression", () => {
  assert.equal(
    lib.SHIM_SCRIPT,
    'exec "${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}" "$@"',
  );
  assert.deepEqual(lib.crookArgv(["status", "--json"]), ["-c", lib.SHIM_SCRIPT, "crook", "status", "--json"]);
});

test("the sh launcher passes awkward arguments through unchanged", () => {
  const dir = mkdtempSync(join(tmpdir(), "crook-mod-"));
  const fake = join(dir, "fake-crook");
  // Prints each argument NUL-terminated so newlines in a value survive.
  writeFileSync(fake, '#!/bin/sh\nfor a in "$@"; do printf "%s\\0" "$a"; done\n');
  chmodSync(fake, 0o755);
  const task = `it's "quoted" $HOME \`id\` $(id)\nsecond line; rm -rf /\n-leading`;
  const args = lib.newArgs("login-bug", task);
  const out = execFileSync("sh", lib.crookArgv(args), { env: { CROOK_BIN: fake, PATH: process.env.PATH } });
  assert.deepEqual(out.toString().split("\0").slice(0, -1), ["new", "login-bug", `--task=${task}`]);
});

test("the launcher exits 126 or 127 when no crook exists, and follows MISE_DATA_DIR then XDG_DATA_HOME", () => {
  const dir = mkdtempSync(join(tmpdir(), "crook-mod-"));
  const base = { PATH: process.env.PATH, HOME: dir };
  const missing = spawnSync("sh", lib.crookArgv(["status"]), { env: { ...base, CROOK_BIN: join(dir, "nope") } });
  assert.ok([126, 127].includes(missing.status), `exit ${missing.status}`); // dash says 127, macOS sh 126
  const text = lib.errorText({ code: missing.status, stdout: "", stderr: missing.stderr.toString() });
  assert.match(text, /crook was not found/);
  assert.ok(text.includes(join(dir, "nope")), text);

  for (const [env, shims] of [
    [{ MISE_DATA_DIR: join(dir, "m") }, join(dir, "m", "shims")],
    [{ XDG_DATA_HOME: join(dir, "x") }, join(dir, "x", "mise", "shims")],
    [{}, join(dir, ".local", "share", "mise", "shims")],
  ]) {
    execFileSync("mkdir", ["-p", shims]);
    writeFileSync(join(shims, "crook"), "#!/bin/sh\necho shim\n");
    chmodSync(join(shims, "crook"), 0o755);
    const r = spawnSync("sh", lib.crookArgv([]), { env: { ...base, ...env } });
    assert.equal(r.stdout.toString().trim(), "shim", JSON.stringify(env));
  }
});

test("newArgs adds --task only for a non-blank task, as one argument", () => {
  assert.deepEqual(lib.newArgs("a", ""), ["new", "a"]);
  assert.deepEqual(lib.newArgs("a", "  \n "), ["new", "a"]);
  assert.deepEqual(lib.newArgs("a", undefined), ["new", "a"]);
  assert.deepEqual(lib.newArgs("a", " do it \n"), ["new", "a", "--task=do it"]);
  assert.deepEqual(lib.openArgs("a"), ["open", "a"]);
  assert.deepEqual(lib.closeArgs("a"), ["close", "a"]);
  assert.deepEqual(lib.statusArgs(), ["status", "--json"]);
});

test("checkSlug follows D27 and D36", () => {
  for (const ok of ["a", "login-bug", "x1", "a-b-c", "1abc", "sixteen-chars-ok", "tx"]) {
    assert.equal(lib.checkSlug(ok).ok, true, ok);
  }
  assert.equal("sixteen-chars-ok".length, 16);
  for (const bad of ["A", "a_b", "-a", "a-", "a--b", "a b", "é", "seventeen-chars-x", "t7", "t1x", "t12-abc"]) {
    const r = lib.checkSlug(bad);
    assert.equal(r.ok, false, bad);
    assert.notEqual(r.message, "", bad);
  }
  assert.deepEqual(lib.checkSlug(""), { ok: false, empty: true, message: "" });
  assert.equal(lib.checkSlug("a".repeat(17)).message, "At most 16 characters.");
  assert.equal(lib.checkSlug("a".repeat(16)).ok, true);
});

test("errorText", () => {
  assert.equal(lib.errorText({ code: 0, stdout: "x", stderr: "y" }), null);
  assert.match(lib.errorText({ code: 127, stdout: "", stderr: "" }), /^crook was not found or cannot be run\. Install it with mise/);
  assert.match(lib.errorText({ code: 126, stdout: "", stderr: "" }), /^crook was not found or cannot be run\./);
  assert.match(lib.errorText({ code: 127, stdout: "", stderr: "sh: exec: /x/crook: not found\n" }), /\(sh: exec: \/x\/crook: not found\)/);
  assert.equal(lib.errorText({ code: 1, stdout: "", stderr: "\n  error: slug taken \nmore\n" }), "error: slug taken");
  assert.equal(lib.errorText({ code: 1, stdout: "\nout line\n", stderr: "  \n" }), "out line");
  assert.equal(lib.errorText({ code: 3, stdout: "", stderr: "" }), "crook exited with code 3");
  assert.equal(
    lib.errorText({ code: 1, stdout: "", stderr: "warning: kept /x\nerror: boom\n" }),
    "error: boom",
  );
  assert.equal(lib.errorText({ code: 1, stdout: "", stderr: "warning: only this\n" }), "warning: only this");
  assert.equal(lib.errorText({ thrown: new Error("timed out") }), "Could not run crook: timed out");
  assert.equal(lib.errorText({ thrown: "odd" }), "Could not run crook: odd");
});

test("warnings keeps only kept-worktree lines, without the warning prefix", () => {
  assert.deepEqual(
    lib.warnings(
      "warning: kept worktree /a: unpushed commits\nnote\n  warning: kept worktree /b: dirty\n" +
        "warning: the current link was not moved\nwarning: kept worktree \nwarning:\n",
    ),
    ["kept worktree /a: unpushed commits", "kept worktree /b: dirty"],
  );
  assert.deepEqual(lib.warnings(""), []);
  assert.deepEqual(lib.warnings(undefined), []);
});

test("closeToast", () => {
  assert.deepEqual(lib.closeToast("a", ""), { title: "Flock a closed", level: "success" });
  // Other warnings do not make a close "with warnings".
  assert.deepEqual(lib.closeToast("a", "warning: current link not moved\n"), { title: "Flock a closed", level: "success" });
  const t = lib.closeToast("a", "warning: kept worktree /x: one\nwarning: other\nwarning: kept worktree /y: two\n");
  assert.equal(t.level, "warning");
  assert.equal(t.body, "kept worktree /x: one\nkept worktree /y: two");
});

test("parseStatus reads the fields the mod uses and ignores the rest", () => {
  const r = lib.parseStatus(
    status([flock("a", { coordinator: { x: 1 }, children: [], extra: true })], { future: 1, hc_error: "no core" }),
  );
  assert.deepEqual(r, {
    flocks: [{ slug: "a", folder: flock("a").folder, state: "active", space: "crook-a" }],
    crookVersion: "0.1.0",
    hcError: "no core",
  });
  assert.equal(lib.parseStatus(status([])).hcError, null);
  assert.deepEqual(lib.parseStatus(status([])).flocks, []);
});

test("parseStatus refuses what it cannot read", () => {
  const bad = (text) => lib.parseStatus(text).error;
  assert.match(bad(JSON.stringify({ version: 2, flocks: [] })), /format 2.*`crook setup`/);
  assert.match(bad(JSON.stringify({ flocks: [] })), /format undefined.*`crook setup`/);
  assert.match(bad("not json"), /`crook setup`/);
  assert.match(bad(""), /`crook setup`/);
  assert.match(bad("[]"), /`crook setup`/);
  assert.match(bad("null"), /`crook setup`/);
  assert.match(bad(JSON.stringify({ version: 1, flocks: "x" })), /no list of flocks/);
});

test("parseStatus drops entries without a slug and defaults missing strings", () => {
  const r = lib.parseStatus(JSON.stringify({ version: 1, flocks: [null, 3, {}, { slug: 5 }, { slug: "ok" }] }));
  assert.deepEqual(r.flocks, [{ slug: "ok", folder: "", state: "", space: "" }]);
  assert.equal(r.crookVersion, "");
});

test("folderName and flockDate", () => {
  assert.equal(lib.folderName("/a/b/2026-11-27-login-bug"), "2026-11-27-login-bug");
  assert.equal(lib.folderName("/a/b/2026-11-27-login-bug/"), "2026-11-27-login-bug");
  assert.equal(lib.folderName("C:\\a\\b"), "b");
  assert.equal(lib.folderName(""), "");
  assert.equal(lib.flockDate("/x/2026-11-27-login-bug"), "2026-11-27");
  assert.equal(lib.flockDate("/x/2026-11-27"), "2026-11-27");
  assert.equal(lib.flockDate("/x/login-bug"), "");
  assert.equal(lib.flockDate("/x/2026-11-271-a"), "");
});

test("liveFlocks: a flock runs only with a live coordinator terminal showing an agent (D52)", () => {
  const agent = { agent: true };
  const shell = { agent: false, shell: true };
  const live = lib.liveFlocks([
    { state: "live", program: agent, labels: { "crook.flock": "a" } },
    { state: "live", program: agent, labels: { "crook.flock": "b" } },
    // A bare shell that kept the label after a core restore is paused.
    { state: "live", program: shell, labels: { "crook.flock": "restored" } },
    { state: "live", labels: { "crook.flock": "noprogram" } },
    // A live child alone does not make its flock run.
    { state: "live", program: agent, labels: { "crook.flock": "kids", "crook.child": "c" } },
    { state: "exited", program: agent, labels: { "crook.flock": "gone" } },
    { state: "live", program: agent, labels: {} },
    { state: "live", program: agent, labels: null },
    { state: "live", program: agent, labels: ["crook.flock=arr"] },
    { state: "live", program: agent, labels: { "crook.flock": 7 } },
    { state: "live", program: agent, labels: { "crook.flock": "" } },
    { state: "live", program: agent },
    null,
  ]);
  assert.deepEqual([...live].sort(), ["a", "b"]);
  assert.deepEqual([...lib.liveFlocks(undefined)], []);
});

test("sortFlocks: active only, running first, newest first, then slug", () => {
  const f = (slug, date, state = "active") => flock(slug, { state, folder: `/x/${date}-${slug}` });
  const list = [
    f("old-paused", "2026-01-01"),
    f("new-paused", "2026-11-01"),
    f("old-running", "2026-02-01"),
    f("done-one", "2026-12-01", "done"),
    f("b-same", "2026-05-05"),
    f("a-same", "2026-05-05"),
    flock("nodate", { folder: "/x/nodate" }),
  ];
  const sorted = lib.sortFlocks(list, new Set(["old-running"]));
  assert.deepEqual(
    sorted.map((x) => x.slug),
    ["old-running", "new-paused", "a-same", "b-same", "old-paused", "nodate"],
  );
  assert.equal(sorted[0].running, true);
  assert.equal(sorted[1].running, false);
  assert.equal(sorted[1].date, "2026-11-01");
  assert.deepEqual(lib.sortFlocks([], new Set()), []);
});

test("versionHint", () => {
  assert.equal(lib.versionHint(null, "0.1.0"), null);
  assert.equal(lib.versionHint(undefined, "0.1.0"), null);
  assert.equal(lib.versionHint("0.1.0\n", "0.1.0"), null);
  assert.equal(lib.versionHint("0.0.9", "0.1.0"), "Run `crook setup` to update this dialog.");
  assert.equal(lib.versionHint("0.0.9", ""), null);
});

test("main.js starts programs in one place, through the shim", () => {
  const main = readFileSync(join(root, "mod", "main.js"), "utf8");
  assert.equal(main.match(/\bexec\(/g).length, 1);
  assert.match(main, /exec\("sh", crookArgv\(args\), \{ cwd: home\(\), timeout: CROOK_TIMEOUT_MS \}\)/);
  assert.ok(!/\bhc\(/.test(main), "the mod must not call hc directly");
  assert.ok(!/space\.new/.test(main), "the mod never calls space.new (Q24)");
  assert.equal(lib.CROOK_TIMEOUT_MS, 120_000);
});

test("mod.json has the shape the plan sets", () => {
  const mod = JSON.parse(readFileSync(join(root, "mod", "mod.json"), "utf8"));
  assert.equal(mod.place, "none");
  assert.equal(mod.overlay.kind, "dialog");
  assert.equal(mod.overlay.width, 560);
  assert.equal(mod.palette.length, 1);
  assert.equal(mod.palette[0].command, "mod.open");
  assert.deepEqual(mod.palette[0].args, { name: "crook" });
});

test("status JSON validates against schema/status.schema.json", { skip: !existsSync(SCHEMA) && "schema/status.schema.json does not exist yet (binary task 7.2)" }, () => {
  const schema = JSON.parse(readFileSync(SCHEMA, "utf8"));
  const sample = JSON.parse(
    status([
      flock("a", {
        notes_folder: "/n",
        coordinator: { session_id: "s", terminal: null },
        children: [],
      }),
    ]),
  );
  assert.deepEqual(validate(schema, sample), []);
  assert.notDeepEqual(validate(schema, { ...sample, version: 2 }), []);
  // What the mod reads must come back out of what the schema accepts.
  assert.equal(lib.parseStatus(JSON.stringify(sample)).error, undefined);
});
