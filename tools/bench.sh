#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Measures how fast terminals take in output and, for Kudari, how long Qt Quick needs for
# each frame meanwhile, to compare terminals and a change to the drawing before and after:
#
#   tools/bench.sh [--binary PATH] [--runs N] [--warmup N] [--seed N] [--loads LOAD,...]
#                  [--set GROUP.KEY=VALUE] [--scale N] [--detail] [--keep] [--keep-env]
#                  [TERMINAL...]
#
# The terminals are kudari, yakuake, konsole, alacritty, kitty, ghostty, contour and xterm;
# without any, every one of them that is installed. Kudari is the installed one, or else the
# release build (cargo build --release); --binary names another. Each run measures every
# terminal once, in an order shuffled for each run; --seed repeats an order. Five runs count
# by default, after one warm-up run that does not (--warmup). The report shows for each load
# the median of the runs, its spread, a 95 % interval of the median, the fastest and the
# slowest run and the runs far from the others; the CPU time of each terminal during the
# load and its busiest threads, and the memory after each load; for Kudari, how often and
# how fast Qt Quick drew meanwhile, and with --detail the times of its frames in detail.
# --loads picks some of plain, color, frames and seq, or idle: ten seconds without output,
# for the CPU time and the wakeups of a terminal that waits. --set gives Kudari a setting
# for the measurement, such as --set Terminal.BlinkingCursor=true, once for each. --scale
# scales the screen as for a HiDPI display, and --keep keeps the data and the logs of the
# terminals.
#
# The report starts with what was measured on what: the build of Kudari with its hash and
# origin, the processor and its clock, the versions of Qt, KWin and Mesa. It warns about a
# busy computer, a clock that is not set for performance, and a release build that is newer
# than the Kudari measured. Variables of Qt, Mesa, KWin and the dynamic loader (QT_*, QSG_*,
# QML_*, MESA_*, LIBGL_*, __GL_*, KWIN_*, LD_*) would change the measurement silently; they
# are removed, and --keep-env keeps them.
#
# All of them run full screen in a virtual KWin of 2560x1440, xterm through Xwayland, on a
# D-Bus and with settings of their own, so the desktop and its settings stay out of it. The
# loads are plain text, text in 256 colors, 300 full screens with colors of their own in
# every cell, as a busy program draws them, and the numbers up to 2,000,000. Each terminal
# keeps its own font, so the number of cells differs between them. The graphics card of the
# computer draws, so compare only runs on the same computer. Needs kwin_wayland with
# Xwayland, qdbus6, dbus-run-session and python3, and kscreen-doctor for --scale.
set -eu
tools=$(cd "$(dirname "$0")" && pwd)
all="kudari yakuake konsole alacritty kitty ghostty contour xterm"
die() { echo "bench: $*" >&2; exit 1; }
stamp() { date +%Y-%m-%dT%H:%M:%S.%3N; }

# As the shell of the terminal under test: waits until the window is up, then times each load.
if [ "${KUDARI_BENCH_ROLE:-}" = workload ]; then
    while [ ! -e "$KUDARI_BENCH_WORK/go" ]; do sleep 0.2; done
    terminal_pid=$(cat "$KUDARI_BENCH_WORK/pid")
    sleep 1
    size=$(stty size)
    rows=${size% *} cols=${size#* }
    python3 "$tools/bench.py" frames "$KUDARI_BENCH_WORK" "$rows" "$cols"
    echo "size $rows $cols" > "$KUDARI_BENCH_OUT"
    clear
    for load in $KUDARI_BENCH_LOADS; do
        sleep 1
        # The CPU time of the threads of the terminal, just outside the time of the load.
        python3 "$tools/bench.py" cpu "$KUDARI_BENCH_CPU" "$terminal_pid" "$load" start
        start=$(stamp)
        case $load in
            frames) cat "$KUDARI_BENCH_WORK/frames-${rows}x$cols.txt" ;;
            seq) seq 1 2000000 ;;
            idle) sleep 10 ;;
            *) cat "$KUDARI_BENCH_WORK/$load.txt" ;;
        esac
        end=$(stamp)
        python3 "$tools/bench.py" cpu "$KUDARI_BENCH_CPU" "$terminal_pid" "$load" end
        sleep 1
        echo "$load $start $end" >> "$KUDARI_BENCH_OUT"
        clear
    done
    echo "done" >> "$KUDARI_BENCH_OUT"
    exec sleep 100000
