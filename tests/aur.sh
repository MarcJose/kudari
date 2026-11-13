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
# downloads from there, such as a package of a release that is not published yet. Without it,
# kudari-bin is skipped while GitHub does not know its release, which happens between a new
# first release and the update of the PKGBUILD by tools/aur.py.
set -eu
die() { echo "aur: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || die "needs root, to install the dependencies and the package"
case ${1:-} in
    git | bin) name=kudari-$1 ;;
    *) die "usage: tests/aur.sh git|bin" ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
if [ "$name" = kudari-bin ] && [ -z "${SRCDEST:-}" ]; then
    source=$(sed -n 's/^\tsource_x86_64 = //p' "$root/packaging/aur/$name/.SRCINFO")
    status=$(curl --silent --head --location --output /dev/null --write-out '%{http_code}' "$source")
    if [ "$status" = 404 ]; then
        echo "::notice::kudari-bin skipped: $source is not published (yet)"
        exit 0
    fi
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
id builder > /dev/null 2>&1 || useradd --create-home builder
cp "$root/packaging/aur/$name/PKGBUILD" "$work/"
if [ "$name" = kudari-git ]; then
    # This commit instead of main on GitHub; makepkg clones it as the user who builds. In the
    # CI, the checkout belongs to another user than root, which git distrusts without the
    # safe.directory.
    commit=$(git -c safe.directory="$root" -C "$root" rev-parse HEAD)
    sed -i "s|^source=(\"git+\$url.git\")|source=(\"kudari::git+file://$root#commit=$commit\")|" "$work/PKGBUILD"
    grep -q "^source=(\"kudari::git+file://$root#commit=$commit\")" "$work/PKGBUILD" ||
        die "the source of kudari-git did not change"
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
