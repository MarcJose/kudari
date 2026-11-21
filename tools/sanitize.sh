#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Runs the GUI test with the C++ of Kudari checked while it runs: the bridges to Qt and KDE
# and the code that cxx-qt makes, built with AddressSanitizer and UndefinedBehaviorSanitizer.
# They tell an access outside of memory, a use after free or undefined behaviour with where
# it happened, and end Kudari, which fails the test.
#
#   tools/sanitize.sh
#
# The Rust code is not instrumented; the bridge of the Windows front end is not built. The
# build gets a directory of its own and is never one to hand out.
set -eu
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}/sanitize"
# With the target named, the flags of rustc reach only Kudari, not the build scripts and
# the macros that rustc itself loads, which the runtime of the sanitizers would stop.
target=$(rustc -vV | sed -n 's/^host: //p')
sanitizers="-fsanitize=address,undefined"
export CXXFLAGS="${CXXFLAGS:-} $sanitizers -fno-sanitize-recover=undefined -fno-omit-frame-pointer -g"
# Rust links without the default libraries, so the runtimes are named. The one of
# AddressSanitizer has to be the first library Kudari loads: a linker of its own names them
# before everything else, so that neither Kudari needs them preloaded nor do the shells in
# its terminals get them.
mkdir -p "$CARGO_TARGET_DIR"
linker=$CARGO_TARGET_DIR/linker
printf '#!/bin/sh\nexec %s -Wl,--push-state,--no-as-needed -lasan -lubsan -Wl,--pop-state "$@" %s\n' \
    "${CC:-cc}" "$sanitizers" > "$linker"
chmod +x "$linker"
export RUSTFLAGS="${RUSTFLAGS:-} -C linker=$linker"
# Qt, Mesa and KDE keep memory until the end on purpose; a report of each would hide the rest.
export ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0:abort_on_error=1:halt_on_error=1}"
export UBSAN_OPTIONS="${UBSAN_OPTIONS:-print_stacktrace=1:halt_on_error=1}"
cargo build --locked --features test-hooks --target "$target"
# Kudari runs two to three times slower under the sanitizers.
KUDARI_GUI_TIMEOUT="${KUDARI_GUI_TIMEOUT:-45m}" tests/gui/run.sh "$CARGO_TARGET_DIR/$target/debug/kudari"