fi

# As the session of the virtual KWin, which gives it WAYLAND_DISPLAY and DISPLAY: starts each
# terminal in turn and waits for its loads. Messages go to progress.log, failures to failed.
if [ "${KUDARI_BENCH_ROLE:-}" = session ]; then
    work=$KUDARI_BENCH_WORK
    say() { echo "$*" >> "$work/progress.log"; }
    fail() { echo "$*" > "$work/failed"; exit 1; }
    # Any other end before the last run, as a command that fails under set -e, is a failure
    # as well, not a measurement with runs missing.
    trap '[ -e "$work/complete" ] || [ -e "$work/failed" ] || echo "the measurement ended early; see the logs in $work" > "$work/failed"' EXIT
    # Drop-down terminals on the session bus, which a call to their name would start instead.
    on_bus() {
        tries=0
        until qdbus6 2> /dev/null | grep -qx " *$1"; do
            tries=$((tries + 1))
            if [ "$tries" -gt 100 ] || ! kill -0 "$pid" 2> /dev/null; then
                fail "$terminal did not start; see $log"
            fi
            sleep 0.1
        done
        sleep 2
        # A terminal that ended meanwhile would leave the name to the bus, which starts the
        # installed one in its place.
        owner=$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.GetConnectionUnixProcessID "$1" 2> /dev/null || true)
        [ "$owner" = "$pid" ] || fail "$terminal ended after it started; see $log"
    }
    # KWin's --scale only enlarges the virtual screen; the scale that programs see is a setting
    # of the screen.
    if [ "$KUDARI_BENCH_SCALE" != 1 ]; then
        output=$(kscreen-doctor -o 2> /dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk '/^Output:/ { print $3; exit }')
        kscreen-doctor "output.$output.scale.$KUDARI_BENCH_SCALE" > /dev/null 2>&1 || fail "could not scale the screen to $KUDARI_BENCH_SCALE"
        sleep 1
    fi
    # The warm-up runs come first, as w1, w2 and so on; the report leaves them out.
    round=1
    while [ "$round" -le $((KUDARI_BENCH_WARMUP + KUDARI_BENCH_RUNS)) ]; do
        if [ "$round" -le "$KUDARI_BENCH_WARMUP" ]; then
            run=w$round title="Warm-up run $round of $KUDARI_BENCH_WARMUP"
        else
            run=$((round - KUDARI_BENCH_WARMUP)) title="Run $((round - KUDARI_BENCH_WARMUP)) of $KUDARI_BENCH_RUNS"
        fi
        # A terminal that always comes first or after the same one would carry what the order
        # does to it, as a cold cache or the heat of the one before, into every run.
        # shellcheck disable=SC2086 # the terminals are words of their own
        for terminal in $(python3 "$tools/bench.py" order "$run" --seed "$KUDARI_BENCH_SEED" $KUDARI_BENCH_TERMINALS); do
            say "$title: $terminal"
            rm -f "$work/go"
            log="$work/$terminal-$run.log"
            export XDG_CONFIG_HOME="$work/config-$terminal-$run" KUDARI_BENCH_OUT="$work/$terminal-$run.txt"
            export KUDARI_BENCH_CPU="$work/$terminal-$run.cpu"
            export XDG_DATA_HOME="$work/share-$terminal-$run" KUDARI_BENCH_ROLE=workload
            mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME"
            shell="$tools/bench.sh"
            case $terminal in
                kudari)
                    printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n[Window]\nWidth=100\nHeight=100\n' \
                        > "$XDG_CONFIG_HOME/kudarirc"
                    cat "$work/kudarirc-settings" >> "$XDG_CONFIG_HOME/kudarirc"
                    # The frame times of Qt Quick, with the time of day to match them to the loads.
                    SHELL=$shell QSG_RENDER_TIMING=1 \
                        QT_MESSAGE_PATTERN='%{time yyyy-MM-ddTHH:mm:ss.zzz} %{category} %{message}' \
                        "$KUDARI_BENCH_BINARY" > "$log" 2>&1 &
                    pid=$!
                    on_bus io.github.marcjose.kudari
                    qdbus6 io.github.marcjose.kudari /kudari/window io.github.marcjose.kudari.toggleWindowState > /dev/null ||
                        fail "could not open $terminal; see $log"
                    ;;
                yakuake)
                    printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n[Window]\nWidth=100\nHeight=100\n[Animation]\nFrames=0\n' \
                        > "$XDG_CONFIG_HOME/yakuakerc"
                    SHELL=$shell yakuake > "$log" 2>&1 &
                    pid=$!
                    on_bus org.kde.yakuake
                    qdbus6 org.kde.yakuake /yakuake/window toggleWindowState > /dev/null ||
                        fail "could not open $terminal; see $log"
                    ;;
                konsole) konsole --nofork --hide-menubar --hide-tabbar -e "$shell" > "$log" 2>&1 & pid=$! ;;
                alacritty) alacritty -e "$shell" > "$log" 2>&1 & pid=$! ;;
                kitty) kitty -o confirm_os_window_close=0 "$shell" > "$log" 2>&1 & pid=$! ;;
                ghostty)
                    ghostty --gtk-single-instance=false --confirm-close-surface=false -e "$shell" > "$log" 2>&1 &
                    pid=$!
                    ;;
                contour) contour terminal execute "$shell" > "$log" 2>&1 & pid=$! ;;
                xterm) xterm -e "$shell" > "$log" 2>&1 & pid=$! ;;
            esac
            # Time for the window to come up and to go full screen.
            sleep 3
            echo "$pid" > "$work/pid"
            touch "$work/go"
            tries=0
            until grep -q '^done' "$KUDARI_BENCH_OUT" 2> /dev/null; do
                tries=$((tries + 1))
                if [ "$tries" -gt 600 ] || ! kill -0 "$pid" 2> /dev/null; then
                    kill "$pid" 2> /dev/null || true
                    fail "$terminal did not finish its loads; see $log"
                fi
                sleep 0.5
            done
            kill "$pid" 2> /dev/null || true
            wait "$pid" 2> /dev/null || true
            sleep 1
        done
        round=$((round + 1))
    done
    : > "$work/complete"
    exit 0
