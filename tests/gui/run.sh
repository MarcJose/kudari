#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# End-to-end test: runs Kudari in a virtual KWin on a private D-Bus session and drives it
# with real input. Nothing touches the desktop or the configuration of the user.
#
#   cargo build --features test-hooks
#   tests/gui/run.sh [path/to/kudari]
#
# Needs kwin_wayland, qdbus6, wayland-scanner, a C compiler and dbus-run-session. With
# KUDARI_GUI_KEEP set to an absolute directory, the images of the pixel checks and the logs stay there
# after the test, as for a check that fails only in the CI.
set -u

here=$(cd "$(dirname "$0")" && pwd)
binary=${1:-${CARGO_TARGET_DIR:-$here/../../target}/debug/kudari}
if [ ! -x "$binary" ]; then
    echo "No Kudari at $binary; build it with: cargo build --features test-hooks" >&2
    exit 2
fi
if [ -z "${KUDARI_GUI_TEST:-}" ]; then
    # Services started on the private bus inherit stdout and could keep a pipe open for
    # long after the test, so the report goes through a file.
    report=$(mktemp)
    # The bus starts services with its own environment and from the user's service files.
    # Without these, it can neither start an installed Kudari nor give one the desktop.
    # A run takes a few minutes, even with coverage; one that hangs ends with what it left.
    timeout --kill-after=30s "${KUDARI_GUI_TIMEOUT:-15m}" \
        env -u WAYLAND_DISPLAY -u DISPLAY XDG_CONFIG_HOME=/nonexistent XDG_DATA_HOME=/nonexistent \
        KUDARI_GUI_TEST=1 dbus-run-session -- "$0" "$binary" > "$report" 2>&1 < /dev/null &
    test=$!
    # Ctrl+C reaches this shell, tail and grep, but not timeout, which runs in a process
    # group of its own. timeout passes TERM on to its group, the private bus, KWin and
    # Kudari, and the trap of the test below cleans up.
    trap 'kill -TERM "$test" 2>/dev/null; wait "$test"; rm -f "$report"; exit 130' INT TERM
    # Each check as it comes, so that a run that hangs shows where.
    tail -n +1 -f --pid="$test" "$report" | grep --line-buffered -E '^(ok|FAIL|[0-9]+ checks|Error in)'
    wait "$test"
    status=$?
    if [ "$status" -ge 124 ]; then
        echo "The GUI test did not finish in time; its last output:"
        tail -n 30 "$report"
    fi
    rm -f "$report"
    exit "$status"
fi

work=$(mktemp -d)
kwin=
kudari=
# Waits up to five seconds for a process to end, as Kudari after quitting: an end of its own
# writes what is left, such as the profile of a run for the coverage.
wait_quit() { # pid
    tries=0
    while kill -0 "$1" 2>/dev/null && [ "$tries" -lt 50 ]; do sleep 0.1; tries=$((tries + 1)); done
}
finish() {
    [ -n "$kudari" ] && wait_quit "$kudari"
    kill "$kudari" 2>/dev/null
    sleep 0.5
    kill -9 "$kudari" "$kwin" 2>/dev/null
    if [ -n "${KUDARI_GUI_KEEP:-}" ]; then
        mkdir -p "$KUDARI_GUI_KEEP"
        (cd "$work" && find . \( -name '*.png' -o -name '*.log' \) -exec cp --parents {} "$KUDARI_GUI_KEEP/" \;)
    fi
    rm -rf "$work"
}
trap finish EXIT
trap 'exit 2' INT TERM
mkdir -p "$work/config" "$work/data/applications" "$work/data/kudari/keytabs" "$work/home" "$work/source/folder"
mkdir -m 700 "$work/run"
cd "$work/home" || exit 2

wayland-scanner client-header "$here/fake-input.xml" "$work/fake-input-client.h" || exit 2
wayland-scanner private-code "$here/fake-input.xml" "$work/fake-input-code.c" || exit 2
cc -O1 -I"$work" "$here/fake_input.c" "$work/fake-input-code.c" -lwayland-client -lm -o "$work/input" || exit 2
# Reads an image without libraries: Qt writes PNG with 8 bits a channel and no interlacing.
cat > "$work/pngread.py" <<'EOF'
import struct
import zlib


