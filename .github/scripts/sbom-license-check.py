#!/usr/bin/env python3
"""Licence check against SBOM files in the repository.

Fallback for .github/workflows/dependency-review.yml when GitHub's dependency
review is not available (private repositories without GitHub Advanced Security).
Uses the same policy file as the dependency review action:

  allow-licenses               allowed SPDX licence identifiers
  allow-dependencies-licenses  package URLs (purl) exempt from the licence check
  fail-on-scopes               scopes to check (default: runtime)

Every package in SPDX or CycloneDX JSON SBOMs found in the repository is checked.
Like the action, a licence that is not allowed or an invalid licence expression
fails the check; packages without a detected licence are only reported.

Environment:
  POLICY        local path or OWNER/REPO/PATH@REF of the policy file
  POLICY_TOKEN  token for a central policy in a private/internal repository
  GH_TOKEN      fallback token for reading a central policy
  REQUIRE_SBOM  "true": fail if no SBOM is found (default: warn only)
  SBOM_DIR      directory searched for SBOMs (default: the repository, ".")
  IGNORE_SCOPE  "true": check every package regardless of its scope – for SBOMs generated
                without development dependencies, where "optional" still means shipped

Only the Python standard library is used.
"""

import json
import os
import re
import sys
import urllib.request
from pathlib import Path

SKIP_DIRS = {".git", "node_modules", ".venv", "venv", "__pycache__"}
UNKNOWN = {None, "", "NOASSERTION", "NONE"}


def unquote(value):
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        return value[1:-1]
    return value


def parse_policy(text):
    """Parses the subset of YAML used by the policy file (scalars and lists)."""
    policy, key = {}, None
    for raw in text.splitlines():
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        line = re.sub(r"\s+#.*$", "", raw)
        match = re.match(r"^([A-Za-z0-9_-]+):\s*(.*)$", line)
        if match:
            key, value = match.group(1), match.group(2).strip()
            if value.startswith("["):
                policy[key] = [unquote(v) for v in value.strip("[]").split(",") if v.strip()]
            elif value:
                policy[key] = unquote(value)
            else:
                policy[key] = []
            continue
        match = re.match(r"^\s+-\s*(.+)$", line)
        if match and isinstance(policy.get(key), list):
            policy[key].append(unquote(match.group(1)))
    return policy


def as_list(value):
    if value is None:
        return []
    if isinstance(value, str):
        return [v.strip() for v in value.split(",") if v.strip()]
    return value


def load_policy(spec):
    match = re.match(r"^([^/]+)/([^/]+)/(.+)@(.+)$", spec)
    if match and not Path(spec).exists():
        owner, repo, path, ref = match.groups()
        api = os.environ.get("GITHUB_API_URL", "https://api.github.com")
        if not api.startswith("https://"):
            raise ValueError(f"GITHUB_API_URL must be an https URL: {api}")
        token = os.environ.get("POLICY_TOKEN") or os.environ.get("GH_TOKEN", "")
        request = urllib.request.Request(
            f"{api}/repos/{owner}/{repo}/contents/{path}?ref={ref}",
            headers={"Accept": "application/vnd.github.raw", "Authorization": f"Bearer {token}"},
        )
        # The URL comes from the workflow environment (https only, checked above), not from the PR.
        # nosemgrep: python.lang.security.audit.dynamic-urllib-use-detected.dynamic-urllib-use-detected
        with urllib.request.urlopen(request, timeout=30) as response:
            return parse_policy(response.read().decode("utf-8"))
    return parse_policy(Path(spec).read_text(encoding="utf-8"))


def satisfies(expression, allowed):
    """Evaluates an SPDX licence expression against the allowed identifiers.

    Raises ValueError for expressions that cannot be parsed.
    """
    tokens = re.findall(r"\(|\)|[^\s()]+", expression)
    pos = 0

    def peek():
        return tokens[pos].upper() if pos < len(tokens) else None

    def take():
        nonlocal pos
        pos += 1
        return tokens[pos - 1]

    def parse_or():
        result = parse_and()
        while peek() == "OR":
            take()
            right = parse_and()
            result = result or right
        return result

    def parse_and():
        result = parse_atom()
        while peek() == "AND":
            take()
            right = parse_atom()
            result = result and right
        return result

    def parse_atom():
        if peek() is None or peek() in ("AND", "OR", "WITH", ")"):
            raise ValueError(f"invalid licence expression: {expression}")
        if peek() == "(":
            take()
            result = parse_or()
            if peek() != ")":
                raise ValueError(f"invalid licence expression: {expression}")
            take()
            return result
        licence = take().rstrip("+").lower()
        if peek() == "WITH":
            take()
            if peek() is None:
                raise ValueError(f"invalid licence expression: {expression}")
            take()
        return licence in allowed

    result = parse_or()
    if pos != len(tokens):
        raise ValueError(f"invalid licence expression: {expression}")
    return result


def purl_base(purl):
    """pkg:npm/express@4.0.0?x=y -> pkg:npm/express (the version is not compared)."""
    base = re.split(r"[?#]", purl or "")[0]
    return re.sub(r"@[^/@]*$", "", base).lower()