fi

binary=
runs=5
scale=1
loads="plain color frames seq"
keep=
keep_env=
warmup=1
settings=
seed=
detail=
terminals=
while [ $# -gt 0 ]; do
    case $1 in
        --binary) binary=$2; shift 2 ;;
        --runs) runs=$2; shift 2 ;;
        --keep) keep=1; shift ;;
        --scale) scale=$2; shift 2 ;;
        --loads)
            loads=$(echo "$2" | tr ',' ' ')
            for load in $loads; do
                case $load in plain | color | frames | seq | idle) ;; *) die "no load $load; there are plain, color, frames, seq and idle" ;; esac
            done
            shift 2
            ;;
        --keep-env) keep_env=1; shift ;;
        --warmup) warmup=$2; shift 2 ;;
        --seed) seed=$2; shift 2 ;;
        --detail) detail=1; shift ;;
        --set)
            case $2 in
                [A-Za-z]*.[A-Za-z]*=*) ;;
                *) die "--set needs GROUP.KEY=VALUE, such as Terminal.BlinkingCursor=true" ;;
            esac
            [ "$(printf '%s' "$2" | wc -l)" -eq 0 ] || die "--set takes one line"
            settings="$settings$(printf '\n%s' "$2")"
            shift 2
            ;;
        -h | --help) sed -n '4,41s/^# \{0,1\}//p' "$0"; exit 0 ;;
        *)
            case " $all " in
                *" $1 "*) terminals="$terminals $1"; shift ;;
                *) die "usage: tools/bench.sh [--binary PATH] [--runs N] [--warmup N] [--seed N] [--loads LOAD,...] [--scale N] [--detail] [--keep] [--keep-env] [TERMINAL...]; the terminals are: $all" ;;
            esac
            ;;
    esac
