#!/bin/sh
# Real polkitd + pkexec inside a throwaway ROOTLESS podman container (no network at run time).
# Building the image once needs network access (pacman installs polkit). Same isolation rules as
# run-in-container.sh: no --privileged, only single read-only file mounts, nothing from the host
# /usr. Note: no `no-new-privileges` here, because pkexec is a setuid binary; the setuid only
# applies inside the container's user namespace (root there is your unprivileged uid on the host).
set -eu
cd "$(dirname "$0")/../.."
command -v podman >/dev/null || { echo "need podman (rootless)" >&2; exit 1; }
[ "$(podman info --format '{{.Host.Security.Rootless}}')" = true ] || { echo "podman is not rootless" >&2; exit 1; }
[ "$(id -u)" != 0 ] || { echo "do not run as root" >&2; exit 1; }
cargo build --bin hidforge-helper
test_bin=$(cargo test --test root_install --no-run --message-format=json 2>/dev/null |
  sed -n 's/.*"executable":"\([^"]*root_install-[^"]*\)".*/\1/p' | tail -1)
[ -x "$test_bin" ] || { echo "could not locate the root_install test binary" >&2; exit 1; }
podman build -q -t hidforge-polkit-test -f tests/root/Containerfile.polkit tests/root >/dev/null
exec podman run --rm --network=none --pids-limit 256 --memory 1g \
  -v "$PWD/tests/root/polkit-e2e.sh:/work/polkit-e2e.sh:ro" \
  -v "$PWD/target/debug/hidforge-helper:/work/hidforge-helper:ro" \
  -v "$test_bin:/work/root_install:ro" \
  localhost/hidforge-polkit-test sh /work/polkit-e2e.sh
