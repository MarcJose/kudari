#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# End-to-end test: runs Kudari in a virtual KWin on a private D-Bus session and drives it
# with real input. Nothing touches the desktop or the configuration of the user.
#
#   cargo build --features test-hooks
#   tests/gui/run.sh [path/to/kudari]
#
# Needs kwin_wayland, qdbus6, wayland-scanner, a C compiler and dbus-run-session.
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
    env -u WAYLAND_DISPLAY -u DISPLAY XDG_CONFIG_HOME=/nonexistent XDG_DATA_HOME=/nonexistent \
        KUDARI_GUI_TEST=1 dbus-run-session -- "$0" "$binary" > "$report" 2>&1 < /dev/null
    status=$?
    grep -E '^(ok|FAIL|[0-9]+ checks|Error in)' "$report"
    rm -f "$report"
    exit "$status"
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
mkdir -p "$work/config" "$work/data/applications" "$work/data/kudari/keytabs" "$work/home" "$work/source/folder"
mkdir -m 700 "$work/run"
cd "$work/home" || exit 2

wayland-scanner client-header "$here/fake-input.xml" "$work/fake-input-client.h" || exit 2
wayland-scanner private-code "$here/fake-input.xml" "$work/fake-input-code.c" || exit 2
cc -O1 -I"$work" "$here/fake_input.c" "$work/fake-input-code.c" -lwayland-client -lm -o "$work/input" || exit 2

cat > "$work/data/applications/kudari-gui-test.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kudari GUI test
Exec=$work/input
NoDisplay=true
X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input
DESKTOP
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
kbuildsycoca6 >/dev/null 2>&1

# KWin needs a render device; where there is no GPU, such as in the CI, the kernel module
# vgem provides one, on which Mesa draws in software.
kwin_wayland --virtual --no-lockscreen --socket="$WAYLAND_DISPLAY" --width 1280 --height 800 > "$work/kwin.log" 2>&1 &
kwin=$!
sleep 3
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
    sleep 3
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
window() { qdbus6 "$service" /kudari/window "io.github.marcjose.kudari.$@"; }
sessions() { qdbus6 "$service" /kudari/sessions "io.github.marcjose.kudari.$@"; }
hook() { qdbus6 "$service" /kudari/test "io.github.marcjose.kudari.Test.$@"; }
input() { "$work/input" "$@"; }
# With KUDARI_GUI_GRABS set to a directory, saves images of the window there, to find where
# the checks have to click.
grab() { if [ -n "${KUDARI_GUI_GRABS:-}" ]; then hook Grab "$KUDARI_GUI_GRABS/$1"; sleep 0.5; fi; }
run() { sessions runCommand "$1" 2>/dev/null; sleep 0.6; }
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
    while [ "$tries" -lt "$1" ]; do input key 108; sleep 0.1; tries=$((tries + 1)); done
    input key 28
    sleep 1.2
}
end_input() { input key 28; sleep 0.2; input key 29 32; sleep 0.4; }

window toggleWindowState; sleep 1.5
check "the window opens" visible=1 "$(state)"
input click 600 150; sleep 0.5

run "echo blocked > $work/blocked"
check "commands over D-Bus are off by default" "" "$(cat "$work/blocked" 2>/dev/null)"
hook SetSetting AllowRemoteCommands true; sleep 0.3
run "echo allowed > $work/allowed"
check "commands over D-Bus work after opting in" "allowed|" "$(content "$work/allowed")"

run "printf 'zwei worte\n'; printf 'hallo  \n'"
hook SaveOutput 0 "$work/saved"; sleep 0.4
check "saved output is private" 600 "$(stat -c %a "$work/saved")"

hook SetSetting KeyBindings test; sleep 0.3
run "cat > $work/keys"
input key 63; sleep 0.2; end_input
check "a keytab file changes keys" "KEYTAB|" "$(content "$work/keys")"
hook SetSetting KeyBindings ""; sleep 0.2

run "sleep 300"
hook MenuValue signal 15; sleep 0.5
run "echo back > $work/signal"
check "a signal reaches the foreground program" "back|" "$(content "$work/signal")"

sessions splitTerminalLeftRight 0 >/dev/null; sleep 0.8
check "splitting adds a terminal" 0,1 "$(sessions terminalIdsForSessionId 0)"
sessions removeTerminal 1; sleep 0.8
check "closing a terminal removes it" 0 "$(sessions terminalIdsForSessionId 0)"

