#!/usr/bin/env bash
#
# Pull request checks (run by .github/workflows/pr-lint.yml, required status checks):
#   conventions  PR title, branch name, commit messages (Conventional Commits) and sign-off
#   changelog    CHANGELOG.md must be updated (skip: label "skip-changelog" or bot author)
#
# Inputs (environment): PR_TITLE, PR_AUTHOR, PR_LABELS (comma-separated),
#                       HEAD_REF, BASE_REF, BASE_SHA, HEAD_SHA
set -euo pipefail

BRANCH_PATTERN='^((feature|fix|hotfix|chore|docs|refactor|test|ci|build|perf)/[a-z0-9._-]+(/[a-z0-9._-]+)*|release/v?[0-9]+\.[0-9]+(\.[0-9]+)?|dependabot/.+)$'
COMMIT_PATTERN='^((feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([A-Za-z0-9._/-]+\))?!?: .+|Merge .+|Revert ".+")'
GIT_FLOW=true
CHECKS="conventions|changelog"
# Branches that may be merged into main (git flow, Dependabot security updates, baseline PR).
MAIN_SOURCES='^(develop|release/.+|hotfix/.+|dependabot/.+|chore/repo-baseline)$'

errors=0
error() { printf '::error::%s\n' "$*"; errors=$((errors + 1)); }
pass()  { printf 'ok: %s\n' "$*"; }

check_conventions() {
  local sha author subject body commit_errors

  if [[ $PR_TITLE =~ $COMMIT_PATTERN ]]; then
    pass "PR title"
  else
    error "PR title '$PR_TITLE' must follow Conventional Commits, e.g. 'feat(api): add login' (it becomes the squash commit message)."
  fi

  # Git flow: develop → main, and main → develop brings releases and hotfixes back (merge commit).
  if [[ $HEAD_REF =~ $BRANCH_PATTERN ]] \
     || { $GIT_FLOW && [[ $HEAD_REF == develop || ($HEAD_REF == main && $BASE_REF == develop) ]]; }; then
    pass "branch name '$HEAD_REF'"
  else
    error "Branch name '$HEAD_REF' does not match the naming convention (e.g. feature/123-short-name, fix/…, release/1.2.0)."
  fi

  if [[ $BASE_REF == main && ! $HEAD_REF =~ $MAIN_SOURCES ]]; then
    error "Only develop, release/*, hotfix/* and dependabot/* may be merged into main – target develop instead."
  fi

  commit_errors=$errors
  while read -r sha; do
    [[ -n $sha ]] || continue
    author=$(git log -1 --format=%an "$sha")
    subject=$(git log -1 --format=%s "$sha")
    body=$(git log -1 --format=%B "$sha")
    if [[ ! $subject =~ $COMMIT_PATTERN ]]; then
      error "Commit ${sha:0:7} '$subject' does not follow Conventional Commits."
    fi
    if [[ $author != *"[bot]" ]] && ! grep -q '^Signed-off-by: ' <<< "$body"; then
      error "Commit ${sha:0:7} is not signed off (git commit -s)."
    fi
  done < <(git rev-list --no-merges "$BASE_SHA..$HEAD_SHA")
  if ((errors == commit_errors)); then pass "commit messages and sign-off"; fi
}

check_changelog() {
  if [[ ",${PR_LABELS:-}," == *",skip-changelog,"* ]]; then
    pass "changelog skipped (label skip-changelog)"
  elif [[ ${PR_AUTHOR:-} == *"[bot]" ]]; then
    pass "changelog skipped (bot author ${PR_AUTHOR})"
  elif git diff --name-only "$BASE_SHA...$HEAD_SHA" | grep -qx 'CHANGELOG.md'; then
    pass "CHANGELOG.md updated"
  else
    error "Add an entry to CHANGELOG.md (section [Unreleased]) or set the label 'skip-changelog'."
  fi
}

case ${1-} in
  conventions) check_conventions ;;
  changelog)   check_changelog ;;
  *)           echo "usage: $0 $CHECKS" >&2; exit 2 ;;
esac
exit $((errors > 0))
