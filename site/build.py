#!/usr/bin/env python3
"""Build the static Code site with a shared shell and the real fixture gallery.

The playground uses a pinned published engine, not a source-built runtime.
Native-link, stress and intentionally invalid fixtures remain excluded as
before; check_examples.mjs checks the generated collection and visible previews
against the exact engine included in the Pages artifact.

Usage: build.py <repo-root> <dist-dir>
"""
import html
import json
import os
import re
import shutil
import sys
from pathlib import Path

from performance import render as render_performance
from lessons import load_plan, outline, render_lesson, lesson_assets

# Ordered: first matching prefix wins. `fail_`/`emit_`/`particle_` are the
# most specific and must be checked before the generic value prefixes.
CATEGORY_BY_PREFIX = [
    ("fail_", "Errors"),
    ("emit_", "Particles"),
    ("particle_", "Particles"),
    ("loop_", "Loops"),
    ("if_", "Conditionals"),
    ("block_", "Scoping"),
    ("assert_", "Assertions"),
    ("let_", "Values & Expressions"),
    ("variables_", "Values & Expressions"),
    ("literal_", "Values & Expressions"),
    ("array_", "Values & Expressions"),
    ("object_", "Values & Expressions"),
    ("nested_", "Values & Expressions"),
    ("multiline_", "Values & Expressions"),
    ("string_", "Values & Expressions"),
    ("arithmetic_", "Values & Expressions"),
    ("comparison_", "Values & Expressions"),
    ("inequality_", "Values & Expressions"),
    ("logical_", "Values & Expressions"),
    ("unary_", "Values & Expressions"),
    ("operator_", "Values & Expressions"),
    ("parens_", "Values & Expressions"),
    ("field_", "Values & Expressions"),
    ("index_", "Values & Expressions"),
    ("chained_", "Values & Expressions"),
    ("invalid_", "Values & Expressions"),
    ("values_", "Values & Expressions"),
    ("reassignment_", "Values & Expressions"),
    ("self_", "Values & Expressions"),
]
DEFAULT_CATEGORY = "Values & Expressions"
MODULE_SNIPPETS = {
    "SETUP": "get-cli.sh",
    "HELLO": "hello/main.code",
    "GREETER": "reuse/greeter.code",
    "CALLER": "reuse/main.code",
    "SECOND": "reuse/team.code",
    "COMPILED": "reuse/compiled.code",
}


def category_for(name: str) -> str:
    for prefix, cat in CATEGORY_BY_PREFIX:
        if name.startswith(prefix):
            return cat
    return DEFAULT_CATEGORY


def first_comment(source: str) -> str:
    """The fixture's first `|` comment line, stripped — the author's own
    one-line explanation of the feature."""
    for line in source.splitlines():
        s = line.strip()
        if s.startswith("|"):
            text = s[1:].strip()
            if text:
                return text
    return ""


def truncate(text: str, limit: int = 110) -> str:
    if len(text) <= limit:
        return text
    cut = text[:limit].rsplit(" ", 1)[0]
    return cut.rstrip(",;: ") + "…"


# A new language fixture may require capabilities absent from the published
# playground engine. This is deliberately version-specific, not a permanent
# exclusion: upgrading the pin makes the full fixture run through the gate.
BROWSER_INCOMPATIBILITIES = {
    "2.16.0": {
        "handler_recursion": "This engine refuses handler re-entry; the fixture remains in the language suite.",
    },
}


def engine_version(repo_root: Path) -> str:
    configured = os.environ.get("PLAYGROUND_CODE_WASM_VERSION")
    if configured:
        return configured
    workflow = (repo_root / ".github/workflows/pages.yml").read_text()
    match = re.search(r'PLAYGROUND_CODE_WASM_VERSION:\s*"([^"]+)"', workflow)
    if not match:
        raise ValueError("The Pages workflow must declare its published browser-engine pin")
    return match.group(1)