def spdx_packages(doc):
    roots = set(doc.get("documentDescribes", []))
    for rel in doc.get("relationships", []):
        if rel.get("relationshipType") == "DESCRIBES":
            roots.add(rel.get("relatedSpdxElement"))
    for pkg in doc.get("packages", []):
        if pkg.get("SPDXID") in roots:
            continue  # the product itself
        licence = pkg.get("licenseConcluded")
        if licence in UNKNOWN:
            licence = pkg.get("licenseDeclared")
        purl = next((ref.get("referenceLocator") for ref in pkg.get("externalRefs", [])
                     if ref.get("referenceType") == "purl"), None)
        # SPDX has no scopes: every package counts as shipped (runtime).
        yield pkg.get("name"), pkg.get("versionInfo"), None if licence in UNKNOWN else licence, purl, "runtime"


def cdx_components(components):
    for component in components or []:
        yield component
        yield from cdx_components(component.get("components"))


def cdx_packages(doc):
    for component in cdx_components(doc.get("components")):
        expressions = []
        for entry in component.get("licenses") or []:
            if entry.get("expression"):
                expressions.append(entry["expression"])
            elif entry.get("license"):
                expressions.append(entry["license"].get("id") or entry["license"].get("name"))
        expressions = [e for e in expressions if e]
        # Several licence entries are treated conservatively: all of them must be allowed.
        licence = None
        if len(expressions) == 1:
            licence = expressions[0]
        elif expressions:
            licence = " AND ".join(f"({e})" for e in expressions)
        scope = "runtime" if component.get("scope", "required") == "required" else "development"
        yield component.get("name"), component.get("version"), licence, component.get("purl"), scope


def find_sboms(root):
    for path in sorted(root.rglob("*.json")):
        if SKIP_DIRS.intersection(path.parts):
            continue
        name = path.name.lower()
        if not (name.endswith((".spdx.json", ".cdx.json", ".cyclonedx.json"))
                or name in ("bom.json", "sbom.json") or "sbom" in name):
            continue
        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if isinstance(doc, dict) and "spdxVersion" in doc:
            yield path, spdx_packages(doc)
        elif isinstance(doc, dict) and doc.get("bomFormat") == "CycloneDX":
            yield path, cdx_packages(doc)


def annotate(level, message):
    print(f"::{level}::{message}")


def main():
    policy = load_policy(os.environ.get("POLICY", ".github/dependency-review-config.yml"))
    allowed = {lic.lower() for lic in as_list(policy.get("allow-licenses"))}
    exempt = {purl_base(p) for p in as_list(policy.get("allow-dependencies-licenses"))}
    scopes = set(as_list(policy.get("fail-on-scopes")) or ["runtime"])
    if not allowed:
        annotate("error", "Policy has no allow-licenses – nothing to check against.")
        return 1

    ignore_scope = os.environ.get("IGNORE_SCOPE") == "true"
    forbidden, unknown, checked, seen = [], [], 0, set()
    sboms = list(find_sboms(Path(os.environ.get("SBOM_DIR") or ".")))
    for path, packages in sboms:
        for name, version, licence, purl, scope in packages:
            key = purl_base(purl) + "@" + str(version) if purl else f"{name}@{version}"
            # Per licence: the same package listed twice with different licences is checked twice.
            if (key, licence) in seen or (scope not in scopes and not ignore_scope):
                continue
            seen.add((key, licence))
            checked += 1
            label = f"{name}@{version}" + (f" ({purl})" if purl else "")
            if purl and purl_base(purl) in exempt:
                continue
            if licence is None:
                unknown.append(f"{label} in {path}")
                continue
            try:
                if not satisfies(licence, allowed):
                    forbidden.append(f"{label}: {licence} (in {path})")
            except ValueError as error:
                forbidden.append(f"{label}: {error} (in {path})")

    if not sboms:
        message = ("No SBOM (SPDX/CycloneDX JSON) found in the repository – licence check skipped. "
                   "Commit an SBOM generated by your build, e.g. sbom.spdx.json or bom.json.")
        annotate("error" if os.environ.get("REQUIRE_SBOM") == "true" else "warning", message)
        return 1 if os.environ.get("REQUIRE_SBOM") == "true" else 0

    print(f"Checked {checked} packages from {len(sboms)} SBOM(s): " + ", ".join(str(p) for p, _ in sboms))
    for entry in unknown:
        annotate("warning", f"No licence detected, review manually: {entry}")
    for entry in forbidden:
        annotate("error", f"Licence not allowed: {entry}")

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as out:
            out.write("## Licence check (SBOM fallback)\n\n")
            out.write(f"{checked} packages checked, {len(forbidden)} not allowed, {len(unknown)} without licence.\n\n")
            for entry in forbidden:
                out.write(f"- :x: {entry}\n")
            for entry in unknown:
                out.write(f"- :warning: {entry}\n")
    return 1 if forbidden else 0


if __name__ == "__main__":
    sys.exit(main())
