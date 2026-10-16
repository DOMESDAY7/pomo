"""Trunk post_build hook: derive en/index.html from the built index.html.

Blocks between <!-- i18n:NAME --> and <!-- /i18n:NAME --> markers are replaced
with the matching block from i18n/en.html. The page lives one level down, so a
<base href="../"> keeps every relative URL (icons, manifest, sw.js, links)
pointing at the site root.
"""

import os
import re
import sys
from pathlib import Path

BLOCK = re.compile(r"<!-- i18n:(\w+) -->(.*?)<!-- /i18n:\1 -->", re.S)


def main() -> None:
    staging = Path(os.environ.get("TRUNK_STAGING_DIR", "dist"))
    source = Path(os.environ.get("TRUNK_SOURCE_DIR", ".")) / "i18n" / "en.html"

    page = (staging / "index.html").read_text(encoding="utf-8")
    blocks = {m.group(1): m.group(0) for m in BLOCK.finditer(source.read_text(encoding="utf-8"))}

    found = set()

    def swap(m: re.Match) -> str:
        name = m.group(1)
        if name not in blocks:
            sys.exit(f"build_en: no '{name}' block in {source}")
        found.add(name)
        return blocks[name]

    page = BLOCK.sub(swap, page)
    missing = set(blocks) - found
    if missing:
        sys.exit(f"build_en: markers missing from index.html: {sorted(missing)}")

    page, n = re.subn(r'<html lang="fr">', '<html lang="en">', page, count=1)
    if n != 1:
        sys.exit('build_en: <html lang="fr"> not found')
    page = page.replace("<head>", '<head>\n    <base href="../" />', 1)

    out = staging / "en"
    out.mkdir(exist_ok=True)
    (out / "index.html").write_text(page, encoding="utf-8")
    print(f"build_en: wrote {out / 'index.html'}")


if __name__ == "__main__":
    main()