def read(path):
    """The width, the height, the channels and the rows of bytes of a PNG."""
    data = open(path, "rb").read()
    chunks, at = {}, 8
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at:at + 8])
        chunks.setdefault(kind, []).append(data[at + 8:at + 8 + length])
        at += 12 + length
    width, height, depth, kind = struct.unpack(">IIBB", chunks[b"IHDR"][0][:10])
    channels = {2: 3, 6: 4}[kind]
    raw = zlib.decompress(b"".join(chunks[b"IDAT"]))
    stride = width * channels
    rows, previous = [], bytearray(stride)
    for y in range(height):
        line = raw[y * (stride + 1):(y + 1) * (stride + 1)]
        kind_of_filter, row = line[0], bytearray(line[1:])
        for x in range(stride):
            left = row[x - channels] if x >= channels else 0
            up = previous[x]
            corner = previous[x - channels] if x >= channels else 0
            if kind_of_filter == 1:
                row[x] = (row[x] + left) & 255
            elif kind_of_filter == 2:
                row[x] = (row[x] + up) & 255
            elif kind_of_filter == 3:
                row[x] = (row[x] + (left + up) // 2) & 255
            elif kind_of_filter == 4:
                guess = left + up - corner
                # On a tie, the first of left, up and corner, as PNG says.
                apart_left, apart_up, apart_corner = abs(guess - left), abs(guess - up), abs(guess - corner)
                if apart_left <= apart_up and apart_left <= apart_corner:
                    near = left
                elif apart_up <= apart_corner:
                    near = up
                else:
                    near = corner
                row[x] = (row[x] + near) & 255
        rows.append(row)
        previous = row
    return width, height, channels, rows
EOF
# Compares two images of the same size: how many pixels differ, and by how much at most in a
# channel; or why it cannot.
cat > "$work/pixels.py" <<'EOF'
import sys
from pngread import read
first, second = read(sys.argv[1]), read(sys.argv[2])
if first[:3] != second[:3]:
    print(f"sizes {first[:2]} and {second[:2]}")
    sys.exit()
width, height, channels, rows = first
others = second[3]
differ, most = 0, 0
for row, other in zip(rows, others):
    for x in range(0, width * channels, channels):
        gap = max(abs(a - b) for a, b in zip(row[x:x + 3], other[x:x + 3]))
        if gap:
            differ += 1
            most = max(most, gap)
print(f"{differ} {most}")
EOF
# The output the pixel checks paint: colors of all kinds, the styles of text and of lines,
# box drawing and shades, wide characters, an emoji and combining marks. The underscores reach
# across their cells, where the line at a column crosses them.
cat > "$work/catalogue" <<'EOF'
printf '__________ WWWWWWWWWW\n'
printf '\033[31mred\033[42mgreen\033[0m \033[38;5;208m208\033[48;5;19mon19\033[0m \033[38;2;200;120;40mtrue\033[0m\n'
printf '\033[1mbold\033[0m \033[3mitalic\033[0m \033[2mdim\033[0m \033[7minverse\033[0m \033[9mstrike\033[0m\n'
printf '\033[4msingle\033[0m \033[4:2mdouble\033[0m \033[4:3mcurly\033[0m \033[4:4mdotted\033[0m \033[4:5mdashed\033[0m\n'
printf '─│┼╭╮╯╰ ═║╬ ░▒▓█ ▀▄ 日本語 🙂 e\314\201 ñ \364\217\277\275\n'
EOF

# Every cell of the screen but the last line with a background, a line under and one through
# its character, all in colors that differ from those of the cells beside it.
cat > "$work/full" <<'EOF'
columns=$(tput cols) lines=$(tput lines)
python3 -c '
import sys
columns, lines = int(sys.argv[1]), int(sys.argv[2])
for y in range(lines - 1):
    cells = (f"\033[48;5;{(x + y) % 256};38;5;{(x * 7 + y * 3) % 256};4;9m{chr(33 + (x + y) % 90)}" for x in range(columns))
    print("".join(cells) + "\033[0m")
' "$columns" "$lines"
EOF

cat > "$work/data/applications/kudari-gui-test.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kudari GUI test
Exec=$work/input
NoDisplay=true
X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input
DESKTOP
# A program for links of a scheme of its own, which notes each link it gets.
cat > "$work/data/applications/kudari-gui-scheme.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kudari GUI scheme
Exec=sh -c 'echo "\$1" >> $work/scheme-links' sh %u
MimeType=x-scheme-handler/kudaritest;
NoDisplay=true
DESKTOP
printf '[Default Applications]\nx-scheme-handler/kudaritest=kudari-gui-scheme.desktop\n' > "$work/config/mimeapps.list"
printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n' > "$work/config/kudarirc"
printf 'key F5 : "KEYTAB"\n' > "$work/data/kudari/keytabs/test.keytab"
printf 'hello\n' > "$work/home/notes.txt"
printf '#!/bin/sh\necho "$@" >> %s\n' "$work/edited" > "$work/editor"
chmod +x "$work/editor"
for name in a b c; do echo "$name" > "$work/source/$name.txt"; done
# A global theme of its own, so that the test does not need those of Plasma: dark colors.
theme="$work/data/plasma/look-and-feel/kudari.test.dark"
mkdir -p "$theme/contents"
printf '{"KPlugin": {"Id": "kudari.test.dark", "Name": "Kudari Test Dark"}}\n' > "$theme/metadata.json"
printf '[kdeglobals][General]\nColorScheme=BreezeDark\n' > "$theme/contents/defaults"

export XDG_DATA_HOME="$work/data" XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
export XDG_RUNTIME_DIR="$work/run"
export XDG_DATA_DIRS="$work/data:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export HOME="$work/home" SHELL=/bin/sh
export QT_QPA_PLATFORM=wayland WAYLAND_DISPLAY=kudari-gui-test QT_FORCE_STDERR_LOGGING=1
# Qt Quick draws in software, as the CI has no graphics device. That also keeps text, and so
# where the checks click, the same everywhere.
export QT_QUICK_BACKEND=software
export QT_LOGGING_RULES='*.warning=true;qml*=true'
# The shell counts its prompts in a file, so that a check can wait until a command is done;
# the prompt itself looks as before.
prompts="$work/prompts"
: > "$prompts"
cat > "$work/shrc" <<EOF
PS1='\$(printf . >> "$prompts")\\s-\\v\\\$ '
EOF
export ENV="$work/shrc"
kbuildsycoca6 >/dev/null 2>&1

# KWin needs a render device; where there is no GPU, such as in the CI, the kernel module
# vgem provides one, on which Mesa draws in software.
kwin_wayland --virtual --no-lockscreen --socket="$WAYLAND_DISPLAY" --width 1280 --height 800 > "$work/kwin.log" 2>&1 &
kwin=$!
tries=0
until [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] || [ "$tries" -gt 100 ]; do
    tries=$((tries + 1))
    sleep 0.1
done
# Waits for Kudari on the bus. A call to a name nobody owns would start a Kudari from
# the service files instead.
started() {
    tries=0
    until qdbus6 2>/dev/null | grep -q 'io\.github\.marcjose\.kudari$'; do
        tries=$((tries + 1))
        if [ "$tries" -gt 100 ] || ! kill -0 "$kudari" 2>/dev/null; then
            echo "Error in startup: Kudari did not appear on the bus"
            tail -n 30 "$work/kwin.log" "$work/log" "$work/first.log" 2>/dev/null | sed 's/^/Error in startup: /'
            exit 1
        fi
        sleep 0.1
    done
    tries=0
    until [ "$(hook Started 2>/dev/null)" = true ] || [ "$tries" -gt 100 ]; do
        tries=$((tries + 1))
        sleep 0.1
    done
    settle 300 10000
}
"$binary" > "$work/log" 2>&1 &
kudari=$!
started

failures=0
checks=0
check() { # name, expected, actual
    checks=$((checks + 1))
    if [ "$2" = "$3" ]; then
        echo "ok    $1"
    else
        echo "FAIL  $1: expected [$2], got [$3]"
        failures=$((failures + 1))
    fi
}
service=io.github.marcjose.kudari
# Calls method $2 of object $1 with the remaining arguments.
call() { call_object=$1 call_method=$2; shift 2; qdbus6 "$service" "/kudari/$call_object" "io.github.marcjose.kudari.$call_method" "$@"; }
window() { call window "$@"; }
sessions() { call sessions "$@"; }
hook() { call_method=$1; shift; call test "Test.$call_method" "$@"; }
input() { "$work/input" "$@"; }
# With KUDARI_GUI_GRABS set to a directory, saves images of the window there, to find where
# the checks have to click.
grab() { if [ -n "${KUDARI_GUI_GRABS:-}" ]; then hook Grab "$KUDARI_GUI_GRABS/$1"; sleep 0.5; fi; }
# Waits until the user interface has settled, for at least QUIET milliseconds (150 by default).
settle() { hook Settle "${1:-150}" "${2:-5000}" > /dev/null; }
# Notes for whoever works on the test, such as a command whose prompt did not come.
note() { echo "$*" >> "${KUDARI_GUI_NOTES:-/dev/null}"; }
# Waits for a prompt after the first COUNT, up to five seconds.
wait_prompt() { # count, what for
    tries=0
    until [ "$(wc -c < "$prompts")" -gt "$1" ]; do
        tries=$((tries + 1))
        if [ "$tries" -gt 500 ]; then
            note "no prompt after: $2"
            return
        fi
        sleep 0.01
    done
}
# Runs a command in the active terminal and waits until it is done and shown.
run() { run_before=$(wc -c < "$prompts"); sessions runCommand "$1" 2>/dev/null; wait_prompt "$run_before" "$1"; settle; }
# Runs a command in a given terminal and waits until it is done and shown.
run_in() { run_before=$(wc -c < "$prompts"); sessions runCommandInTerminal "$1" "$2"; wait_prompt "$run_before" "$2"; settle; }
# Notes the prompts so far; wait_shells then waits for the first prompt of COUNT new shells.
mark() { mark_before=$(wc -c < "$prompts"); }
wait_shells() { wait_prompt $((mark_before + ${1:-1} - 1)) "${1:-1} new shells"; settle; }
# Runs a command that keeps running, such as one that reads input.
run_open() { sessions runCommand "$1" 2>/dev/null; sleep 0.6; }
state() { hook State | cut -d' ' -f1; }
# Waits until the window is $1 pixels high: it takes its new size in its own time, which
# software rendering makes longer.
wait_height() {
    tries=0
    while [ "$(hook State | sed -n 's/.*height=\([0-9]*\).*/\1/p')" != "$1" ] && [ "$tries" -lt 50 ]; do
        sleep 0.1
        tries=$((tries + 1))
    done
    # KWin takes clicks on the new area only once it shows the new size.
    sleep 1
}
# Waits until a file has content, which is quicker and safer than a long sleep.
content() {
    tries=0
    while [ ! -s "$1" ] && [ "$tries" -lt 40 ]; do sleep 0.1; tries=$((tries + 1)); done
    tr '\n' '|' < "$1" 2>/dev/null
}
# Moves the selection to entry number $1 of an open menu and activates it.
menu_entry() {
    tries=0
    while [ "$tries" -lt "$1" ]; do input key 108; settle; tries=$((tries + 1)); done
    input key 28
    sleep 1.2
}
end_input() { input key 28; sleep 0.2; input key 29 32; settle; }

window toggleWindowState; sleep 1.5
check "the window opens" visible=1 "$(state)"

# With one screen, the window opens on it without asking KWin where the pointer is, which
# takes a script in KWin and a wait for its answer.
for _ in 1 2; do window toggleWindowState; sleep 0.5; done
check "with one screen, the window opens without a script in KWin" no \
    "$([ -e "$XDG_RUNTIME_DIR/kudari-pointer-output.js" ] && echo yes || echo no)"
input click 600 150; sleep 0.5

run_open "echo blocked > $work/blocked"
check "commands over D-Bus are off by default" "" "$(cat "$work/blocked" 2>/dev/null)"
hook SetSetting AllowRemoteCommands true; settle
run "echo allowed > $work/allowed"
check "commands over D-Bus work after opting in" "allowed|" "$(content "$work/allowed")"

run "printf 'zwei worte\n'; printf 'hallo  \n'"
hook SaveOutput 0 "$work/saved"; sleep 0.4
check "saved output is private" 600 "$(stat -c %a "$work/saved")"

hook SetSetting KeyBindings test; settle
run_open "cat > $work/keys"
input key 63; settle; end_input
check "a keytab file changes keys" "KEYTAB|" "$(content "$work/keys")"
hook SetSetting KeyBindings ""; settle

run_open "sleep 300"
hook MenuValue signal 15; sleep 0.5
run "echo back > $work/signal"
check "a signal reaches the foreground program" "back|" "$(content "$work/signal")"

mark; sessions splitTerminalLeftRight 0 >/dev/null; wait_shells
check "splitting adds a terminal" 0,1 "$(sessions terminalIdsForSessionId 0)"
sessions removeTerminal 1; settle
check "closing a terminal removes it" 0 "$(sessions terminalIdsForSessionId 0)"

hook SetSetting EditorCommand "$work/editor PATH:LINE"; hook SetSetting OpenLinksByClick true; settle
run "clear; printf 'see notes.txt:12:3 end\n'"
input click 130 8
check "a plain click opens a file link in the editor" "$work/home/notes.txt:12|" "$(content "$work/edited")"
# A look at a slow or hanging drive holds up neither the window nor the terminal: the click
# opens nothing, and a command goes through at once.
rm -f "$work/edited"
hook SlowFileLooks 3000
input click 130 8
started=$(date +%s%N)
run "echo still-here"
taken=$((($(date +%s%N) - started) / 1000000))
sleep 3
hook SlowFileLooks 0; settle
check "a slow look at a file holds up neither the window nor the terminal" "yes nothing opened" \
    "$([ "$taken" -lt 1500 ] && echo yes || echo "no: $taken ms") $([ -e "$work/edited" ] && echo opened || echo nothing opened)"
# The link numbers find the file on a thread of their own, and a number opens it.
rm -f "$work/edited"
hook SetSetting UrlHintsModifier ctrl+alt; settle
run "clear; printf 'see notes.txt:12:3 end\n'"
input key 29 56 2; settle
check "a link number opens the file it shows" "$work/home/notes.txt:12|" "$(content "$work/edited")"
# With Shift for the numbers, the key of 1 types "!"; its place still opens the first link.
rm -f "$work/edited"
hook SetSetting UrlHintsModifier ctrl+shift; settle
input key 29 42 2; settle
check "a link number opens with Shift held as well" "$work/home/notes.txt:12|" "$(content "$work/edited")"
hook SetSetting UrlHintsModifier none; settle
hook SetSetting OpenLinksByClick false; hook SetSetting EditorCommand ""; settle
# KIO follows a desktop entry of type Link to its address, past the schemes allowed, also one
# named .Desktop, which got past Kudari before. Kudari opens none, also asked to open the
# file directly: the program for the scheme gets the link that goes to it, and not the one
# of the entry.
printf '[Desktop Entry]\nType=Link\nURL=kudaritest://entry\n' > "$work/home/entry.Desktop"
hook OpenUrl "kudaritest://direct"; hook OpenUrl "$work/home/entry.Desktop"; sleep 3
check "a desktop entry under any name is not opened" "kudaritest://direct|" "$(content "$work/scheme-links")"

run_open "clear; cat > $work/pasted"
hook Popup "$work/source/a.txt"; sleep 1; menu_entry 1; end_input
check "dropping pastes the quoted path" "'$work/source/a.txt' |" "$(content "$work/pasted")"
hook Popup "$work/source/a.txt"; sleep 1; menu_entry 2
check "dropping copies a file" "a|" "$(content "$work/home/a.txt")"
check "copying keeps the source" "a|" "$(content "$work/source/a.txt")"
# The keyboard of the terminal is blocked: the menu of a drop types nothing either.
run_open "clear; cat > $work/blocked"
blocked=$(sessions activeSessionId)
sessions setSessionKeyboardInputEnabled "$blocked" false; settle
hook Popup "$work/source/a.txt"; sleep 1; menu_entry 1
sessions setSessionKeyboardInputEnabled "$blocked" true; settle
end_input
check "a blocked terminal takes no path from the menu of a drop" "|" "$(content "$work/blocked")"
hook Popup "$work/source/b.txt"; sleep 1; menu_entry 3
check "dropping moves a file" "b|" "$(content "$work/home/b.txt")"
check "moving removes the source" no "$([ -e "$work/source/b.txt" ] && echo yes || echo no)"
hook Popup "$work/source/c.txt"; sleep 1; menu_entry 4
check "dropping links a file" "$work/source/c.txt" "$(readlink "$work/home/c.txt")"

run "clear; seq 1 900"
hook Trigger adjust-history; sleep 1.2
# The dialog tells the memory the lines take, as they are typed.
asked=$(hook Property historyLines memory | tr '\n' ' ' | sed 's/<[^>]*>//g')
input key 29 30; settle; input key 6; input key 11; settle
typed=$(hook Property historyLines memory | tr '\n' ' ' | sed 's/<[^>]*>//g')
input key 28; settle
hook SaveOutput 0 "$work/short"; sleep 0.4
check "the scrollback of a terminal can shrink" yes "$([ "$(wc -l < "$work/short")" -lt 100 ] && echo yes || echo no)"
told=no
case "$asked/$typed" in
    *"About "[1-9]*" of memory."*/*"About "[1-9]*" of memory."*) [ "$asked" != "$typed" ] && told=yes ;;
esac
check "the dialog for the lines of a terminal tells their memory as they are typed" yes \
    "$([ $told = yes ] && echo yes || echo "no: $asked / $typed")"
run "clear; echo saved here"
hook Trigger file_save_as; sleep 1.5; input key 28; sleep 1
check "saving asks for a file and writes the output there" yes \
    "$(grep -q 'saved here' "$work/home/output.txt" 2>/dev/null && echo yes || echo no)"
hook Trigger options_configure_keybinding; sleep 2
check "the shortcut dialog leaves Kudari responsive" visible=1 "$(state)"
input key 1; settle

run "clear"
hook SetSetting SilenceSeconds 1; sleep 0.2
hook Trigger toggle-session-monitor-once; settle
sessions setSessionMonitorSilenceEnabled 0 true; sleep 0.3
check "silence counts from switching it on" true "$(sessions isSessionMonitorSilenceEnabled 0)"
sleep 2.2
check "a one-shot monitor ends after its alert" false "$(sessions isSessionMonitorSilenceEnabled 0)"

background() { hook TerminalBackground; }
light=$(background)
check "the terminal takes the colors of the theme" yes "$([ -n "$light" ] && echo yes || echo no)"
hook SetSetting Skin theme:kudari.test.dark; settle
check "a fixed theme changes the terminal colors" yes "$([ "$(background)" != "$light" ] && echo yes || echo no)"
hook SetSetting Skin system; settle
check "the terminal returns to the system theme" "$light" "$(background)"
cp /usr/share/color-schemes/BreezeDark.colors "$XDG_CONFIG_HOME/kdeglobals"
kwriteconfig6 --notify --file kdeglobals --group General --key KudariTest 1; sleep 1.5
check "the terminal follows a theme change while running" yes "$([ "$(background)" != "$light" ] && echo yes || echo no)"

# Programs ask for the background, as neovim does to choose light or dark colors, and for
# the size in pixels, as image viewers do. The device attributes come last: programs take
# their answer as the end of the answers.
cat > "$work/ask" <<'EOF'
import os, re, select, sys, termios, tty
saved = termios.tcgetattr(0)
tty.setraw(0)
os.write(1, b"\033]11;?\033\\\033[14t\033[c")
reply = b""
while not re.search(rb"\033\[\?[0-9;]*c", reply) and select.select([0], [], [], 2)[0]:
    reply += os.read(0, 256)
termios.tcsetattr(0, termios.TCSAFLUSH, saved)
color = re.search(rb"\033\]11;rgb:(..)../(..)../(..)..\033\\", reply)
size = re.search(rb"\033\[4;(\d+);(\d+)t", reply)
columns, lines = os.get_terminal_size()
fits = bool(size) and int(size[1]) > 0 and int(size[1]) % lines == 0 and int(size[2]) % columns == 0
order = bool(color and size) and color.start() < size.start() < reply.index(b"\033[?")
with open(sys.argv[1], "w") as found:
    found.write("#" + b"".join(color.groups()).decode() if color else "none")
    found.write(f" {fits} {order}\n")
EOF
run_open "python3 $work/ask $work/asked"; sleep 1
check "programs get the background, the size in pixels and the attributes in order" \
    "$(background) True True" "$(cat "$work/asked" 2>/dev/null)"
# A program that switched flow control off keeps it off while the settings change: nano and
# emacs save with Ctrl+S.
run_open "sh -c 'stty -ixon; sleep 2; stty -a; stty ixon' > $work/ixon"
hook SetSetting SilenceSeconds 2; sleep 2.5
hook SetSetting SilenceSeconds 1; sleep 0.2
check "a change of the settings leaves flow control to the program" -ixon \
    "$(tr -s ' ;' '\n' < "$work/ixon" | grep -x -- '-\{0,1\}ixon')"

# A setting as the file has it. Kudari writes the file a moment after the last of a row of
# changes; the hook writes it now.
setting() { hook FlushSettings > /dev/null; sed -n "s/^$1=//p" "$work/config/kudarirc"; }

# The bar at the bottom: two tabs titled "~ : sh", the close button of the second at 221.
sessions addSession >/dev/null; sleep 1
grab tabs
input click 221 377; sleep 1
check "the close button of a tab closes its session" 0 "$(sessions sessionIdList)"
published() { hook Stats | python3 -c 'import json, sys; print(json.load(sys.stdin)["settings_published"])'; }
hook ResetStats
input drag 640 398 640 600; wait_height 600
dragged=$(published)
check "the grip below the bar changes the height" 75 "$(setting Height)"
# While it is dragged, the grip only shows the height: the settings take it once, as it is
# let go, rather than at each move, where each write saved the file.
check "the grip saves the height once, as it is let go" 1 "$dragged"
# A change of the settings reaches the file a moment after the last of a row of changes, as
# the steps of a slider come, rather than with a write of its own each.
hook SetSetting AutoSaveSeconds 7
early=$(sed -n 's/^AutoSaveSeconds=//p' "$work/config/kudarirc")
sleep 1
late=$(sed -n 's/^AutoSaveSeconds=//p' "$work/config/kudarirc")
hook SetSetting AutoSaveSeconds 10; settle
check "a change of the settings reaches the file a moment later" "[] [7]" "[$early] [$late]"
input drag 640 598 640 400; wait_height 400
check "the grip works both ways" 50 "$(setting Height)"
hook Trigger options_configure; sleep 2
grab settings
input click 700 354; sleep 0.3
input key 29 30; settle
input key 8; settle; input key 11; settle
input click 700 275; sleep 0.5
check "a number stays when leaving its field" 70 "$(setting Height)"
input click 700 354; sleep 0.5
check "visiting a field does not change it" "" "$(setting Width)"
input key 29 30; settle
input key 14; settle
input click 700 275; sleep 0.5
check "an emptied field keeps the number" 70 "$(setting Height)"
input click 250 316; sleep 1.2
input click 700 354; sleep 0.3
input click 700 433; sleep 0.5
check "a field showing its special value stays at 0" "" "$(setting FontSize)"
input click 700 354; sleep 0.3
input key 29 30; settle
input key 2; settle; input key 3; settle
input click 700 433; sleep 0.5
check "a special value can be overwritten" 12 "$(setting FontSize)"
input key 1; settle
# A size above 72 points, from Konsole perhaps, shows as it is and stays.
hook SetSetting FontSize 96; settle
hook Trigger options_configure; sleep 2
input click 250 316; sleep 1.2
check "the settings show a font size above 72" 96 "$(hook Property fontSize value)"
# The page of the terminal tells what the lines of history take, for each terminal and for all
# open now, in the units of the language.
input click 250 352; sleep 1.2
history=$(printf '%s|%s|%s' "$(hook Property historyMemory text)" "$(hook Property historyMemoryAll text)" \
    "$(hook Property historyWarning visible)" | sed 's/<[^>]*>//g')
told=no
case $history in
    "About "[1-9]*" for each terminal as wide as the active one.|About "[1-9]*" for the terminals open now.|false") told=yes ;;
esac
check "the settings tell what the history takes" yes "$([ $told = yes ] && echo yes || echo "no: $history")"
input click 250 316; sleep 1.2
input click 700 354; sleep 0.3
input click 700 433; sleep 0.5
check "a font size above 72 stays" 96 "$(setting FontSize)"
input key 1; settle
hook SetSetting Height 50; hook SetSetting FontSize 0; settle

hook Trigger options_configure; sleep 2
for row in 208 244 280 316 352 388; do input click 250 "$row"; sleep 0.7; done
input key 1; settle

# The actions of the menus and shortcuts, each checked where its effect can be seen.
active() { sessions activeSessionId; }
tabs() { call tabs "$@"; }
count() { sessions terminalIdsForSessionId "$1" | tr ',' '\n' | grep -c .; }
first=$(active)
# The first size of a new terminal is no change of its size to show.
shown() { hook Property sizeHint visible | grep -q true && echo yes || echo no; }
mark; hook Trigger new-session; wait_shells
check "a new terminal shows no size" no "$(shown)"
hook SetSetting Width 70; settle
check "a new size shows the columns and lines" yes "$(shown)"
hook SetSetting Width 90; sleep 1.2
sessions removeSession "$(active)"; settle
# The search bar is as wide as the terminal, beside the scrollbar wherever that is.
for position in left hidden right; do
    hook SetSetting ScrollBarPosition "$position"; settle
    hook Trigger edit_find; settle
    check "the search bar lines up with the terminal, scrollbar $position" \
        "$(hook Property terminal x) $(hook Property terminal width)" \
        "$(hook Property searchBar x) $(hook Property searchBar width)"
    input key 1; settle
done
hook Trigger new-session-two-horizontal; sleep 1; left_right=$(active)
hook Trigger new-session-two-vertical; sleep 1; top_bottom=$(active)
hook Trigger new-session-quad; sleep 1.2; quad=$(active)
check "sessions open with two, two and four terminals" "2 2 4" "$(count "$left_right") $(count "$top_bottom") $(count "$quad")"
mark; hook Trigger split-auto; wait_shells
mark; hook Trigger split-left-right; wait_shells
mark; hook Trigger split-top-bottom; wait_shells
check "the split actions add terminals" 7 "$(count "$quad")"
before=$(sessions activeTerminalId)
hook Trigger next-terminal; settle
moved=$(sessions activeTerminalId)
hook Trigger previous-terminal; settle
check "the terminal actions move the focus there and back" "yes $before" \
    "$([ "$moved" != "$before" ] && echo yes || echo no) $(sessions activeTerminalId)"
for side in right left top bottom; do hook Trigger "grow-terminal-$side"; settle; done
hook Trigger close-active-terminal; settle
check "closing the active terminal" 6 "$(count "$quad")"
hook Trigger previous-session; settle
check "the previous session" "$top_bottom" "$(active)"
hook Trigger next-session; settle
check "the next session" "$quad" "$(active)"
hook Trigger switch-to-session-1; settle
check "switching to the first tab" "$first" "$(active)"
hook Trigger move-session-right; settle
check "moving a tab to the right" "$first" "$(tabs sessionAtTab 1)"
hook Trigger move-session-left; settle
check "moving it back" "$first" "$(tabs sessionAtTab 0)"
hook Trigger rename-session; settle
for code in 38 24 34 31 28; do input key "$code"; settle; done
sleep 0.4
check "renaming the tab of a session" logs "$(tabs tabTitle "$first")"
tabs setTabTitle "$first" ""
hook Toggle toggle-session-prevent-closing true; settle
check "locking a session" false "$(sessions isSessionClosable "$first")"
hook Toggle toggle-session-prevent-closing false; settle
hook Toggle toggle-session-keyboard-input true; settle
check "blocking the keyboard input of a session" false "$(sessions isSessionKeyboardInputEnabled "$first")"
hook Toggle toggle-session-keyboard-input false; settle
hook Toggle toggle-session-monitor-activity true; settle
hook Toggle toggle-session-monitor-silence true; settle
check "watching a session" "true true" \
    "$(sessions isSessionMonitorActivityEnabled "$first") $(sessions isSessionMonitorSilenceEnabled "$first")"
for kind in activity silence process once; do hook Toggle "toggle-session-monitor-$kind" false; done
sleep 0.3
sessions raiseSession "$quad"; settle
hook Trigger close-session; settle
check "closing the active session" "$first,$left_right,$top_bottom" "$(sessions sessionIdList)"
sessions raiseSession "$first"; settle

hook Trigger increase-window-width; settle
hook Trigger increase-window-height; settle
check "the window grows" "100 60" "$(setting Width) $(setting Height)"
hook Trigger decrease-window-width; settle
hook Trigger decrease-window-height; settle
check "the window shrinks" "90 50" "$(setting Width) $(setting Height)"
hook Toggle keep-open false; settle
check "the window can stop staying open" false "$(setting KeepOpen)"
hook Toggle keep-open true; settle
hook Trigger view-full-screen; settle
hook Trigger view-full-screen; settle
for action in enlarge-font enlarge-font shrink-font reset-font-size; do hook Trigger "$action"; settle; done
# Steps of the zoom in a row wrap the terminal at the first and after the last, not at each,
# which would hold the terminal and tell its program the size each time: five in a moment.
hook ResetStats
for _ in 1 2 3 4 5; do hook Trigger enlarge-font; done
sleep 0.5; settle
resizes=$(hook Stats | python3 -c 'import json, sys; print(json.load(sys.stdin).get("resizes", "none"))')
hook Trigger reset-font-size; settle
check "steps of the zoom in a row wrap the terminal at most twice" yes \
    "$([ "$resizes" -ge 1 ] 2>/dev/null && [ "$resizes" -le 2 ] && echo yes || echo "no: $resizes")"
hook Toggle allow-mouse-tracking false; hook Toggle allow-mouse-tracking true

run "clear; seq 1 900"
hook Trigger clear-scrollback; settle
hook SaveOutput "$(sessions activeTerminalId)" "$work/scrollback"; sleep 0.4
check "clearing the scrollback keeps only the screen" yes "$([ "$(wc -l < "$work/scrollback")" -lt 100 ] && echo yes || echo no)"
run "seq 1 30"
hook Trigger clear-history-and-reset; settle
hook SaveOutput "$(sessions activeTerminalId)" "$work/reset"; sleep 0.4
check "resetting the terminal clears it" 0 "$(grep -cxE '[0-9]+' "$work/reset")"
# Auto save writes the text of a terminal when it may have changed, and only then: each write
# puts a new file in the place of the old one, with an inode of its own.
hook SetSetting AutoSaveSeconds 1; settle
run "clear; echo vorher"
saving=$(sessions activeTerminalId)
hook AutoSave "$saving" "$work/autosave.txt"
inode() { # waits a while for the file, then tells its inode, or "none"
    tries=0
    while [ ! -s "$work/autosave.txt" ] && [ "$tries" -lt 80 ]; do sleep 0.1; tries=$((tries + 1)); done
    stat -c %i "$work/autosave.txt" 2>/dev/null || echo none
}
written=$(inode)
sleep 3
idle=$(inode)
run "echo nachher"; sleep 2.5
again=$(inode)
hook AutoSave "$saving" ""
hook SetSetting AutoSaveSeconds 10; settle
saves=no
if [ "$written" != none ] && [ "$idle" = "$written" ] && [ "$again" != "$idle" ] && grep -q nachher "$work/autosave.txt"; then
    saves=yes
fi
check "auto save writes the text when it changed, and only then" yes \
    "$([ "$saves" = yes ] && echo yes || echo "no: inodes $written $idle $again, terminal $saving, active $(sessions activeTerminalId)")"
run "echo gefunden"
hook Trigger edit_find; settle
for code in 34 18 33; do input key "$code"; settle; done
hook Trigger edit_find_next; settle
hook Trigger edit_find_prev; settle
# Matches in new output appear on the scrollbar a moment later, without searching again.
marks() { hook Property terminal searchLines | grep -o '[0-9.][0-9.]*' | wc -l; }
before=$(marks)
run "seq 1 50; echo gefunden"; sleep 0.6
check "matches in new output reach the scrollbar" yes "$([ "$(marks)" -gt "$before" ] && echo yes || echo "no: $before $(marks)")"
# Every highlighted match lies on the word, also after output that came between the refresh
# that found the matches and the paint: a frame and its matches come from the same text.
hook SlowRefresh 40
run "seq 1 30000 | sed 's/^/x gefunden /'"; settle
hook SlowRefresh 0
matched=$(hook MatchedText "$(sessions activeTerminalId)")
astray=$(printf '%s\n' "$matched" | grep -vx gef | head -n 3 | tr '\n' '|')
check "highlighted matches lie on the word after much output" "yes" "$([ -n "$matched" ] && [ -z "$astray" ] && echo yes || echo "no: ${astray:-none}")"
input key 1; settle
# Output that comes between a refresh and its frame, with nothing after it for a while: the
# frame shows it, and the scrollbar follows it as well, here a history after a clear screen.
run "clear"
hook SlowRefresh 60
mark
run_open "printf 'a\\n'; sleep 0.03; seq 1 200; sleep 3"
sleep 1
late=$(hook Property terminal scrollSize | grep -o '[0-9.][0-9.]*' | head -n 1)
hook SlowRefresh 0
wait_prompt "$mark_before" "the output after a refresh"; settle
check "output between a refresh and its frame reaches the scrollbar" yes \
    "$(awk -v size="$late" 'BEGIN { print (size != "" && size < 1) ? "yes" : "no: " size }')"
# While typing, the search finds the newest match at once: the view stays at the bottom with
# the one there, rather than going to the first of the history. One that lies further back
# than a search while typing reads is found after a pause, and the field does not show "not
# found" meanwhile.
# A property of the active terminal, or of another object of its pane: the hook gives those of
# all panes, in the same order as their terminals.
pane_value() { # name, property
    awk -v ids="$(hook Property terminal terminalId)" -v values="$(hook Property "$1" "$2")" \
        -v active="$(sessions activeTerminalId)" \
        'BEGIN { n = split(ids, id, ","); split(values, value, ","); for (i = 1; i <= n; i++) if (id[i] == active) print value[i] }'
}
position() { pane_value terminal scrollPosition; }
# In a tab of its own with a history long enough; the terminal before keeps 50 lines.
hook SetSetting HistoryLines 10000; settle
before_search=$(sessions activeSessionId)
mark; searching=$(sessions addSession); wait_shells
run "clear; echo tief; seq 1 3000; echo tief; seq 1 5"
hook Trigger edit_find; settle
for code in 20 23 18 33; do input key "$code"; done
sleep 0.5; settle
newest=$(position)
input key 1; settle
run "clear; echo weit; seq 1 3000"
hook Trigger edit_find; settle
for code in 17 18 23 20; do input key "$code"; done
during=$(pane_value searchBar failed)
sleep 0.5; settle
far="$during $(pane_value searchBar failed) $(position)"
input key 1; settle
sessions removeSession "$searching" > /dev/null; settle
sessions raiseSession "$before_search" > /dev/null
hook SetSetting HistoryLines 1000; settle
check "a search while typing finds the newest match" yes \
    "$(awk -v at="$newest" 'BEGIN { print (at != "" && at > 0.9) ? "yes" : "no: " at }')"
check "a match far back is found after a pause, and not missed meanwhile" "false false yes" \
    "$(echo "$far" | awk '{ print $1, $2, ($3 != "" && $3 < 0.5) ? "yes" : "no: " $3 }')"
# The marks on the scrollbar, in a tab of its own: a match on every line of a history longer
# than the rail, but at most one mark for each pixel row of the rail, down to its end, and one
# item draws them all, in yellow on the rail at the right edge of the window.
hook SetSetting HistoryLines 10000; settle
before_marks=$(sessions activeSessionId)
mark; marked=$(sessions addSession); wait_shells
run "seq 1 12000 | sed 's/^/x gefunden /'"
hook Trigger edit_find; settle
for code in 34 18 33; do input key "$code"; done
sleep 1; settle
spread=$(printf '%s\n%s\n' "$(hook Property terminal searchRows)" "$(hook Property terminal searchLines)" | python3 -c '
import re, sys
rows = max(int(row) for row in sys.stdin.readline().split(","))
marks = [float(mark) for mark in re.findall(r"[0-9.]+", sys.stdin.read())]
print("yes" if 0 < len(marks) <= rows and max(marks) > 0.95 else f"no: {len(marks)} marks for {rows} rows, last {max(marks, default=0)}")')
hook Grab "$work/marks"; sleep 1
drawn=$(cd "$work" && python3 -c '
import glob
from pngread import read
width, height, channels, rows = read(sorted(glob.glob("marks-*.png"))[0])
def yellow(row, x):
    red, green, blue = row[x * channels:x * channels + 3]
    return red > 200 and green > 200 and blue < 100
lit = sum(1 for row in rows if any(yellow(row, x) for x in range(width - 20, width)))
print("yes" if lit > 50 else f"no: {lit} rows")')
input key 1; settle
sessions removeSession "$marked" > /dev/null; settle
sessions raiseSession "$before_marks" > /dev/null
hook SetSetting HistoryLines 1000; settle
check "the scrollbar shows the matches once a pixel row, to the end of the history" yes "$spread"
check "the marks of the matches are drawn on the scrollbar" yes "$drawn"
# A panic breaks only its terminal, in the thread of its PTY and while it is drawn: its tab
# closes, Kudari goes on, and the terminal of the tab before still takes commands.
for place in pty paint; do
    before_panic=$(sessions sessionIdList)
    mark; sessions addSession > /dev/null; wait_shells
    if [ "$place" = pty ]; then
        # Only the output asks for it, not the echo of the command.
        sessions runCommand "printf 'KUDARI-TEST-''PANIC-PTY'" 2>/dev/null
    else
        hook PanicNextPaint
        sessions runCommand "echo drawn" 2>/dev/null
    fi
    sleep 1; settle
    after_panic=$(sessions sessionIdList)
    alive=$(kill -0 "$kudari" 2>/dev/null && echo yes || echo no)
    run "echo after-panic"
    check "a panic in the $place breaks only its terminal, and the others go on" "yes $before_panic" \
        "$alive $after_panic"
done
for action in edit_select_all edit_copy select-line select-mode select-mode; do hook Trigger "$action"; settle; done

# A blinking cursor that a program hid, as htop does, stops blinking: each blink would draw
# the same frame again, and a fading cursor 30 of them a second.
hook SetSetting BlinkingCursor true; sleep 0.3
run "clear"
# The terminals that do not blink, of all of them: one more than before only while the
# program hides the cursor. Terminals in tabs in the background learn of the setting when
# they show, so they count as well, before and after.
blinks() { hook Property terminal cursorBlinks | grep -o false | wc -l; }
shown_blinks=$(blinks)
run "printf '\\033[?25l'"
hidden_blinks=$(blinks)
run "printf '\\033[?25h'"
check "a blinking cursor stops blinking while a program hides it" "1 0" "$((hidden_blinks - shown_blinks)) $(($(blinks) - shown_blinks))"
hook SetSetting BlinkingCursor false; sleep 0.3

# A blinking cursor rests, steady and shown, after a while without input or output, and a key
# wakes it. Only the terminal with the focus blinks, so at most one rests.
hook SetSetting BlinkingCursor true; hook SetSetting CursorBlinkTimeout 1; sleep 0.3
run "clear"
sleep 1.5
resting() { hook Property terminal cursorResting | grep -o true | wc -l; }
rested=$(resting)
rested_hidden=$(hook Property terminal cursorHidden | grep -o true | wc -l)
input key 42; settle
check "a blinking cursor rests shown after a while and a key wakes it" "1 0 0" "$rested $rested_hidden $(resting)"
hook SetSetting CursorBlinkTimeout 15; hook SetSetting BlinkingCursor false; sleep 0.3

# The D-Bus interface, each method checked against its counterpart.
mark; two_h=$(sessions addSessionTwoHorizontal); wait_shells 2
mark; two_v=$(sessions addSessionTwoVertical); wait_shells 2
mark; four=$(sessions addSessionQuad); wait_shells 4
check "D-Bus opens sessions with two, two and four terminals" "2 2 4" "$(count "$two_h") $(count "$two_v") $(count "$four")"
mark; sessions splitSessionAuto "$two_h" >/dev/null; wait_shells
mark; sessions splitSessionLeftRight "$two_h" >/dev/null; wait_shells
mark; sessions splitSessionTopBottom "$two_h" >/dev/null; wait_shells
mark; terminal=$(sessions splitTerminalTopBottom "$(sessions terminalIdsForSessionId "$two_h" | cut -d, -f1)"); wait_shells
check "D-Bus splits sessions and terminals" 6 "$(count "$two_h")"
check "a new terminal belongs to its session" "$two_h yes" \
    "$(sessions sessionIdForTerminalId "$terminal") $(sessions terminalIdList | tr ',' '\n' | grep -qx "$terminal" && echo yes)"
grown=""
for side in Right Left Top Bottom; do grown="$grown$(sessions "tryGrowTerminal$side" "$terminal" 10) "; done
check "growing a terminal answers with pixels or -1" yes "$(echo "$grown" | grep -qE '^(-?[0-9]+ ){4}$' && echo yes)"

sessions setTerminalKeyboardInputEnabled "$terminal" false; settle
check "D-Bus blocks the keyboard of one terminal" "false true true" \
    "$(sessions isTerminalKeyboardInputEnabled "$terminal") $(sessions hasTerminalsWithKeyboardInputEnabled "$two_h") $(sessions hasTerminalsWithKeyboardInputDisabled "$two_h")"
sessions setSessionKeyboardInputEnabled "$two_h" true; settle
check "D-Bus gives a session its keyboard back" true "$(sessions isTerminalKeyboardInputEnabled "$terminal")"
for kind in Activity Silence; do
    sessions "setTerminalMonitor${kind}Enabled" "$terminal" true; sleep 0.3
    check "D-Bus watches one terminal for $kind" "true true true" \
        "$(sessions "isTerminalMonitor${kind}Enabled" "$terminal") $(sessions "hasTerminalsWithMonitor${kind}Enabled" "$two_h") $(sessions "hasTerminalsWithMonitor${kind}Disabled" "$two_h")"
    sessions "setSessionMonitor${kind}Enabled" "$two_h" false; sleep 0.3
    check "D-Bus stops watching a session for $kind" false "$(sessions "isTerminalMonitor${kind}Enabled" "$terminal")"
done

sessions setSessionClosable "$two_h" false; settle
check "a locked session is reported" true "$(sessions hasUnclosableSessions)"
sessions removeTerminal "$terminal"; settle
check "the terminals of a locked session stay" 6 "$(count "$two_h")"
sessions setSessionClosable "$two_h" true; settle
sessions removeTerminal "$terminal"; settle
check "D-Bus closes a terminal" 5 "$(count "$two_h")"
for session in "$two_h" "$two_v" "$four"; do sessions removeSession "$session"; settle; done
check "D-Bus closes sessions" no "$(sessions sessionIdList | tr ',' '\n' | grep -qxE "$two_h|$two_v|$four" && echo yes || echo no)"
# A tab whose shell is stopped closes whole: the shell ends, and nothing waits for it forever.
mark; stopped=$(sessions addSession); wait_shells
sessions raiseSession "$stopped"; settle
run "echo \$\$ > $work/shell-pid"
shell_pid=$(cat "$work/shell-pid")
kill -STOP "$shell_pid"; sleep 0.3
sessions removeSession "$stopped"; sleep 1.5
check "closing a tab with a stopped shell ends the shell" gone \
    "$(kill -0 "$shell_pid" 2>/dev/null && echo running || echo gone)"
kill -CONT "$shell_pid" 2>/dev/null
sessions raiseSession "$first"; settle

# Output in a terminal that nobody sees, in a tab in the background or with the window
# hidden, only marks it stale; it catches up when it shows.
first_terminal=$(sessions activeTerminalId)
# Counted against the terminals of other tabs, which stay stale until they show.
stale() { echo $(($(hook Property terminal stale | grep -o true | wc -l) - stale_before)); }
stale_before=0
stale_before=$(stale)
# The terminals without a size, against those of other tabs as well: Qt frees the image of
# a terminal in a tab in the background.
sizeless() { echo $(($(hook Property terminal width | tr -s ', ' '\n' | grep -cxE '0(\.0*)?') - sizeless_before)); }
sizeless_before=0
sizeless_before=$(sizeless)
mark; background=$(sessions addSession); wait_shells
sessions raiseSession "$background"; settle
run_in "$first_terminal" "seq 1 40"
stale_in_background=$(stale)
sizeless_in_background=$(sizeless)
sessions raiseSession "$first"; settle
stale_shown=$(stale)
# Now the new tab is the one in the background; closed, it leaves none.
sizeless_shown=$(sizeless)
sessions removeSession "$background"; settle
sizeless_closed=$(sizeless)
window toggleWindowState; sleep 1
run_in "$first_terminal" "seq 1 40"
stale_hidden=$(stale)
window toggleWindowState; sleep 1
check "output nobody sees waits until the terminal shows" "1 0 1 0" "$stale_in_background $stale_shown $stale_hidden $(stale)"

# Hidden, with no terminal watched or saved, the window looks at its terminals for titles only
# every 4 s, which only D-Bus sees then; shown, it looks at once rather than after its timer.
polls() { hook Stats | python3 -c 'import json, sys; print(json.load(sys.stdin)["polls"])'; }
window toggleWindowState; sleep 1
hook ResetStats; sleep 3
polls_hidden=$(polls)
hook ResetStats; window toggleWindowState; sleep 0.4
polls_shown=$(polls)
looked="hidden $polls_hidden, shown $polls_shown"
[ "$polls_hidden" -le 1 ] && [ "$polls_shown" -ge 1 ] && looked=yes
check "a hidden window looks at its terminals seldom, and at once when it shows" yes "$looked"

# The counters of the test build: output, its refresh and paint, the lock of the terminal, the
# queue of the Qt thread and the frame all count.
hook ResetStats
run "seq 1 20"
counted=$(hook Stats | python3 -c '
import json, sys
stats, terminal = json.load(sys.stdin), sys.argv[1]
needed = [stats["wakeups"], stats["reports"], stats["refreshes"], stats["paints"].get(terminal, 0)]
needed += [stats[name]["count"] for name in ("lock_wait", "lock_hold", "queue", "frame_build", "frame_draw")]
print("yes" if all(needed) else stats)' "$first_terminal")
check "the counters count the work of a terminal" yes "$counted"

# What is painted while nothing happens: nothing. A paint costs the render thread and the
# graphics card, and on a laptop the battery. Each check counts for two seconds, and a cursor
# that blinks shows that the counting sees the paints that are there.
paints() { # terminal id, or "all" for the terminals of all tabs
    hook Stats | python3 -c '
import json, sys
paints, terminal = json.load(sys.stdin)["paints"], sys.argv[1]
print(sum(paints.values()) if terminal == "all" else paints.get(terminal, 0))' "$1"
}
quiet() { hook ResetStats; sleep 2; paints "${1:-all}"; }
run "clear"
idle=$(quiet)
hook SetSetting BlinkingCursor true; sleep 0.3
blinking=$(quiet)
hook SetSetting CursorBlinkTimeout 1; sleep 1.5
rested=$(quiet)
hook SetSetting CursorBlinkTimeout 15; input key 42; sleep 0.3
window toggleWindowState; sleep 1
hidden=$(quiet)
window toggleWindowState; sleep 1
# Only the terminal with the focus blinks; the other one of a split stays as it is.
mark; split=$(sessions splitTerminalLeftRight "$first_terminal"); wait_shells
input key 42; settle
unfocused=$first_terminal
[ "$(sessions activeTerminalId)" = "$first_terminal" ] && unfocused=$split
hook ResetStats; sleep 2
beside=$(paints "$unfocused")
sessions removeTerminal "$split"; sleep 0.6
hook SetSetting BlinkingCursor false; sleep 0.3
check "nothing is painted while nothing happens" "0 yes 0 0 0" \
    "$idle $([ "$blinking" -gt 0 ] && echo yes || echo "no: $blinking") $rested $hidden $beside"
# Output alone gets at most the frames a second of the setting, here 15 over two seconds of
# output that goes on; as many as the screen shows, it gets more.
flood() { # frames a second
    hook SetSetting OutputFrameRate "$1"; settle
    run_open "clear; timeout 3 sh -c 'while :; do echo flood; done'"
    hook ResetStats; sleep 2
    paints "$(sessions activeTerminalId)"
    sleep 1; settle
}
capped=$(flood 15)
free=$(flood 0)
check "output alone gets the frames a second of the setting" yes \
    "$([ "$capped" -le 36 ] && [ "$free" -gt 50 ] && echo yes || echo "no: $capped with 15 a second, $free with no limit")"
# What is typed shows at once, also under such a limit: here 2 frames a second for output that
# comes ten times a second, and five keys 100 ms apart, each with a frame of its own.
hook SetSetting OutputFrameRate 2; settle
run_open "clear; timeout 3 sh -c 'while :; do echo tick; sleep 0.1; done'"
hook ResetStats
for _ in 1 2 3 4 5; do input key 30; sleep 0.1; done; sleep 0.1
typed_frames=$(paints "$(sessions activeTerminalId)")
sleep 2; settle
hook SetSetting OutputFrameRate 0; settle
run "clear"
check "what is typed gets its frame at once under a limit for output" yes \
    "$([ "$typed_frames" -ge 5 ] && echo yes || echo "no: $typed_frames frames for five keys")"
mark; background=$(sessions addSession); wait_shells
sessions raiseSession "$background"; settle
hook ResetStats
run_in "$first_terminal" "seq 1 40"
# Neither does it refresh: the terminal of the new tab has no output either.
behind="$(paints "$first_terminal") $(hook Stats | python3 -c 'import json, sys; print(json.load(sys.stdin)["refreshes"])')"
sessions removeSession "$background"; settle
sessions raiseSession "$first"; settle
check "output in a tab in the background neither refreshes nor paints" "0 0" "$behind"
# A change of a setting paints the terminals anew only where they show it: a slider in the
# settings changes them at every step.
# A paint that is asked for can come after the settling, where frames are slow to draw.
some() {
    tries=0
    while [ "$(paints all)" = 0 ] && [ "$tries" -lt 20 ]; do sleep 0.1; tries=$((tries + 1)); done
    count=$(paints all)
    # Where it fails, how far it came: refreshes, paints of lines, frames drawn, the window,
    # and whether the terminal waits to be seen.
    [ "$count" -gt 0 ] && echo yes || echo "no: $count $(hook Stats | python3 -c '
import json, sys
stats = json.load(sys.stdin)
print("refreshes", stats["refreshes"], "line_paints", stats["line_paints"], "draws", stats["frame_draw"]["count"])') $(state) stale=$(pane_value terminal stale)"
}
hook ResetStats; hook SetSetting ToggleToFocus true; settle; sleep 0.5
by_window=$(paints all)
# From a known start: what the checks before left there is told where it fails.
bold_before="$(setting BoldIntense)"
hook SetSetting BoldIntense true; settle
hook ResetStats; hook SetSetting BoldIntense false; settle
by_bold="$(pane_value terminal boldIntense) $(some)"
case $by_bold in *no:*) by_bold="$by_bold stored-before=$bold_before" ;; esac
hook ResetStats; hook SetSetting Skin theme:kudari.test.dark; settle
by_skin=$(some)
hook SetSetting Skin system; hook SetSetting BoldIntense true; hook SetSetting ToggleToFocus false; settle
check "a setting paints the terminals anew only where they show it" "0 false yes yes" "$by_window $by_bold $by_skin"
# A key that the shell echoes paints the terminal once.
run "clear"
hook ResetStats
input key 30; settle
typed=$(paints "$first_terminal")
# That paint takes only the lines that changed, into the image the frame before left.
typed_lines=$(hook Stats | python3 -c 'import json, sys; print(json.load(sys.stdin)["line_paints"])')
input key 14; settle
check "a typed key paints once" yes "$([ "$typed" -ge 1 ] && [ "$typed" -le 1 ] && echo yes || echo "no: $typed")"
check "a typed key paints only the lines that changed" 1 "$typed_lines"

# The pixels of a terminal: drawn in runs of glyphs as they always are, against each text drawn
# on its own; and what the window shows, from the image and the cursor in its scene graph,
# against the same frame painted anew.
paint_check() { # directory, what for, how far a color value of the window may differ (0), what to show (the catalogue)
    # The line at a column goes over the text: a fill after glyphs.
    hook SetSetting VerticalLine 3; settle
    # Text that other text replaces in a frame of its own: a frame that paints only the lines
    # that changed must clear them first.
    run "clear; sh ${4:-$work/catalogue}; printf 'WWWWWWWW'; sleep 0.3; printf '\\riiii\\n'"
    settle
    hook PaintTerminals "$1/paint"
    settle
    hook SetSetting VerticalLine 0; settle
    runs="" shown=""
    for painted in "$1"/paint-[0-9]*.png; do
        case $painted in *-shown.png) continue ;; esac
        terminal=${painted##*/paint-}
        terminal=${terminal%.png}
        # The same glyphs of the same font in the same places: not a pixel differs.
        differ=$(cd "$work" && python3 pixels.py "$painted" "$1/paint-reference-$terminal.png")
        [ "$differ" = "0 0" ] || runs="$runs $terminal: $differ"
        differ=$(cd "$work" && python3 pixels.py "$painted" "${painted%.png}-shown.png")
        [ "$differ" = "0 0" ] || [ "${differ#* }" -le "${3:-0}" ] || shown="$shown $terminal: $differ"
    done
    check "$2, runs of glyphs draw what each text drawn on its own draws" "" "$runs"
    check "$2, the window shows what a frame painted anew shows" "" "$shown"
}
mkdir -p "$work/pixels"
paint_check "$work/pixels" "at 100 %"
# With a badge, a frame of lines paints it again under the lines it paints, and a typed key
# still paints only those. The badge follows the program without output of its own: the poll
# makes it anew and has a terminal whose badge changed paint it.
hook SetSetting BadgeEnabled true; hook SetSetting BadgeText "%n"; settle
run "clear"
hook ResetStats
input key 30; settle
badge_lines=$(hook Stats | python3 -c 'import json, sys; print(json.load(sys.stdin)["line_paints"])')
input key 14; settle
check "with a badge, a typed key paints only the lines that changed" 1 "$badge_lines"
mkdir -p "$work/pixels-badge"
paint_check "$work/pixels-badge" "with a badge"
run "clear"
mkdir -p "$work/badge"
hook PaintTerminals "$work/badge/prompt"; settle
mark
# The program changes half a second after the last output, and nothing paints for it. The
# poll makes the badge anew twice a second; the window has to have painted it before the
# check takes it, as on a slow computer the poll may come just before.
run_open "sleep 0.5; timeout 4 cat"
sleep 2.5
settle
hook PaintTerminals "$work/badge/program"; settle
# The corner of the badge, at the top right: what the window shows there has to differ from
# the corner at the prompt and to match the frame painted anew. Only the corner, as a tooltip
# of the pointer, which stays where an earlier check left it, may show elsewhere meanwhile.
cat > "$work/corner" <<'EOF2'
import sys
from pngread import read
def corner(path):
    width, height, channels, rows = read(path)
    return [bytes(row[width * channels * 2 // 3:]) for row in rows[: height // 4]]
before, shown, painted = (corner(path) for path in sys.argv[1:])
if shown == before:
    print("no: the badge of the prompt")
elif shown != painted:
    print("no: not the badge of the program")
else:
    print("yes")
EOF2
for before in "$work"/badge/prompt-[0-9]*-shown.png; do break; done
for shown in "$work"/badge/program-[0-9]*-shown.png; do break; done
check "the badge follows the program without output" yes \
    "$(cd "$work" && python3 corner "$before" "$shown" "${shown%-shown.png}.png")"
wait_prompt "$mark_before" "the end of timeout"; settle
hook SetSetting BadgeEnabled false; settle
check "a terminal in a tab in the background has no size until it shows" "1 1 0" "$sizeless_in_background $sizeless_shown $sizeless_closed"
run "clear"

run_in "$(sessions activeTerminalId)" "echo dbus > $work/dbus"
check "D-Bus runs a command in a given terminal" "dbus|" "$(content "$work/dbus")"
# A terminal split over D-Bus starts where the one split is.
mkdir -p "$work/split-here"
run "cd $work/split-here"
mark; split=$(sessions splitTerminalLeftRight "$(sessions activeTerminalId)"); wait_shells
run_in "$split" "pwd > $work/split-dir"
check "a terminal split over D-Bus starts where the one split is" "$work/split-here|" "$(content "$work/split-dir")"
sessions removeTerminal "$split"; sleep 0.6
run "clear; cd"
run_open "clear; cat > $work/typed"
hook Key A; sleep 0.2
# What an input method composes shows as text, markup and all: ten cells, not a bold "vor".
hook InputMethod "<b>vor</b>" ""; sleep 0.2
composed=$(awk -v width="$(pane_value preeditText implicitWidth)" -v cell="$(pane_value terminal cellWidth)" \
    'BEGIN { print (cell > 0 && width >= 9.5 * cell) ? "yes" : "no: " width " for cells of " cell }')
hook InputMethod "" "ü"; sleep 0.2
end_input
check "keys and input methods reach the terminal" "aü|" "$(content "$work/typed")"
check "what an input method composes shows as text" yes "$composed"

# The real menus, opened as a click would open them and walked with the keyboard.
hook SetSetting Width 100; hook SetSetting TabActivityColor "#ff0000"; settle
hook Popup session; sleep 1; menu_entry 2
pair=$(active)
check "the session menu opens a session" 2 "$(count "$pair")"
hook Popup main; sleep 1; menu_entry 4
check "the main menu switches an option" false "$(setting KeepOpen)"
hook Popup main; sleep 1; menu_entry 4
hook Popup main; sleep 1; menu_entry 5; menu_entry 5
check "a submenu of the main menu sets the width" 60 "$(setting Width)"
hook SetSetting Width 100; settle
left=$(sessions terminalIdsForSessionId "$pair" | cut -d, -f1)
hook Popup tab; sleep 1; menu_entry 7; menu_entry 1
check "the tab menu blocks the keyboard of one terminal" false "$(sessions isTerminalKeyboardInputEnabled "$left")"
sessions setSessionKeyboardInputEnabled "$pair" true; settle
hook Popup tab; sleep 1; menu_entry 5; input key 28; sleep 1
hook Popup tab; sleep 1; menu_entry 6
first_closable=$(sessions isSessionClosable "$pair")
hook Popup tab; sleep 1; menu_entry 6
check "a tab color can be chosen and reset" "true false" "$first_closable $(sessions isSessionClosable "$pair")"
hook Trigger close-session; settle; input key 1; settle
check "a locked session asks before closing" yes "$(sessions sessionIdList | tr ',' '\n' | grep -qx "$pair" && echo yes)"
sessions setSessionClosable "$pair" true; settle

run_in "$left" "stty size > $work/size-before"
grab splitter
input drag 640 150 440 150; sleep 1
run_in "$left" "stty size > $work/size-after"
check "dragging the splitter resizes the terminals" yes \
    "$([ -s "$work/size-after" ] && ! cmp -s "$work/size-before" "$work/size-after" && echo yes)"
# A tab in the background keeps its size; it used to grow by the scroll bar and wrap again
# on every switch of tabs.
sessions raiseSession "$first"; settle
run_in "$left" "stty size > $work/size-hidden"
check "a hidden tab keeps its columns" "$(cat "$work/size-after")" "$(cat "$work/size-hidden" 2>/dev/null)"
sessions raiseSession "$pair"; settle
input rclick 200 150; sleep 1; menu_entry 1
check "the terminal menu splits the terminal" 3 "$(count "$pair")"
sessions runCommandInTerminal "$left" "exit"; sleep 1
check "a terminal closes when its shell exits" 2 "$(count "$pair")"

sessions raiseSession "$first"; settle
hook SetSetting EditorCommand "$work/editor PATH:LINE"; settle
rm -f "$work/edited"
run "clear; printf 'see notes.txt:12:3 end\n'"
input rclick 130 8; sleep 1; menu_entry 1
check "the terminal menu opens a link" "$work/home/notes.txt:12|" "$(content "$work/edited")"
hook SetSetting EditorCommand ""; settle

input drag 60 377 200 377; sleep 1
check "dragging a tab moves it" yes "$([ "$(tabs sessionAtTab 0)" != "$first" ] && echo yes)"

hook SetSetting ConfirmQuit true; settle
hook Trigger file_quit; settle; input key 1; settle
check "quitting with several sessions asks first" yes "$(kill -0 "$kudari" 2>/dev/null && echo yes)"
hook SetSetting ConfirmQuit false; settle

# What programs in the terminal ask for with control sequences.
run "clear; stty raw -echo; printf '\033[6n'; dd bs=1 count=2 of=$work/report 2>/dev/null; stty sane"
check "the terminal answers a program" "033[" "$(od -An -c "$work/report" | tr -d ' ')"
run "clear; printf '\033]52;c;%s\a' \$(printf osc | base64)"
run_open "clear; cat > $work/osc"
hook Trigger edit_paste; settle; end_input
check "programs can copy to the clipboard" "osc|" "$(content "$work/osc")"
# With the setting on, only the terminal in front copies: a program in a tab behind copies a
# moment after another tab came to the front, and the clipboard keeps what it had; the
# program of the tab in front copies.
hook SetSetting ProgramsCopyOnlyWithFocus true; settle
copying=$(active)
run_open "clear; sleep 2; printf '\033]52;c;%s\a' \$(printf behind | base64)"
mark; hook Trigger new-session; wait_shells
in_front=$(active)
sleep 2.5
run_open "clear; cat > $work/osc-behind"
hook Trigger edit_paste; settle; end_input
run "clear; printf '\033]52;c;%s\a' \$(printf front | base64)"
run_open "clear; cat > $work/osc-front"
hook Trigger edit_paste; settle; end_input
sessions removeSession "$in_front" > /dev/null; settle
sessions raiseSession "$copying" > /dev/null; settle
hook SetSetting ProgramsCopyOnlyWithFocus false; settle
check "with the setting, only the terminal in front copies" "osc| front|" \
    "$(content "$work/osc-behind") $(content "$work/osc-front")"
# A blocked keyboard: the wheel sends no arrow keys to a program on the alternate screen.
before_wheel=$(active)
mark; hook Trigger new-session; wait_shells
wheeled=$(active)
run_open "clear; stty raw -echo; printf '\033[?1049h'; timeout 3 cat > $work/wheel; printf '\033[?1049l'; stty sane"
sessions setSessionKeyboardInputEnabled "$wheeled" false; settle
input wheel 130 8 15 3; sleep 0.3
# Given back its keyboard, the terminal takes the arrow key of one notch again.
sessions setSessionKeyboardInputEnabled "$wheeled" true; settle
input wheel 130 8 15 1; sleep 2.5
wheel=$(tr '\033' E < "$work/wheel")
check "a blocked terminal takes no arrow keys from the wheel" one \
    "$(echo "$wheel" | grep -qxE 'EO[AB]' && echo one || echo "$wheel")"
sessions removeSession "$wheeled"; settle
sessions raiseSession "$before_wheel"; settle
# Output into a scrolling region below the top while the view is scrolled back used to scroll
# the view past the history, and the next frame ended Kudari with all its terminals.
mark; region=$(sessions addSession); wait_shells
sessions raiseSession "$region"; settle
hook SetSetting HistoryLines 10000; settle
# Some history, a moment to scroll back a page with Shift+Page Up, then the output.
run_open "clear; seq 300; sleep 2; printf '\\033[2;%dr\\033[%dH' \$(tput lines) \$(tput lines); seq 5000; printf '\\033[r'"
input key 42 104; sleep 5
check "output into a region below the top while scrolled back leaves Kudari running" yes \
    "$(kill -0 "$kudari" 2>/dev/null && [ -n "$(state)" ] && echo yes || echo no)"
hook SetSetting HistoryLines 1000; settle
sessions removeSession "$region"; settle
sessions raiseSession "$before_wheel"; settle
# Two quick clicks at different places are two single clicks: the second selects no word.
run "clear; printf '\033]52;c;%s\a' \$(printf before | base64)"
run "clear; printf 'apple %s\n' bananabananabananabananabananabananabananabanana"
input click2 12 8 300 8; sleep 0.3
hook Trigger edit_copy; settle
run_open "clear; cat > $work/clicks"
hook Trigger edit_paste; settle; end_input
check "two quick clicks at different places select no word" no \
    "$(content "$work/clicks" | grep -q banana && echo yes || echo no)"
# sh has no bracketed paste, so it would run pasted lines one by one: Kudari asks first.
run "clear; printf '\033]52;c;%s\a' \$(printf 'one\ntwo\n' | base64)"
run_open "clear; cat > $work/lines"
hook Trigger edit_paste; settle; input key 1; settle
check "pasting several lines asks first" "" "$(cat "$work/lines")"
hook Trigger edit_paste; settle; input key 28; settle; input key 29 32; settle
check "the lines arrive once the user agrees" "one|two|" "$(content "$work/lines")"
run_open "clear; cat > $work/dropped"
hook DropText "$(printf 'three\nfour\n')"; sleep 1; input key 1; settle
check "dropping several lines of text asks first as well" "" "$(cat "$work/dropped")"
end_input
# A link that a program set to another address than its text shows tells that address when the
# pointer rests on it, before a click opens it; one whose text is its address tells nothing.
run "clear; printf '\033]8;;https://evil.example/login\033\\\\https://bank.example\033]8;;\033\\\\ \033]8;;https://kde.org\033\\\\https://kde.org\033]8;;\033\\\\\n'"
input move 40 8; sleep 0.8
told="$(pane_value linkLine hint) $(pane_value linkHint visible)"
input move 230 8; sleep 0.8
told="$told / $(pane_value linkLine hint) $(pane_value linkHint visible)"
input move 300 8; sleep 0.3
check "a link to another address than its text tells it" "https://evil.example/login true /  false" "$told"
# Moves within one cell look for no link, as the link, its address and the color under the
# pointer can be no others there; into the next cell, they look again.
lookups() { hook Stats | python3 -c 'import json, sys; print(json.load(sys.stdin)["link_lookups"])'; }
input move 60 8; sleep 0.3
hook ResetStats
for y in 6 9 7 10 5 8; do input move 60 "$y"; done; sleep 0.3
within=$(lookups)
input move 60 30; sleep 0.3
beside=$(lookups)
input move 300 8; sleep 0.3
check "moves within a cell look for no link, into the next one they do" "0 yes" \
    "$within $([ "$beside" -gt 0 ] && echo yes || echo "no: $beside")"
mkdir -p "$work/bin"; ln -sf "$(command -v sleep)" "$work/bin/ssh"
hook SetSetting RemoteTitleFormat "%w on %H"; settle
run_open "clear; printf '\033]0;remote\a'; $work/bin/ssh 3"
sleep 1
check "a program sets the title of an ssh tab" "remote on 3" "$(tabs tabTitle "$first")"
sleep 2
# A reset leaves programs that read lines alone: Ctrl+L would go into the file.
run_open "clear; cat > $work/reset"
hook Trigger clear-history-and-reset; settle; end_input
check "a reset types nothing into a program that reads lines" "" "$(tr -d '\n' < "$work/reset" | od -An -c | tr -d ' ')"
# In a pipeline whose first program ended, the title names the one that runs.
run_open "true | sleep 3"; sleep 1.5
check "a pipeline shows its program after its first one ended" yes \
    "$(tabs tabTitle "$first" | grep -q sleep && echo yes || echo "no: $(tabs tabTitle "$first")")"
sleep 2
# No window of X11 is the terminal's, nor one of Alacritty.
run "echo \"[\$WINDOWID][\$ALACRITTY_WINDOW_ID]\" > $work/windowid"
check "shells get no number of a window" "[][]" "$(cat "$work/windowid" 2>/dev/null)"
# A title is text: the markup of a program shows as it is, and loads nothing from the network.
python3 -m http.server --bind 127.0.0.1 18765 --directory "$work" > "$work/http.log" 2>&1 &
http=$!
hook SetSetting TitleFormat "%w"; settle
run "clear; printf '\033]2;<img src=\"http://127.0.0.1:18765/t.png\">x\a'"
sleep 2
check "a title shows its markup as text" '<img src="http://127.0.0.1:18765/t.png">x' "$(tabs tabTitle "$first")"
# A notification names the session by that title. KNotification takes markup, and the text
# of a notification is markup of KDE's translations, in which the title is text: the image
# loads nothing there either.
hook Toggle toggle-session-monitor-process true; settle
run "sleep 1"; sleep 1
hook Toggle toggle-session-monitor-process false; settle
check "a notification shows the title of its session as text" \
    '<html>Process finished in monitored terminal in session "&lt;img src=&quot;http://127.0.0.1:18765/t.png&quot;&gt;x".</html>' \
    "$(hook LastNotification)"
kill "$http"
check "a title loads nothing from the network" no "$(grep -q 't.png' "$work/http.log" && echo yes || echo no)"
# A script names the tab with marks of direction, so that `exe.txt` would show as `txt.exe`:
# the tab shows the name without them.
tabs setTabTitle "$first" "$(printf 'photo\342\200\256gpj.exe')"
check "a name from a script shows nothing that turns the text around" photogpj.exe "$(tabs tabTitle "$first")"
tabs setTabTitle "$first" ""; settle
hook SetSetting TitleFormat "%d : %n"; settle
run_open "clear; stty raw -echo; printf '\033[?1000h\033[?1006h'; timeout 2 cat > $work/mouse; printf '\033[?1000l\033[?1006l'; stty sane"
# Three turns of 15 pixels, as a touchpad sends them, are three lines of 15 pixels: one
# notch of a wheel, and one report for a program.
input click 130 8; input wheel 130 8 15 3; sleep 2
check "programs that ask get clicks and the wheel" yes \
    "$(tr '\033' E < "$work/mouse" | grep -qE '^E\[<0;[0-9]+;1ME\[<0;[0-9]+;1mE\[<6[45];' && echo yes)"
check "a notch of small turns is one report" 1 "$(tr '\033' E < "$work/mouse" | grep -oE 'E\[<6[45];' | wc -l)"
run_open "clear; stty raw -echo; printf '\033[?1049h'; timeout 2 cat > $work/arrows; printf '\033[?1049l'; stty sane"
# A sideways wheel scrolls nothing; it used to count as down.
input wheel 130 8 15 3 1; input wheel 130 8 15 3; sleep 1
check "the wheel sends an arrow key for each line to full-screen programs" yes \
    "$(tr '\033' E < "$work/arrows" | grep -qE '^(EO[AB]){3}$' && echo yes)"

# The kitty keyboard protocol: a program learns which of its flags Kudari carries out, and a
# key that Shift changes has the code of its base key, as 2 for Ctrl+Shift+2, which types @.
hook SetSetting KittyKeyboard true; settle
cat > "$work/kitty" <<'EOF'
import os, re, select, sys, termios, tty
saved = termios.tcgetattr(0)
tty.setraw(0)
os.write(1, b"\033[>31u\033[?u")
reply = b""
while not re.search(rb"\033\[\?\d+u", reply) and select.select([0], [], [], 2)[0]:
    reply += os.read(0, 256)
os.write(1, b"\033[<u\033[>1u")
key = b""
while not re.search(rb"\033\[[0-9;:]+u", key) and select.select([0], [], [], 5)[0]:
    key += os.read(0, 256)
os.write(1, b"\033[<u")
termios.tcsetattr(0, termios.TCSAFLUSH, saved)
with open(sys.argv[1], "wb") as out:
    out.write((reply + b"|" + key).replace(b"\033", b"E"))
EOF
run_open "clear; python3 $work/kitty $work/kitty-out"
sleep 0.5; input key 29 42 3; sleep 1
check "a program learns the kitty flags that Kudari carries out" "E[?25u" "$(cut -d'|' -f1 "$work/kitty-out")"
check "Ctrl+Shift+2 has the code of the 2 in the kitty protocol" "E[50;6u" "$(cut -d'|' -f2 "$work/kitty-out")"
hook SetSetting KittyKeyboard false; settle

hook Toggle toggle-session-monitor-once true; settle
sessions setSessionMonitorActivityEnabled "$first" true; settle
watched=$(sessions activeTerminalId)
sessions raiseSession "$pair"; settle
sessions runCommandInTerminal "$watched" "echo activity"; sleep 1
check "a one-shot activity monitor ends after its alert" false "$(sessions isSessionMonitorActivityEnabled "$first")"
sessions raiseSession "$first"; settle
hook Toggle toggle-session-monitor-process true; settle
run_open "sleep 1"; sleep 1.5
for mode in visual none notify; do hook SetSetting BellMode "$mode"; settle; run "printf '\a'"; done
hook Toggle toggle-session-monitor-once false; hook SetSetting BellMode notify; hook SetSetting TabActivityColor ""

# AI agents reach Kudari through `kudari mcp`, which their client starts once and keeps.
# `agent` calls a tool of such a server and prints the text of its result, after "error: "
# when the tool failed; `agent_once` starts a server of its own for one call.
request() { # id, tool, arguments as JSON
    printf '%s\n%s\n%s\n' \
        '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"gui-test","version":"1"}}}' \
        '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
        "{\"jsonrpc\":\"2.0\",\"id\":$1,\"method\":\"tools/call\",\"params\":{\"name\":\"$2\",\"arguments\":$3}}"
}
result() { python3 -c '
import json, sys
for line in sys.stdin:
    message = json.loads(line)
    if message.get("id") != 0:
        result = message["result"]
        print(("error: " if result["isError"] else "") + result["content"][0]["text"])'
}
start_agent() {
    rm -f "$work/mcp-in" "$work/mcp-out"
    mkfifo "$work/mcp-in" "$work/mcp-out"
    "$binary" mcp < "$work/mcp-in" > "$work/mcp-out" 2>> "$work/mcp.log" &
    agent_pid=$!
    exec 7> "$work/mcp-in" 8< "$work/mcp-out"
    calls=0
}
stop_agent() { exec 7>&- 8<&-; wait "$agent_pid" 2>/dev/null; }
send() { # tool, arguments as JSON
    calls=$((calls + 1))
    request "$calls" "$1" "$2" | if [ "$calls" -eq 1 ]; then cat; else tail -n 1; fi >&7
}
# The next answer of the agent, past its notifications; nothing after a minute without one.
# A shell of its own reads each line, as plain sh has no timeout for `read`; it reads a pipe
# byte by byte, so nothing of the next line gets lost.
receive() {
    # shellcheck disable=SC2016 # the inner shell expands $line
    while agent_line=$(timeout 60 sh -c 'IFS= read -r line && printf "%s\n" "$line"' <&8); do
        case $agent_line in *'"id":0'*) ;; *) echo "$agent_line" | result; return ;; esac
    done
}
agent() { send "$1" "$2"; receive; }
agent_once() { request 1 "$1" "$2" | "$binary" mcp 2>> "$work/mcp.log" | result; }
# A call of an agent starts no Kudari: on a bus of its own with a service file for Kudari,
# whose program only notes that it was started, `kudari mcp` says that Kudari does not run.
mkdir -p "$work/bus/services"
printf '#!/bin/sh\necho started >> %s\n' "$work/bus/started" > "$work/bus/kudari"
chmod +x "$work/bus/kudari"
printf '[D-BUS Service]\nName=io.github.marcjose.kudari\nExec=%s\n' "$work/bus/kudari" \
    > "$work/bus/services/io.github.marcjose.kudari.service"
cat > "$work/bus/session.conf" <<BUS
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$work/bus/socket</listen>
  <servicedir>$work/bus/services</servicedir>
  <policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy>
</busconfig>
BUS
dbus-daemon --config-file="$work/bus/session.conf" --nofork > "$work/bus/log" 2>&1 &
own_bus=$!
tries=0
until [ -S "$work/bus/socket" ] || [ "$tries" -gt 50 ]; do sleep 0.1; tries=$((tries + 1)); done
not_running=$(request 1 list_terminals '{}' | DBUS_SESSION_BUS_ADDRESS="unix:path=$work/bus/socket" timeout 30 "$binary" mcp 2>> "$work/mcp.log" | result)
sleep 1
kill "$own_bus" 2>/dev/null
check "a call of an agent starts no Kudari" \
    "error: Kudari is not running. The user can start it from the application menu or with the command kudari. started: no" \
    "$not_running started: $([ -e "$work/bus/started" ] && echo yes || echo no)"
question() {
    for _ in $(seq 50); do
        question_text=$(hook AgentQuestion)
        [ -n "$question_text" ] && { echo "$question_text"; return; }
        sleep 0.1
    done
}
field() { python3 -c "import json, sys; print(json.load(sys.stdin)['$1'])"; }
refused() { grep -q "^error: $1" && echo yes; }

start_agent
check "agents may not read by default" yes "$(agent list_terminals '{}' | refused 'Reading terminals is switched off')"
check "the switched off interface shows no access" false "$(hook AgentAccess)"
hook SetSetting AllowAgentReading true; settle
send list_terminals '{}'
asked=$(question)
check "Kudari asks before an agent reads" yes "$(echo "$asked" | grep -q 'wants to read your terminals' && echo yes)"
check "the question keeps its two paragraphs" yes "$(echo "$asked" | grep -q '</p><p>Allow this only' && echo yes)"
check "the question names the program that started kudari mcp" yes \
    "$(echo "$asked" | grep -qF "The request comes from $(cat /proc/$$/comm)." && echo yes)"
hook AnswerAgent true
check "an allowed agent reads the terminals" yes "$(receive | grep -q '"sessions"' && echo yes)"
check "the bar shows that an agent has access" true "$(hook AgentAccess)"
check "another agent has to ask for itself" yes "$( (agent_once list_terminals '{}' > "$work/other" &); \
    question | grep -q 'read your terminals' && hook AnswerAgent false; sleep 0.5; refused 'The user did not allow' < "$work/other")"
# Two dialogs at once: the window stays below the one left open until it closes as well.
(agent_once list_terminals '{}' > "$work/third" &)
question > /dev/null
protected=$(active)
sessions setSessionClosable "$protected" false; settle
hook Trigger close-session; settle; input key 1; settle
check "a dialog left open keeps the window below it" true "$(hook DialogOpen)"
hook AnswerAgent false; sleep 0.6
check "the window comes back when the last dialog closed" false "$(hook DialogOpen)"
sessions setSessionClosable "$protected" true; settle
# About Kudari shows its dialog without waiting for it: the window stays below until it closes.
hook Popup main; sleep 1; menu_entry 1; sleep 0.5
check "the window stays below the dialog about Kudari" true "$(hook DialogOpen)"
input key 1; sleep 1
check "the window comes back when the dialog about Kudari closed" false "$(hook DialogOpen)"
check "agents may not type by default" yes "$(agent type_text '{"text": "ls"}' | refused 'Controlling terminals is switched off')"
hook SetSetting AllowAgentControl true; settle
check "more rights end the access granted before" false "$(hook AgentAccess)"
users=$(sessions activeSessionId)
send open_session '{"title": "Agent"}'
check "the question names the new rights" yes "$(question | grep -q 'read and control' && echo yes)"
hook AnswerAgent true
opened=$(receive)
own=$(echo "$opened" | field session)
mine=$(echo "$opened" | field terminal)
check "an agent opens a session of its own" Agent "$(tabs tabTitle "$own")"
# The user may be typing elsewhere, a password even: the new session stays behind.
check "the session of an agent opens behind the tab of the user" "$users" "$(sessions activeSessionId)"
# The user goes there to confirm the commands of the agent.
sessions raiseSession "$own"; settle
agent type_text "{\"terminal\": $mine, \"text\": \"echo typed > $work/agent-typed\", \"execute\": true}" > "$work/answer"
sleep 0.6
check "the command of an agent waits for Return" "no yes" \
    "$([ -e "$work/agent-typed" ] && echo yes || echo no) $(grep -q 'presses Return' "$work/answer" && echo yes)"
input key 28
check "Return runs the command of an agent" "typed|" "$(content "$work/agent-typed")"
check "with confirmation an agent cannot press escape" yes \
    "$(agent send_key "{\"terminal\": $mine, \"key\": \"escape\"}" | refused 'escape could form')"
agent type_text "{\"terminal\": $mine, \"text\": \"read -r reply; echo got-\$reply > $work/agent-read\", \"execute\": true}" > /dev/null
sleep 0.4; input key 28; settle
check "a script reading input is no prompt for an agent" yes \
    "$(agent type_text "{\"terminal\": $mine, \"text\": \"y\"}" | refused 'The shell is not editing')"
input key 28; settle
check "the script got nothing from the agent" "got-|" "$(content "$work/agent-read")"
hook SetSetting ConfirmAgentCommands false; settle
# The agent was allowed when its commands waited for Return; Kudari asks about it again.
check "commands that no longer wait end the access granted before" false "$(hook AgentAccess)"
send type_text "{\"terminal\": $mine, \"text\": \"echo ran-by-agent\", \"execute\": true}"
check "Kudari asks again before commands of an agent run at once" yes \
    "$(question | grep -q 'read and control' && echo yes)"
hook AnswerAgent true
receive > /dev/null; sleep 0.6
check "without confirmation an agent runs commands" yes \
    "$(agent read_terminal "{\"terminal\": $mine, \"lines\": 5}" | grep -qx 'ran-by-agent' && echo yes)"
agent type_text "{\"terminal\": $mine, \"text\": \"sleep 30\", \"execute\": true}" > /dev/null; sleep 0.6
hook SetSetting ConfirmAgentCommands true; settle
check "with confirmation an agent cannot type into a running program" yes \
    "$(agent type_text "{\"terminal\": $mine, \"text\": \"y\"}" | refused 'A program is running')"
check "an agent stops a program with ctrl+c" "Pressed ctrl+c." "$(agent send_key "{\"terminal\": $mine, \"key\": \"ctrl+c\"}")"
check "an agent cannot press Return as a key" yes "$(agent send_key '{"key": "enter"}' | refused 'Unknown key')"
check "an agent cannot close the sessions of the user" yes \
    "$(agent close_session "{\"session\": $first}" | refused 'Agents may close only')"
agent close_session "{\"session\": $own}" > /dev/null; sleep 0.6
check "an agent closes its own session" no "$(sessions sessionIdList | tr ',' '\n' | grep -qx "$own" && echo yes || echo no)"
hook RevokeAgents; sleep 0.3
check "the user ends the access of an agent" "false yes" \
    "$(hook AgentAccess) $(agent read_terminal '{}' | refused 'The user did not allow')"
stop_agent
start_agent
send list_terminals '{}'
question > /dev/null; hook AnswerAgent true; receive > /dev/null
before=$(hook AgentAccess)
stop_agent; sleep 0.5
check "the access ends with the agent" "true false" "$before $(hook AgentAccess)"
request 1 list_terminals '{}' | "$binary" mcp > /dev/null 2>> "$work/mcp.log" &
once=$!
check "the question goes away with its agent" "yes|" \
    "$(question | grep -q 'wants to read' && echo yes)|$(kill "$once"; sleep 0.5; hook AgentQuestion)"
start_agent
hook SetSetting AllowAgentReading false; settle
check "reading can be switched off" yes "$(agent read_terminal '{}' | refused 'Reading terminals is switched off')"
stop_agent
hook SetSetting AllowAgentControl false
sessions raiseSession "$first"; settle

activate() { gdbus call --session --dest "$service" --object-path /io/github/marcjose/kudari \
    --method org.freedesktop.Application.Activate '@a{sv} {}' >/dev/null; sleep 1; }
activate
hidden=$(state)
activate
check "activating the application toggles the window" "visible=0 visible=1" "$hidden $(state)"

"$binary" > /dev/null 2>&1; sleep 1
check "a second start toggles the running window" visible=0 "$(state)"

check "Kudari still runs" yes "$(kill -0 "$kudari" 2>/dev/null && echo yes || echo no)"
# Only our own files: Kirigami logs a TypeError of its own when a dialog page goes away.
pattern='(io/github/marcjose/kudari.*(TypeError|ReferenceError|is not a type|Cannot assign)|Unable to create)'
check "our QML reports no errors" 0 "$(grep -cE "$pattern" "$work/log")"
grep -E "$pattern" "$work/log" | head -5 | sed 's/^/Error in QML: /'

hook Trigger file_quit
wait_quit "$kudari"
check "quitting ends the program" no "$(kill -0 "$kudari" 2>/dev/null && echo yes || echo no)"
# Skins used to be images made at runtime; the bars are drawn now.
check "no skin directory is created" "" "$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type d -name 'kudari-*' -print -quit)"

# Started from the menu, the bus starts Kudari and activates it as soon as it has its name,
# before its window exists. The window opens, and stays open where the start opens it anyway.
early_activation() { # settings
    rm -rf "$work/early"
    mkdir -p "$work/early"
    printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n%s' "$1" > "$work/early/kudarirc"
    XDG_CONFIG_HOME="$work/early" "$binary" > "$work/early.log" 2>&1 &
    early=$!
    # Only listed: a call to a name nobody owns would start another Kudari.
    tries=0
    until qdbus6 2>/dev/null | grep -q 'io\.github\.marcjose\.kudari$'; do
        tries=$((tries + 1))
        if [ "$tries" -ge 500 ] || ! kill -0 "$early" 2>/dev/null; then
            break
        fi
    done
    gdbus call --session --dest "$service" --object-path /io/github/marcjose/kudari \
        --method org.freedesktop.Application.Activate '@a{sv} {}' > /dev/null
    sleep 3
    state
    hook Trigger file_quit
    wait_quit "$early"
    kill -9 "$early" 2>/dev/null
}
check "activating Kudari while it starts opens the window" visible=1 "$(early_activation '')"
check "activating it while it starts keeps a window open that the start opens" visible=1 \
    "$(early_activation '[Behavior]\nOpenAfterStart=true\n')"

# A first start: the welcome dialog keeps the shortcut and takes over the settings of Yakuake.
mkdir "$work/first"
printf '[Window]\nWidth=80\n' > "$work/first/yakuakerc"
XDG_CONFIG_HOME="$work/first" "$binary" > "$work/first.log" 2>&1 &
kudari=$!
started
input key 29 28; sleep 1
first_setting() { sed -n "s/^$1=//p" "$work/first/kudarirc"; }
check "accepting the welcome takes over Yakuake's settings without asking" "80 false" \
    "$(first_setting Width) $(first_setting FirstRun)"
hook Trigger file_quit
wait_quit "$kudari"
check "quitting after the first start ends the program" no "$(kill -0 "$kudari" 2>/dev/null && echo yes || echo no)"

# At a fractional scale, as 125 %, the cells measure whole pixels of the device: the lines of
# tables are as thick in every cell, and as far apart. With a cell of 9 logical pixels, 11.25
# of the device, they used to vary from column to column.
# A window of 89 % gives the terminal a width that is no whole number of pixels of the device,
# as 886 logical pixels make 1107.5: its image has to lie on whole pixels all the same, which
# the pixels below compare.
mkdir "$work/scaled"
printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n[Window]\nKeepOpen=true\nWidth=89\n[Terminal]\nFontSize=11\n[Security]\nAllowRemoteCommands=true\n' \
    > "$work/scaled/kudarirc"
# It writes its counters once a second as well, as the bench reads them.
KUDARI_STATS=1 QT_SCALE_FACTOR=1.25 XDG_CONFIG_HOME="$work/scaled" "$binary" > "$work/scaled.log" 2>&1 &
kudari=$!
started
window toggleWindowState; sleep 1.5
run "clear; printf '│%.0s' \$(seq 60); echo; for i in 1 2 3; do echo; printf '─%.0s' \$(seq 60); echo; done"
sleep 1
hook Grab "$work/scaled/grab"; sleep 1
cat > "$work/lines" <<'EOF'
import sys
from pngread import read
width, height, channels, rows = read(sys.argv[1])
stride = width * channels
background = rows[height // 2][stride - 5 * channels:stride - 5 * channels + 3]
def lit(x, y):
    pixel = rows[y][x * channels:x * channels + 3]
    return sum(abs(a - b) for a, b in zip(pixel, background)) > 120
def runs(values):
    found, start = [], None
    for index, on in enumerate(values + [False]):
        if on and start is None:
            start = index
        elif not on and start is not None:
            found.append((start, index - start))
            start = None
    return found
# The row through the vertical lines has the most runs; its first 60 away from the edges of
# the window, which has a frame, are the lines.
inner = lambda found: [run for run in found if run[0] > 0 and run[0] + run[1] < width]
bars = max((inner(runs([lit(x, y) for x in range(width)])) for y in range(height // 2)), key=len)[:60]
middle = bars[len(bars) // 2][0] + bars[len(bars) // 2][1] // 2
across = [x for x in range(bars[0][0], bars[-1][0], 3)]
# The horizontal lines: the rows lit all along, above the bar of the tabs at the bottom.
lines = runs([all(lit(x, y) for x in across) for y in range(height // 2)])
steps = lambda found: {b[0] - a[0] for a, b in zip(found, found[1:])}
widths = lambda found: {length for _, length in found}
print("even" if len(bars) == 60 and len(lines) == 3 and all(len(group) == 1 for group in (widths(bars), steps(bars), widths(lines), steps(lines)))
      else f"bars {sorted(widths(bars))} apart {sorted(steps(bars))}, lines {lines}")
EOF
check "at 125 %, the lines of tables are as thick and as far apart in every cell" even \
    "$(python3 "$work/lines" "$work"/scaled/grab-*.png 2>&1 | tail -n 1)"
mkdir -p "$work/scaled/pixels"
paint_check "$work/scaled/pixels" "at 125 %"

# A translucent terminal replaces every pixel of its image with each frame, transparent ones
# as well: Qt no longer clears the image first, so text that went away must not stay.
hook SetSetting Opacity 60; settle
run "clear; printf '█%.0s' \$(seq 300); echo"
sleep 0.8
hook Grab "$work/scaled/full"; sleep 1
run "clear"
sleep 0.8
hook Grab "$work/scaled/cleared"; sleep 1
# The pixels in the upper half of the window, where the text is, that stand out from its
# most common color, the background, whatever the colors of the scheme.
cat > "$work/text" <<'EOF'
import collections
import sys
from pngread import read
width, height, channels, rows = read(sys.argv[1])
pixels = [tuple(row[x:x + 3]) for row in rows[:height // 2] for x in range(0, width * channels, channels)]
background = collections.Counter(pixels).most_common(1)[0][0]
print(sum(1 for pixel in pixels if sum(abs(a - b) for a, b in zip(pixel, background)) > 120))
EOF
full=$(python3 "$work/text" "$work"/scaled/full-*.png 2>&1 | tail -n 1)
cleared=$(python3 "$work/text" "$work"/scaled/cleared-*.png 2>&1 | tail -n 1)
check "a translucent terminal keeps nothing of text that went away" yes \
    "$([ "$full" -gt $((cleared * 10)) ] 2>/dev/null && echo yes || echo "no: $full pixels of text, then $cleared")"
lines=$(sed -n 's/^kudari-stats [0-9.]* //p' "$work/scaled.log" | python3 -c '
import json, sys
found = [json.loads(line) for line in sys.stdin]
print("yes" if found and all("lock_wait" in stats for stats in found) and found[-1]["refreshes"] > 0 else len(found))')
check "with KUDARI_STATS=1 the counters come once a second" yes "$lines"
hook Trigger file_quit
wait_quit "$kudari"

# Qt Quick through its RHI, as on a computer with graphics, here OpenGL that Mesa draws in
# software: the texture of a terminal keeps its storage there and takes only the parts of the
# frame that changed, which the window shows as the pixels below compare.
mkdir "$work/rhi"
# The painted frame first, whichever way the default draws; the graphics card further down.
printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n[Window]\nKeepOpen=true\nReleaseGraphicsAfter=1\n[Appearance]\nRenderer=cpu\n[Security]\nAllowRemoteCommands=true\n' \
    > "$work/rhi/kudarirc"
env -u QT_QUICK_BACKEND QSG_RHI_BACKEND=opengl LIBGL_ALWAYS_SOFTWARE=1 QSG_INFO=1 XDG_CONFIG_HOME="$work/rhi" \
    "$binary" > "$work/rhi.log" 2>&1 &
kudari=$!
started
window toggleWindowState; sleep 1.5
if grep -qi 'rhi.*opengl\|opengl.*rhi' "$work/rhi.log"; then drawn=yes; else drawn=$(tail -n 5 "$work/rhi.log" | tr '\n' ' '); fi
check "without the software backend, Qt Quick draws through the RHI" yes "$drawn"
mkdir -p "$work/rhi/pixels"
paint_check "$work/rhi/pixels" "through the RHI"
# Hidden for long, the window gives up its graphics, here after the 1 s of its settings, and
# builds them again when it shows: Qt Quick makes its QRhi a second time, and the terminals
# show what they did. With the setting at 0, it keeps them however long it is hidden.
window toggleWindowState; sleep 3
window toggleWindowState; sleep 1.5; settle
check "a window hidden for long gives up its graphics and makes them again" 2 \
    "$(grep -c 'Creating QRhi' "$work/rhi.log")"
mkdir -p "$work/rhi/released"
paint_check "$work/rhi/released" "through the RHI, after giving up its graphics"
hook SetSetting ReleaseGraphicsAfter 0; settle
window toggleWindowState; sleep 3
window toggleWindowState; sleep 1.5; settle
check "with the setting at never, a hidden window keeps its graphics" 2 \
    "$(grep -c 'Creating QRhi' "$work/rhi.log")"
# On the graphics card, the fills and glyphs of a frame are geometry, the glyphs from an atlas:
# the window shows what the same frame painted whole shows, but for blending, which can differ
# by a step of a color value, and its fills and glyphs went there.
gpu_drawn() {
    hook Stats | python3 -c 'import json, sys; s = json.load(sys.stdin); print(*(s.get("gpu_" + n, "none") for n in ("fills", "runs", "texts", "fallbacks")))'
}
hook ResetStats; run "clear; sh $work/catalogue"
on_cpu=$(gpu_drawn)
hook SetSetting Renderer gpu; settle
hook ResetStats
mkdir -p "$work/rhi/gpu"
paint_check "$work/rhi/gpu" "through the RHI on the graphics card" 1
# A screen with a background, a line under and one through the text of each cell, in colors
# that change from cell to cell: more corners than the geometry has room for at first, which
# grows within the frame; then the catalogue again, after which none of them may show.
mkdir -p "$work/rhi/gpu-full" "$work/rhi/gpu-after"
paint_check "$work/rhi/gpu-full" "through the RHI on the graphics card, a full screen" 1 "$work/full"
paint_check "$work/rhi/gpu-after" "through the RHI on the graphics card, after a full screen" 1
# The badge lies between the background and the cells, an image of its own there: over the
# cells of a full screen, it shows only where they let it through.
hook SetSetting BadgeEnabled true; hook SetSetting BadgeText "%n"; settle
mkdir -p "$work/rhi/gpu-badge"
paint_check "$work/rhi/gpu-badge" "through the RHI on the graphics card, with a badge" 1 "$work/full"
hook SetSetting BadgeEnabled false; settle
# A character of the catalogue that no font has goes there as a text of its own, as does any
# that the fonts of a computer lack: no frame is painted instead.
check "on the graphics card, the fills, glyphs and texts of a frame go there and not with the setting cpu" "0 0 0 0 yes" \
    "$on_cpu $(gpu_drawn | awk '{ print ($1 > 0 && $2 > 0 && $3 > 0 && $4 == 0) ? "yes" : "no: " $0 }')"
# A typed key builds only the lines that changed there, into their pieces of the frame; the
# frames of lines in the checks above show what the frame painted whole shows.
run "clear"
hook ResetStats
input key 30; settle
gpu_lines=$(hook Stats | python3 -c 'import json, sys; s = json.load(sys.stdin); print(s["line_paints"], s["gpu_fallbacks"])')
input key 14; settle
check "on the graphics card, a typed key builds only the lines that changed" "1 0" "$gpu_lines"
# Translucent, the window shows through the terminal what lies behind it: on the graphics card
# what it shows with the frame painted, but for a step of a color value.
hook SetSetting Translucency true; hook SetSetting BackgroundColorOpacity 60
hook SetSetting Renderer cpu; settle
run "clear; sh $work/catalogue"
mkdir -p "$work/rhi/translucent-cpu" "$work/rhi/translucent-gpu"
hook PaintTerminals "$work/rhi/translucent-cpu/paint"; settle
hook SetSetting Renderer gpu; settle
hook PaintTerminals "$work/rhi/translucent-gpu/paint"; settle
for painted in "$work"/rhi/translucent-cpu/paint-[0-9]*-shown.png; do break; done
differ=$(cd "$work" && python3 pixels.py "$painted" "$work/rhi/translucent-gpu/${painted##*/}")
check "translucent, the window on the graphics card shows what it shows painted" yes \
    "$([ "${differ#* }" -le 1 ] 2>/dev/null && echo yes || echo "no: $differ")"
hook SetSetting Translucency false; settle
hook Trigger file_quit
wait_quit "$kudari"

# At 125 % and at 200 %, on the graphics card from the start: the tiles of glyphs lie on the
# pixels of the device that the painted frame covers, and the badge with them.
for scale in 1.25 2; do
    mkdir -p "$work/rhi-$scale/pixels"
    printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n[Window]\nKeepOpen=true\n[Appearance]\nRenderer=gpu\n[Terminal]\nBadgeEnabled=true\nBadgeText=%%n\n[Security]\nAllowRemoteCommands=true\n' \
        > "$work/rhi-$scale/kudarirc"
    env -u QT_QUICK_BACKEND QSG_RHI_BACKEND=opengl LIBGL_ALWAYS_SOFTWARE=1 QT_SCALE_FACTOR=$scale XDG_CONFIG_HOME="$work/rhi-$scale" \
        "$binary" > "$work/rhi-$scale.log" 2>&1 &
    kudari=$!
    started
    window toggleWindowState; sleep 1.5
    hook ResetStats
    paint_check "$work/rhi-$scale/pixels" "through the RHI on the graphics card at $scale" 1
    check "through the RHI at $scale, the frames go to the graphics card" yes \
        "$(gpu_drawn | awk '{ print ($2 > 0 && $4 == 0) ? "yes" : "no: " $0 }')"
    hook Trigger file_quit
    wait_quit "$kudari"
done

# Two screens, and one goes away while the window is open: KWin closes the window, and the
# next shortcut shows it again rather than hiding a window that is gone.
kill "$kwin" 2>/dev/null
wait "$kwin" 2>/dev/null
kwin_wayland --virtual --no-lockscreen --socket="$WAYLAND_DISPLAY" --output-count 2 --width 1280 --height 800 \
    > "$work/kwin2.log" 2>&1 &
kwin=$!
sleep 3
mkdir "$work/screens"
printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n[Window]\nKeepOpen=true\n' > "$work/screens/kudarirc"
XDG_CONFIG_HOME="$work/screens" "$binary" > "$work/screens.log" 2>&1 &
kudari=$!
started
# Calls to Kudari while the window opens: with two screens, it asks KWin for the screen
# under the pointer over the same connection, and the calls still get their answers at once,
# not the empty one that a timeout gives after a second.
: > "$work/answers"
ask_often() {
    asked=0
    while [ "$asked" -lt 60 ]; do
        before=$(date +%s%N)
        answer=$(sessions sessionIdList 2>&1) || answer=failed
        took=$((($(date +%s%N) - before) / 1000000))
        if [ "$took" -gt 500 ]; then answer="slow $took ms"; fi
        echo "$answer" >> "$work/answers"
        asked=$((asked + 1))
    done
}
ask_often &
first=$!
ask_often &
second=$!
ask_often &
third=$!
for _ in 1 2 3 4 5 6 7 8; do window toggleWindowState; sleep 0.15; done
wait "$first" "$second" "$third"
check "calls while the window opens get their answers at once" "180 0" \
    "$(wc -l < "$work/answers") $(grep -c -v '^[0-9][0-9,]*$' "$work/answers")"
grep -v '^[0-9][0-9,]*$' "$work/answers" | sort | uniq -c | sed 's/^/Error in the answers: /'
check "with two screens, the window asks KWin where the pointer is" yes \
    "$([ -e "$XDG_RUNTIME_DIR/kudari-pointer-output.js" ] && echo yes || echo no)"
window toggleWindowState; sleep 1.5
shown=$(state)
output=$(kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk '/^Output:/ { name = $3 } END { print name }')
kscreen-doctor "output.$output.disable" > /dev/null 2>&1; sleep 2
window toggleWindowState; sleep 1.5
check "the shortcut shows the window again after a screen went away" "visible=1 visible=1" "$shown $(state)"
# A setting changed just before quitting reaches the file, though its moment had not come.
hook SetSetting AutoSaveSeconds 9
hook Trigger file_quit
wait_quit "$kudari"
check "quitting after a screen went away ends the program" no "$(kill -0 "$kudari" 2>/dev/null && echo yes || echo no)"
check "a setting changed just before quitting is saved" 9 "$(sed -n 's/^AutoSaveSeconds=//p' "$work/screens/kudarirc")"

# Calls back into the controller or a terminal item while one of their methods still runs:
# each pair is in reentry.txt, and one more fails, so that a change in the order of the work
# cannot add one unnoticed. A pair no longer seen is told, to take it out there.
sed -n 's/^kudari: reentrant //p' "$work/log" "$work"/*.log 2>/dev/null | LC_ALL=C sort -u > "$work/reentries"
grep -v -e '^#' -e '^$' "$here/reentry.txt" | LC_ALL=C sort -u > "$work/reentries-known"
check "Qt calls back into Kudari only where it is known to" "" \
    "$(LC_ALL=C comm -23 "$work/reentries" "$work/reentries-known" | paste -sd ';' -)"
LC_ALL=C comm -13 "$work/reentries" "$work/reentries-known" | sed 's/^/Reentry no longer seen: /'

echo "$checks checks, $failures failed"
if [ "$failures" != 0 ]; then
    tail -n 25 "$work/kwin.log" "$work/log" 2>/dev/null | sed 's/^/Error in log: /'
fi
[ "$failures" = 0 ]