hook SetSetting EditorCommand "$work/editor PATH:LINE"; hook SetSetting OpenLinksByClick true; sleep 0.3
run "clear; printf 'see notes.txt:12:3 end\n'"
input click 130 8
check "a plain click opens a file link in the editor" "$work/home/notes.txt:12|" "$(content "$work/edited")"
hook SetSetting OpenLinksByClick false; hook SetSetting EditorCommand ""; sleep 0.3

run "clear; cat > $work/pasted"
hook Popup "$work/source/a.txt"; sleep 1; menu_entry 1; end_input
check "dropping pastes the quoted path" "'$work/source/a.txt' |" "$(content "$work/pasted")"
hook Popup "$work/source/a.txt"; sleep 1; menu_entry 2
check "dropping copies a file" "a|" "$(content "$work/home/a.txt")"
check "copying keeps the source" "a|" "$(content "$work/source/a.txt")"
hook Popup "$work/source/b.txt"; sleep 1; menu_entry 3
check "dropping moves a file" "b|" "$(content "$work/home/b.txt")"
check "moving removes the source" no "$([ -e "$work/source/b.txt" ] && echo yes || echo no)"
hook Popup "$work/source/c.txt"; sleep 1; menu_entry 4
check "dropping links a file" "$work/source/c.txt" "$(readlink "$work/home/c.txt")"

run "clear; seq 1 900"
hook Trigger adjust-history; sleep 1.2
input key 29 30; sleep 0.2; input key 6; input key 11; sleep 0.2; input key 28; sleep 0.6
hook SaveOutput 0 "$work/short"; sleep 0.4
check "the scrollback of a terminal can shrink" yes "$([ "$(wc -l < "$work/short")" -lt 100 ] && echo yes || echo no)"
run "clear; echo saved here"
hook Trigger file_save_as; sleep 1.5; input key 28; sleep 1
check "saving asks for a file and writes the output there" yes \
    "$(grep -q 'saved here' "$work/home/output.txt" 2>/dev/null && echo yes || echo no)"
hook Trigger options_configure_keybinding; sleep 2
check "the shortcut dialog leaves Kudari responsive" visible=1 "$(state)"
input key 1; sleep 0.6

run "clear"
hook SetSetting SilenceSeconds 1; sleep 0.2
hook Trigger toggle-session-monitor-once; sleep 0.2
sessions setSessionMonitorSilenceEnabled 0 true; sleep 0.3
check "silence counts from switching it on" true "$(sessions isSessionMonitorSilenceEnabled 0)"
sleep 2.2
check "a one-shot monitor ends after its alert" false "$(sessions isSessionMonitorSilenceEnabled 0)"

background() { hook TerminalBackground; }
light=$(background)
check "the terminal takes the colors of the theme" yes "$([ -n "$light" ] && echo yes || echo no)"
hook SetSetting Skin theme:kudari.test.dark; sleep 1
check "a fixed theme changes the terminal colors" yes "$([ "$(background)" != "$light" ] && echo yes || echo no)"
hook SetSetting Skin system; sleep 1
check "the terminal returns to the system theme" "$light" "$(background)"
cp /usr/share/color-schemes/BreezeDark.colors "$XDG_CONFIG_HOME/kdeglobals"
kwriteconfig6 --notify --file kdeglobals --group General --key KudariTest 1; sleep 1.5
check "the terminal follows a theme change while running" yes "$([ "$(background)" != "$light" ] && echo yes || echo no)"

setting() { sed -n "s/^$1=//p" "$work/config/kudarirc"; }

