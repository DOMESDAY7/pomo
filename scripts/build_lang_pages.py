"""Trunk post_build hook: derive one page per language from the built index.html,
then copy the standalone pages of pages/ (e.g. comparison pages) into the site.

For every i18n/<code>.html, the blocks between <!-- i18n:NAME --> and
<!-- /i18n:NAME --> markers in index.html are replaced with the matching
blocks of that file, and the result is written to <code>/index.html. Each
page lives one level down, so a <base href="../"> keeps every relative URL
(icons, manifest, sw.js, language links) pointing at the site root.
"""

import os
import re
import shutil
import sys
from pathlib import Path

BLOCK = re.compile(r"<!-- i18n:(\w+) -->(.*?)<!-- /i18n:\1 -->", re.S)


def build(page: str, source: Path, staging: Path) -> None:
    code = source.stem
    blocks = {m.group(1): m.group(0) for m in BLOCK.finditer(source.read_text(encoding="utf-8"))}
    found = set()

    def swap(m: re.Match) -> str:
        name = m.group(1)
        if name not in blocks:
            sys.exit(f"build_lang_pages: no '{name}' block in {source}")
        found.add(name)
        return blocks[name]

    out = BLOCK.sub(swap, page)
    missing = set(blocks) - found
    if missing:
        sys.exit(f"build_lang_pages: markers missing from index.html: {sorted(missing)}")

    out, n = re.subn(r'<html lang="fr">', f'<html lang="{code}">', out, count=1)
    if n != 1:
        sys.exit('build_lang_pages: <html lang="fr"> not found')
    out = out.replace("<head>", '<head>\n    <base href="../" />', 1)

    target = staging / code
    target.mkdir(exist_ok=True)
    (target / "index.html").write_text(out, encoding="utf-8")
    print(f"build_lang_pages: wrote {target / 'index.html'}")


def main() -> None:
    staging = Path(os.environ.get("TRUNK_STAGING_DIR", "dist"))
    sources = sorted((Path(os.environ.get("TRUNK_SOURCE_DIR", ".")) / "i18n").glob("*.html"))
    if not sources:
        sys.exit("build_lang_pages: no i18n/*.html found")
    page = (staging / "index.html").read_text(encoding="utf-8")
    for source in sources:
        build(page, source, staging)

    pages = Path(os.environ.get("TRUNK_SOURCE_DIR", ".")) / "pages"
    if pages.is_dir():
        shutil.copytree(pages, staging, dirs_exist_ok=True)
        print(f"build_lang_pages: copied {pages}/ into the site")


if __name__ == "__main__":
    main()
