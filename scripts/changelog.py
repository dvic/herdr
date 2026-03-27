#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from datetime import date
from pathlib import Path

SECTION_RE = re.compile(r"^##\s+(?:\[(?P<bracketed>[^\]]+)\]|(?P<plain>.+?))\s*$", re.MULTILINE)
VERSION_WITH_DATE_RE = re.compile(r"^(?P<version>.+?)\s+-\s+\d{4}-\d{2}-\d{2}$")


@dataclass(frozen=True)
class Section:
    title: str
    start: int
    end: int
    body_start: int


class ChangelogError(ValueError):
    pass


def normalize_title(raw_title: str) -> str:
    title = raw_title.strip()
    match = VERSION_WITH_DATE_RE.match(title)
    if match:
        title = match.group("version").strip()
    if title.startswith("[") and title.endswith("]"):
        title = title[1:-1].strip()
    return title


def parse_sections(text: str) -> list[Section]:
    matches = list(SECTION_RE.finditer(text))
    sections: list[Section] = []

    for index, match in enumerate(matches):
        title = normalize_title(match.group("bracketed") or match.group("plain") or "")
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        body_start = match.end()
        if body_start < len(text) and text[body_start : body_start + 1] == "\n":
            body_start += 1
        sections.append(Section(title=title, start=match.start(), end=end, body_start=body_start))

    return sections


def find_section(text: str, wanted_title: str) -> Section:
    for section in parse_sections(text):
        if section.title == wanted_title:
            return section
    raise ChangelogError(f"section not found: {wanted_title}")


def extract_section_body(text: str, wanted_title: str) -> str:
    section = find_section(text, wanted_title)
    body = text[section.body_start : section.end].strip("\n")
    if not body.strip():
        raise ChangelogError(f"section is empty: {wanted_title}")
    return body + "\n"


def prepare_release(text: str, version: str, release_date: str) -> str:
    unreleased = None
    existing_version = False

    for section in parse_sections(text):
        if section.title == "Unreleased":
            unreleased = section
        if section.title == version:
            existing_version = True

    if existing_version:
        raise ChangelogError(f"version already exists in changelog: {version}")
    if unreleased is None:
        raise ChangelogError("missing Unreleased section")

    unreleased_body = text[unreleased.body_start : unreleased.end].strip("\n")
    if not unreleased_body.strip():
        raise ChangelogError("Unreleased section is empty")

    prefix = text[: unreleased.start].rstrip("\n")
    suffix = text[unreleased.end :].strip("\n")

    rebuilt = f"## Unreleased\n\n## [{version}] - {release_date}\n\n{unreleased_body}"
    if suffix:
        rebuilt += f"\n\n{suffix}"

    if prefix:
        return f"{prefix}\n\n{rebuilt}\n"
    return rebuilt + "\n"


def load_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ChangelogError(f"changelog not found: {path}") from exc


def write_text(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")


def cmd_prepare(args: argparse.Namespace) -> int:
    path = Path(args.path)
    original = load_text(path)
    updated = prepare_release(original, args.version, args.date)
    write_text(path, updated)
    return 0


def cmd_extract(args: argparse.Namespace) -> int:
    path = Path(args.path)
    body = extract_section_body(load_text(path), args.version)
    if args.output:
        write_text(Path(args.output), body)
    else:
        sys.stdout.write(body)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Prepare and extract changelog release notes")
    subparsers = parser.add_subparsers(dest="command", required=True)

    prepare = subparsers.add_parser("prepare", help="Move Unreleased into a versioned section")
    prepare.add_argument("--path", default="CHANGELOG.md")
    prepare.add_argument("--version", required=True)
    prepare.add_argument("--date", default=str(date.today()))
    prepare.set_defaults(func=cmd_prepare)

    extract = subparsers.add_parser("extract", help="Extract a version section body")
    extract.add_argument("--path", default="CHANGELOG.md")
    extract.add_argument("--version", required=True)
    extract.add_argument("--output")
    extract.set_defaults(func=cmd_extract)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    try:
        return args.func(args)
    except ChangelogError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
