#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# The checks of .github/workflows/security.yml, run locally:
#
#   tools/security.sh
#
# Secrets in the whole history (gitleaks, trufflehog), known vulnerabilities and malicious
# crates, licenses and sources of the dependencies (cargo-deny, osv-scanner), the workflows
# (actionlint, zizmor) and the shell scripts (shellcheck). A tool that is not installed is
# skipped and reported.
set -eu

cd "$(dirname "$0")/.."
skipped=

have() {
    if command -v "$1" > /dev/null; then
        return 0
    fi
    skipped="$skipped $1"
    echo "(skipped: $1 is not installed)"
    return 1
}

echo "== Secrets"
if have gitleaks; then gitleaks git --no-banner --redact --verbose --exit-code 1 --log-level warn .; fi
# Not --no-update: the trufflehog-bin package of the AUR passes it already and refuses it twice.
if have trufflehog; then trufflehog git "file://$PWD" --results=verified,unknown --fail --log-level=-1; fi

echo "== Dependencies"
if cargo deny --version > /dev/null 2>&1; then
    cargo deny --locked check --hide-inclusion-graph
else
    skipped="$skipped cargo-deny"
    echo "(skipped: cargo-deny is not installed)"
fi
if have osv-scanner; then osv-scanner scan source --lockfile Cargo.lock; fi

echo "== Workflows"
if have actionlint; then actionlint; fi
# Without a token zizmor leaves out the audits that ask GitHub.
if have zizmor; then zizmor --offline .github; fi

echo "== Shell scripts"
if have shellcheck; then git ls-files -z '*.sh' '.githooks/*' | xargs -0 shellcheck; fi

if [ -n "$skipped" ]; then
    echo "Security checks passed, but these were skipped:$skipped"
else
    echo "Security checks passed."
fi
