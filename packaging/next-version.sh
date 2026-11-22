#!/usr/bin/env bash
# Prints the version after <current> for a bump kind.
#   patch|minor|major  bump that number, drop any -rc.N      (0.1.0-rc.1 minor -> 0.2.0)
#   rc                 next release candidate                (0.1.0-rc.1 -> 0.1.0-rc.2, 0.1.0 -> 0.1.1-rc.1)
#   release            final release of the current rc      (0.1.0-rc.1 -> 0.1.0)
#   auto               pick patch|minor|major from the Conventional Commit messages on stdin:
#                      breaking (`type!:` or `BREAKING CHANGE:`) -> major, `feat:` -> minor, else patch.
#                      Before 1.0 (Cargo semver) one level lower: breaking -> minor, feat -> patch.
# With a third argument "true", patch|minor|major start a release candidate (-rc.1).
set -euo pipefail

cur=$1 kind=$2 pre=${3:-false}
[[ $cur =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-rc\.([0-9]+))?$ ]] || { echo "bad version '$cur'" >&2; exit 1; }
ma=${BASH_REMATCH[1]} mi=${BASH_REMATCH[2]} pa=${BASH_REMATCH[3]} rc=${BASH_REMATCH[5]}

if [ "$kind" = auto ]; then
  log=$(cat)
  [ -n "$log" ] || { echo "no commits since the last release" >&2; exit 1; }
  if grep -qE '^([a-z]+(\([^)]*\))?!:|BREAKING[ -]CHANGE:)' <<<"$log"; then kind=major
  elif grep -qE '^feat(\([^)]*\))?:' <<<"$log"; then kind=minor
  else kind=patch; fi
  if [ "$ma" = 0 ]; then case $kind in major) kind=minor ;; minor) kind=patch ;; esac; fi
fi

case $kind in
  major) next="$((ma + 1)).0.0" ;;
  minor) next="$ma.$((mi + 1)).0" ;;
  patch) next="$ma.$mi.$((pa + 1))" ;;
  rc) if [ -n "$rc" ]; then echo "$ma.$mi.$pa-rc.$((rc + 1))"; else echo "$ma.$mi.$((pa + 1))-rc.1"; fi; exit ;;
  release) [ -n "$rc" ] || { echo "$cur is not a release candidate" >&2; exit 1; }; echo "$ma.$mi.$pa"; exit ;;
  *) echo "unknown bump '$kind'" >&2; exit 1 ;;
esac
if [ "$pre" = true ]; then echo "$next-rc.1"; else echo "$next"; fi
