#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Checks that commit subjects follow Conventional Commits (https://www.conventionalcommits.org),
# from which git-cliff writes the changelog and tools/release.py picks the next version:
#
#   tools/commit-subject.sh SUBJECT          one subject, such as the title of a pull request
#   tools/commit-subject.sh --range A..B     every commit in that range
#
#   feat(terminal): open file links in the editor
#   fix!: keep the settings of Yakuake on import       (! marks a breaking change)
#
# The commit-msg hook and the workflow for pull requests both use it.
set -eu

types='feat|fix|perf|refactor|docs|test|build|ci|chore|style|revert'
max_length=100

# Prints the problem with a subject, or nothing.
problem() {
    # Merges, reverts and fixups that Git writes itself pass as they are.
    if printf '%s\n' "$1" | grep -qE '^(Merge |Revert "|fixup! |squash! |amend! )'; then
        return
    fi
    if ! printf '%s\n' "$1" | grep -qE "^($types)(\([a-z0-9-]+\))?!?: [^ ]"; then
        echo "does not follow Conventional Commits"
    elif [ "${#1}" -gt "$max_length" ]; then
        echo "is longer than $max_length characters"
    fi
}

explain() {
    cat >&2 <<MESSAGE
Expected "type(scope): description", the scope being optional, with type one of
    ${types}
for example "feat(tabs): close a tab with a middle click" or "fix: keep the window height".
MESSAGE
}

case ${1:-} in
    --range)
        [ $# = 2 ] || { echo "usage: tools/commit-subject.sh --range A..B" >&2; exit 2; }
        # Assigned first, so that a range Git does not know stops the script.
        commits=$(git rev-list --reverse "$2")
        failed=
        for commit in $commits; do
            subject=$(git log -1 --format=%s "$commit")
            found=$(problem "$subject")
            if [ -n "$found" ]; then
                printf 'commit %s %s:\n    %s\n' "$(git rev-parse --short "$commit")" "$found" "$subject" >&2
                failed=1
            fi
        done
        [ -z "$failed" ] || { explain; exit 1; }
        ;;
    *)
        [ $# = 1 ] || { echo "usage: tools/commit-subject.sh SUBJECT | --range A..B" >&2; exit 2; }
        found=$(problem "$1")
        if [ -n "$found" ]; then
            printf 'the subject %s:\n    %s\n' "$found" "$1" >&2
            explain
            exit 1
        fi
        ;;
esac
