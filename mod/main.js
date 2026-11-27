// The Flocks dialog: list, start, open and close crook flocks. Every step is a crook call (ADR 0020), and
// crook's own error text is what the dialog shows. It draws no flock tree (ADR 0021).

import { View, div } from "gpui-kit";
import { v_flex, h_flex, InputState, Input, TextareaState, Textarea } from "gpui-base";
import { run, props, closeOverlay, theme, spaces, terminals, exec, home, appHome, readTextFile, notify, checking } from "herdr";
import {
  CROOK_TIMEOUT_MS,
  crookArgv,
  statusArgs,
  newArgs,
  openArgs,
  closeArgs,
  errorText,
  parseStatus,
  checkSlug,
  liveFlocks,
  sortFlocks,
  versionHint,
  closeToast,
} from "./lib.js";

const SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPACE_WAIT_MS = 10_000;

// The only place the mod starts a program. Returns { ok, stdout, stderr, error }.
async function runCrook(args) {
  if (checking()) return { ok: false, stdout: "", stderr: "", error: "Not run while herdr-app checks the mod." };
  try {
    const r = await exec("sh", crookArgv(args), { cwd: home(), timeout: CROOK_TIMEOUT_MS });
    const error = errorText(r);
    return { ok: error === null, stdout: r.stdout ?? "", stderr: r.stderr ?? "", error };
  } catch (thrown) {
    return { ok: false, stdout: "", stderr: "", error: errorText({ thrown }) };
  }
}

// `crook status --json`, parsed: { flocks, crookVersion, hcError } or { error }.
async function readStatus() {
  const r = await runCrook(statusArgs());
  return r.ok ? parseStatus(r.stdout) : { error: r.error };
}

async function readMarker() {
  try {
    return await readTextFile(`${appHome()}/mods/crook/.crook-version`);
  } catch {
    return null;
  }
}

export default class Flocks extends View {
  init(_props, cx) {
    this.view = "list"; // "list" | "new" | "close"
    this.busy = false;
    this.busyText = "working…";
    this.error = ""; // shown under the form, or under the confirm
    this.sel = null; // the selected flock's slug; null is the "+ New flock…" row
    this.retry = null; // what Retry does after a failed Open
    this.flocks = undefined; // undefined while loading
    this.listError = "";
    this.hcError = null;
    this.hint = null;
    this.target = null; // the slug the close confirm is about
    this.refreshId = 0;
    this.name = "";
    this.nameInput = InputState.new({ placeholder: "login-bug" });
    this.nameInput.on("change", (_e, cx) => {
      this.name = this.nameInput.value();
      this.error = "";
      cx.notify();
    });
    this.nameInput.on("submit", (_e, cx) => this.create(cx));
    this.nameFocused = true;
    this.nameInput.on("focus", (_e, cx) => { this.nameFocused = true; cx.notify(); });
    this.nameInput.on("blur", (_e, cx) => { this.nameFocused = false; cx.notify(); });
    this.task = TextareaState.new({ placeholder: "What the coordinator should start on", rows: 6 });
    this.task.on("change", (_e, cx) => cx.notify());
    this.handle = cx.focus_handle();
    this.handle.focus();
    this.refresh(cx);
  }

  dismiss() {
    closeOverlay(props().overlay);
  }

  // Re-read the flock list. A late answer from an older call is dropped.
  refresh(cx) {
    if (checking()) return;
    const id = ++this.refreshId;
    this.listError = "";
    this.retry = null;
    cx.spawn(async (cx) => {
      let status;
      try {
        status = await readStatus();
      } catch (thrown) {
        status = { error: String(thrown?.message ?? thrown) };
      }
      if (id !== this.refreshId) return;
      if (status.error) {
        this.flocks = [];
        this.listError = status.error;
        this.hcError = null;
        this.hint = null;
        this.retry = null;
      } else {
        this.flocks = status.flocks;
        this.hcError = status.hcError;
        const hint = versionHint(await readMarker(), status.crookVersion);
        if (id !== this.refreshId) return;
        this.hint = hint;
      }
      cx.notify();
    });
  }