# The bar at the bottom: two tabs titled "~ : sh", the close button of the second at 221.
sessions addSession >/dev/null; sleep 1
grab tabs
input click 221 377; sleep 1
check "the close button of a tab closes its session" 0 "$(sessions sessionIdList)"
input drag 640 398 640 600; wait_height 600
check "the grip below the bar changes the height" 75 "$(setting Height)"
input drag 640 598 640 400; wait_height 400
check "the grip works both ways" 50 "$(setting Height)"
hook Trigger options_configure; sleep 2
grab settings
input click 700 354; sleep 0.3
input key 29 30; sleep 0.2
input key 8; sleep 0.1; input key 11; sleep 0.4
input click 700 275; sleep 0.5
check "a number stays when leaving its field" 70 "$(setting Height)"
input click 700 354; sleep 0.5
check "visiting a field does not change it" "" "$(setting Width)"
input key 29 30; sleep 0.2
input key 14; sleep 0.2
input click 700 275; sleep 0.5
check "an emptied field keeps the number" 70 "$(setting Height)"
input click 250 316; sleep 1.2
input click 700 354; sleep 0.3
input click 700 433; sleep 0.5
check "a field showing its special value stays at 0" "" "$(setting FontSize)"
input click 700 354; sleep 0.3
input key 29 30; sleep 0.2
input key 2; sleep 0.1; input key 3; sleep 0.4
input click 700 433; sleep 0.5
check "a special value can be overwritten" 12 "$(setting FontSize)"
input key 1; sleep 0.6
hook SetSetting Height 50; hook SetSetting FontSize 0; sleep 0.5

hook Trigger options_configure; sleep 2
for row in 208 244 280 316 352 388; do input click 250 "$row"; sleep 0.7; done
input key 1; sleep 0.5

# The actions of the menus and shortcuts, each checked where its effect can be seen.
active() { sessions activeSessionId; }
tabs() { qdbus6 "$service" /kudari/tabs "io.github.marcjose.kudari.$@"; }
count() { sessions terminalIdsForSessionId "$1" | tr ',' '\n' | grep -c .; }
first=$(active)
hook Trigger new-session-two-horizontal; sleep 1; left_right=$(active)
hook Trigger new-session-two-vertical; sleep 1; top_bottom=$(active)
hook Trigger new-session-quad; sleep 1.2; quad=$(active)
check "sessions open with two, two and four terminals" "2 2 4" "$(count "$left_right") $(count "$top_bottom") $(count "$quad")"
hook Trigger split-auto; sleep 0.8
hook Trigger split-left-right; sleep 0.8
hook Trigger split-top-bottom; sleep 0.8
check "the split actions add terminals" 7 "$(count "$quad")"
before=$(sessions activeTerminalId)
hook Trigger next-terminal; sleep 0.4
moved=$(sessions activeTerminalId)
hook Trigger previous-terminal; sleep 0.4
check "the terminal actions move the focus there and back" "yes $before" \
    "$([ "$moved" != "$before" ] && echo yes || echo no) $(sessions activeTerminalId)"
for side in right left top bottom; do hook Trigger "grow-terminal-$side"; sleep 0.2; done
hook Trigger close-active-terminal; sleep 0.8
check "closing the active terminal" 6 "$(count "$quad")"
hook Trigger previous-session; sleep 0.4
check "the previous session" "$top_bottom" "$(active)"
hook Trigger next-session; sleep 0.4
check "the next session" "$quad" "$(active)"
hook Trigger switch-to-session-1; sleep 0.4
check "switching to the first tab" "$first" "$(active)"
hook Trigger move-session-right; sleep 0.4
check "moving a tab to the right" "$first" "$(tabs sessionAtTab 1)"
hook Trigger move-session-left; sleep 0.4
check "moving it back" "$first" "$(tabs sessionAtTab 0)"
hook Trigger rename-session; sleep 0.6
for code in 38 24 34 31 28; do input key "$code"; sleep 0.1; done
sleep 0.4
check "renaming the tab of a session" logs "$(tabs tabTitle "$first")"
tabs setTabTitle "$first" ""
hook Toggle toggle-session-prevent-closing true; sleep 0.3
check "locking a session" false "$(sessions isSessionClosable "$first")"
hook Toggle toggle-session-prevent-closing false; sleep 0.3
hook Toggle toggle-session-keyboard-input true; sleep 0.3
check "blocking the keyboard input of a session" false "$(sessions isSessionKeyboardInputEnabled "$first")"
hook Toggle toggle-session-keyboard-input false; sleep 0.3
hook Toggle toggle-session-monitor-activity true; sleep 0.3
hook Toggle toggle-session-monitor-silence true; sleep 0.3
check "watching a session" "true true" \
    "$(sessions isSessionMonitorActivityEnabled "$first") $(sessions isSessionMonitorSilenceEnabled "$first")"