def main() -> None:
    repo_root = Path(sys.argv[1])
    dist_dir = Path(sys.argv[2])
    tests_dir = repo_root / "tests"
    site_dir = repo_root / "site"

    examples = []
    version = engine_version(repo_root)
    incompatibilities = BROWSER_INCOMPATIBILITIES.get(version, {})
    omitted = []

    # The playground's own examples come first, because they are the only
    # ones that *say* anything: every fixture in tests/ is silent, since
    # printing needs a module and a fixture that links one is held back
    # below. These link `console`, which the page supplies (see playground.mjs's
    # `consoleModule`) — so they are playground-only by nature, not by
    # exception, and `site/check_examples.mjs` runs them like the rest.
    for path in sorted(site_dir.glob("examples/*.code")):
        source = path.read_text()
        examples.append(
            {
                "name": path.stem,
                "category": "Printing",
                "description": truncate(first_comment(source)),
                "code": source,
            }
        )

    for path in sorted(tests_dir.glob("*.code")):
        name = path.stem
        if name.startswith("fail_") and name != "fail_undefined_variable":
            continue
        if name.startswith("stress_"):
            continue
        if name in incompatibilities:
            omitted.append({"name": name, "reason": incompatibilities[name]})
            continue
        source = path.read_text()
        # Native links can also occur inside handlers. The browser supplies
        # only console, so neither form belongs in this fixture gallery.
        if re.search(r"^\s*link\s", source, re.MULTILINE):
            continue
        examples.append(
            {
                "name": name,
                "category": category_for(name),
                "description": truncate(first_comment(source)),
                "code": source,
            }
        )

    # Escaping "</" guards against a fixture ever containing a "</script>"
    # substring, which would otherwise close the embedding <script> tag
    # early and corrupt the page.
    examples_json = json.dumps(examples).replace("</", "<\\/")

    dist_dir.mkdir(parents=True, exist_ok=True)
    by_name = {example["name"]: example for example in examples}
    previews = [
        {**by_name[demo["name"]], **demo}
        for demo in json.loads((site_dir / "demos.json").read_text())
    ]
    home = previews[0]
    memory = next(preview for preview in previews if preview["name"] == "value_snapshot")
    layout = (site_dir / "layout.html").read_text()
    navigation = (site_dir / "navigation.html").read_text()
    footer = (site_dir / "footer.html").read_text()
    pages = {
        "index": (
            "home", "Code — built for reuse, compiled for performance",
            "Compose programs with reusable modules. Use the interpreter, compile to native code, or target WebAssembly.",
        ),
        "learn": (
            "learn", "Learn Code — start with a working program",
            "Run your first Code program in the browser, then explore the language and native CLI.",
        ),
        "playground": (
            "playground", "Code playground — write and run",
            "Edit and run Code in your browser, with the complete collection of language examples.",
        ),
        "docs": (
            "docs", "Code documentation — reference and integration",
            "Language reference, modules, CLI commands, WebAssembly embedding and native module authoring.",
        ),
        "modules": (
            "docs", "Code modules — install, link and reuse",
            "Install a published module, reuse your own handlers with private state, and choose native or browser capabilities.",
        ),
        "execution": (
            "docs", "How Code runs — interpreter, native builds and WebAssembly",
            "Choose cached native execution, bytecode interpretation or a compiled application. See the two ways Code runs with WebAssembly.",
        ),
        "memory": (
            "docs", "Code memory and values — share storage, preserve snapshots",
            "See independent value updates, automatic reference counting and copy-on-write in a short runnable Code example.",
        ),
        "compare": (
            "performance", "Code performance — native speed, cached runs and shared memory",
            "Measured native runtime, cold and cached runs, and snapshot memory for public Code v2.23.0, with reproducible evidence.",
        ),
    }
    plan = load_plan(site_dir)
    # A reused preview/output directory must not retain a withdrawn lesson.
    for lesson in plan:
        if not lesson['ready']:
            (dist_dir / lesson['route']).unlink(missing_ok=True)
    lesson_pages = {lesson['route'][:-5]: lesson for lesson in plan if lesson['ready']}
    for name, lesson in lesson_pages.items():
        pages[name] = ('learn', f'Learn Code — {lesson["title"]}', lesson['goal'])
    lesson_examples = []
    assets = lesson_assets(site_dir, home["code"], plan)
    for filename, (page, title, description) in pages.items():
        if filename in lesson_pages:
            content, variants = render_lesson(site_dir, plan, lesson_pages[filename], previews, assets, by_name)
            lesson_examples.extend(variants)
        else:
            content = (site_dir / f"{filename}.html").read_text()
        output = layout.replace("__CONTENT__", content)
        nav = navigation
        for key in ["home", "learn", "playground", "docs", "performance"]:
            nav = nav.replace(
                f"__CURRENT_{key.upper()}__",
                'aria-current="page"' if page == key else "",
            )
        scripts = ""
        if filename in lesson_pages:
            scripts = '<script type="module" src="__ROOT__lesson.mjs"></script>'
        if page == "playground":
            scripts = (
                '<script id="examples-data" type="application/json">'
                + examples_json
                + '</script>\n<script id="featured-data" type="application/json">'
                + json.dumps(previews).replace("</", "<\\/")
                + '</script>\n<script type="module" src="playground.mjs"></script>'
            )
        replacements = {
            "__NAVIGATION__": nav, "__FOOTER__": footer,
            "__TITLE__": html.escape(title), "__DESCRIPTION__": html.escape(description),
            "__PAGE__": page, "__SCRIPTS__": scripts,
            "__CANONICAL__": "https://codelovesme.github.io/code/" + ("" if filename == "index" else filename + ".html"),
            "__ENGINE_VERSION__": html.escape(version),
            "__EXAMPLE_COUNT__": str(len(examples)),
            "__HOME_CODE__": html.escape(home["code"].strip()),
            "__HOME_OUTPUT__": html.escape(home["output"]),
            "__MEMORY_CODE__": html.escape(memory["code"].strip()),
            "__MEMORY_OUTPUT__": html.escape(memory["output"]),
        }
        if filename == "compare":
            replacements.update(render_performance(repo_root))
        if filename == "modules":
            replacements.update({
                f"__MODULE_{name}__": html.escape((site_dir / "module-examples" / path).read_text().strip())
                for name, path in MODULE_SNIPPETS.items()
            })
        replacements['__LEARNING_PATH__'] = outline(plan, include_starter=False)
        replacements['__ROOT__'] = '../' * len(Path(filename).parent.parts)
        for placeholder, value in replacements.items():
            output = output.replace(placeholder, value)
        destination = dist_dir / f'{filename}.html'
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(output)

    asset_dir = dist_dir / 'lesson-examples'
    for old in asset_dir.rglob('*'):
        if old.is_file() and old.relative_to(asset_dir).as_posix() not in assets:
            old.unlink()
    for name, content in assets.items():
        destination = dist_dir / 'lesson-examples' / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(content)
    (dist_dir / "lesson-examples.json").write_text(json.dumps(lesson_examples, indent=2) + "\n")
    (dist_dir / "examples.json").write_text(json.dumps(examples, indent=2) + "\n")
    (dist_dir / "demos.json").write_text(json.dumps(previews, indent=2) + "\n")
    (dist_dir / "browser-compatibility.json").write_text(
        json.dumps({"engine": version, "omitted": omitted}, indent=2) + "\n"
    )
    for item in omitted:
        print(f"Browser {version} omits {item['name']}: {item['reason']}")
    for name in ["site.css", "navigation.css", "navigation.js", "playground.mjs", "lesson.mjs"]:
        shutil.copyfile(site_dir / name, dist_dir / name)
    for asset in sorted(site_dir.glob("*.png")):
        shutil.copyfile(asset, dist_dir / asset.name)
    shutil.copytree(site_dir / "module-examples", dist_dir / "module-examples", dirs_exist_ok=True)
    shutil.copytree(site_dir / "execution-examples", dist_dir / "execution-examples", dirs_exist_ok=True)
    print(f"Built {len(pages)} pages and {len(examples)} examples")


if __name__ == "__main__":
    main()
