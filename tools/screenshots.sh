#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Takes the screenshots of the KDE version for the README and AppStream: Kudari in a virtual
# KWin on a private D-Bus session, as in tests/gui/run.sh, with a copy of its own repository
# to show, so that nothing of the user's desktop, files or name ends up in the images.
#
#   cargo build --features test-hooks
#   tools/screenshots.sh [OUTDIR] [path/to/kudari]
#
# Writes kde-terminal.png and kde-settings.png, by default to docs/screenshots.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
out=$(mkdir -p "${1:-$root/docs/screenshots}" && cd "${1:-$root/docs/screenshots}" && pwd)
binary=${2:-${CARGO_TARGET_DIR:-$root/target}/debug/kudari}
if [ ! -x "$binary" ]; then
    echo "No Kudari at $binary; build it with: cargo build --features test-hooks" >&2
    exit 2
fi
if [ -z "${KUDARI_SCREENSHOTS:-}" ]; then
    env -u WAYLAND_DISPLAY -u DISPLAY XDG_CONFIG_HOME=/nonexistent XDG_DATA_HOME=/nonexistent \
        KUDARI_SCREENSHOTS=1 dbus-run-session -- "$0" "$out" "$binary" < /dev/null
    exit $?
fi

work=$(mktemp -d)
kwin=
kudari=
finish() {
    kill "$kudari" 2>/dev/null
    sleep 0.5
    kill -9 "$kudari" "$kwin" 2>/dev/null
    rm -rf "$work"
}
trap finish EXIT
trap 'exit 2' INT TERM
mkdir -p "$work/config" "$work/data" "$work/home"
mkdir -m 700 "$work/run"

# What the terminals show: Kudari's own repository, as a fresh clone under a neutral home.
git clone -q --branch main --single-branch "file://$root" "$work/home/kudari" || exit 2
git -C "$work/home/kudari" config alias.lg "log --color=always --format='%C(yellow)%h%C(reset) %<(80,trunc)%s'"
# A dark global theme of its own, as in the GUI test.
theme="$work/data/plasma/look-and-feel/kudari.screenshots.dark"
mkdir -p "$theme/contents"
printf '{"KPlugin": {"Id": "kudari.screenshots.dark", "Name": "Dark"}}\n' > "$theme/metadata.json"
printf '[kdeglobals][General]\nColorScheme=BreezeDark\n' > "$theme/contents/defaults"
cp /usr/share/color-schemes/BreezeDark.colors "$work/config/kdeglobals" || exit 2
printf '[General]\nColorScheme=BreezeDark\n[KDE]\nLookAndFeelPackage=kudari.screenshots.dark\n' >> "$work/config/kdeglobals"
printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n[Window]\nWidth=100\nHeight=60\n[Security]\nAllowRemoteCommands=true\n' \
    > "$work/config/kudarirc"

export XDG_DATA_HOME="$work/data" XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
export XDG_RUNTIME_DIR="$work/run"
export XDG_DATA_DIRS="$work/data:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export HOME="$work/home" SHELL=/bin/bash LANGUAGE=en
export QT_QPA_PLATFORM=wayland WAYLAND_DISPLAY=kudari-screenshots QT_QUICK_BACKEND=software
# A prompt without user or host: only the directory, in the colors of a common setup.
export PS1='\[\e[1;34m\]\w\[\e[0m\] \[\e[1;32m\]❯\[\e[0m\] ' BAT_THEME=OneHalfDark
kbuildsycoca6 >/dev/null 2>&1

kwin_wayland --virtual --no-lockscreen --socket="$WAYLAND_DISPLAY" --width 1600 --height 1000 > "$work/kwin.log" 2>&1 &
kwin=$!
sleep 3
"$binary" > "$work/log" 2>&1 &
kudari=$!
tries=0
until qdbus6 2>/dev/null | grep -q 'io\.github\.marcjose\.kudari$'; do
    tries=$((tries + 1))
    if [ "$tries" -gt 100 ] || ! kill -0 "$kudari" 2>/dev/null; then
        echo "Kudari did not start" >&2
        tail -n 20 "$work/log" >&2
        exit 1
    fi
    sleep 0.1
done
sleep 3

service=io.github.marcjose.kudari
# Calls method $2 of object $1 with the remaining arguments.
call() { object=$1 method=$2; shift 2; qdbus6 "$service" "/kudari/$object" "$method" "$@"; }
window() { method=$1; shift; call window "io.github.marcjose.kudari.$method" "$@"; }
sessions() { method=$1; shift; call sessions "io.github.marcjose.kudari.$method" "$@"; }
tabs() { method=$1; shift; call tabs "io.github.marcjose.kudari.$method" "$@"; }
hook() { method=$1; shift; call test "io.github.marcjose.kudari.Test.$method" "$@"; }
type_in() { sessions runCommandInTerminal "$1" "$2" > /dev/null; sleep 1; }
# Saves the visible window as OUT, whichever it is: Kudari shows one at a time here.
take() {
    rm -f "$work"/grab-*.png
    hook Grab "$work/grab"
    sleep 1
    for image in "$work"/grab-*.png; do
        mv "$image" "$1"
        return
    done
    echo "No window to take for $1" >&2
    exit 1
}

window toggleWindowState
sleep 2
first=$(sessions activeSessionId)
tabs setTabTitle "$first" "kudari"
type_in 0 "cd ~/kudari && clear"
type_in 0 "bat --style=numbers --line-range 64:91 crates/core/src/command.rs"
right=$(sessions splitTerminalLeftRight 0)
sleep 1.5
type_in "$right" "cd ~/kudari && clear"
type_in "$right" "git lg"
type_in "$right" "tree -d -L 2 --noreport crates"
second=$(sessions addSession)
tabs setTabTitle "$second" "build"
third=$(sessions addSession)
tabs setTabTitle "$third" "notes"
sessions raiseSession "$first" > /dev/null
sleep 2
take "$out/kde-terminal.png"

hook Trigger options_configure
sleep 3
# The terminal window gives way to the settings.
take "$out/kde-settings.png"
ls -la "$out"