for kind in activity silence process once; do hook Toggle "toggle-session-monitor-$kind" false; done
sleep 0.3
sessions raiseSession "$quad"; sleep 0.4
hook Trigger close-session; sleep 1
check "closing the active session" "$first,$left_right,$top_bottom" "$(sessions sessionIdList)"
sessions raiseSession "$first"; sleep 0.4

hook Trigger increase-window-width; sleep 0.6
hook Trigger increase-window-height; sleep 0.6
check "the window grows" "100 60" "$(setting Width) $(setting Height)"
hook Trigger decrease-window-width; sleep 0.6
hook Trigger decrease-window-height; sleep 0.6
check "the window shrinks" "90 50" "$(setting Width) $(setting Height)"
hook Toggle keep-open false; sleep 0.3
check "the window can stop staying open" false "$(setting KeepOpen)"
hook Toggle keep-open true; sleep 0.3
hook Trigger view-full-screen; sleep 1
hook Trigger view-full-screen; sleep 1
for action in enlarge-font enlarge-font shrink-font reset-font-size; do hook Trigger "$action"; sleep 0.3; done
hook Toggle allow-mouse-tracking false; hook Toggle allow-mouse-tracking true

run "clear; seq 1 900"
hook Trigger clear-scrollback; sleep 0.6
hook SaveOutput "$(sessions activeTerminalId)" "$work/scrollback"; sleep 0.4
check "clearing the scrollback keeps only the screen" yes "$([ "$(wc -l < "$work/scrollback")" -lt 100 ] && echo yes || echo no)"
run "seq 1 30"
hook Trigger clear-history-and-reset; sleep 0.6
hook SaveOutput "$(sessions activeTerminalId)" "$work/reset"; sleep 0.4
check "resetting the terminal clears it" 0 "$(grep -cxE '[0-9]+' "$work/reset")"
run "echo gefunden"
hook Trigger edit_find; sleep 0.6
for code in 34 18 33; do input key "$code"; sleep 0.1; done
hook Trigger edit_find_next; sleep 0.2
hook Trigger edit_find_prev; sleep 0.2
input key 1; sleep 0.4
for action in edit_select_all edit_copy select-line select-mode select-mode; do hook Trigger "$action"; sleep 0.3; done

# The D-Bus interface, each method checked against its counterpart.
two_h=$(sessions addSessionTwoHorizontal); sleep 1
two_v=$(sessions addSessionTwoVertical); sleep 1
four=$(sessions addSessionQuad); sleep 1.2
check "D-Bus opens sessions with two, two and four terminals" "2 2 4" "$(count "$two_h") $(count "$two_v") $(count "$four")"
sessions splitSessionAuto "$two_h" >/dev/null; sleep 0.6
sessions splitSessionLeftRight "$two_h" >/dev/null; sleep 0.6
sessions splitSessionTopBottom "$two_h" >/dev/null; sleep 0.6
terminal=$(sessions splitTerminalTopBottom "$(sessions terminalIdsForSessionId "$two_h" | cut -d, -f1)"); sleep 0.6
check "D-Bus splits sessions and terminals" 6 "$(count "$two_h")"
check "a new terminal belongs to its session" "$two_h yes" \
    "$(sessions sessionIdForTerminalId "$terminal") $(sessions terminalIdList | tr ',' '\n' | grep -qx "$terminal" && echo yes)"
grown=""
for side in Right Left Top Bottom; do grown="$grown$(sessions "tryGrowTerminal$side" "$terminal" 10) "; done
check "growing a terminal answers with pixels or -1" yes "$(echo "$grown" | grep -qE '^(-?[0-9]+ ){4}$' && echo yes)"

sessions setTerminalKeyboardInputEnabled "$terminal" false; sleep 0.3
check "D-Bus blocks the keyboard of one terminal" "false true true" \
    "$(sessions isTerminalKeyboardInputEnabled "$terminal") $(sessions hasTerminalsWithKeyboardInputEnabled "$two_h") $(sessions hasTerminalsWithKeyboardInputDisabled "$two_h")"