  rows() {
    return sortFlocks(this.flocks ?? [], liveFlocks(terminals()));
  }

  // The selected row: 0 is the "+ New flock…" row. A slug that left the list falls back to it.
  index(rows) {
    return this.sel === null ? 0 : rows.findIndex((f) => f.slug === this.sel) + 1;
  }

  show(view, cx) {
    this.view = view;
    this.error = "";
    // The target mounts on the next frame, so ask again shortly after.
    const focus = () => (view === "new" ? this.nameInput.focus() : this.handle.focus());
    focus();
    cx.timer.after(80, focus);
    cx.notify();
  }

  // Wait for the flock's space to show up in the app, then select it. False when it never did.
  async selectSpace(id, cx, waitMs) {
    const deadline = Date.now() + waitMs;
    for (;;) {
      if (spaces().some((s) => s.id === id)) {
        run("space.select", { id });
        return true;
      }
      if (Date.now() >= deadline) return false;
      await cx.sleep(200);
    }
  }

  // After `new` or `open`: read the flock's space from status again, then select it.
  async arrive(slug, fallbackSpace, cx) {
    const status = await readStatus();
    const space = (status.flocks ?? []).find((f) => f.slug === slug)?.space || fallbackSpace;
    if (space && (await this.selectSpace(space, cx, SPACE_WAIT_MS))) return;
    notify({ title: "Flock started; its space has not shown yet.", level: "info" });
  }

  // Runs `work` as the one busy section: `busy` always resets, and a throw shows as an error.
  // `fail(message)` says where an error is shown.
  async guarded(cx, text, work, fail) {
    this.busy = true;
    this.busyText = text;
    cx.notify();
    try {
      await work();
    } catch (thrown) {
      fail(String(thrown?.message ?? thrown));
    } finally {
      this.busy = false;
      cx.notify();
    }
  }

  create(cx) {
    if (this.busy) return;
    const slug = this.name;
    if (!checkSlug(slug).ok) return;
    const task = this.task.value();
    this.error = "";
    cx.spawn((cx) =>
      this.guarded(cx, "starting…", async () => {
        const r = await runCrook(newArgs(slug, task));
        if (!r.ok) {
          this.error = r.error;
          return;
        }
        await this.arrive(slug, "", cx);
        this.dismiss();
      }, (m) => { this.error = m; }),
    );
  }

  open(flock, cx) {
    if (this.busy || !flock) return;
    this.listError = "";
    this.retry = null;
    cx.spawn((cx) =>
      this.guarded(cx, flock.running ? "working…" : "starting…", async () => {
        const failed = (message) => {
          this.listError = message;
          this.retry = (cx) => this.open(this.rows().find((f) => f.slug === flock.slug) ?? flock, cx);
        };
        if (flock.running) {
          // Nothing to start: only show its space, which must already be in the app.
          if (flock.space && (await this.selectSpace(flock.space, cx, 0))) return this.dismiss();
          return failed("The flock is running, but its space is not in the app.");
        }
        const r = await runCrook(openArgs(flock.slug));
        if (!r.ok) return failed(r.error);
        await this.arrive(flock.slug, flock.space, cx);
        this.dismiss();
      }, (m) => {
        this.listError = m;
        this.retry = (cx) => this.open(this.rows().find((f) => f.slug === flock.slug) ?? flock, cx);
      }),
    );
  }

  confirmClose(flock, cx) {
    if (this.busy || !flock) return;
    this.target = flock.slug;
    this.show("close", cx);
  }

  closeFlock(cx) {
    if (this.busy || !this.target) return;
    const slug = this.target;
    this.error = "";
    cx.spawn((cx) =>
      this.guarded(cx, "closing…", async () => {
        const r = await runCrook(closeArgs(slug));
        if (!r.ok) {
          this.error = r.error;
          return;
        }
        notify(closeToast(slug, r.stderr));
        this.show("list", cx);
        this.refresh(cx);
      }, (m) => { this.error = m; }),
    );
  }

