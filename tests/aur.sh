#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Builds a package for the AUR as an AUR helper would, installs it and starts the program;
# the CI runs it as root in an Arch container:
#
#   tests/aur.sh git    kudari-git, from the commit checked out here
#   tests/aur.sh bin    kudari-bin, from the release that its PKGBUILD names
#
# makepkg does not run as root, so a user of its own builds. With SRCDEST set, makepkg takes
# downloads from there, such as a package of a release that is not published yet.
set -eu
die() { echo "aur: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || die "needs root, to install the dependencies and the package"
case ${1:-} in
    git | bin) name=kudari-$1 ;;
    *) die "usage: tests/aur.sh git|bin" ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
id builder > /dev/null 2>&1 || useradd --create-home builder
cp "$root/packaging/aur/$name/PKGBUILD" "$work/"
if [ "$name" = kudari-git ]; then
    # This commit instead of main on GitHub; makepkg clones it as the user who builds.
    sed -i "s|^source=(\"git+\$url.git\")|source=(\"kudari::git+file://$root#commit=$(git -C "$root" rev-parse HEAD)\")|" \
        "$work/PKGBUILD"
    grep -q '^source=("kudari::git+file://' "$work/PKGBUILD" || die "the source of kudari-git did not change"
    runuser -u builder -- git config --global --add safe.directory "$root"
fi
chmod -R a+rwX "$work"

# What makepkg --syncdeps would install, without the sudo that it needs for that.
# shellcheck disable=SC2046 # the list of packages is meant to split
pacman -S --noconfirm --needed $(sed -n 's/^\t\(make\)\?depends = //p' "$root/packaging/aur/$name/.SRCINFO")
(cd "$work" && runuser -u builder -- env ${SRCDEST:+SRCDEST="$SRCDEST"} makepkg --noconfirm)
package=$(find "$work" -maxdepth 1 -name "$name-[0-9]*.pkg.tar.zst" | head -1)
[ -n "$package" ] || die "makepkg made no package"

# Warnings stay: namcap cannot see what Kudari loads through QML and plugins.
findings=$(namcap "$package")
echo "$findings"
if echo "$findings" | grep -q ' E: '; then
    die "namcap found errors in $name"
fi
pacman -U --noconfirm "$package"
QT_QPA_PLATFORM=offscreen kudari --version
pacman -R --noconfirm "$name"