sessions setSessionKeyboardInputEnabled "$two_h" true; sleep 0.3
check "D-Bus gives a session its keyboard back" true "$(sessions isTerminalKeyboardInputEnabled "$terminal")"
for kind in Activity Silence; do
    sessions "setTerminalMonitor${kind}Enabled" "$terminal" true; sleep 0.3
    check "D-Bus watches one terminal for $kind" "true true true" \
        "$(sessions "isTerminalMonitor${kind}Enabled" "$terminal") $(sessions "hasTerminalsWithMonitor${kind}Enabled" "$two_h") $(sessions "hasTerminalsWithMonitor${kind}Disabled" "$two_h")"
    sessions "setSessionMonitor${kind}Enabled" "$two_h" false; sleep 0.3
    check "D-Bus stops watching a session for $kind" false "$(sessions "isTerminalMonitor${kind}Enabled" "$terminal")"
done

sessions setSessionClosable "$two_h" false; sleep 0.3
check "a locked session is reported" true "$(sessions hasUnclosableSessions)"
sessions removeTerminal "$terminal"; sleep 0.6
check "the terminals of a locked session stay" 6 "$(count "$two_h")"
sessions setSessionClosable "$two_h" true; sleep 0.3
sessions removeTerminal "$terminal"; sleep 0.6
check "D-Bus closes a terminal" 5 "$(count "$two_h")"
for session in "$two_h" "$two_v" "$four"; do sessions removeSession "$session"; sleep 0.8; done
check "D-Bus closes sessions" no "$(sessions sessionIdList | tr ',' '\n' | grep -qxE "$two_h|$two_v|$four" && echo yes || echo no)"
sessions raiseSession "$first"; sleep 0.4

sessions runCommandInTerminal "$(sessions activeTerminalId)" "echo dbus > $work/dbus"; sleep 0.6
check "D-Bus runs a command in a given terminal" "dbus|" "$(content "$work/dbus")"
run "clear; cat > $work/typed"
hook Key A; sleep 0.2
hook InputMethod "vor" ""; sleep 0.2
hook InputMethod "" "ü"; sleep 0.2
end_input
check "keys and input methods reach the terminal" "aü|" "$(content "$work/typed")"

# The real menus, opened as a click would open them and walked with the keyboard.
hook SetSetting Width 100; hook SetSetting TabActivityColor "#ff0000"; sleep 0.8
hook Popup session; sleep 1; menu_entry 2
pair=$(active)
check "the session menu opens a session" 2 "$(count "$pair")"
hook Popup main; sleep 1; menu_entry 4
check "the main menu switches an option" false "$(setting KeepOpen)"
hook Popup main; sleep 1; menu_entry 4
hook Popup main; sleep 1; menu_entry 5; menu_entry 5
check "a submenu of the main menu sets the width" 60 "$(setting Width)"
hook SetSetting Width 100; sleep 0.8
left=$(sessions terminalIdsForSessionId "$pair" | cut -d, -f1)
hook Popup tab; sleep 1; menu_entry 7; menu_entry 1
check "the tab menu blocks the keyboard of one terminal" false "$(sessions isTerminalKeyboardInputEnabled "$left")"
sessions setSessionKeyboardInputEnabled "$pair" true; sleep 0.3
hook Popup tab; sleep 1; menu_entry 5; input key 28; sleep 1
hook Popup tab; sleep 1; menu_entry 6
first_closable=$(sessions isSessionClosable "$pair")
hook Popup tab; sleep 1; menu_entry 6
check "a tab color can be chosen and reset" "true false" "$first_closable $(sessions isSessionClosable "$pair")"
hook Trigger close-session; sleep 1; input key 1; sleep 0.6
check "a locked session asks before closing" yes "$(sessions sessionIdList | tr ',' '\n' | grep -qx "$pair" && echo yes)"
sessions setSessionClosable "$pair" true; sleep 0.3

sessions runCommandInTerminal "$left" "stty size > $work/size-before"; sleep 0.6
grab splitter
input drag 640 150 440 150; sleep 1
sessions runCommandInTerminal "$left" "stty size > $work/size-after"; sleep 0.6
check "dragging the splitter resizes the terminals" yes \
    "$([ -s "$work/size-after" ] && ! cmp -s "$work/size-before" "$work/size-after" && echo yes)"
input rclick 200 150; sleep 1; menu_entry 1
check "the terminal menu splits the terminal" 3 "$(count "$pair")"
sessions runCommandInTerminal "$left" "exit"; sleep 1
check "a terminal closes when its shell exits" 2 "$(count "$pair")"

