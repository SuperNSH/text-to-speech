#!/usr/bin/env bash
# Publishes the crates to crates.io. Used by .github/workflows/publish.yml.
#   verify         packages the crates and builds them from the packages; needs no token
#   publish VER    uploads the crates that are not on crates.io at VER yet; needs CARGO_REGISTRY_TOKEN
# A published version cannot be replaced or deleted, so a rerun skips what is already there.
set -euo pipefail
cd "$(dirname "$0")/.."

# In dependency order.
CRATES=(nsh-text-to-speech)
TRIES=${TRIES:-30}
WAIT=${WAIT:-10}
# crates.io takes one new crate every ten minutes after the first few.
LIMIT_TRIES=${LIMIT_TRIES:-24}
LIMIT_WAIT=${LIMIT_WAIT:-620}

LOCKED=()
git ls-files --error-unmatch Cargo.lock > /dev/null 2>&1 && LOCKED=(--locked)

# Prints the crate's index lines; nothing when crates.io does not have it yet (404).
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

# A failed index read must stop the script, not count as "not published".
has() {
  local lines
  lines=$(index "$1") || exit 1
  grep -qF "\"vers\":\"$2\"" <<< "$lines"
}

crate_version() {
  cargo metadata --no-deps --format-version 1 ${LOCKED[@]+"${LOCKED[@]}"} | jq -er --arg c "$1" '.packages[] | select(.name == $c) | .version'
}

verify() {
  local crate args=()
  for crate in "${CRATES[@]}"; do args+=(-p "$crate"); done
  # Together, so that a crate can build against the packaged version of another.
  cargo package ${LOCKED[@]+"${LOCKED[@]}"} "${args[@]}"
}

# `verify` built the package in a step without the token; --no-verify keeps build scripts away from it.
upload() {
  local i log
  log=$(mktemp)
  for ((i = 0; ; i++)); do
    if cargo publish ${LOCKED[@]+"${LOCKED[@]}"} --no-verify -p "$1" 2>&1 | tee "$log"; then
      rm -f "$log"
      return 0
    fi
    if ! grep -q "429" "$log" || [ "$i" -ge "$LIMIT_TRIES" ]; then
      rm -f "$log"
      return 1
    fi
    echo "crates.io limits new crates; trying $1 again in $LIMIT_WAIT seconds"
    sleep "$LIMIT_WAIT"
  done
}

publish() {
  local version=$1 crate v i
  for crate in "${CRATES[@]}"; do
    v=$(crate_version "$crate")
    [ "$v" = "$version" ] || { echo "$crate is $v, not $version" >&2; exit 1; }
  done
  for crate in "${CRATES[@]}"; do
    if has "$crate" "$version"; then
      echo "$crate $version is on crates.io already"
      continue
    fi
    upload "$crate"
    for ((i = 0; ; i++)); do
      has "$crate" "$version" && break
      [ "$i" -lt "$TRIES" ] || { echo "$crate $version did not appear on crates.io" >&2; exit 1; }
      sleep "$WAIT"
    done
    echo "published $crate $version"
  done
}

case ${1:-} in
  verify) verify ;;
  publish) publish "${2:?usage: publish-crates.sh publish VERSION}" ;;
  *) echo "usage: publish-crates.sh verify | publish VERSION" >&2; exit 2 ;;
esac
