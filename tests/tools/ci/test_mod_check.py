"""Tests for ``toolr ci mod-check`` and ``mod-herdr-check``: the real mod and deliberately broken copies."""

from __future__ import annotations

import json
import shutil
import stat
import subprocess
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.ci import SHIM
from tools.ci import check_mod
from tools.ci import herdr_app_check
from tools.ci import mod_check
from tools.ci import mod_herdr_check

REPO_ROOT = Path(__file__).resolve().parents[3]

needs_node = pytest.mark.skipif(shutil.which("node") is None, reason="node is not installed")


@pytest.fixture
def mod(tmp_path: Path) -> Path:
    """A copy of the real mod folder, as `<tmp>/mod`, for a test to break."""
    dest = tmp_path / "mod"
    shutil.copytree(REPO_ROOT / "mod", dest, ignore=shutil.ignore_patterns("gpui-kit.d.ts", "jsconfig.json"))
    return dest


def _mod_json(mod: Path) -> dict:
    return json.loads((mod / "mod.json").read_text(encoding="utf-8"))


def _write_mod_json(mod: Path, data: object) -> None:
    (mod / "mod.json").write_text(json.dumps(data), encoding="utf-8")


def _problems(mod: Path, needle: str) -> bool:
    return any(needle in e for e in check_mod(mod))


@needs_node
def test_the_real_mod_passes() -> None:
    assert check_mod(REPO_ROOT / "mod") == []


def test_a_missing_mod_folder_fails(tmp_path: Path) -> None:
    assert check_mod(tmp_path / "nope")


@needs_node
def test_a_syntax_error_fails(mod: Path) -> None:
    with (mod / "main.js").open("a", encoding="utf-8") as handle:
        handle.write("\nconst = ;\n")
    assert _problems(mod, "main.js: node --check failed")


@needs_node
def test_a_script_is_read_as_a_module_whatever_the_folder_holds(mod: Path) -> None:
    # `import` and `export` only parse as a module; a CommonJS read would fail on the real files.
    (mod / "package.json").write_text('{"type": "commonjs"}', encoding="utf-8")
    assert check_mod(mod) == []


def test_a_folder_without_scripts_fails(mod: Path) -> None:
    for script in mod.glob("*.js"):
        script.unlink()
    assert _problems(mod, "no .js files")


def test_mod_json_that_does_not_parse_fails(mod: Path) -> None:
    (mod / "mod.json").write_text("{", encoding="utf-8")
    assert _problems(mod, "not valid JSON")


def test_mod_json_that_is_not_an_object_fails(mod: Path) -> None:
    _write_mod_json(mod, [])
    assert _problems(mod, "must be a JSON object")


@pytest.mark.parametrize("key", ["place", "title", "overlay", "palette"])
def test_a_missing_top_level_field_fails(mod: Path, key: str) -> None:
    data = _mod_json(mod)
    del data[key]
    _write_mod_json(mod, data)
    assert _problems(mod, f"missing `{key}`")


@pytest.mark.parametrize("key", ["kind", "width", "height"])
def test_a_missing_overlay_field_fails(mod: Path, key: str) -> None:
    data = _mod_json(mod)
    del data["overlay"][key]
    _write_mod_json(mod, data)
    assert _problems(mod, f"overlay is missing `{key}`")


@pytest.mark.parametrize("key", ["title", "command", "args", "words"])
def test_a_missing_palette_field_fails(mod: Path, key: str) -> None:
    data = _mod_json(mod)
    del data["palette"][0][key]
    _write_mod_json(mod, data)
    assert _problems(mod, f"palette[0] is missing `{key}`")


@pytest.mark.parametrize(
    ("path", "value"),
    [
        (("title",), 3),
        (("overlay",), "dialog"),
        (("overlay", "width"), "560"),
        (("overlay", "height"), True),
        (("palette",), {}),
        (("palette", 0, "args"), []),
    ],
)
def test_a_wrong_type_fails(mod: Path, path: tuple, value: object) -> None:
    data = _mod_json(mod)
    node = data
    for key in path[:-1]:
        node = node[key]
    node[path[-1]] = value
    _write_mod_json(mod, data)
    assert _problems(mod, "must be a")


def test_an_empty_palette_fails(mod: Path) -> None:
    data = _mod_json(mod)
    data["palette"] = []
    _write_mod_json(mod, data)
    assert _problems(mod, "palette must not be empty")


def test_a_palette_entry_that_is_not_an_object_fails(mod: Path) -> None:
    data = _mod_json(mod)
    data["palette"] = ["x"]
    _write_mod_json(mod, data)
    assert _problems(mod, "palette[0] must be an object")


def _append(mod: Path, text: str) -> None:
    with (mod / "main.js").open("a", encoding="utf-8") as handle:
        handle.write(text)


