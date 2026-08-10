#!/usr/bin/env python3
"""Network-free structural checks for the framework-free T4E site."""

from __future__ import annotations

import json
import re
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
        self.catalog_ids: list[str] = []
        self.theme_values: list[str] = []
        self.inline_handlers: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        if identifier := values.get("id"):
            self.ids.append(identifier)
        if demo_id := values.get("data-demo"):
            self.demo_ids.append(demo_id)
        if catalog_ids := values.get("data-catalog-ids"):
            self.catalog_ids.extend(catalog_ids.split())
        if theme_value := values.get("data-theme-value"):
            self.theme_values.append(theme_value)
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
    assert parser.theme_values == [
        "future",
        "amber",
        "green_screen",
        "terracotta",
    ], "site theme switch must mirror every T4E theme"
    assert ">RETRO<" in html, "Green Screen must use its Retro Green display name"

    for reference in parser.local_references:
        path = SITE / urlparse(reference).path.removeprefix("./")
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

    korean = re.compile(r"[\uac00-\ud7a3]")
    assert not korean.search(html), "site/index.html must use English source copy"
    assert not korean.search(
        (SITE / "demos.json").read_text(encoding="utf-8")
    ), "site/demos.json must use English source copy"

    logo_match = re.search(
        r'<pre class="ascii-logo"[^>]*>(.*?)</pre>', html, flags=re.DOTALL
    )
    assert logo_match, "complete production ASCII logo is missing"
    canonical_logo = (ROOT / "assets/branding/t4e-ascii.txt").read_text(
        encoding="utf-8"
    )
    assert logo_match.group(1).strip("\n") == canonical_logo.strip("\n"), (
        "production ASCII logo must exactly match the canonical source"
    )

    catalog_source = (ROOT / "registry/catalog.yaml").read_text(encoding="utf-8")
    tools_source = catalog_source.split("\ntools:\n", maxsplit=1)[1]
    registry_ids = set(re.findall(r"^  - id: ([a-z0-9-]+)$", tools_source, re.MULTILINE))
    missing_catalog_ids = sorted(set(parser.catalog_ids) - registry_ids)
    assert not missing_catalog_ids, (
        "site examples must exist in the T4E catalog: " + ", ".join(missing_catalog_ids)
    )
    assert f"<dt>{len(registry_ids)}</dt><dd>CATALOG TOOLS</dd>" in html, (
        "site catalog total must match registry/catalog.yaml"
    )
    support_tool_count = len(re.findall(r"^    tags: \[support\]$", tools_source, re.MULTILINE))
    interactive_app_count = len(registry_ids) - support_tool_count
    assert f"<dt>{interactive_app_count}</dt><dd>INTERACTIVE APPS</dd>" in html, (
        "site application total must match launchable catalog entries"
    )

    expected_metadata = {
        'property="og:site_name"',
        'property="og:image:alt"',
        'name="twitter:title"',
        'name="twitter:description"',
        'name="twitter:image"',
        'name="robots"',
        'rel="canonical"',
    }
    missing_metadata = sorted(item for item in expected_metadata if item not in html)
    assert not missing_metadata, "missing SEO metadata: " + ", ".join(missing_metadata)

    structured_data_match = re.search(
        r'<script type="application/ld\+json">(.*?)</script>', html, flags=re.DOTALL
    )
    assert structured_data_match, "SoftwareApplication structured data is missing"
    structured_data = json.loads(structured_data_match.group(1))
    assert structured_data["@type"] == "SoftwareApplication"
    assert structured_data["softwareVersion"] == "0.5.3"
    assert structured_data["releaseNotes"].endswith("/releases/tag/v0.5.3")
    assert structured_data["featureList"], "structured feature list must not be empty"

    styles = (SITE / "styles.css").read_text(encoding="utf-8")
    script = (SITE / "app.js").read_text(encoding="utf-8")
    for theme in parser.theme_values:
        assert f'[data-theme="{theme}"]' in styles, f"missing {theme} site palette"
    assert "localStorage" in script, "site theme choice must persist between visits"
    assert "aria-pressed" in script, "site theme switch state must be announced"

    manifest = json.loads((SITE / "site.webmanifest").read_text(encoding="utf-8"))
    assert manifest["start_url"] == "./"
    assert (SITE / ".nojekyll").exists()
    print(f"static site checks passed ({len(demos)} demos, {len(parser.ids)} HTML IDs)")


if __name__ == "__main__":
    main()
