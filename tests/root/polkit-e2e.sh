#!/bin/sh
# Runs INSIDE a throwaway rootless container only (real polkitd + pkexec text agent).
# Started by tests/root/run-polkit-e2e.sh; refuses to run anywhere else.
[ -e /run/.containerenv ] || { echo "not in a container"; exit 2; }
set -u
echo root:rootpw | chpasswd; echo tester:testpw | chpasswd; usermod -aG wheel tester
getent passwd polkitd >/dev/null || useradd -r -s /usr/bin/nologin polkitd
mkdir -p /run/dbus; dbus-uuidgen --ensure; dbus-daemon --system --fork; sleep 1
/usr/lib/polkit-1/polkitd --no-debug >/tmp/polkitd.log 2>&1 &
sleep 2
mkdir -p /home/tester/dl && cp /work/hidforge-helper /home/tester/dl/ && chown -R tester /home/tester/dl && chmod 755 /home/tester/dl/hidforge-helper
H=/home/tester/dl/hidforge-helper
identity() {
  n=$(stat -c %.9Z "$1" | cut -d. -f2 | sed 's/^0*//')
  echo "$(stat -c %d:%i:%s "$1"):$(stat -c %Z "$1").${n:-0}"
}
# pw_run "<passwords separated by ;>" "<command>" : run command as tester on a pty, typing passwords
pw_run() {
  ( sleep 2; for p in $(echo "$1" | tr ';' ' '); do printf '%s\n' "$p"; sleep 2; done; sleep 2 ) |
    script -qec "su tester --session-command '$2; echo exit=\$?'" /dev/null 2>&1 | tr -d '\r' | sed 's/\x1b\[[0-9;]*m//g'
}
echo "== A. install through real pkexec (generic action), matching identity"
ID=$(identity $H)
pw_run testpw "pkexec $H --install-self $ID"
ls -l /usr/libexec/hidforge-helper /usr/share/polkit-1/actions/org.hidforge.helper.policy /var/lib/hidforge/self-install 2>&1
cat /var/lib/hidforge/self-install
sleep 2
echo "== B. policy recognised by polkitd"
pkaction --action-id org.hidforge.helper 2>&1 | head -3
pkaction --action-id org.hidforge.helper --verbose 2>&1 | grep -E "implicit (any|inactive|active)|exec.path|description|message"
echo "== C. dedicated action used for the installed helper; no credential caching (two prompts)"
pw_run "testpw;testpw" "pkexec /usr/libexec/hidforge-helper --version; pkexec /usr/libexec/hidforge-helper --version"
echo "== D. installed helper refuses to reinstall itself"
pw_run testpw "pkexec /usr/libexec/hidforge-helper --install-self"
echo "== E. stale identity (file modified after the GUI looked at it)"
rm -f /usr/libexec/hidforge-helper /usr/share/polkit-1/actions/org.hidforge.helper.policy /var/lib/hidforge/self-install; rmdir /var/lib/hidforge
ID=$(identity $H); touch $H
pw_run testpw "pkexec $H --install-self $ID"
ls /usr/libexec/hidforge-helper 2>&1
echo "== F. cancel at the prompt installs nothing"
ID=$(identity $H)
( sleep 2; printf '\004'; sleep 2 ) | script -qec "su tester --session-command 'pkexec $H --install-self $ID; echo exit=\$?'" /dev/null 2>&1 | tr -d '\r' | sed 's/\x1b\[[0-9;]*m//g' | tail -5
ls /usr/libexec/hidforge-helper 2>&1
echo "== G. install again, then uninstall through the dedicated action"
pw_run testpw "pkexec $H --install-self $ID" | tail -3
sleep 2
pw_run testpw "pkexec /usr/libexec/hidforge-helper --uninstall-self"
ls /usr/libexec/hidforge-helper /usr/share/polkit-1/actions/org.hidforge.helper.policy /var/lib/hidforge 2>&1

echo "== H. GUI code path (selfinstall::install/uninstall) through real pkexec, helper beside the program"
mkdir -p /home/tester/app && cp /work/hidforge-helper /work/root_install /home/tester/app/ && mv /home/tester/app/root_install /home/tester/app/hidforge
chown -R tester /home/tester/app
export HIDFORGE_ISOLATED_TEST=1
T=/home/tester/app/hidforge
pw_run "testpw;testpw;testpw" "HIDFORGE_ISOLATED_TEST=1 $T --ignored gui_flow_helper_beside_the_program --test-threads=1 --nocapture" | grep -E "AUTHENTICAT|test |panicked|exit=|ok$|FAILED"
echo "== I. same, AppImage-style (APPIMAGE set, helper staged under XDG_RUNTIME_DIR)"
mkdir -p /run/user/1000 && chown tester /run/user/1000 && chmod 700 /run/user/1000
pw_run "testpw;testpw;testpw" "HIDFORGE_ISOLATED_TEST=1 APPIMAGE=1 XDG_RUNTIME_DIR=/run/user/1000 $T --ignored gui_flow_appimage_staging --test-threads=1 --nocapture" | grep -E "AUTHENTICAT|test |panicked|exit=|ok$|FAILED"
ls /run/user/1000
echo "== J. GUI without any agent or terminal (what a bare window manager session looks like)"
su tester -c "HIDFORGE_ISOLATED_TEST=1 $T --ignored gui_flow_without_agent_gives_the_sudo_hint --test-threads=1 --nocapture </dev/null" 2>&1 | grep -E "ERR:|test |panicked|ok$|FAILED"
echo "== K. no agent + AppImage staging: the hinted file must still exist"
su tester -c "HIDFORGE_ISOLATED_TEST=1 APPIMAGE=1 XDG_RUNTIME_DIR=/run/user/1000 $T --ignored gui_flow_without_agent_gives_the_sudo_hint --test-threads=1 --nocapture </dev/null" 2>&1 | grep -E "ERR:|test |ok$|FAILED"
ls -la /run/user/1000/hidforge-setup-*/ 2>&1 | head -4