  key(e, cx) {
    // While crook runs nothing else may happen. Whether the app still closes the dialog on Esc is unmeasured.
    if (this.busy) {
      if (e.key === "escape" || e.key === "enter") cx.stop_propagation();
      return;
    }
    if (e.key === "escape") {
      if (this.view === "list") this.dismiss();
      else this.show("list", cx);
      return cx.stop_propagation();
    }
    if (this.view === "close" && e.key === "enter") {
      this.closeFlock(cx);
      return cx.stop_propagation();
    }
    if (this.view !== "list") return;
    const rows = this.rows();
    const n = rows.length + 1;
    const at = this.index(rows);
    if (e.key === "down" || e.key === "up") {
      const next = (at + (e.key === "down" ? 1 : n - 1)) % n;
      this.sel = next === 0 ? null : rows[next - 1].slug;
    } else if (e.key === "enter") {
      if (at === 0) this.show("new", cx);
      else this.open(rows[at - 1], cx);
    } else return;
    cx.stop_propagation();
    cx.notify();
  }

  button(id, text, primary, action) {
    const t = theme();
    return h_flex().id(id).h(32).px(14).items_center().rounded(8).border_1()
      .border_color(primary ? t.solid : t.input).bg(primary ? t.solid : t.raised)
      .text_color(primary ? t.solid_foreground : t.foreground).text_size(12.5).font_medium()
      .cursor_pointer().hover((s) => s.opacity(0.88)).on_click(action).child(text);
  }

  title(text) {
    const t = theme();
    return h_flex().items_center()
      .child(div().flex_1().text_size(15).font_semibold().text_color(t.foreground).child(text))
      .child(div().text_size(11).text_color(t.dim).child("Esc"));
  }

  spinner() {
    const t = theme();
    return h_flex().gap(6).items_center().text_size(12).text_color(t.dim)
      .child(div().frames(SPINNER, 90))
      .child(this.busyText);
  }

  renderList() {
    const t = theme();
    const rows = this.rows();
    const at = this.index(rows);
    const loading = this.flocks === undefined && !this.listError;
    const newRow = h_flex().id("crook-new").flex_none().h(36).px(10).items_center().rounded(7).cursor_pointer()
      .text_color(t.foreground)
      .when(at === 0, (el) => el.bg(t.selected))
      .when(at !== 0, (el) => el.hover((s) => s.bg(t.hover)))
      .on_click((_e, cx) => { if (!this.busy) this.show("new", cx); })
      .child("+ New flock…");
    // A row click only selects; Open and Close… are the buttons on the selected row, so one click never fires twice.
    const flockRow = (f, i) => {
      const on = this.sel === f.slug;
      return h_flex().id(`crook-flock-${i}`).flex_none().h(44).px(10).gap(8).items_center().rounded(7).cursor_pointer()
        .when(on, (el) => el.bg(t.selected))
        .when(!on, (el) => el.hover((s) => s.bg(t.hover)))
        .on_click((_e, cx) => { if (!this.busy) { this.sel = f.slug; cx.notify(); } })
        .child(
          v_flex().flex_1().min_w(0)
            .child(div().truncate().text_color(t.foreground).child(f.slug))
            .child(div().truncate().text_size(11.5).text_color(t.dim).child(f.date || "no date")),
        )
        .child(div().flex_none().text_size(11).text_color(f.running ? t.accent : t.dim).child(f.running ? "running" : "paused"))
        .when(on, (el) =>
          el.child(this.button(`crook-open-${i}`, "Open", true, (_e, cx) => this.open(f, cx)))
            .child(this.button(`crook-close-${i}`, "Close…", false, (_e, cx) => this.confirmClose(f, cx))),
        );
    };
    return v_flex().size_full().p(18).gap(10)
      .child(this.title("Flocks"))
      .child(
        v_flex().id("crook-list").flex_1().min_h(0).gap(2).overflow_y_scroll()
          .child(newRow)
          .when(loading, (el) => el.child(div().p(10).text_color(t.muted).child("…")))
          .children(rows.map(flockRow))
          .when(Boolean(this.listError), (el) =>
            el.child(
              v_flex().p(10).gap(8).items_start()
                .child(div().text_color(t.blocked).child(this.listError))
                .when(!this.busy, (el) => el.child(this.button("crook-retry", "Retry", false, (_e, cx) => (this.retry ? this.retry(cx) : this.refresh(cx))))),
            ),
          )
          .when(Boolean(this.hcError), (el) => el.child(div().px(10).text_size(11.5).text_color(t.dim).child(`hc: ${this.hcError}`))),
      )
      .when(this.busy, (el) => el.child(this.spinner()))
      .child(
        v_flex().gap(2)
          .child(div().text_size(11).text_color(t.dim).child("↑↓ choose · ↵ open · esc close"))
          .when(Boolean(this.hint), (el) => el.child(div().text_size(11).text_color(t.dim).child(this.hint))),
      );
  }

