#!/usr/bin/env bash
# Builds rterm inside a Rust container, so no toolchain is needed on the host.
# Works with any Docker-CLI-compatible runtime (Docker Engine, Podman, Rancher Desktop).
#   ./build.sh                 # cargo build --release
#   ./build.sh test            # any cargo subcommand, including fmt and clippy
set -euo pipefail

cd "$(dirname "$0")"
BASE_IMAGE=${RTERM_BUILD_IMAGE:-rust:1-bookworm}   # bookworm's older glibc keeps the binary portable
IMAGE=rterm-build
CARGO_CACHE=${RTERM_CARGO_CACHE:-$HOME/.cache/rterm-cargo}
mkdir -p "$CARGO_CACHE"

# The base image lacks rustfmt and clippy; this derived image is cached after the first run.
printf 'FROM %s\nRUN rustup component add rustfmt clippy\n' "$BASE_IMAGE" | docker build -q -t "$IMAGE" - >/dev/null

if [[ $# -eq 0 ]]; then
  set -- build --release
fi

exec docker run --rm \
  -u "$(id -u):$(id -g)" \
  -e CARGO_HOME=/cargo \
  -v "$CARGO_CACHE:/cargo" \
  -v "$PWD:/src" \
  -w /src \
  "$IMAGE" cargo "$@"
