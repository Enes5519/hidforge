#!/bin/sh
# Runs tests/root/scenarios.sh as (container) root inside throwaway ROOTLESS podman containers.
# NEVER run the scenarios directly on your machine: they write to /usr, /etc and /var.
#
#   cargo build --bin hidforge-helper
#   tests/root/run-in-container.sh
#
# Isolation: rootless podman only (container root maps to your unprivileged uid, so it holds no
# power over the host); no --privileged, no extra capabilities, no network; the only host paths
# mounted are two single files, read-only. The container's /usr is the image's own layer.
# HIDFORGE_TEST_IMAGE: image whose glibc is at least as new as the one the helper was built with.
set -eu
cd "$(dirname "$0")/../.."
command -v podman >/dev/null || { echo "need podman (rootless); docker is not accepted" >&2; exit 1; }
[ "$(podman info --format '{{.Host.Security.Rootless}}')" = true ] ||
  { echo "podman is not rootless; refusing" >&2; exit 1; }
[ "$(id -u)" != 0 ] || { echo "do not run as root; refusing" >&2; exit 1; }
# Always build from the current sources, so a stale binary can never be the one under test; the
# checksum is handed to the container, which refuses to run if the mounted file differs.
cargo build --locked --bin hidforge-helper
helper=target/debug/hidforge-helper
sum=$(sha256sum "$helper" | cut -d' ' -f1)
echo "testing $helper $(sha256sum "$helper" | cut -c1-16) ($("$helper" --version))"
# Tripwire: nothing the helper is built from may be newer than the binary.
newer=$(find src assets locales build.rs Cargo.toml Cargo.lock -newer "$helper" -type f | head -3)
[ -z "$newer" ] || { echo "binary is older than: $newer" >&2; exit 1; }

run() { # mode, image, extra podman flags...
  mode=$1; image=$2; shift 2
  podman run --rm --network=none --security-opt no-new-privileges \
    --pids-limit 256 --memory 1g --cpus 2 \
    -e HIDFORGE_ISOLATED_TEST=1 -e HIDFORGE_HELPER_SHA256="$sum" "$@" \
    -v "$PWD/$helper:/work/hidforge-helper:ro" \
    -v "$PWD/tests/root/scenarios.sh:/work/scenarios.sh:ro" \
    "$image" sh /work/scenarios.sh "$mode"
}
# One image per package manager (pacman, dpkg, rpm); override the first with HIDFORGE_TEST_IMAGE.
# The helper must be built with a glibc no newer than the image's.
for image in "${HIDFORGE_TEST_IMAGE:-docker.io/library/archlinux:latest}" \
  docker.io/library/debian:stable-slim registry.fedoraproject.org/fedora-minimal:latest; do
  echo "=== $image"
  run all "$image"
done
# Immutable distribution: root filesystem read-only (needs no capability or mount).
run readonly "${HIDFORGE_TEST_IMAGE:-docker.io/library/archlinux:latest}" --read-only
