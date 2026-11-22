#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Everything that has to pass before a commit. Used by the CI as well.
set -eu

cd "$(dirname "$0")/.."
target=${CARGO_TARGET_DIR:-target}

# The tools of the checks, before any of them runs: a missing one would stop them halfway
# with no more than "command not found". CONTRIBUTING.md names where they come from.
# qmllint lies among the tools of Qt, which qmake names, the one that cxx-qt builds with.
qmllint=$("${QMAKE:-qmake6}" -query QT_HOST_BINS 2>/dev/null)/qmllint
[ -x "$qmllint" ] || qmllint=/usr/lib/qt6/bin/qmllint
missing=""
for tool in namcap vale g++ checkXML6 xsltproc "$qmllint"; do
    command -v "$tool" > /dev/null || missing="$missing $tool"
done
if [ -n "$missing" ]; then
    echo "The checks need:$missing; see Setting up in CONTRIBUTING.md." >&2
    exit 1
fi

echo "== Version"
python3 tools/release.py --check

echo "== Third-party licenses"
python3 tools/third_party_licenses.py --check

echo "== Generated tables"
python3 tools/box_drawing.py --check
# The shaders of frames on the graphics card, where Qt Shader Tools are there to bake them.
if command -v qsb6 >/dev/null || command -v qsb >/dev/null || [ -x /usr/lib/qt6/bin/qsb ]; then
    tools/shaders.sh --check
fi

echo "== Translations"
python3 tools/translations.py --check --strict --quiet

echo "== Tools"
python3 tests/bench_test.py

echo "== Packages for the AUR"
python3 tools/aur.py --check
for pkgbuild in packaging/aur/*/PKGBUILD; do
    findings=$(namcap "$pkgbuild")
    if [ -n "$findings" ]; then
        echo "$findings"
        exit 1
    fi
done

echo "== Formatting"
cargo fmt --all --check

echo "== Rust lints"
cargo clippy --locked --workspace --all-targets --features test-hooks -- -D warnings

echo "== Unit tests"
# With the features of the lints: the build scripts of cxx-qt, which run moc and the QML
# compiler, then run once and not again for a second set. The counters come with them, and
# so do their tests. The CI builds without them, as the release does.
cargo test --locked --workspace --features test-hooks
# The parser in third_party/vte is not part of the workspace; its tests compare the ways
# through the ground state that Kudari changed with the original.
cargo test --locked -p kudari-vte

# The checks after the Rust ones need nothing of each other: they run side by side, each
# with its output kept until it is shown in order.
cpp_warnings() {
# The headers of Qt and KDE are system headers here: only our own code counts.
# The newest generated header: other builds in the target directory can be older.
header=$(find "$target" -path '*/kudari/src/kde.cxx.h' -printf '%T@ %p\n' | sort -n | tail -n 1 | cut -d' ' -f2-)
generated=$(dirname "$(dirname "$(dirname "$header")")")
# Where build.rs takes them from as well: the qmake of cxx-qt and the frameworks of CMake.
qt_headers=$("${QMAKE:-qmake6}" -query QT_INSTALL_HEADERS 2>/dev/null || echo /usr/include/qt6)
includes="-isystem $qt_headers"
# The private headers of QtCore and QtGui, for the key mapper of qtbridge.cpp.
qt_version=$("${QMAKE:-qmake6}" -query QT_VERSION 2>/dev/null || true)
for module in QtCore QtGui; do
    includes="$includes -isystem $qt_headers/$module/$qt_version -isystem $qt_headers/$module/$qt_version/$module"
done
for module in "$qt_headers"/Qt* "${KUDARI_KF6_PREFIX:-/usr}"/include/KF6/*; do
    [ -d "$module" ] && includes="$includes -isystem $module"
done
# shellcheck disable=SC2086 # the include list is meant to split
g++ -std=c++17 -fPIC -fsyntax-only -Icpp "-I$generated" -DQT_GUI_LIB -DQT_WIDGETS_LIB -DQT_CORE_LIB $includes \
    -Wall -Wextra -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor \
    -Woverloaded-virtual -Wno-unused-parameter -Werror cpp/kdebridge.cpp
header=$(find "$target" -path '*/kudari-qt/src/qt.cxx.h' -printf '%T@ %p\n' | sort -n | tail -n 1 | cut -d' ' -f2-)
generated=$(dirname "$(dirname "$(dirname "$header")")")
# shellcheck disable=SC2086 # the include list is meant to split
g++ -std=c++17 -fPIC -fsyntax-only -Icrates/qt/cpp "-I$generated" -DQT_GUI_LIB -DQT_WIDGETS_LIB -DQT_CORE_LIB $includes \
    -Wall -Wextra -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor \
    -Woverloaded-virtual -Wno-unused-parameter -Werror crates/qt/cpp/qtbridge.cpp
