#!/usr/bin/env bash
# Installs a published crook release the way the maintainer does, `mise use -g packslip:...`, in a
# scratch HOME, and checks what hooks and the mod depend on (0019, 0020). Used by install-smoke.yml:
#
#   scripts/install-smoke.sh <version>
#
# Needs `mise` and `jq` on PATH, and a published release with its packslip. It writes nothing outside
# the scratch folder: the mise data folder is left at its default, which is under the scratch HOME, so
# the shim sits at the exact path the hooks fall back to.
set -euo pipefail

fail() {
  echo "::error::install-smoke: $*" >&2
  exit 1
}

[ "$#" -eq 1 ] || {
  echo "usage: $0 <version>" >&2
  exit 2
}
version="${1#v}"
printf '%s' "$version" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-rc\.(0|[1-9][0-9]*))?$' \
  || fail "'$1' is not X.Y.Z or X.Y.Z-rc.N"
mise_bin="$(command -v mise)" || fail "mise is not on PATH"
jq_bin="$(command -v jq)" || fail "jq is not on PATH"
tool="packslip:github.com/s0undt3ch/shepherds-crook"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/home" "$work/cwd"
home="$(cd "$work/home" && pwd -P)"
app_home="$home/app"

# Nothing is inherited but a token (anonymous GitHub calls hit the rate limit, and mise then drops packslip
# resources), and nothing sets MISE_DATA_DIR or an XDG folder: the data folder takes its default, under $home.
extra=()
[ -z "${GITHUB_TOKEN:-}" ] || extra+=("GITHUB_TOKEN=$GITHUB_TOKEN")
run() {
  (
    cd "$work/cwd"
    env -i PATH=/usr/bin:/bin HOME="$home" HERDR_APP_HOME="$app_home" \
      MISE_CEILING_PATHS="$work" MISE_YES=1 \
      MISE_MINIMUM_RELEASE_AGE=0 ${extra[@]+"${extra[@]}"} "$@"
  )
}

echo "-- install"
run "$mise_bin" use -g "$tool@$version" || fail "mise use -g $tool@$version failed"

echo "-- the shim, at the path the hooks call"
data="$home/.local/share/mise"
shim="$data/shims/crook"
if [ ! -x "$shim" ]; then
  echo "mise data folder contents:" >&2
  ls -la "$data" "$data/shims" >&2 2>&1 || true
  fail "no executable shim at $shim (a packslip install made none, or mise keeps its data elsewhere)"
fi

echo "-- crook --version through the shim"
out="$(run "$shim" --version)" || fail "$shim --version failed"
echo "$out"
[ "$out" = "crook $version" ] || fail "expected 'crook $version', got '$out'"

echo "-- the install folder"
where="$(run "$mise_bin" where "$tool")" || fail "mise where $tool failed"
install="$(cd "$where" && pwd -P)"
for rel in crook plugin/.claude-plugin/plugin.json mod/mod.json; do
  [ -e "$install/$rel" ] || fail "$install has no $rel"
done
[ "$("$jq_bin" -r .version "$install/plugin/.claude-plugin/plugin.json")" = "$version" ] \
  || fail "the installed plugin.json does not say $version"

echo "-- crook setup, through the shim"
# Its trust step only prints commands (Q22), so nothing here touches Claude's config.
run "$shim" setup </dev/null || fail "crook setup failed"
link="$home/.local/share/crook/current"
[ -L "$link" ] || fail "setup did not make $link"
[ "$(cd "$link" && pwd -P)" = "$install" ] \
  || fail "$link points at $(readlink "$link"), not the mise install folder $install"
marker="$app_home/mods/crook/.crook-version"
[ -f "$marker" ] || fail "setup did not write $marker"
[ "$(tr -d '[:space:]' <"$marker")" = "$version" ] || fail "$marker does not hold $version"

echo "-- crook doctor --json"
# No hc on a runner, so doctor exits 1; the report on standard output is what matters.
set +e
report="$(run "$shim" doctor --json </dev/null 2>/dev/null)"
set -e
field() { printf '%s' "$report" | "$jq_bin" -r "$1" 2>/dev/null || true; }
canonical() { [ -d "$1" ] && (cd "$1" && pwd -P) || printf '%s' "$1"; }
[ "$(canonical "$(field .plugin_dir)")" = "$install/plugin" ] \
  || fail "doctor reports plugin_dir $(field .plugin_dir), not $install/plugin"
[ "$(canonical "$(field .mod_dir)")" = "$install/mod" ] \
  || fail "doctor reports mod_dir $(field .mod_dir), not $install/mod"
[ "$(canonical "$(field .herdr_app_home)")" = "$app_home" ] \
  || fail "doctor reports herdr_app_home $(field .herdr_app_home), not $app_home"
[ "$(field '.checks[] | select(.id == "shim") | .status')" = "ok" ] \
  || fail "doctor does not report the shim as present: $(field '.checks[] | select(.id == "shim")')"

echo "-- crook completion print zsh"
completion="$(run "$shim" completion print zsh)" || fail "crook completion print zsh failed"
[ -n "$completion" ] || fail "crook completion print zsh printed nothing"

echo "install-smoke: ok ($version)"
