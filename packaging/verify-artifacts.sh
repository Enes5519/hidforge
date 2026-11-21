#!/bin/sh
# Verify release artifacts before they are published (CI) or tested locally.
#   packaging/verify-artifacts.sh <version> [dir]
# Checks the tar.gz always; the .deb and AppImage when present. Exits non-zero on any problem.
set -eu
version=${1:?usage: verify-artifacts.sh <version> [dir]}
dir=$(cd "${2:-.}" && pwd)
fail=0
bad() { echo "FAIL: $*" >&2; fail=1; }
ok() { echo "ok:   $*"; }

[ -f "$dir/SHA256SUMS" ] && (cd "$dir" && sha256sum -c SHA256SUMS >/dev/null) && ok "SHA256SUMS matches" || bad "SHA256SUMS missing or wrong"

tgz="$dir/hidforge-$version-x86_64.tar.gz"
[ -f "$tgz" ] || { bad "missing $tgz"; exit 1; }
root=hidforge-$version-x86_64
expected="$root/ $root/LICENSE $root/README.md $root/hidforge $root/hidforge-helper $root/hidforge.desktop $root/icons/ $root/icons/hidforge.svg"
for s in 16 24 32 48 64 128 256 512; do expected="$expected $root/icons/hidforge-$s.png"; done
listing=$(tar tzf "$tgz" | sort | tr '\n' ' ')
want=$(for e in $expected; do echo "$e"; done | sort | tr '\n' ' ')
[ "$listing" = "$want" ] && ok "tar.gz contains exactly the expected files" || bad "tar.gz contents differ: $listing"
tar tzf "$tgz" | grep -qE '(^/|\.\.)' && bad "tar.gz has absolute or .. paths"

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
tar xzf "$tgz" -C "$work"
t=$work/$root
for f in hidforge hidforge-helper; do
  [ -x "$t/$f" ] && readelf -h "$t/$f" | grep -q 'Advanced Micro Devices X86-64' && ok "$f is an executable x86-64 ELF" || bad "$f is not an executable x86-64 ELF"
  [ ! -u "$t/$f" ] && [ ! -g "$t/$f" ] || bad "$f has a setuid/setgid bit"
done
got=$("$t/hidforge-helper" --version 2>&1 || true)
[ "$got" = "hidforge-helper $version" ] && ok "helper reports $version" || bad "helper reports '$got', expected $version"
# The root helper: C runtime only, no RPATH/RUNPATH, no GUI toolkit.
readelf -d "$t/hidforge-helper" | grep -qE 'RPATH|RUNPATH' && bad "helper has RPATH/RUNPATH"
extra=$(readelf -d "$t/hidforge-helper" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | grep -vE '^(libc|libgcc_s|libm|libdl|libpthread|ld-linux[^ ]*)\.so' || true)
[ -z "$extra" ] && ok "helper links only the C runtime" || bad "helper links: $extra"
nm -C "$t/hidforge-helper" 2>/dev/null | grep -qiE 'gpui|tokio|zbus' && bad "helper contains GUI/async toolkit symbols" || ok "helper has no GUI toolkit symbols"
# glibc ceiling: the newest GLIBC_x.y symbol either binary needs must not exceed GLIBC_MAX (CI sets
# 2.35, the oldest supported runner). Unset = report only.
for f in hidforge hidforge-helper; do
  need=$(readelf -W --dyn-syms "$t/$f" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1)
  if [ -n "${GLIBC_MAX:-}" ]; then
    [ "$(printf '%s\n%s\n' "${need:-0}" "$GLIBC_MAX" | sort -V | tail -1)" = "$GLIBC_MAX" ] && ok "$f needs glibc <= $GLIBC_MAX (needs ${need:-none})" || bad "$f needs glibc $need, newer than $GLIBC_MAX"
  else echo "info: $f needs glibc ${need:-none} (GLIBC_MAX not set)"; fi
done
# The GUI starts headless CLI modes without a display (dynamic libraries resolve).
env -i LC_ALL=C "$t/hidforge" --check >"$work/check.out" 2>&1; rc=$?
{ [ $rc -eq 0 ] || [ $rc -eq 1 ]; } && grep -q . "$work/check.out" && ok "hidforge --check runs (exit $rc)" || bad "hidforge --check failed (exit $rc): $(head -2 "$work/check.out")"
[ -f "$t/hidforge.desktop" ] && grep -q '^Exec=hidforge$' "$t/hidforge.desktop" && ok "desktop file present" || bad "desktop file wrong"

deb="$dir/hidforge_${version}_amd64.deb"
if [ -f "$deb" ]; then
  if command -v dpkg-deb >/dev/null; then
    c=$(dpkg-deb -c "$deb")
    for path in ./usr/bin/hidforge ./usr/libexec/hidforge-helper ./usr/share/polkit-1/actions/org.hidforge.helper.policy ./usr/share/hidforge/package-managed ./usr/share/applications/hidforge.desktop; do
      echo "$c" | grep -q " $path\$" || bad "deb lacks $path"
    done
    # cargo-deb writes a pre-release as `~` and appends a revision: 0.1.0-rc.1 -> 0.1.0~rc.1-1.
    dv=$(dpkg-deb -f "$deb" Version); want=$(echo "$version" | sed 's/-/~/')
    echo "$dv" | grep -qE "^$(echo "$want" | sed 's/[.]/\\./g')(-[0-9]+)?\$" && ok "deb has version $dv" || bad "deb version '$dv' does not match $version"
    ok ".deb contents checked"
  else echo "skip: dpkg-deb not installed, .deb not inspected"; fi
fi
img="$dir/hidforge-$version-x86_64.AppImage"
if [ -f "$img" ]; then
  [ -x "$img" ] || chmod +x "$img"
  (cd "$work" && "$img" --appimage-extract >/dev/null 2>&1) && [ -x "$work/squashfs-root/usr/bin/hidforge" ] && [ -x "$work/squashfs-root/usr/bin/hidforge-helper" ] \
    && ok "AppImage holds hidforge and hidforge-helper" || bad "AppImage lacks hidforge or hidforge-helper"
  # The helper runs as root from a user-writable staging dir: no RPATH/RUNPATH, same as the other artifacts.
  readelf -d "$work/squashfs-root/usr/bin/hidforge-helper" 2>/dev/null | grep -qE '\((RPATH|RUNPATH)\)' \
    && bad "AppImage hidforge-helper has RPATH/RUNPATH" || ok "AppImage hidforge-helper has no RPATH/RUNPATH"
fi
[ $fail -eq 0 ] && echo "all artifact checks passed" || { echo "artifact verification FAILED" >&2; exit 1; }
