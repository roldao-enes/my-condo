#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
  printf 'Usage: %s ghcr.io/roldao-enes/my-condo@sha256:<64-hex-digest>\n' "$0" >&2
  exit 2
fi

image=$1
if ! printf '%s\n' "$image" | grep -Eq '^ghcr\.io/roldao-enes/my-condo@sha256:[[:xdigit:]]{64}$'; then
  printf 'Expected an immutable GHCR image digest for roldao-enes/my-condo.\n' >&2
  exit 2
fi

cd "$(dirname "$0")"

compose_file=compose.cloud.yaml
env_file=.env.cloud

if [ ! -f "$compose_file" ] || [ ! -f "$env_file" ]; then
  printf 'Run this script from the deployment directory containing %s and %s.\n' "$compose_file" "$env_file" >&2
  exit 1
fi

old_image=$(awk -F= '$1 == "APP_IMAGE" { count++; value = substr($0, index($0, "=") + 1) } END { if (count != 1) exit 1; print value }' "$env_file") || {
  printf '%s must contain exactly one APP_IMAGE entry.\n' "$env_file" >&2
  exit 1
}

export APP_IMAGE="$image"

compose() {
  docker compose -f "$compose_file" --env-file "$env_file" "$@"
}

compose config --quiet
compose --profile tools pull
compose run --rm migrate
compose up -d --wait --wait-timeout 180

if printf '%s\n' "$old_image" | grep -Eq '^ghcr\.io/roldao-enes/my-condo@sha256:[[:xdigit:]]{64}$' && [ "$old_image" != "$image" ]; then
  previous_tmp=$(mktemp .app-image.previous.XXXXXX)
  trap 'rm -f "$previous_tmp"' EXIT
  printf '%s\n' "$old_image" > "$previous_tmp"
  chmod 600 "$previous_tmp"
  mv "$previous_tmp" .app-image.previous
  trap - EXIT
fi

env_tmp=$(mktemp .env.cloud.XXXXXX)
trap 'rm -f "$env_tmp"' EXIT
awk -v image="$image" 'BEGIN { count = 0 } /^APP_IMAGE=/ { print "APP_IMAGE=" image; count++; next } { print } END { if (count != 1) exit 1 }' "$env_file" > "$env_tmp"
chmod 600 "$env_tmp"
mv "$env_tmp" "$env_file"
trap - EXIT

printf 'Deployed %s and updated %s.\n' "$image" "$env_file"
if [ -f .app-image.previous ]; then
  printf 'Previous digest for rollback: %s\n' "$(cat .app-image.previous)"
fi
