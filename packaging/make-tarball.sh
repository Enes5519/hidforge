#!/bin/sh
# Build hidforge-<version>-x86_64.tar.gz from target/release (used by CI and for local checks).
#   packaging/make-tarball.sh <version> [output-dir]
set -eu
cd "$(dirname "$0")/.."
version=${1:?usage: make-tarball.sh <version> [output-dir]}
out=${2:-.}
d=hidforge-$version-x86_64
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$d/icons"
cp target/release/hidforge target/release/hidforge-helper assets/hidforge.desktop README.md LICENSE "$stage/$d/"
cp assets/brand/icons/hidforge.svg assets/brand/icons/hidforge-*.png "$stage/$d/icons/"
# Reproducible-ish: fixed owner, sorted names; mtime from the last commit when available.
mtime=$(git log -1 --format=%ct 2>/dev/null || date +%s)
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$mtime" -C "$stage" -czf "$out/$d.tar.gz" "$d"
echo "$out/$d.tar.gz"
