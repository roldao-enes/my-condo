#!/usr/bin/env bash
# Sets the version of every workspace crate (Cargo.toml files and Cargo.lock) to $1.
#   scripts/release/bump.sh 0.2.0
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

version="${1:?usage: bump.sh MAJOR.MINOR.PATCH}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Not a SemVer version: $version" >&2; exit 1; }

names=""
for manifest in crates/*/Cargo.toml; do
  # Replace only the first `version = ` line, which is the one in [package].
  awk -v v="$version" '!done && /^version = "/ { print "version = \"" v "\""; done=1; next } { print }' \
    "$manifest" > "$manifest.tmp" && mv "$manifest.tmp" "$manifest"
  names="$names $(sed -n 's/^name = "\(.*\)"/\1/p' "$manifest" | head -1)"
done

# Cargo.lock lists our own crates as `name = ".."` followed by `version = ".."`.
awk -v v="$version" -v names="$names" '
  BEGIN { n = split(names, a, " "); for (i = 1; i <= n; i++) ours[a[i]] = 1 }
  /^name = "/ { name = $3; gsub(/"/, "", name); hit = (name in ours) }
  hit && /^version = "/ { print "version = \"" v "\""; hit = 0; next }
  { print }
' Cargo.lock > Cargo.lock.tmp && mv Cargo.lock.tmp Cargo.lock

echo "Set the version of:$names to $version"