sessions raiseSession "$first"; sleep 0.4
hook SetSetting EditorCommand "$work/editor PATH:LINE"; sleep 0.3
rm -f "$work/edited"
run "clear; printf 'see notes.txt:12:3 end\n'"
input rclick 130 8; sleep 1; menu_entry 1
check "the terminal menu opens a link" "$work/home/notes.txt:12|" "$(content "$work/edited")"
hook SetSetting EditorCommand ""; sleep 0.3

input drag 60 377 200 377; sleep 1
check "dragging a tab moves it" yes "$([ "$(tabs sessionAtTab 0)" != "$first" ] && echo yes)"

hook SetSetting ConfirmQuit true; sleep 0.3
hook Trigger file_quit; sleep 1; input key 1; sleep 0.6
check "quitting with several sessions asks first" yes "$(kill -0 "$kudari" 2>/dev/null && echo yes)"
hook SetSetting ConfirmQuit false; sleep 0.3

# What programs in the terminal ask for with control sequences.
run "clear; stty raw -echo; printf '\033[6n'; dd bs=1 count=2 of=$work/report 2>/dev/null; stty sane"
check "the terminal answers a program" "033[" "$(od -An -c "$work/report" | tr -d ' ')"
run "clear; printf '\033]52;c;%s\a' \$(printf osc | base64)"
run "clear; cat > $work/osc"
hook Trigger edit_paste; sleep 0.3; end_input
check "programs can copy to the clipboard" "osc|" "$(content "$work/osc")"
# sh has no bracketed paste, so it would run pasted lines one by one: Kudari asks first.
run "clear; printf '\033]52;c;%s\a' \$(printf 'one\ntwo\n' | base64)"
run "clear; cat > $work/lines"
hook Trigger edit_paste; sleep 1; input key 1; sleep 0.6
check "pasting several lines asks first" "" "$(cat "$work/lines")"
hook Trigger edit_paste; sleep 1; input key 28; sleep 0.6; input key 29 32; sleep 0.4
check "the lines arrive once the user agrees" "one|two|" "$(content "$work/lines")"
run "clear; cat > $work/dropped"
hook DropText "$(printf 'three\nfour\n')"; sleep 1; input key 1; sleep 0.6
check "dropping several lines of text asks first as well" "" "$(cat "$work/dropped")"
end_input
mkdir -p "$work/bin"; ln -sf "$(command -v sleep)" "$work/bin/ssh"
hook SetSetting RemoteTitleFormat "%w on %H"; sleep 0.3
run "clear; printf '\033]0;remote\a'; $work/bin/ssh 3"
sleep 1
check "a program sets the title of an ssh tab" "remote on 3" "$(tabs tabTitle "$first")"
sleep 2
run "clear; stty raw -echo; printf '\033[?1000h\033[?1006h'; timeout 2 cat > $work/mouse; printf '\033[?1000l\033[?1006l'; stty sane"
input click 130 8; input wheel 130 8 15; sleep 2
check "programs that ask get clicks and the wheel" yes \
    "$(tr '\033' E < "$work/mouse" | grep -qE '^E\[<0;[0-9]+;1ME\[<0;[0-9]+;1mE\[<6[45];' && echo yes)"
run "clear; stty raw -echo; printf '\033[?1049h'; timeout 2 cat > $work/arrows; printf '\033[?1049l'; stty sane"
input wheel 130 8 15; sleep 2
check "the wheel sends arrow keys to full-screen programs" yes \
    "$(tr '\033' E < "$work/arrows" | grep -qE '^(EO[AB]){3}$' && echo yes)"

hook Toggle toggle-session-monitor-once true; sleep 0.2
sessions setSessionMonitorActivityEnabled "$first" true; sleep 0.3
watched=$(sessions activeTerminalId)
sessions raiseSession "$pair"; sleep 0.4
sessions runCommandInTerminal "$watched" "echo activity"; sleep 1
check "a one-shot activity monitor ends after its alert" false "$(sessions isSessionMonitorActivityEnabled "$first")"
sessions raiseSession "$first"; sleep 0.4
hook Toggle toggle-session-monitor-process true; sleep 0.2
run "sleep 1"; sleep 1.5
for mode in visual none notify; do hook SetSetting BellMode "$mode"; sleep 0.3; run "printf '\a'"; done
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
receive() {
    local line
    while IFS= read -r -t 60 line <&8; do
        case $line in *'"id":0'*) ;; *) echo "$line" | result; return ;; esac
    done
}
agent() { send "$1" "$2"; receive; }
agent_once() { request 1 "$1" "$2" | "$binary" mcp 2>> "$work/mcp.log" | result; }
question() {
    local text
    for _ in $(seq 50); do
        text=$(hook AgentQuestion)
        [ -n "$text" ] && { echo "$text"; return; }
        sleep 0.1
    done
}
field() { python3 -c "import json, sys; print(json.load(sys.stdin)['$1'])"; }
refused() { grep -q "^error: $1" && echo yes; }

