#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Smoke test of the Windows front end on Linux, without a screen: it opens its window with
# a terminal, saves the window as an image and quits; a second start hands over to the
# first, and `mcp` reaches the running one. Nothing touches the configuration of the user.
#
#   cargo build -p kudari-windows
#   tests/windows/run.sh [path/to/kudari-windows]
set -u

binary=${1:-${CARGO_TARGET_DIR:-$(dirname "$0")/../../target}/debug/kudari-windows}
if [ ! -x "$binary" ]; then
    echo "No kudari-windows at $binary; build it with: cargo build -p kudari-windows" >&2
    exit 2
fi
# The test changes into a directory of its own.
binary=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")
work=$(mktemp -d)
# On Windows the shell is Git Bash, whose paths the program does not know.
native() {
    if command -v cygpath > /dev/null; then cygpath -w "$1"; else printf '%s' "$1"; fi
}
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/config" "$work/home"
mkdir -m 700 "$work/run"
printf '[Behavior]\nOpenAfterStart=true\n[Dialogs]\nFirstRun=false\n' > "$work/config/kudari-windows.ini"
cd "$work/home" || exit 2

# With `background` set, the shell becomes the program, so that `kill` reaches it.
start() {
    ${background:+exec} env -u WAYLAND_DISPLAY -u DISPLAY -u APPDATA XDG_CONFIG_HOME="$(native "$work/config")" HOME="$work/home" \
        XDG_RUNTIME_DIR="$work/run" \
        SHELL=/bin/sh QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_FORCE_STDERR_LOGGING=1 \
        MSYS2_ARG_CONV_EXCL='*' timeout "$@"
}
start 60 "$binary" --screenshot="$(native "$work/window.png")" > "$work/log" 2>&1
status=$?

# A second start hands over to the first and ends at once.
background=yes start 60 "$binary" > "$work/first.log" 2>&1 &
first=$!
sleep 3
start 20 "$binary" > "$work/second.log" 2>&1
second=$?
running=$(kill -0 "$first" 2>/dev/null && echo yes)

# An agent through `mcp`: the tools, and a refusal, as the settings allow agents nothing.
mcp() {
    printf '%s\n' \
        '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
        '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
        '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_terminals","arguments":{}}}' |
        start 20 "$binary" mcp
}
mcp > "$work/mcp.json" 2> "$work/mcp.log"
kill "$first" 2>/dev/null
wait "$first" 2>/dev/null
mcp > "$work/alone.json" 2> "$work/alone.log"

failures=0
check() { # name, condition result
    if [ "$2" = yes ]; then
        echo "ok    $1"
    else
        echo "FAIL  $1"
        failures=$((failures + 1))
    fi
}
check "the program ends by itself" "$([ "$status" = 0 ] && echo yes)"
check "the window is saved as an image" "$([ -s "$work/window.png" ] && echo yes)"
pattern='(io/github/marcjose/kudari.*(TypeError|ReferenceError|is not a type|Cannot assign|is not defined)|Unable to create)'
check "the QML reports no errors" "$(grep -qE "$pattern" "$work/log" || echo yes)"
check "a second start ends at once" "$([ "$second" = 0 ] && echo yes)"
check "the first one keeps running" "$running"
check "an agent finds the tools" "$(grep -q '"list_terminals"' "$work/mcp.json" && echo yes)"
check "an agent hears that agents are switched off" \
    "$(grep -q 'Reading terminals is switched off' "$work/mcp.json" && echo yes)"
check "an agent hears when Kudari is not running" \
    "$(grep -q 'Kudari is not running' "$work/alone.json" && echo yes)"
grep -E "$pattern" "$work/log" | head -5 | sed 's/^/Error in QML: /'
if [ "$failures" != 0 ]; then
    echo "Exit status $status; the end of the log:"
    tail -n 30 "$work/log"
    echo "What mcp answered:"
    cat "$work/mcp.json" "$work/mcp.log" "$work/alone.json" "$work/alone.log"
fi
echo "$failures failed"
[ "$failures" = 0 ]
