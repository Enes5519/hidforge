#!/bin/sh
# Root integration scenarios for `hidforge-helper --install-self` / `--uninstall-self`.
# They change real system paths, so they refuse to run outside a container.
set -u

if [ "${HIDFORGE_ISOLATED_TEST:-}" != 1 ] || { [ ! -e /run/.containerenv ] && [ ! -e /.dockerenv ]; }; then
  echo "refusing: run through tests/root/run-in-container.sh (container only)" >&2
  exit 2
fi
[ "$(id -u)" = 0 ] || { echo "must be root inside the container" >&2; exit 2; }

HELPER=/usr/libexec/hidforge-helper
POLICY=/usr/share/polkit-1/actions/org.hidforge.helper.policy
MANIFEST=/var/lib/hidforge/self-install
MARKER=/usr/share/hidforge/package-managed
MODE=${1:-all}
pass=0 fail=0

[ -z "${HIDFORGE_HELPER_SHA256:-}" ] || [ "$(sha256sum </work/hidforge-helper | cut -d' ' -f1)" = "$HIDFORGE_HELPER_SHA256" ] ||
  { echo "refusing: mounted helper is not the binary the launcher built" >&2; exit 2; }
# A user-owned-style copy outside /usr, like an extracted tarball.
mkdir -p /tmp/dl && cp /work/hidforge-helper /tmp/dl/hidforge-helper && chmod 755 /tmp/dl/hidforge-helper
SRC=/tmp/dl/hidforge-helper

reset() {
  chmod 755 /usr/libexec /usr/share/polkit-1/actions 2>/dev/null
  rm -rf "$HELPER" "$POLICY" /var/lib/hidforge /usr/share/hidforge /tmp/victim /usr/libexec/other /usr/libexec/.hidforge-helper.*
  mkdir -p /usr/libexec /usr/share/polkit-1/actions
}
# Same format as selfinstall::identity: dev:ino:size:ctime_sec.ctime_nsec (no zero padding).
identity() {
  n=$(stat -c %.9Z "$1" | cut -d. -f2 | sed 's/^0*//')
  echo "$(stat -c %d:%i:%s "$1"):$(stat -c %Z "$1").${n:-0}"
}
check() { # name, condition exit status
  if [ "$2" = 0 ]; then pass=$((pass + 1)); echo "ok   - $1"; else fail=$((fail + 1)); echo "FAIL - $1"; fi
}
same() { [ "$(sha256sum <"$1")" = "$(sha256sum <"$2")" ]; } # no cmp/diff in minimal images
ok() { "$@" >/tmp/out 2>&1; }
refuses() { ! "$@" >/tmp/out 2>&1; }

if [ "$MODE" = readonly ]; then
  # The container root is read-only (podman --read-only): like Silverblue or SteamOS.
  refuses "$SRC" --install-self; check "read-only root: install fails" $?
  grep -qi 'read-only' /tmp/out; check "read-only root: actionable message" $?
  [ ! -e /var/lib/hidforge/self-install ]; check "read-only root: no record left behind" $?
  cat /tmp/out
  echo "passed: $pass, failed: $fail"
  [ "$fail" = 0 ]
  exit
fi

reset
ok "$SRC" --install-self "$(identity $SRC)"
check "fresh install with matching identity" $?
[ "$(stat -c %a $HELPER)" = 755 ] && [ "$(stat -c %a $POLICY)" = 644 ] && [ "$(stat -c %U $HELPER)" = root ]
check "modes 755/644, owned by root" $?
grep -q '^version=' $MANIFEST; check "record written" $?
same "$SRC" "$HELPER"; check "installed helper is the executed binary" $?
refuses $HELPER --install-self; check "installed helper cannot reinstall itself" $?
ok $HELPER --uninstall-self; check "uninstall via installed helper" $?
[ ! -e $HELPER ] && [ ! -e $POLICY ] && [ ! -e $MANIFEST ]; check "uninstall removed all of ours" $?

reset
id=$(identity $SRC); touch "$SRC"
refuses "$SRC" --install-self "$id"; check "changed file (identity mismatch) installs nothing" $?
[ ! -e $HELPER ] && [ ! -e $POLICY ]; check "nothing written after identity mismatch" $?

reset
echo foreign >$HELPER
refuses "$SRC" --install-self; check "foreign helper without record: refused" $?
[ "$(cat $HELPER)" = foreign ]; check "foreign helper untouched" $?

reset
mkdir -p /usr/share/hidforge && : >$MARKER
refuses "$SRC" --install-self; check "package marker blocks install" $?
mkdir -p /var/lib/hidforge && echo version=0.0.1 >$MANIFEST && echo pkg >$HELPER
refuses $HELPER --uninstall-self; check "package marker blocks uninstall" $?
[ "$(cat $HELPER)" = pkg ]; check "package file survives" $?

reset
"$SRC" --install-self >/dev/null 2>&1
echo victim >/tmp/victim; rm -f $HELPER; ln -s /tmp/victim $HELPER
ok "$SRC" --install-self; check "update replaces a symlink at the target" $?
[ "$(cat /tmp/victim)" = victim ] && [ ! -L $HELPER ]; check "symlink target never written through" $?

reset
"$SRC" --install-self >/dev/null 2>&1
rm -f $HELPER; echo other >/usr/libexec/other; ln /usr/libexec/other $HELPER
ok "$SRC" --install-self; check "update replaces a hard link at the target" $?
[ "$(cat /usr/libexec/other)" = other ]; check "other hard-link name untouched" $?

reset
mkdir -p /var/lib/hidforge && echo version=999.0.0 >$MANIFEST
refuses "$SRC" --install-self; check "downgrade refused" $?