  renderNew() {
    const t = theme();
    const slug = checkSlug(this.name);
    const bad = !slug.ok && !slug.empty;
    const field = (label) => div().pt(10).pb(6).text_size(11.5).text_color(t.dim).child(label);
    // While crook runs the fields are plain text, so nothing can be typed.
    const frozen = (text) =>
      div().px(10).py(8).rounded(9).border_1().border_color(t.input).bg(t.background).text_size(12.8)
        .text_color(t.muted).child(text);
    return v_flex().size_full().p(18)
      .child(this.title("New flock"))
      .child(field("Name"))
      .child(
        this.busy
          ? frozen(this.name)
          : Input.new(this.nameInput).h(36).px(10).rounded(9).border_1()
              .border_color(this.nameFocused ? t.accent : t.input).bg(t.background).text_size(12.8).text_color(t.foreground),
      )
      .child(
        div().pt(6).text_size(11.5).text_color(bad ? t.blocked : t.dim)
          .child(bad ? slug.message : "Lower-case letters, digits and dashes. Used for the folder, the space and branch names."),
      )
      .child(field("First task (optional)"))
      .child(
        this.busy
          ? frozen(this.task.value())
          : Textarea.new(this.task).h(120).px(10).py(8).rounded(9).border_1().border_color(t.input).bg(t.background).text_size(12.8).text_color(t.foreground),
      )
      .child(
        div().pt(6).text_size(11.5).text_color(t.dim)
          .child("What the coordinator should start on. Leave empty to start it with its standard prompt."),
      )
      .when(Boolean(this.error), (el) => el.child(div().pt(10).text_color(t.blocked).child(this.error)))
      .child(div().flex_1())
      .child(
        h_flex().pt(16).gap(8).justify_end().items_center()
          .when(this.busy, (el) => el.child(this.spinner()))
          .when(!this.busy, (el) =>
            el.child(this.button("crook-back", "Back", false, (_e, cx) => this.show("list", cx)))
              .child(
                slug.ok
                  ? this.button("crook-create", "Create", true, (_e, cx) => this.create(cx))
                  : h_flex().h(32).px(14).items_center().rounded(8).border_1().border_color(t.input).bg(t.raised)
                      .text_color(t.dim).text_size(12.5).font_medium().child("Create"),
              ),
          ),
      );
  }

  renderClose() {
    const t = theme();
    return v_flex().size_full().p(18).gap(12)
      .child(this.title(`Close flock ${this.target}?`))
      .child(div().text_color(t.muted).child("Ends its coordinator and children, marks it done, and removes clean worktrees. Branches stay."))
      .when(Boolean(this.error), (el) => el.child(div().text_color(t.blocked).child(this.error)))
      .child(div().flex_1())
      .child(
        h_flex().gap(8).justify_end().items_center()
          .when(this.busy, (el) => el.child(this.spinner()))
          .when(!this.busy, (el) =>
            el.child(this.button("crook-cancel", "Cancel", false, (_e, cx) => this.show("list", cx)))
              .child(this.button("crook-confirm", "Close flock", true, (_e, cx) => this.closeFlock(cx))),
          ),
      );
  }

  render() {
    const t = theme();
    const body = this.view === "new" ? this.renderNew() : this.view === "close" ? this.renderClose() : this.renderList();
    return v_flex().id("crook-root").track_focus(this.handle).on_key_down((e, cx) => this.key(e, cx))
      .size_full().min_h(0).bg(t.popover).text_color(t.foreground).text_size(13)
      .child(body);
  }
}
