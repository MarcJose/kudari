#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# The scenarios of `tools/bench.sh --scenarios`: what Kudari costs when it starts, shows and
# waits, rather than how fast it takes in output. bench.sh runs this as the session of its
# virtual KWin, on its own D-Bus and with settings of its own, once for each run:
#
#   startup     from the start of the process to its name on the bus and to its first frame,
#               with the phases it marks itself through KUDARI_TIMING
#   show        from toggleWindowState to the next frame: the first time and the times after
#   second      a second start, which only toggles the running Kudari, to its end
#   version     kudari --version to its end
#   idle        ten seconds each of CPU time, wakeups and frames: never shown, shown, hidden,
#               a blinking cursor, one that rests, a fading one, output in a tab in the
#               background, and output while the window is hidden
#   memory      after the start, with 5 and with 10 tabs, and after closing them again
#
# Every time is the clock of the computer in seconds since 1970, in events-RUN.txt, next to
# the logs of each Kudari; `tools/bench.py scenarios` makes the report.
set -eu

tools=$(cd "$(dirname "$0")" && pwd)
work=$KUDARI_BENCH_WORK
binary=$KUDARI_BENCH_BINARY
service=io.github.marcjose.kudari
say() { echo "$*" >> "$work/progress.log"; }
fail() { echo "$*" > "$work/failed"; exit 1; }
now() { date +%s.%N; }
event() { echo "$*" >> "$events"; }
window() { qdbus6 "$service" /kudari/window "$service.$1" > /dev/null; }
sessions() { method=$1; shift; qdbus6 "$service" /kudari/sessions "$service.$method" "$@"; }
sample() { python3 "$tools/bench.py" cpu "$cpu" "$pid" "$1" "$2"; }
trap '[ -e "$work/complete" ] || [ -e "$work/failed" ] || echo "the scenarios ended early; see the logs in $work" > "$work/failed"' EXIT

# Starts a Kudari named NAME with the settings given, one GROUP.KEY=VALUE each, on top of
# those of --set, and waits until it owns its name on the bus.
start() { # name, settings...
    name=$1
    shift
    config="$work/config-$name-$run"
    mkdir -p "$config"
    {
        printf '[Dialogs]\nFirstRun=false\nConfirmQuit=false\n[Window]\nWidth=100\nHeight=100\n'
        for setting in "$@"; do printf '[%s]\n%s\n' "${setting%%.*}" "${setting#*.}"; done
        cat "$work/kudarirc-settings"
    } > "$config/kudarirc"
    log="$work/$name-$run.log"
    cpu="$work/$name-$run.cpu"
    event spawn "$name" "$(now)"
    XDG_CONFIG_HOME=$config SHELL=/bin/sh QSG_RENDER_TIMING=1 KUDARI_TIMING=1 \
        QT_MESSAGE_PATTERN='%{time yyyy-MM-ddTHH:mm:ss.zzz} %{category} %{message}' \
        "$binary" > "$log" 2>&1 &
    pid=$!
    tries=0
    until qdbus6 2> /dev/null | grep -qx " *$service"; do
        tries=$((tries + 1))
        if [ "$tries" -gt 1000 ] || ! kill -0 "$pid" 2> /dev/null; then
            fail "Kudari did not start; see $log"
        fi
        sleep 0.01
    done
    event bus "$name" "$(now)"
}

stop() {
    kill "$pid" 2> /dev/null || true
    tries=0
    while kill -0 "$pid" 2> /dev/null && [ "$tries" -lt 50 ]; do sleep 0.1; tries=$((tries + 1)); done
    kill -9 "$pid" 2> /dev/null || true
    wait "$pid" 2> /dev/null || true
    sleep 1
}

# Ten seconds of a scenario, between two samples of the threads of Kudari.
idle() { # scenario
    sample "$1" start
    event idle "$1" "$(now)"
    sleep 10
    event idle-end "$1" "$(now)"
    sample "$1" end
}

# Toggles the window and notes when, as show or hide.
toggle() { # show or hide, name
    event "$1" "$2" "$(now)"
    window toggleWindowState
}

round=1
while [ "$round" -le $((KUDARI_BENCH_WARMUP + KUDARI_BENCH_RUNS)) ]; do
    if [ "$round" -le "$KUDARI_BENCH_WARMUP" ]; then
        run=w$round title="Warm-up run $round of $KUDARI_BENCH_WARMUP"
    else
        run=$((round - KUDARI_BENCH_WARMUP)) title="Run $((round - KUDARI_BENCH_WARMUP)) of $KUDARI_BENCH_RUNS"
    fi
    events="$work/events-$run.txt"
    : > "$events"

    say "$title: a Kudari that was never shown, and its first showing"
    start hidden
    sleep 3
    idle never-shown
    toggle show hidden
    sleep 2
    stop

    say "$title: start, showing, second starts, memory and output nobody sees"
    # The output in the background comes as a command over D-Bus, which needs this.
    start main Behavior.OpenAfterStart=true Security.AllowRemoteCommands=true
    sleep 3
    idle shown
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        toggle hide main
        sleep 0.7
        toggle show main
        sleep 0.7
    done
    toggle hide main
    sleep 1
    idle hidden
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        event second-start main "$(now)"
        "$binary" > /dev/null 2>&1
        event second-end main "$(now)"
        sleep 0.7
    done
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        event version-start main "$(now)"
        "$binary" --version > /dev/null 2>&1
        event version-end main "$(now)"
    done
    # The second starts toggled the window ten times: it is hidden again.
    toggle show main
    sleep 1
    sample memory-1 start
    sample memory-1 end
    first=$(sessions activeSessionId)
    for _ in 2 3 4 5; do sessions addSession > /dev/null; sleep 0.5; done
    sleep 1
    sample memory-5 start
    sample memory-5 end
    for _ in 6 7 8 9 10; do sessions addSession > /dev/null; sleep 0.5; done
    sleep 1
    sample memory-10 start
    sample memory-10 end
    for session in $(sessions sessionIdList | tr ',' ' '); do
        [ "$session" = "$first" ] || sessions removeSession "$session" > /dev/null
        sleep 0.2
    done
    sleep 2
    sample memory-closed start
    sample memory-closed end
    # Ten lines a second in a tab in the background, then with the window hidden.
    behind=$(sessions addSession)
    sessions raiseSession "$first" > /dev/null
    sleep 1
    terminal=$(sessions terminalIdsForSessionId "$behind" | cut -d, -f1)
    sessions runCommandInTerminal "$terminal" 'while :; do date; sleep 0.1; done' > /dev/null
    sleep 2
    idle output-behind
    toggle hide main
    sleep 1
    idle output-hidden
    stop

    say "$title: a blinking cursor, and one that rests"
    start blinking Behavior.OpenAfterStart=true Terminal.BlinkingCursor=true
    sleep 2
    idle blinking
    # It rests after 15 seconds without input or output.
    sleep 5
    idle rested
    stop

    say "$title: a fading cursor"
    start fading Behavior.OpenAfterStart=true Terminal.BlinkingCursor=true Terminal.AnimatedCursor=true
    sleep 2
    idle fading
    stop
    round=$((round + 1))
done
: > "$work/complete"