@pytest.mark.parametrize(
    "line",
    [
        'await exec("crook", ["status"]);',
        "await exec('crook', ['status']);",
        'await exec("sh", ["-c", "crook status"]);',
        "await exec(program, args);",
        'await exec(\n  "crook", []);',
        'await exec\n("crook", []);',
        'await exec/**/("crook", []);',
        'await exec?.("crook", []);',
        'await exec("sh", ["-c", "x"]);',
        'const s = "// not a comment"; await exec("crook", []);',
        'const s = `${await exec("crook", [])}`;',
        'await exec(`sh`, crookArgv([]));',
    ],
)
def test_an_exec_that_skips_the_shim_fails(mod: Path, line: str) -> None:
    _append(mod, f"\nasync function bad() {{ {line} }}\n")
    assert _problems(mod, 'exec must run "sh"')


def test_exec_used_without_a_direct_call_fails(mod: Path) -> None:
    _append(mod, '\nconst run = exec;\n')
    assert _problems(mod, "without being called directly")


def test_an_aliased_import_of_exec_is_checked(mod: Path) -> None:
    _append(mod, 'import { exec as ex } from "herdr";\nasync function bad() { await ex("crook", []); }\n')
    assert _problems(mod, 'exec must run "sh"')


@pytest.mark.parametrize("call", ['h.exec("crook", [])', 'h?.exec("crook", [])', 'h\n.exec("crook", [])'])
def test_a_namespace_import_of_exec_is_checked(mod: Path, call: str) -> None:
    _append(mod, f'import * as h from "herdr";\nasync function bad() {{ await {call}; }}\n')
    assert _problems(mod, 'exec must run "sh"')


def test_a_namespace_index_is_refused(mod: Path) -> None:
    _append(mod, 'import * as h from "herdr";\nasync function bad() { await h["exec"]("crook"); }\n')
    assert _problems(mod, "hides which function")


def test_an_aliased_exec_through_the_shim_passes(mod: Path) -> None:
    _append(mod, 'import { exec as ex } from "herdr";\nasync function good() { await ex("sh", crookArgv([])); }\n')
    assert not _problems(mod, "exec must run")


def test_a_multi_line_exec_through_the_shim_passes(mod: Path) -> None:
    _append(mod, '\nasync function good() {\n  await exec(\n    "sh",\n    crookArgv(["status"]),\n    {},\n  );\n}\n')
    assert not _problems(mod, "exec must")


def test_a_script_in_a_subfolder_is_checked(mod: Path) -> None:
    (mod / "sub").mkdir()
    (mod / "sub" / "x.mjs").write_text('await exec("crook", []);\n', encoding="utf-8")
    assert _problems(mod, "x.mjs")


def test_an_exec_through_the_shim_passes(mod: Path) -> None:
    _append(mod, '\nasync function good() { await exec("sh", crookArgv(["status"]), {}); }\n')
    assert not _problems(mod, "exec must run")


def test_a_regex_exec_and_a_comment_are_not_calls(mod: Path) -> None:
    _append(
        mod,
        '\nconst m = /x\'"/.exec("x");\n// exec("crook", [])\nconst n = 1; // exec("crook")\n'
        '/* exec("crook") */ const t = `exec("crook")`; const q = "exec(\\"crook\\")"; const d = 4 / 2 / 1;\n',
    )
    assert not _problems(mod, "exec must run")


def test_old_shim_text_in_a_comment_does_not_pass(mod: Path) -> None:
    lib = mod / "lib.js"
    text = lib.read_text(encoding="utf-8")
    lib.write_text(
        text.replace("SHIM_SCRIPT =", f'// exec {SHIM} "$@"\nexport const SHIM_SCRIPT = "other"; const OLD =', 1), encoding="utf-8"
    )
    assert _problems(mod, "SHIM_SCRIPT is not assigned exactly")


def test_a_missing_shim_script_fails(mod: Path) -> None:
    lib = mod / "lib.js"
    lib.write_text(lib.read_text(encoding="utf-8").replace("SHIM_SCRIPT", "SOMETHING_ELSE"), encoding="utf-8")
    assert _problems(mod, "SHIM_SCRIPT is not defined")


def test_a_missing_lib_fails(mod: Path) -> None:
    (mod / "lib.js").unlink()
    assert _problems(mod, "cannot be checked")


def test_a_changed_shim_script_fails(mod: Path) -> None:
    lib = mod / "lib.js"
    lib.write_text(lib.read_text(encoding="utf-8").replace("CROOK_BIN", "CROOK_BINARY"), encoding="utf-8")
    assert _problems(mod, "SHIM_SCRIPT is not assigned exactly")


def test_the_shim_constant_matches_the_script_in_lib() -> None:
    assert f'exec {SHIM} "$@"' in (REPO_ROOT / "mod" / "lib.js").read_text(encoding="utf-8")


def _run(command, repo: Path) -> tuple[int, str]:
    ctx = make_context(repo)
    try:
        command(ctx)
    except SystemExit as exc:
        return int(exc.code or 0), ctx.stdout + ctx.stderr
    return 0, ctx.stdout + ctx.stderr