reset
"$SRC" --install-self >/dev/null 2>&1; echo keep >/usr/libexec/other
$HELPER --uninstall-self >/dev/null 2>&1
[ "$(cat /usr/libexec/other)" = keep ]; check "uninstall leaves unrelated files" $?

reset
mkdir -p /var/lib/hidforge && echo version=0.0.0 >/var/lib/hidforge/self-install && echo junk >$HELPER
echo junk >/usr/libexec/.hidforge-helper.1.tmp
ok "$SRC" --install-self; check "recovers from interrupted state (record + junk helper)" $?
same "$SRC" "$HELPER" && [ ! -e /usr/libexec/.hidforge-helper.1.tmp ]; check "recovery left a clean install" $?

reset
chmod 777 /usr/libexec
refuses "$SRC" --install-self; check "group/world-writable directory refused" $?
chmod 755 /usr/libexec

reset
# A real package (pacman) owns the helper and policy, and ships no package-managed marker. Even with
# an old record of ours lying around, nothing may be overwritten or deleted.
if command -v pacman >/dev/null && command -v bsdtar >/dev/null; then
  rm -rf /tmp/pkg && mkdir -p /tmp/pkg/usr/libexec /tmp/pkg/usr/share/polkit-1/actions
  echo pkg-helper >/tmp/pkg/usr/libexec/hidforge-helper; chmod 755 /tmp/pkg/usr/libexec/hidforge-helper
  echo pkg-policy >/tmp/pkg/usr/share/polkit-1/actions/org.hidforge.helper.policy
  printf 'pkgname = hidforge-fake\npkgver = 1-1\npkgdesc = fake\nbuilddate = 0\npackager = t\nsize = 20\narch = any\n' >/tmp/pkg/.PKGINFO
  (cd /tmp/pkg && bsdtar --zstd -cf /tmp/fake.pkg.tar.zst .PKGINFO usr)
  printf '[options]\nSigLevel = Never\nArchitecture = auto\n' >/tmp/pacman.conf
  if pacman --config /tmp/pacman.conf -U --noconfirm /tmp/fake.pkg.tar.zst >/tmp/pacman.log 2>&1; then
    mkdir -p /var/lib/hidforge && echo version=0.0.1 >$MANIFEST
    refuses "$SRC" --install-self; check "package-owned (pacman) files: install refused" $?
    refuses $HELPER --uninstall-self; check "package-owned (pacman) files: uninstall refused" $?
    [ "$(cat $HELPER)" = pkg-helper ] && [ "$(cat $POLICY)" = pkg-policy ]; check "package-owned files untouched" $?
    pacman --config /tmp/pacman.conf -R --noconfirm hidforge-fake >/dev/null 2>&1
  else
    echo "skip - could not install the fake pacman package: $(tail -1 /tmp/pacman.log)"
  fi
fi

reset
# Same with a real .deb (dpkg -S) when the image has dpkg.
if command -v dpkg-deb >/dev/null && command -v dpkg >/dev/null; then
  rm -rf /tmp/deb && mkdir -p /tmp/deb/DEBIAN /tmp/deb/usr/libexec /tmp/deb/usr/share/polkit-1/actions
  echo pkg-helper >/tmp/deb/usr/libexec/hidforge-helper; chmod 755 /tmp/deb/usr/libexec/hidforge-helper
  echo pkg-policy >/tmp/deb/usr/share/polkit-1/actions/org.hidforge.helper.policy
  printf 'Package: hidforge-fake\nVersion: 1\nArchitecture: all\nMaintainer: t <t@t>\nDescription: fake\n' >/tmp/deb/DEBIAN/control
  if dpkg-deb --build /tmp/deb /tmp/fake.deb >/dev/null 2>&1 && dpkg -i /tmp/fake.deb >/tmp/dpkg.log 2>&1; then
    mkdir -p /var/lib/hidforge && echo version=0.0.1 >$MANIFEST
    refuses "$SRC" --install-self; check "package-owned (dpkg) files: install refused" $?
    refuses $HELPER --uninstall-self; check "package-owned (dpkg) files: uninstall refused" $?
    [ "$(cat $HELPER)" = pkg-helper ] && [ "$(cat $POLICY)" = pkg-policy ]; check "package-owned (dpkg) files untouched" $?
    dpkg -r hidforge-fake >/dev/null 2>&1
  else
    echo "skip - could not install the fake deb: $(tail -1 /tmp/dpkg.log)"
  fi
fi

reset
# Eight installers at once (separate processes): the lock serializes them; the losers say so.
i=0; while [ $i -lt 8 ]; do ( "$SRC" --install-self >/tmp/par.$i 2>&1; echo $? >/tmp/par.$i.rc ) & i=$((i+1)); done
wait
ok_count=$(cat /tmp/par.*.rc | grep -c '^0$')
[ "$ok_count" -ge 1 ]; check "parallel installs: at least one succeeds ($ok_count of 8)" $?
others=$(for i in 0 1 2 3 4 5 6 7; do [ "$(cat /tmp/par.$i.rc)" = 0 ] || cat /tmp/par.$i; done | grep -vc 'already running')
[ "$others" = 0 ]; check "parallel installs: the only refusal reason is the lock" $?
same "$SRC" "$HELPER" && grep -q '^version=' $MANIFEST && [ -z "$(ls -A /usr/libexec | grep '^\.hidforge-helper\.')" ]
check "parallel installs: consistent result, no staging files left" $?
rm -f /tmp/par.*

reset
echo "passed: $pass, failed: $fail"
[ "$fail" = 0 ]
