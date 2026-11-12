#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Compares the translatable strings in the code with the translations in po/.

    tools/translations.py           report per language
    tools/translations.py --check   also fail on problems, for tests/check.sh
    tools/translations.py --check --strict   fail on any gap in any language
    tools/translations.py --prune   remove entries the code no longer uses

A string is the pair of context and text, as KI18n looks it up. The code calls
tr() and tr_arg() in Rust, translate(), translateWith() and t() in QML, and
i18n(), i18nc() and xi18nc() in C++.
"""

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCES = [
    ("src", ".rs", ["tr", "tr_arg"]),
    ("crates", ".rs", ["tr", "tr_arg"]),
    ("qml", ".qml", ["translate", "translateWith", "t"]),
    ("crates", ".qml", ["translate", "translateWith", "t"]),
    ("cpp", ".cpp", ["i18n", "i18nc", "xi18nc"]),
]
# Functions whose first argument is the text, without a context.
WITHOUT_CONTEXT = {"i18n"}
# Wrappers such as tr() and the t() of the QML pages pass on what they get.
FORWARDING = re.compile(r"\s*context\s*,")

STRING = re.compile(r'"((?:[^"\\]|\\.)*)"', re.S)


def unescape(text):
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m[1], m[1]), text)


def literal_arguments(text, start, count):
    """Reads `count` string literals after an opening parenthesis. Adjacent literals
    join, as in C++. Returns None if an argument is not a literal."""
    arguments = []
    position = start
    for _ in range(count):
        parts = []
        while True:
            while position < len(text) and text[position] in " \t\r\n":
                position += 1
            match = STRING.match(text, position)
            if not match:
                break
            parts.append(unescape(match[1]))
            position = match.end()
        if not parts:
            return None
        arguments.append("".join(parts))
        while position < len(text) and text[position] in " \t\r\n":
            position += 1
        if position < len(text) and text[position] == ",":
            position += 1
    return arguments


def used_strings():
    """(context, text) -> list of places, plus calls whose arguments are not literals."""
    used, dynamic = {}, []
    for folder, suffix, functions in SOURCES:
        call = re.compile(r"(?<![\w.])(?:\w+\.)*(%s)\s*\(" % "|".join(functions))
        for path in sorted((ROOT / folder).rglob("*" + suffix)):
            text = path.read_text(encoding="utf-8")
            for match in call.finditer(text):
                line_start = text.rfind("\n", 0, match.start()) + 1
                line = text[line_start : text.find("\n", match.start())]
                defines = re.search(r"\b(fn|function)\s+$", text[line_start : match.start(1)])
                if defines or line.lstrip().startswith(("//", "*")) or FORWARDING.match(text, match.end()):
                    continue
                function = match[1]
                count = 1 if function in WITHOUT_CONTEXT else 2
                arguments = literal_arguments(text, match.end(), count)
                place = f"{path.relative_to(ROOT)}:{text.count(chr(10), 0, match.start()) + 1}"
                if arguments is None:
                    dynamic.append(place)
                    continue
                key = ("", arguments[0]) if count == 1 else (arguments[0], arguments[1])
                used.setdefault(key, []).append(place)
    return used, dynamic


def po_string(lines):
    return "".join(unescape(STRING.search(line)[1]) for line in lines)


def read_po(path):
    """Entries as dicts with context, text, translations, fuzzy, and their text."""
    entries, block = [], []
    for line in path.read_text(encoding="utf-8").split("\n") + [""]:
        if line.strip():
            block.append(line)
            continue
        if block:
            entries.append(parse_entry(block))
            block = []
    return entries


def parse_entry(block):
    fields, current = {}, None
    for line in block:
        if line.startswith("#,"):
            fields["flags"] = line
        if line.startswith("#"):
            continue
        keyword = line.split(" ", 1)[0]
        if not line.startswith('"'):
            current = keyword
            fields[current] = [line[len(keyword):]]
        elif current:
            fields[current].append(line)
    translations = [po_string(lines) for key, lines in fields.items() if key.startswith("msgstr")]
    return {
        "context": po_string(fields.get("msgctxt", [])),
        "text": po_string(fields.get("msgid", [])),
        "translated": any(translations),
        "fuzzy": "fuzzy" in fields.get("flags", ""),
        # Obsolete entries keep their msgid only in "#~" comments, even when they carry flags.
        "obsolete": not any(line.startswith("msgid") for line in block),
        "block": "\n".join(block),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--check", action="store_true", help="exit with 1 on problems")
    parser.add_argument("--strict", action="store_true", help="with --check, also fail on gaps")
    parser.add_argument("--prune", action="store_true", help="remove unused entries")
    parser.add_argument("--quiet", action="store_true", help="only problems and a summary")
    parser.add_argument("--verbose", action="store_true", help="name every missing string")
    options = parser.parse_args()

    used, dynamic = used_strings()
    for place in dynamic:
        print(f"not a literal, cannot be checked: {place}")

    unused_anywhere, translated_somewhere, rows = set(), set(), []
    for po in sorted((ROOT / "po").glob("*/kudari.po")):
        language = po.parent.name
        entries = [e for e in read_po(po) if e["text"] and not e["obsolete"]]
        known = {(e["context"], e["text"]): e for e in entries}
        # msgfmt leaves fuzzy entries out, so they show up in English as well.
        done = {key for key in used if key in known and known[key]["translated"] and not known[key]["fuzzy"]}
        fuzzy = [key for key in used if key in known and known[key]["fuzzy"]]
        unused = [e for e in entries if (e["context"], e["text"]) not in used]
        missing = [key for key in used if key not in done]
        rows.append((language, len(done), len(missing), len(fuzzy), len(unused)))
        translated_somewhere |= done
        unused_anywhere.update((e["context"], e["text"]) for e in unused)
        # A strict check names what makes it fail.
        if options.verbose or (options.check and options.strict):
            for context, text in missing:
                print(f"{language}: missing [{context}] {text!r}")
        if options.prune:
            kept = [
                e["block"]
                for e in read_po(po)
                if not e["obsolete"] and ((e["context"], e["text"]) in used or not e["text"])
            ]
            po.write_text("\n\n".join(kept) + "\n", encoding="utf-8")
    if options.prune:
        unused_anywhere = set()

    nowhere = sorted(set(used) - translated_somewhere)
    complete = sum(1 for row in rows if row[2] == 0)
    if not options.quiet:
        print(f"{'language':<20}{'translated':>11}{'missing':>9}{'of them fuzzy':>15}{'unused':>8}")
        for language, done, missing, fuzzy, unused in rows:
            print(f"{language:<20}{done:>11}{missing:>9}{fuzzy:>15}{unused:>8}")
        print()
    for context, text in sorted(unused_anywhere):
        print(f"unused: [{context}] {text!r}")
    for context, text in nowhere:
        print(f"no language translates: [{context}] {text!r}")
        for place in used[(context, text)]:
            print(f"    used in {place}")
    gaps = sum(row[2] for row in rows)
    print(
        f"{len(used)} strings, {len(rows)} languages, {complete} complete, {gaps} gaps, "
        f"{len(unused_anywhere)} unused, {len(nowhere)} without any translation"
    )

    failed = dynamic or unused_anywhere or nowhere or (options.strict and gaps)
    return 1 if options.check and failed else 0

if __name__ == "__main__":
    sys.exit(main())
