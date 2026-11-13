#!/usr/bin/env bash
#
# Pull request checks, run by .github/workflows/pr-lint.yml as a required status check:
#   - the PR title and every commit follow Conventional Commits, as tools/commit-subject.sh
#     and the commit-msg hook check them
#   - the branch name follows the naming convention, and only develop, release/*, hotfix/*
#     and dependabot/* go into main (git flow)
#   - every commit is signed off (git commit -s)
#
# CHANGELOG.md needs no entry: tools/release.py writes it from the commits with git-cliff.
#
# Inputs (environment): PR_TITLE, HEAD_REF, BASE_REF, BASE_SHA, HEAD_SHA
set -euo pipefail

BRANCH_PATTERN='^((feature|fix|hotfix|chore|docs|refactor|test|ci|build|perf)/[a-z0-9._-]+(/[a-z0-9._-]+)*|release/v?[0-9]+\.[0-9]+(\.[0-9]+)?|dependabot/.+)$'
MAIN_SOURCES='^(develop|release/.+|hotfix/.+|dependabot/.+)$'

errors=0
error() { printf '::error::%s\n' "$*"; errors=$((errors + 1)); }
pass()  { printf 'ok: %s\n' "$*"; }

if tools/commit-subject.sh "$PR_TITLE"; then
  pass "PR title"
else
  error "The PR title must follow Conventional Commits, as the merge or squash commit takes it."
fi

# Git flow: develop → main, and main → develop brings releases and hotfixes back (merge commit).
if [[ $HEAD_REF =~ $BRANCH_PATTERN || $HEAD_REF == develop || ($HEAD_REF == main && $BASE_REF == develop) ]]; then
  pass "branch name '$HEAD_REF'"
else
  error "Branch name '$HEAD_REF' does not match the naming convention (for example feature/123-short-name, fix/…, release/1.2.0)."
fi

if [[ $BASE_REF == main && ! $HEAD_REF =~ $MAIN_SOURCES ]]; then
  error "Only develop, release/*, hotfix/* and dependabot/* may be merged into main; target develop instead."
fi

if tools/commit-subject.sh --range "$BASE_SHA..$HEAD_SHA"; then
  pass "commit messages"
else
  error "Commits must follow Conventional Commits; see the messages above."
fi

signed_off=$errors
while read -r sha; do
  [[ -n $sha ]] || continue
  if [[ $(git log -1 --format=%an "$sha") != *"[bot]" ]] \
     && ! git log -1 --format=%B "$sha" | grep -q '^Signed-off-by: '; then
    error "Commit ${sha:0:7} is not signed off (git commit -s)."
  fi
done < <(git rev-list --no-merges "$BASE_SHA..$HEAD_SHA")
if ((errors == signed_off)); then pass "sign-off"; fi

exit $((errors > 0))
