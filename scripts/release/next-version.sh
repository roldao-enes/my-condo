#!/usr/bin/env bash
# Prints the next SemVer version derived from the Conventional Commits made since the last release
# tag (vX.Y.Z), or nothing if none of them warrants a release.
#   breaking change (`type!:` or a `BREAKING CHANGE:` footer)  -> MAJOR
#   feat                                                       -> MINOR
#   fix                                                        -> PATCH
#   anything else (docs, chore, ci, ...)                       -> no release
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

current="$(sed -n 's/^version = "\(.*\)"/\1/p' crates/condo-ui/Cargo.toml | head -1)"
if [[ ! "$current" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "Cannot parse the current version '$current' as MAJOR.MINOR.PATCH" >&2; exit 1
fi
major="${BASH_REMATCH[1]}"; minor="${BASH_REMATCH[2]}"; patch="${BASH_REMATCH[3]}"

# Only plain releases count as a baseline, not the -build.N snapshots.
last_tag="$(git tag --list 'v*' --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -1 || true)"
range="${last_tag:+$last_tag..}HEAD"

level=0 # 0 none, 1 patch, 2 minor, 3 major
while read -r sha; do
  [[ -z "$sha" ]] && continue
  subject="$(git log -1 --format=%s "$sha")"
  body="$(git log -1 --format=%b "$sha")"
  if [[ "$subject" =~ ^[a-z]+(\([^\)]*\))?!: ]] || grep -qE '^BREAKING[ -]CHANGE: ' <<<"$body"; then
    (( level < 3 )) && level=3
  elif [[ "$subject" =~ ^feat(\([^\)]*\))?: ]]; then
    (( level < 2 )) && level=2
  elif [[ "$subject" =~ ^fix(\([^\)]*\))?: ]]; then
    (( level < 1 )) && level=1
  fi
done < <(git rev-list --no-merges "$range")

case "$level" in
  0) exit 0 ;;
  1) patch=$((patch + 1)) ;;
  2) minor=$((minor + 1)); patch=0 ;;
  3) major=$((major + 1)); minor=0; patch=0 ;;
esac
echo "${major}.${minor}.${patch}"
