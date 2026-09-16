"""lower_web: a MkDocs page with its TMark constructs spliced (web-profile.md)."""

from __future__ import annotations

import pytest

import tmark

FINDINGS = """---
press:
  declare:
    counters:
      fw: {name: Finding, format: "FW-{n:02d}"}
---

# Findings {#sec:findings}

| Id | Finding |
| --- | --- |
| #(fw:watchdog) | The watchdog does not fire. |

::: texsmith.core.counters
    options:
      show_source: false

See @fw:watchdog and @sec:findings.
"""

INDEX = """# Overview

The blocking issue is finding @fw:watchdog, see @sec:findings.

A Ruby interpolation such as #{user.name} must stay literal.
"""


def test_lower_web_splices_and_keeps_the_rest():
    doc = tmark.parse(FINDINGS, file="findings.md")
    res = tmark.resolve(doc, None, {"path": "findings.md", "numbering": "all"})
    for resolved in (res, res["handle"]):
        out = tmark.lower_web(FINDINGS, doc, resolved)
        assert set(out) == {"text", "diagnostics", "bibliography"}
        text = out["text"]
        assert '<span class="ts-counter" id="fw:watchdog" data-counter="fw" data-key="watchdog">FW-01</span>' in text
        assert "See [FW-01](#fw:watchdog) and [Findings](#sec:findings)." in text
        assert "::: texsmith.core.counters\n    options:\n      show_source: false\n" in text
        assert out["diagnostics"] == []
        assert out["bibliography"] is None
    # Without a resolution, the page is resolved on its own with every series numbered.
    assert tmark.lower_web(FINDINGS, doc)["text"] == out["text"]


def test_lower_web_links_siblings_from_the_site_map():
    findings = tmark.parse(FINDINGS, file="findings.md")
    site = tmark.resolve(findings, None, {"path": "findings.md", "numbering": "all"})
    index = tmark.parse(INDEX, file="index.md")
    resolved = tmark.resolve(index, None, {"path": "index.md", "numbering": "all", "book": site["book"]})
    text = tmark.lower_web(INDEX, index, resolved)["text"]
    assert "finding [FW-01](findings.md#fw:watchdog), see [Findings](findings.md#sec:findings)." in text
    assert "#{user.name} must stay literal." in text


def test_lower_web_options_and_diagnostics():
    doc = tmark.parse(INDEX)
    out = tmark.lower_web(INDEX, doc, None, None, {"sections": "number", "citations": "passthrough", "css_prefix": "x-"})
    assert "[?fw:watchdog]" in out["text"]
    with pytest.raises(TypeError, match="options"):
        tmark.lower_web(INDEX, doc, None, None, {"sections": "chapter"})
    with pytest.raises(TypeError, match="options"):
        tmark.lower_web(INDEX, doc, None, None, {"colour": "red"})
    with pytest.raises(TypeError, match="resolved"):
        tmark.lower_web(INDEX, doc, {"labels": []})


def test_lower_web_includes_through_the_loader():
    class L:
        def load(self, from_path, rel):
            return "## Part {#sec:part}\n\nInside, see @sec:part.\n" if rel == "part.md" else None

    text = "{include}(part.md)\n\nSee @sec:part.\n"
    doc = tmark.parse(text)
    out = tmark.lower_web(text, doc, None, L())
    assert out["text"] == "## Part {#sec:part}\n\nInside, see [Part](#sec:part).\n\nSee [Part](#sec:part).\n"


def test_lower_web_splices_a_fence_include_through_the_loader():
    """`include="file"` on a fence: superfences refuses the option, so the
    lowering reads the file and drops the attribute (spec §Includes)."""

    class L:
        def __init__(self):
            self.seen = []

        def load(self, from_path, rel):
            self.seen.append((from_path, rel))
            return "int main(void) { return 0; }\n" if rel == "src/iota.c" else None

    text = '```c title="iota.c" include="src/iota.c"\n```\n\n```c include="src/gone.c"\n```\n'
    doc = tmark.parse(text, file="guide/page.md")
    loader = L()
    res = tmark.resolve(doc, None, {"path": "guide/page.md"})
    out = tmark.lower_web(text, doc, res, loader, None)
    assert loader.seen == [("guide/page.md", "src/iota.c"), ("guide/page.md", "src/gone.c")]
    assert '```c title="iota.c"\nint main(void) { return 0; }\n```' in out["text"]
    assert "include=" not in out["text"].split("\n\n")[0]
    # A file the loader cannot serve keeps the fence and is reported.
    assert '```c include="src/gone.c"\n```' in out["text"]
    assert [d["code"] for d in out["diagnostics"]] == ["include-missing"]