start_agent
check "agents may not read by default" yes "$(agent list_terminals '{}' | refused 'Reading terminals is switched off')"
check "the switched off interface shows no access" false "$(hook AgentAccess)"
hook SetSetting AllowAgentReading true; sleep 0.3
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
check "agents may not type by default" yes "$(agent type_text '{"text": "ls"}' | refused 'Controlling terminals is switched off')"
hook SetSetting AllowAgentControl true; sleep 0.3
check "more rights end the access granted before" false "$(hook AgentAccess)"
send open_session '{"title": "Agent"}'
check "the question names the new rights" yes "$(question | grep -q 'read and control' && echo yes)"
hook AnswerAgent true
opened=$(receive)
own=$(echo "$opened" | field session)
mine=$(echo "$opened" | field terminal)
check "an agent opens a session of its own" Agent "$(tabs tabTitle "$own")"
agent type_text "{\"terminal\": $mine, \"text\": \"echo typed > $work/agent-typed\", \"execute\": true}" > "$work/answer"
sleep 0.6
check "the command of an agent waits for Return" "no yes" \
    "$([ -e "$work/agent-typed" ] && echo yes || echo no) $(grep -q 'presses Return' "$work/answer" && echo yes)"
input key 28
check "Return runs the command of an agent" "typed|" "$(content "$work/agent-typed")"
check "with confirmation an agent cannot press escape" yes \
    "$(agent send_key "{\"terminal\": $mine, \"key\": \"escape\"}" | refused 'escape could form')"
agent type_text "{\"terminal\": $mine, \"text\": \"read -r reply; echo got-\$reply > $work/agent-read\", \"execute\": true}" > /dev/null
sleep 0.4; input key 28; sleep 0.5
check "a script reading input is no prompt for an agent" yes \
    "$(agent type_text "{\"terminal\": $mine, \"text\": \"y\"}" | refused 'The shell is not editing')"
input key 28; sleep 0.5
check "the script got nothing from the agent" "got-|" "$(content "$work/agent-read")"
hook SetSetting ConfirmAgentCommands false; sleep 0.3
agent type_text "{\"terminal\": $mine, \"text\": \"echo ran-by-agent\", \"execute\": true}" > /dev/null; sleep 0.6
check "without confirmation an agent runs commands" yes \
    "$(agent read_terminal "{\"terminal\": $mine, \"lines\": 5}" | grep -qx 'ran-by-agent' && echo yes)"
agent type_text "{\"terminal\": $mine, \"text\": \"sleep 30\", \"execute\": true}" > /dev/null; sleep 0.6
hook SetSetting ConfirmAgentCommands true; sleep 0.3
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
hook SetSetting AllowAgentReading false; sleep 0.3
check "reading can be switched off" yes "$(agent read_terminal '{}' | refused 'Reading terminals is switched off')"
stop_agent
hook SetSetting AllowAgentControl false
sessions raiseSession "$first"; sleep 0.4

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
tries=0
while kill -0 "$kudari" 2>/dev/null && [ "$tries" -lt 50 ]; do sleep 0.1; tries=$((tries + 1)); done
check "quitting ends the program" no "$(kill -0 "$kudari" 2>/dev/null && echo yes || echo no)"
# Skins used to be images made at runtime; the bars are drawn now.
check "no skin directory is created" "" "$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type d -name 'kudari-*' -print -quit)"

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

echo "$checks checks, $failures failed"
if [ "$failures" != 0 ]; then
    tail -n 25 "$work/kwin.log" "$work/log" 2>/dev/null | sed 's/^/Error in log: /'
fi
[ "$failures" = 0 ]