done
case $runs in '' | *[!0-9]* | 0) die "--runs needs a number of 1 or more" ;; esac
case $warmup in '' | *[!0-9]*) die "--warmup needs a number of 0 or more" ;; esac
[ -n "$seed" ] || seed=$(od -An -N2 -tu2 /dev/urandom | tr -d ' ')
case $seed in *[!0-9]*) die "--seed needs a number" ;; esac
case $scale in '' | *[!0-9.]* | *.*.* | .* | *.) die "--scale needs a number such as 2 or 1.5" ;; esac
local_build="${CARGO_TARGET_DIR:-$tools/../target}/release/kudari"
if [ -z "$binary" ]; then
    binary=$(command -v kudari || echo "$local_build")
fi
if [ -z "$terminals" ]; then
    for terminal in $all; do
        if [ "$terminal" = kudari ]; then
            [ -x "$binary" ] && terminals="$terminals kudari"
        elif command -v "$terminal" > /dev/null; then
            terminals="$terminals $terminal"
        fi
    done
fi
for terminal in $terminals; do
    if [ "$terminal" = kudari ]; then
        [ -x "$binary" ] || die "no Kudari at $binary; install it, build it with cargo build --release, or name one with --binary"
    else
        command -v "$terminal" > /dev/null || die "$terminal is not installed"
    fi
done
[ -n "$terminals" ] || die "none of the terminals is installed: $all"

# Outside: a D-Bus session of its own, which ends with the measurement.
if [ -z "${KUDARI_BENCH_WORK:-}" ]; then
    for program in kwin_wayland Xwayland qdbus6 dbus-run-session python3; do
        command -v "$program" > /dev/null || die "needs $program"
    done
    [ "$scale" = 1 ] || command -v kscreen-doctor > /dev/null || die "--scale needs kscreen-doctor"
    # Some 400 MB of data and logs, kept only when the measurement fails.
    work=$(mktemp -d "${TMPDIR:-/tmp}/kudari-bench.XXXXXX")
    trap 'rm -rf "$work"; exit 1' INT TERM
    echo "Terminals:$terminals; Kudari: $binary; $runs runs; scale $scale"
    # The variables that would change Qt, the drivers or what gets loaded, by name only.
    variables=$(env | grep -oE '^(QT|QSG|QML|MESA|LIBGL|__GL|KWIN|LD)_[A-Za-z0-9_]*=' | tr -d = | sort -u | tr '\n' ' ')
    unset_variables=
    if [ -z "$keep_env" ]; then
        for variable in $variables; do unset_variables="$unset_variables -u $variable"; done
    fi
    case " $terminals " in *" kudari "*) measured=$binary ;; *) measured= ;; esac
    # The settings for Kudari, one a line as given, and as lines of its kudarirc; KConfig
    # joins groups named twice. A value may hold spaces, so the lines split only at newlines.
    printf '%s\n' "$settings" | sed '/^$/d' > "$work/settings"
    while IFS= read -r setting; do
        printf '[%s]\n%s\n' "${setting%%.*}" "${setting#*.}"
    done < "$work/settings" > "$work/kudarirc-settings"
    # shellcheck disable=SC2086 # the Kudari measured is one word or none
    python3 "$tools/bench.py" environment "$work" $measured --local "$local_build" \
        --variables "$(echo $variables | tr ' ' ',')" ${keep_env:+--keep-env} \
        --runs "$runs" --warmup "$warmup" --seed "$seed"
    echo "Making the data ..."
    python3 "$tools/bench.py" data "$work"
    # The bus and the portals it starts complain about what the virtual session lacks; that
    # goes to a file, and the measurement itself writes to file descriptor 3.
    # Qt's own render loop and backend, whatever the environment sets. A Kudari or Yakuake
    # that the bus would start finds neither the settings nor the services of the user.
    # shellcheck disable=SC2086 # the terminals and the variables are words of their own
    if env -u WAYLAND_DISPLAY -u DISPLAY -u QSG_RENDER_LOOP -u QT_QUICK_BACKEND -u QSG_RHI_BACKEND $unset_variables \
        XDG_CONFIG_HOME=/nonexistent XDG_DATA_HOME=/nonexistent KUDARI_BENCH_WORK="$work" \
        dbus-run-session -- "$0" --binary "$binary" --runs "$runs" --warmup "$warmup" --seed "$seed" --scale "$scale" \
        --loads "$(echo $loads | tr " " ",")" $terminals \
        < /dev/null 3>&2 2> "$work/dbus.log"; then
        python3 "$tools/bench.py" report "$work" $terminals ${detail:+--detail}
        if [ -n "$keep" ]; then echo "The data and the logs are in $work"; else rm -rf "$work"; fi
    else
        die "the measurement failed; its files are in $work"
    fi
    exit