@needs_node
def test_the_command_passes_on_the_real_mod() -> None:
    code, output = _run(mod_check, REPO_ROOT)
    assert code == 0, output
    assert "mod-check: ok" in output


def test_the_command_fails_and_names_the_problem(mod: Path) -> None:
    (mod / "mod.json").write_text("{", encoding="utf-8")
    code, output = _run(mod_check, mod.parent)
    assert code == 1
    assert "mod-check:" in output
    assert "mod.json" in output


# herdr-app: a fake that logs where it was called and prints what a real one printed for the real mod.
FAKE_HERDR_APP = """#!/bin/sh
echo "$1 $2 $HERDR_APP_HOME" >> "$FAKE_LOG"
[ -n "$FAKE_ENV_LOG" ] && printf '%s\\n%s\\n%s\\n%s\\n' "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" >> "$FAKE_ENV_LOG"
if [ "$FAKE_FAIL" = 1 ]; then echo "mod is broken" >&2; exit 1; fi
echo 'v_flex :id[Str("crook-root")]'
echo "$FAKE_IDS"
"""


@pytest.fixture
def fake_herdr_app(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    fake = bin_dir / "herdr-app"
    fake.write_text(FAKE_HERDR_APP, encoding="utf-8")
    fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
    log = tmp_path / "log"
    monkeypatch.setenv("PATH", f"{bin_dir}:/usr/bin:/bin")
    monkeypatch.setenv("FAKE_LOG", str(log))
    monkeypatch.setenv("FAKE_IDS", 'h_flex :id[Str("crook-new")]')
    monkeypatch.delenv("FAKE_FAIL", raising=False)
    return log


def test_herdr_app_check_runs_on_a_copy_and_leaves_the_mod_alone(mod: Path, fake_herdr_app: Path) -> None:
    before = sorted(p.name for p in mod.iterdir())
    assert herdr_app_check(mod, "herdr-app") == []
    assert sorted(p.name for p in mod.iterdir()) == before
    calls = fake_herdr_app.read_text(encoding="utf-8").splitlines()
    assert [c.split()[0] for c in calls] == ["check", "check"]
    assert all(str(mod) not in c for c in calls)


def test_herdr_app_check_uses_a_throwaway_app_home(mod: Path, fake_herdr_app: Path) -> None:
    herdr_app_check(mod, "herdr-app")
    homes = {line.split()[-1] for line in fake_herdr_app.read_text(encoding="utf-8").splitlines()}
    assert len(homes) == 1
    assert "crook-mod-check-" in homes.pop()


def test_herdr_app_check_fails_when_herdr_app_does(mod: Path, fake_herdr_app: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("FAKE_FAIL", "1")
    errors = herdr_app_check(mod, "herdr-app")
    assert errors
    assert "mod is broken" in errors[0]


def test_herdr_app_check_fails_when_an_element_id_is_missing(
    mod: Path, fake_herdr_app: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("FAKE_IDS", "")
    assert herdr_app_check(mod, "herdr-app") == ["--print-spec does not show the element id crook-new"]


def test_herdr_app_check_moves_home_and_xdg_folders(mod: Path, fake_herdr_app: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    seen = fake_herdr_app.parent / "env"
    monkeypatch.setenv("FAKE_ENV_LOG", str(seen))
    herdr_app_check(mod, "herdr-app")
    lines = seen.read_text(encoding="utf-8").splitlines()
    assert len(lines) == 8
    assert all("crook-mod-check-" in line for line in lines)


def test_an_element_id_must_be_an_id_not_text(mod: Path, fake_herdr_app: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("FAKE_IDS", 'text "crook-new"')
    assert herdr_app_check(mod, "herdr-app") == ["--print-spec does not show the element id crook-new"]


def test_the_herdr_app_command_skips_when_herdr_app_is_not_installed(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PATH", "/nonexistent")
    code, output = _run(mod_herdr_check, REPO_ROOT)
    assert code == 0
    assert "herdr-app is not installed, skipping" in output


def test_the_herdr_app_command_passes_with_a_fake(fake_herdr_app: Path) -> None:
    code, output = _run(mod_herdr_check, REPO_ROOT)
    assert code == 0, output
    assert "mod-herdr-check: ok" in output


def test_the_herdr_app_command_fails_with_a_failing_fake(fake_herdr_app: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("FAKE_FAIL", "1")
    code, output = _run(mod_herdr_check, REPO_ROOT)
    assert code == 1
    assert "mod-herdr-check:" in output


def test_a_missing_node_fails_instead_of_passing(mod: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PATH", "/nonexistent")
    assert _problems(mod, "node is not on PATH")


def test_a_hung_herdr_app_fails(mod: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    def hang(*args: object, **kwargs: object) -> None:
        raise subprocess.TimeoutExpired("herdr-app", 120)

    monkeypatch.setattr(subprocess, "run", hang)
    assert herdr_app_check(mod, "herdr-app") == ["herdr-app check timed out"]
