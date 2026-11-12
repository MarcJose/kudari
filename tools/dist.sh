#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Makes the files of a release; .github/workflows/release.yml calls it.
#
#   tools/dist.sh source OUTDIR            source tarball with all crates, and its SBOMs
#   tools/dist.sh binary TARBALL OUTDIR    package for x86_64, built from that tarball
#   tools/dist.sh verify DIR               check an installed tree, such as a package
#
# Both archives are reproducible: their contents get the time of the last commit, and the
# build leaves out the directories it ran in. Building the same tarball twice, anywhere,
# gives the same package; the release workflow does exactly that before publishing.
set -eu

die() { echo "dist: $*" >&2; exit 1; }

# A tar archive that depends on nothing but the files and SOURCE_DATE_EPOCH, not on the
# umask either: directories and programs get 755, other files 644.
archive() { # directory to pack, its parent, output file
    tar --sort=name --format=pax --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
        --mode=u+rwX,go+rX,go-w --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
        -C "$2" -cf - "$1" | xz -9 --threads=1 > "$3"
}

source_tarball() {
    out=$(mkdir -p "$1" && cd "$1" && pwd)
    cd "$(dirname "$0")/.."
    version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
    name=kudari-$version
    SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
    export SOURCE_DATE_EPOCH
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT

    git archive --prefix="$name/" HEAD | tar -x -C "$work"
    # The crates go along, so the tarball builds without network.
    mkdir -p "$work/$name/.cargo"
    (cd "$work/$name" && cargo vendor --locked --versioned-dirs --quiet vendor > .cargo/config.toml)
    archive "$name" "$work" "$out/$name.tar.xz"

    (cd "$work/$name" && cargo cyclonedx --format json --spec-version 1.5 --target x86_64-unknown-linux-gnu \
        --override-filename "$name.cdx" > /dev/null && mv "$name.cdx.json" "$out/")
    # The crates as Cargo.lock names them; inside vendor/, syft would also count the lock and
    # test files of the crates themselves.
    SYFT_FILE_METADATA_SELECTION=none syft scan --quiet "dir:$work/$name" --exclude ./vendor \
        --source-name kudari --source-version "$version" \
        -o "spdx-json=$out/$name.spdx.json"
    echo "$out/$name.tar.xz"
}

binary_package() {
    tarball=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
    out=$(mkdir -p "$2" && cd "$2" && pwd)
    name=$(basename "$tarball" .tar.xz)
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT

    tar -x -C "$work" -f "$tarball"
    source=$work/$name
    # Every file of the tarball carries the time of the last commit.
    SOURCE_DATE_EPOCH=$(stat -c %Y "$source/Cargo.toml")
    export SOURCE_DATE_EPOCH
    # Where it was built must not end up in the program; the compilers would write the paths
    # in. Qt's resource compiler takes its times from SOURCE_DATE_EPOCH (see third_party/).
    export RUSTFLAGS="--remap-path-prefix=$source=/build/$name --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
    export CFLAGS="-ffile-prefix-map=$source=/build/$name"
    export CXXFLAGS="$CFLAGS"

    cmake -S "$source" -B "$source/build" -DCMAKE_INSTALL_PREFIX=/usr > /dev/null
    cmake --build "$source/build"
    DESTDIR="$work/package" cmake --install "$source/build" > /dev/null
    "$0" verify "$work/package"
    archive usr "$work/package" "$out/$name-x86_64-linux.tar.xz"
    echo "$out/$name-x86_64-linux.tar.xz"
}

# What an installation must contain, and that the program starts.
verify() {
    cd "$1/usr" || die "$1 holds no usr directory"
    appstreamcli validate --no-net --pedantic share/metainfo/io.github.marcjose.kudari.metainfo.xml
    desktop-file-validate share/applications/io.github.marcjose.kudari.desktop
    for file in bin/kudari share/man/man1/kudari.1 share/locale/de/LC_MESSAGES/kudari.mo \
        share/dbus-1/services/io.github.marcjose.kudari.service share/knotifications6/kudari.notifyrc \
        share/icons/hicolor/scalable/apps/io.github.marcjose.kudari.svg \
        share/doc/kudari/MIT.txt share/doc/kudari/THIRD_PARTY_LICENSES; do
        [ -s "$file" ] || die "missing from the installation: $file"
    done
    QT_QPA_PLATFORM=offscreen bin/kudari --version
}

case ${1:-} in
    source) [ $# = 2 ] || die "usage: tools/dist.sh source OUTDIR"; source_tarball "$2" ;;
    binary) [ $# = 3 ] || die "usage: tools/dist.sh binary TARBALL OUTDIR"; binary_package "$2" "$3" ;;
    verify) [ $# = 2 ] || die "usage: tools/dist.sh verify DIR"; verify "$2" ;;
    *) die "usage: tools/dist.sh source|binary|verify ..." ;;
esac
