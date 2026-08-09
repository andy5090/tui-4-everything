#!/usr/bin/env python3
"""Network-free structural checks for the framework-free T4E site."""

from __future__ import annotations

import json
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlparse


ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / "site"


class SiteParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.ids: list[str] = []
        self.local_references: list[str] = []
        self.demo_ids: list[str] = []
        self.inline_handlers: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        if identifier := values.get("id"):
            self.ids.append(identifier)
        if demo_id := values.get("data-demo"):
            self.demo_ids.append(demo_id)
        for name, value in attrs:
            if name.startswith("on"):
                self.inline_handlers.append(name)
            if name in {"href", "src"} and value and value.startswith("./"):
                self.local_references.append(value)
            if name == "src" and value and urlparse(value).scheme in {"http", "https"}:
                raise AssertionError(f"external runtime script is not allowed: {value}")


def main() -> None:
    html = (SITE / "index.html").read_text(encoding="utf-8")
    parser = SiteParser()
    parser.feed(html)

    assert len(parser.ids) == len(set(parser.ids)), "HTML IDs must be unique"
    assert not parser.inline_handlers, "inline event handlers are not allowed"
    assert "main" in parser.ids, "skip-link target is missing"
    assert "missions" in parser.ids, "demo section is missing"
    assert "install" in parser.ids, "install section is missing"

    for reference in parser.local_references:
        path = SITE / reference.removeprefix("./").split("#", 1)[0]
        assert path.exists(), f"missing local site asset: {reference}"

    payload = json.loads((SITE / "demos.json").read_text(encoding="utf-8"))
    demos = payload["demos"]
    demo_ids = [demo["id"] for demo in demos]
    assert demo_ids == parser.demo_ids, "mission buttons and demo data must stay in sync"
    assert len(demo_ids) == len(set(demo_ids)), "demo IDs must be unique"

    valid_phases = {"request", "review", "run"}
    for demo in demos:
        assert demo["events"], f"{demo['id']} must contain events"
        phases = {event["phase"] for event in demo["events"]}
        assert phases == valid_phases, f"{demo['id']} must demonstrate every phase"
        for event in demo["events"]:
            assert event["lines"], f"{demo['id']} contains an empty event"
            assert all(isinstance(line, str) and line for line in event["lines"])

    manifest = json.loads((SITE / "site.webmanifest").read_text(encoding="utf-8"))
    assert manifest["start_url"] == "./"
    assert (SITE / ".nojekyll").exists()
    print(f"static site checks passed ({len(demos)} demos, {len(parser.ids)} HTML IDs)")


if __name__ == "__main__":
    main()
