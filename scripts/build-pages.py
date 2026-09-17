#!/usr/bin/env python3
"""Assemble the GitHub Pages site into _site/ from docs/, slides/, and markdown write-ups."""

from __future__ import annotations

import re
import shutil
import sys
from pathlib import Path

try:
    import markdown
except ImportError:
    sys.stderr.write("error: the 'markdown' package is required (pip install markdown)\n")
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "_site"
DOCS = ROOT / "docs"
TEMPLATE = (DOCS / "assets" / "page.template.html").read_text(encoding="utf-8")
GITHUB = "https://github.com/davidparry/tdd-bdd-agentic/blob/trunk/"

SITE_PATHS = {
    "student-follow-along.md": "../workshop/",
    "harness/README.md": "../harness/",
    "speaking.md": "../speaking/",
    "student-follow-docs/setup-mcp.md": "../setup/",
    "student-follow-docs/greenfield-flow.md": "../greenfield/",
    "student-follow-docs/pi-path.md": "../pi/",
    "slides/index.html": "../talk/",
    "slides/index.html?30": "../talk30/",
}

PAGES = [
    {
        "src": "student-follow-along.md",
        "dest": "workshop/index.html",
        "title": "Workshop follow-along",
        "description": "Step-by-step companion for the 60-minute spec workshop.",
    },
    {
        "src": "harness/README.md",
        "dest": "harness/index.html",
        "title": "spec harness",
        "description": "Spec-driven BDD/TDD harness with an embedded MCP server.",
    },
    {
        "src": "speaking.md",
        "dest": "speaking/index.html",
        "title": "Speaking",
        "description": "Turn Off the Wi-Fi: a conference session on spec-driven development that delivers on a local model.",
    },
    {
        "src": "student-follow-docs/setup-mcp.md",
        "dest": "setup/index.html",
        "title": "MCP setup",
        "description": "Register the spec-driven-server MCP server with your agent.",
    },
    {
        "src": "student-follow-docs/greenfield-flow.md",
        "dest": "greenfield/index.html",
        "title": "Greenfield flow",
        "description": "The order files would be created in a true greenfield spec-first project.",
    },
    {
        "src": "student-follow-docs/pi-path.md",
        "dest": "pi/index.html",
        "title": "The pi path",
        "description": "Drive the spec-driven server from pi, a free MIT-licensed agent on a local Ollama model.",
    },
]

MERMAID_SNIPPET = """
  <script type="module">
    import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs";
    mermaid.initialize({ startOnLoad: true, theme: "dark" });
  </script>
"""

LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
MERMAID_RE = re.compile(r"```mermaid\s*\n(.*?)```", re.DOTALL)


def repo_relative(path: Path) -> str:
    return path.resolve().relative_to(ROOT).as_posix()


def rewrite_links(text: str, src: Path) -> str:
    src_dir = src.parent

    def repl(match: re.Match[str]) -> str:
        label, href = match.group(1), match.group(2)
        if href.startswith(("http://", "https://", "mailto:", "#")):
            return match.group(0)
        path_part, _, fragment = href.partition("#")
        if not path_part:
            return match.group(0)
        resolved = (src_dir / path_part).resolve()
        try:
            rel = repo_relative(resolved)
        except ValueError:
            return match.group(0)
        mapped = SITE_PATHS.get(rel)
        if mapped:
            suffix = f"#{fragment}" if fragment else ""
            return f"[{label}]({mapped}{suffix})"
        suffix = f"#{fragment}" if fragment else ""
        return f"[{label}]({GITHUB}{rel}{suffix})"

    return LINK_RE.sub(repl, text)


def mermaid_to_div(text: str) -> tuple[str, bool]:
    found = False

    def repl(match: re.Match[str]) -> str:
        nonlocal found
        found = True
        return f'\n<div class="mermaid">\n{match.group(1).strip()}\n</div>\n'

    return MERMAID_RE.sub(repl, text), found


def render_page(src: Path, title: str, description: str, body_md: str, has_mermaid: bool) -> str:
    html_body = markdown.markdown(
        body_md,
        extensions=["tables", "fenced_code", "sane_lists", "toc"],
    )
    return (
        TEMPLATE.replace("{{title}}", title)
        .replace("{{description}}", description)
        .replace("{{root}}", "../")
        .replace("{{mermaid}}", MERMAID_SNIPPET if has_mermaid else "")
        .replace("{{body}}", html_body)
    )


def copy_static() -> None:
    if SITE.exists():
        shutil.rmtree(SITE)
    (SITE / "assets").mkdir(parents=True)
    (SITE / "talk").mkdir()
    (SITE / "talk30").mkdir()
    shutil.copy2(DOCS / "index.html", SITE / "index.html")
    for item in (DOCS / "assets").iterdir():
        if item.name == "page.template.html":
            continue
        shutil.copy2(item, SITE / "assets" / item.name)
    # One deck, two cuts: the same file picks its track from the URL, so /talk/
    # serves the 60-minute workshop and /talk30/ the 30-minute session.
    shutil.copy2(ROOT / "slides" / "index.html", SITE / "talk" / "index.html")
    shutil.copy2(ROOT / "slides" / "index.html", SITE / "talk30" / "index.html")
    # The harness manual is an mdBook committed pre-built (mdbook build manual).
    shutil.copytree(DOCS / "manual", SITE / "manual")
    (SITE / ".nojekyll").write_text("", encoding="utf-8")


def main() -> None:
    copy_static()
    for page in PAGES:
        src = ROOT / page["src"]
        text = src.read_text(encoding="utf-8")
        text = rewrite_links(text, src)
        text, has_mermaid = mermaid_to_div(text)
        html = render_page(src, page["title"], page["description"], text, has_mermaid)
        dest = SITE / page["dest"]
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(html, encoding="utf-8")
        print(f"wrote {dest.relative_to(ROOT)}")
    print(f"site ready at {SITE}")


if __name__ == "__main__":
    main()
