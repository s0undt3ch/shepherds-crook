// Pure logic for the crook mod. It imports nothing, so `node --test` can load it.

// Same expression as the plugin's hook commands (ADR 0019, build-log D22).
export const SHIM_SCRIPT =
  'exec "${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}" "$@"';

// `crook new` and `crook open` may start the hc core and rename the space; a guess until measured in the app.
export const CROOK_TIMEOUT_MS = 120_000;

// The `status --json` format number this mod understands.
export const STATUS_VERSION = 1;

export const SLUG_MAX = 16;

// Arguments for `exec("sh", …)`. Every value is a positional parameter of the script, never part of its text.
export function crookArgv(args) {
  return ["-c", SHIM_SCRIPT, "crook", ...args];
}

export const statusArgs = () => ["status", "--json"];
export const openArgs = (slug) => ["open", slug];
export const closeArgs = (slug) => ["close", slug];

// `--task=<text>`, not `--task <text>`, so a task starting with a dash is still a value to clap.
export function newArgs(slug, task) {
  const text = (task ?? "").trim();
  return text ? ["new", slug, `--task=${text}`] : ["new", slug];
}

// The slug rule from build-log D27 and D36: lower-case words joined by single dashes, at most 16 characters,
// and not shaped like an hc address (`t7`, `t1x`). An empty name is invalid but not worth a complaint.
export function checkSlug(text) {
  const slug = text ?? "";
  if (slug === "") return { ok: false, empty: true, message: "" };
  if (slug.length > SLUG_MAX) return { ok: false, empty: false, message: `At most ${SLUG_MAX} characters.` };
  if (!/^[a-z0-9]+(-[a-z0-9]+)*$/.test(slug)) {
    return { ok: false, empty: false, message: "Lower-case letters, digits and single dashes only." };
  }
  if (/^t[0-9]/.test(slug)) {
    return { ok: false, empty: false, message: "A name starting with t and a digit looks like a terminal address." };
  }
  return { ok: true, empty: false, message: "" };
}

const firstLine = (text) => (text ?? "").split(/\r?\n/).map((l) => l.trim()).find((l) => l !== "") ?? "";
const isWarning = (line) => line.startsWith("warning:");

// What to show for a finished `exec`, or null when crook succeeded. `result` is `{ code, stdout, stderr }`,
// or `{ thrown }` when exec itself rejected (timeout, no `sh`).
export function errorText(result) {
  if (result.thrown !== undefined) {
    const e = result.thrown;
    return `Could not run crook: ${(e && e.message) || String(e)}`;
  }
  const { code, stdout, stderr } = result;
  if (code === 0) return null;
  // `exec` of a missing file exits 127 in dash but 126 in macOS's sh (checked 2026-11-27).
  if (code === 127 || code === 126) {
    const detail = firstLine(stderr);
    return `crook was not found or cannot be run. Install it with mise, run \`crook setup\`, or set \`CROOK_BIN\`.${detail ? ` (${detail})` : ""}`;
  }
  // `warning:` lines are not the failure; they only win when nothing else was said.
  const lines = (text) => (text ?? "").split(/\r?\n/).map((l) => l.trim()).filter((l) => l !== "");
  const pick = (text) => lines(text).find((l) => !isWarning(l)) ?? "";
  return pick(stderr) || pick(stdout) || firstLine(stderr) || firstLine(stdout) || `crook exited with code ${code}`;
}

// The kept-worktree lines of stderr, `warning: kept worktree <path>: <reason>` (build-log D53), without the
// `warning: ` prefix. Other `warning:` lines, such as current-link ones, are not about a close and are dropped.
export function warnings(stderr) {
  const prefix = "warning: kept worktree ";
  return (stderr ?? "")
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.startsWith(prefix) && l.length > prefix.length)
    .map((l) => l.slice("warning: ".length));
}

// Parses `crook status --json`. Returns `{ flocks, crookVersion, hcError }` or `{ error }`.
// Extra fields are ignored; a flock without a string `slug` is dropped.
export function parseStatus(stdout) {
  let doc;
  try {
    doc = JSON.parse(stdout);
  } catch {
    return { error: "crook status gave output this dialog cannot read. Run `crook setup` to update it." };
  }
  if (doc === null || typeof doc !== "object" || Array.isArray(doc)) {
    return { error: "crook status gave output this dialog cannot read. Run `crook setup` to update it." };
  }
  if (doc.version !== STATUS_VERSION) {
    return { error: `crook status uses format ${String(doc.version)}, which this dialog does not know. Run \`crook setup\` to update it.` };
  }
  if (!Array.isArray(doc.flocks)) return { error: "crook status had no list of flocks." };
  const flocks = doc.flocks
    .filter((f) => f && typeof f === "object" && typeof f.slug === "string")
    .map((f) => ({
      slug: f.slug,
      folder: typeof f.folder === "string" ? f.folder : "",
      state: typeof f.state === "string" ? f.state : "",
      space: typeof f.space === "string" ? f.space : "",
    }));
  return {
    flocks,
    crookVersion: typeof doc.crook_version === "string" ? doc.crook_version : "",
    hcError: typeof doc.hc_error === "string" && doc.hc_error !== "" ? doc.hc_error : null,
  };
}

// The last part of an absolute folder path, either separator, ignoring a trailing one.
export function folderName(folder) {
  const parts = (folder ?? "").split(/[\\/]+/).filter((p) => p !== "");
  return parts.length ? parts[parts.length - 1] : "";
}

// The `YYYY-MM-DD` the folder name starts with (ADR 0024), or "".
export function flockDate(folder) {
  const m = /^(\d{4}-\d{2}-\d{2})(?:-|$)/.exec(folderName(folder));
  return m ? m[1] : "";
}

// Slugs of running flocks (build-log D52): a live coordinator terminal, that is one labelled `crook.flock` and not
// `crook.child`, whose program is an agent. A live bare shell (core restore keeps the labels) counts as paused.
// `terminals` is herdr's `terminals()`: `labels` is an object, a live terminal has `state === "live"`, and
// `program.agent` says whether the app sees an agent in it.
export function liveFlocks(terminals) {
  const live = new Set();
  for (const t of terminals ?? []) {
    if (!t || t.state !== "live" || t.program?.agent !== true) continue;
    const labels = t.labels;
    if (labels === null || typeof labels !== "object" || Array.isArray(labels)) continue;
    if (labels["crook.child"] !== undefined) continue;
    const slug = labels["crook.flock"];
    if (typeof slug === "string" && slug !== "") live.add(slug);
  }
  return live;
}

// The rows of the list: active flocks, running first, then newest date, then slug.
export function sortFlocks(flocks, running) {
  return flocks
    .filter((f) => f.state === "active")
    .map((f) => ({ ...f, running: running.has(f.slug), date: flockDate(f.folder) }))
    .sort((a, b) => Number(b.running) - Number(a.running) || b.date.localeCompare(a.date) || a.slug.localeCompare(b.slug));
}

// The footer hint when the installed mod's marker differs from the binary's version. No marker: no hint.
export function versionHint(marker, crookVersion) {
  if (marker === null || marker === undefined || crookVersion === "") return null;
  return marker.trim() === crookVersion ? null : "Run `crook setup` to update this dialog.";
}

// The toast after `crook close`: a warning listing each kept worktree, else a plain success.
export function closeToast(slug, stderr) {
  const kept = warnings(stderr);
  if (kept.length === 0) return { title: `Flock ${slug} closed`, level: "success" };
  return { title: `Flock ${slug} closed, with warnings`, body: kept.join("\n"), level: "warning" };
}