fi

# Inside the D-Bus session: the virtual KWin, which runs this script as its session.
work=$KUDARI_BENCH_WORK
exec 2>&3 3>&-
# The path of a Wayland socket stays below 108 bytes only in a short directory.
base=${TMPDIR:-/tmp}
[ "${#base}" -le 40 ] || base=/tmp
XDG_RUNTIME_DIR=$(mktemp -d "$base/kudari-bench-run.XXXXXX")
export XDG_RUNTIME_DIR XDG_CACHE_HOME="$work/cache"
# A window rule of KWin puts every normal window full screen, those of terminals without an
# option for it as well. Kudari and Yakuake fill the screen through their own settings.
export XDG_CONFIG_HOME="$work/kwin"
mkdir -p "$XDG_CONFIG_HOME"
printf '[General]\ncount=1\nrules=1\n\n[1]\nDescription=full screen\nfullscreen=true\nfullscreenrule=2\ntypes=1\nwmclassmatch=0\n' \
    > "$XDG_CONFIG_HOME/kwinrulesrc"
export QT_QPA_PLATFORM=wayland QT_FORCE_STDERR_LOGGING=1
export KUDARI_BENCH_RUNS="$runs" KUDARI_BENCH_TERMINALS="$terminals" KUDARI_BENCH_BINARY="$binary"
export KUDARI_BENCH_LOADS="$loads" KUDARI_BENCH_SCALE="$scale" KUDARI_BENCH_WARMUP="$warmup" KUDARI_BENCH_SEED="$seed"
: > "$work/progress.log"
tail -f "$work/progress.log" &
progress=$!
trap 'kill "$progress" 2> /dev/null; rm -rf "$XDG_RUNTIME_DIR"' EXIT
KUDARI_BENCH_ROLE=session kwin_wayland --virtual --xwayland --no-lockscreen --socket=kudari-bench \
    --width 2560 --height 1440 --scale "$scale" --exit-with-session "$tools/bench.sh" > "$work/kwin.log" 2>&1 || true
sleep 0.5
if [ -e "$work/failed" ]; then
    die "$(cat "$work/failed")"
fi
grep -q . "$work/progress.log" || die "KWin did not start the measurement; see $work/kwin.log"
[ -e "$work/complete" ] || die "the measurement ended early; its files are in $work"
