#!/usr/bin/env bash
# What .github/workflows/changelog-release.yml runs. The newest CHANGELOG.md heading is the release.
#   version           the newest version in CHANGELOG.md
#   notes VER         that version's changelog entry
#   published VER     succeeds when every crate is on crates.io at VER
#   check VER         fails when VER is below crates.io's newest, or an older tagged version never got there
#   bump VER          writes VER into the manifests, the lock file and the README's install snippets
set -euo pipefail
cd "$(dirname "$0")/.."

eval "$(grep -m1 '^CRATES=(' scripts/publish-crates.sh)"

index() {
  local body status
  body=$(mktemp)
  status=$(curl -sSL --max-time 30 --retry 3 -o "$body" -w '%{http_code}' "https://index.crates.io/${1:0:2}/${1:2:2}/$1")
  case $status in
    200) cat "$body" ;;
    404) ;;
    *) echo "crates.io index answered $status for $1" >&2; rm -f "$body"; return 1 ;;
  esac
  rm -f "$body"
}

versions() { grep -oE '^## \[[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?\]' CHANGELOG.md | sed -E 's/^## \[(.*)\]$/\1/'; }

version() {
  local v
  v=$(versions | sed -n 1p)
  [ -n "$v" ] || { echo "no '## [x.y.z]' heading in CHANGELOG.md" >&2; exit 1; }
  echo "$v"
}

notes() { awk -v h="## [$1]" 'index($0, h) == 1 {f=1; next} f && /^## / {exit} f {print}' CHANGELOG.md | sed -e '/./,$!d'; }

published() {
  local crate lines
  for crate in "${CRATES[@]}"; do
    lines=$(index "$crate") || exit 2
    grep -F "\"vers\":\"$1\"" <<< "$lines" | grep -q '"yanked":false' || return 1
  done
}

check() {
  local lines on newest oldest v
  # The last crate in dependency order is on crates.io only when the others are.
  lines=$(index "${CRATES[${#CRATES[@]} - 1]}") || exit 2
  on=$(grep -oE '"vers":"[^"]+"' <<< "$lines" | cut -d'"' -f4 | sort -V || true)
  [ -n "$on" ] || return 0
  newest=$(tail -1 <<< "$on")
  oldest=$(head -1 <<< "$on")
  if [ "$1" != "$newest" ] && [ "$(printf '%s\n%s\n' "$1" "$newest" | sort -V | head -1)" = "$1" ]; then
    echo "CHANGELOG.md's newest version $1 is below crates.io's $newest" >&2
    exit 1
  fi
  for v in $(versions); do
    [ "$v" != "$1" ] || continue
    # Versions before the first one on crates.io were never meant to be there.
    [ "$(printf '%s\n%s\n' "$v" "$oldest" | sort -V | head -1)" = "$oldest" ] || continue
    git ls-remote --exit-code origin "refs/tags/v$v" > /dev/null 2>&1 || continue
    grep -qxF "$v" <<< "$on" || { echo "tagged v$v is not on crates.io; recover it with: gh workflow run publish.yml -f tag=v$v" >&2; exit 1; }
  done
}

bump() {
  python3 - "$1" "${CRATES[@]}" <<'PY'
import glob, json, re, sys
new, crates = sys.argv[1], sys.argv[2:]
minor = ".".join(new.split(".")[:2])
def edit(path, f):
    try:
        old = open(path, encoding="utf-8").read()
    except FileNotFoundError:
        return
    text = f(old)
    if text != old:
        open(path, "w", encoding="utf-8").write(text)
names = "|".join(map(re.escape, sorted(crates, key=len, reverse=True)))
def manifest(text):
    # [package] and [workspace.package] versions, then the sibling crates' path requirements.
    text = re.sub(r'(?ms)^(\[(?:workspace\.)?package\][^\[]*?^version = )"[^"]*"', rf'\g<1>"{new}"', text)
    return re.sub(rf'(?m)^((?:{names}) = \{{[^}}\n]*\bpath = [^}}\n]*\bversion = )"[^"]*"', rf'\g<1>"{new}"', text)
for path in ["Cargo.toml", *glob.glob("crates/*/Cargo.toml")]:
    edit(path, manifest)
version_field = lambda t: re.sub(r'(?m)^(  "version": )"[^"]*"', rf'\g<1>"{new}"', t, count=1)
for path in ["nsh.json", *glob.glob("packages/*/package.json")]:
    edit(path, version_field)
def snippets(text):
    text = re.sub(rf'(?m)^((?:{names}) = )"[0-9][^"]*"', rf'\g<1>"{minor}"', text)
    text = re.sub(rf'(?m)^((?:{names}) = \{{ version = )"[0-9][^"]*"', rf'\g<1>"{minor}"', text)
    return re.sub(rf'("(?:{names})": )"\^[0-9][^"]*"', rf'\g<1>"^{new}"', text)
for path in ["README.md", *glob.glob("docs/getting-started.md")]:
    edit(path, snippets)
PY
  local crate v
  for crate in "${CRATES[@]}"; do
    v=$(cargo metadata --no-deps --format-version 1 | jq -er --arg c "$crate" '.packages[] | select(.name == $c) | .version')
    [ "$v" = "$1" ] || { echo "$crate is still $v" >&2; exit 1; }
  done
  # The lock file names the workspace's own versions.
  if git ls-files --error-unmatch Cargo.lock > /dev/null 2>&1; then cargo update --workspace --quiet; fi
}

case ${1:-} in
  version) version ;;
  notes | published | check | bump) "$1" "${2:?usage: release.sh $1 VERSION}" ;;
  *) echo "usage: release.sh version | notes VER | published VER | check VER | bump VER" >&2; exit 2 ;;
esac