header=$(find "$target" -path '*/kudari-windows/src/win.cxx.h' -printf '%T@ %p\n' | sort -n | tail -n 1 | cut -d' ' -f2-)
generated=$(dirname "$(dirname "$(dirname "$header")")")
# shellcheck disable=SC2086 # the include list is meant to split
g++ -std=c++17 -fPIC -fsyntax-only -Icrates/windows/cpp "-I$generated" -DQT_GUI_LIB -DQT_WIDGETS_LIB -DQT_CORE_LIB $includes \
    -Wall -Wextra -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor \
    -Woverloaded-virtual -Wno-unused-parameter -Werror crates/windows/cpp/winbridge.cpp
}

qml_checks() {
# Qt's linter cannot see into the types of cxx-qt, so these categories only give false alarms.
# `pragma ComponentBehavior: Bound` and the QML compiler in the build cover part of them.
qmllint() {
    env -u QT_LOGGING_RULES "$qmllint" --max-warnings 0 \
        --import disable --unresolved-type disable --missing-property disable --missing-type disable \
        --incompatible-type disable --unqualified disable \
        -I "$target/cxxqt/qml_modules" -I /usr/lib/qt6/qml "$@"
}
qmllint qml/*.qml qml/form/*.qml crates/windows/qml/form/*.qml
# The settings pages import qrc:/qml/form, which each front end fills with the parts of its
# forms; they are checked against those of both, and the QML of Windows against its own.
for crate in . crates/windows; do
    qrc="$resources/$(basename "$(cd "$crate" && pwd)").qrc"
    {
        echo '<RCC><qresource prefix="/">'
        for file in "$crate"/qml/form/*.qml; do
            echo "<file alias=\"qml/form/$(basename "$file")\">$(pwd)/$file</file>"
        done
        echo '</qresource></RCC>'
    } > "$qrc"
    if [ "$crate" = crates/windows ]; then
        qmllint --resource "$qrc" crates/qt/qml/*.qml crates/windows/qml/*.qml
    else
        qmllint --resource "$qrc" crates/qt/qml/*.qml
    fi
done
}

prose() {
# The English of the documentation and of the comments, as .vale.ini describes. Vale 3.27
# and later read the comments in code through grammars that `vale sync` downloads once;
# that comes before the checks start.
git ls-files -z '*.md' '*.rs' '*.cpp' '*.h' '*.py' '*.qml' '*.ps1' ':(exclude,glob)third_party/*/**' | xargs -0 vale --no-wrap
}

man_page() {
checkXML6 docs/man-kudari.1.docbook
# Its English as well, turned into HTML with the entities of KDocTools resolved. xsltproc
# reports an entity it cannot resolve but still succeeds, so any message of it fails here.
# Below the directory that the trap removes, also when a check here fails.
man="$resources/man"
mkdir "$man"
XML_CATALOG_FILES=/usr/share/kf6/kdoctools/customization/catalog.xml \
    xsltproc --nonet .vale/docbook.xsl docs/man-kudari.1.docbook > "$man/page.html" 2> "$man/errors"
if [ -s "$man/errors" ]; then
    cat "$man/errors"
    exit 1
fi
vale --ext=.html --no-wrap < "$man/page.html"
}

resources=$(mktemp -d)
trap 'rm -rf "$resources"' EXIT
# `vale sync` writes where the other runs of Vale read: it comes before them.
if [ ! -d .vale/styles/config/grammars ] &&
    vale --version | awk '{ split($3, v, "."); exit !(v[1] > 3 || (v[1] == 3 && v[2] >= 27)) }'; then
    vale sync
fi
# A failing command ends its check, as set -e does in the subshell of each.
for check in cpp_warnings qml_checks prose man_page; do
    (set -e; "$check") > "$resources/$check.out" 2>&1 &
    eval "pid_$check=\$!"
done
failed=""
for check in cpp_warnings qml_checks prose man_page; do
    case $check in
        cpp_warnings) title="C++ warnings" ;;
        qml_checks) title="QML" ;;
        prose) title="Prose" ;;
        man_page) title="Man page" ;;
    esac
    echo "== $title"
    status=0
    eval "wait \$pid_$check" || status=$?
    cat "$resources/$check.out"
    [ "$status" = 0 ] || failed="$failed $title;"
done
if [ -n "$failed" ]; then
    echo "Failed:$failed" >&2
    exit 1
fi

echo "All checks passed."
