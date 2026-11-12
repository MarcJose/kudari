#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Measures how much of the Rust code the unit tests, the GUI test and the smoke test of the
# Windows front end run, taken together.
#
#   tools/coverage.sh [--html] [--fail-under PERCENT]
#
# --html also writes a report to browse, --fail-under fails when fewer lines than that are
# covered. Needs cargo-llvm-cov with llvm-tools, and what tests/gui/run.sh needs. The C++
# bridge and the QML files are not measured.
set -eu

cd "$(dirname "$0")/.."
html=
fail_under=
while [ $# -gt 0 ]; do
    case $1 in
        --html) html=1 ;;
        --fail-under) fail_under=${2:?--fail-under needs a percentage}; shift ;;
        *) echo "Usage: tools/coverage.sh [--html] [--fail-under PERCENT]" >&2; exit 2 ;;
    esac
    shift
done

# The instrumented build gets a directory of its own, so the normal build stays as it is.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}/coverage"
eval "$(cargo llvm-cov show-env --sh 2>/dev/null)"
cargo llvm-cov clean --workspace

cargo test --locked --workspace --quiet
cargo build --locked --features test-hooks
tests/gui/run.sh "$CARGO_TARGET_DIR/debug/kudari"
cargo build --locked --package kudari-windows
tests/windows/run.sh "$CARGO_TARGET_DIR/debug/kudari-windows"

if [ -n "$html" ]; then
    cargo llvm-cov report --workspace --html --output-dir "$CARGO_TARGET_DIR/report"
    echo "Report: $CARGO_TARGET_DIR/report/html/index.html"
fi
if [ -n "$fail_under" ]; then
    cargo llvm-cov report --workspace --summary-only --fail-under-lines "$fail_under"
else
    cargo llvm-cov report --workspace --summary-only
fi
