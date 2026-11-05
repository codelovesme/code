"""One ordered lesson plan; only ready content becomes a route or a link."""
import html
import json
from pathlib import PurePosixPath


def load_plan(site_dir):
    plan = json.loads((site_dir / 'lessons.json').read_text())
    routes = set()
    for lesson in plan:
        route = PurePosixPath(lesson['route'])
        if route.is_absolute() or '..' in route.parts or route.parent != PurePosixPath('learn') or route.suffix != '.html':
            raise ValueError(f'Invalid lesson route: {route}')
        if str(route) in routes:
            raise ValueError(f'Duplicate lesson route: {route}')
        routes.add(str(route))
        if lesson['ready'] and not (site_dir / lesson['source']).is_file():
            raise ValueError(f'Ready lesson has no content: {lesson["id"]}')
    return plan


def label(index):
    return 'Quick start' if index == 0 else f'Lesson {index} of 4'


def outline(plan, current=None, include_starter=True):
    items = []
    for index, lesson in enumerate(plan):
        if index == 0 and not include_starter:
            continue
        title = html.escape(lesson['title'])
        current_attr = ' aria-current="step"' if lesson['id'] == current else ''
        heading = (f'<a href="__ROOT__{lesson["route"]}"{current_attr}>{title}</a>' if lesson['ready']
                   else f'<span{current_attr}>{title}</span>')
        state = 'Ready' if lesson['ready'] else 'Coming soon'
        items.append(f'<li id="path-{lesson["id"]}"><span class="lesson-number">{index if index else "Start"}</span>'
                     f'<div><h3>{heading}</h3><p>{html.escape(lesson["goal"])}</p>'
                     f'<p class="lesson-outcome">{html.escape(lesson["outcome"])}</p></div>'
                     f'<span class="lesson-state">{state}</span></li>')
    return '<ol class="learning-path">' + ''.join(items) + '</ol>'


def lesson_assets(site_dir, greeting, plan):
    paths = {path for lesson in plan if lesson['ready']
             for path in lesson.get('snippets', {}).values()}
    assets = {path: (site_dir / 'lesson-examples' / path).read_text().replace('__HOME_CODE__', greeting.strip())
              for path in paths}
    if not any(lesson['id'] == 'first-program' and lesson['ready'] for lesson in plan):
        return assets
    assets['first-program/start.sh'] = '#!/bin/sh\nset -eu\n' + '\n'.join(
        assets[f'first-program/{name}.sh'] for name in ['install', 'project', 'run'])
    assets['first-program/hello.code'] = greeting
    return assets


def render_lesson(site_dir, plan, lesson, previews, assets, examples):
    index = plan.index(lesson)
    content = (site_dir / lesson['source']).read_text()
    neighbors = []
    for position, text in [(index - 1, 'Previous'), (index + 1, 'Next')]:
        if not 0 <= position < len(plan):
            continue
        neighbor = plan[position]
        title = html.escape(neighbor['title'])
        caption = f'← Previous: {title}' if text == 'Previous' else f'Next: {title} →'
        neighbors.append(f'<a href="__ROOT__{neighbor["route"]}" rel="{ "prev" if text == "Previous" else "next"}">{caption}</a>' if neighbor['ready']
                         else f'<span class="lesson-pending">{text}: {title} · coming soon</span>')
    variants = []
    replacements = {
        '__LESSON_LABEL__': label(index), '__LESSON_TITLE__': html.escape(lesson['title']),
        '__LESSON_GOAL__': html.escape(lesson['goal']),
        '__LESSON_OUTLINE__': outline(plan, current=lesson['id']),
        '__LESSON_CONTENT__': content, '__LESSON_NEIGHBORS__': ''.join(neighbors),
    }
    if 'example' in lesson:
        example = next(item for item in previews if item['name'] == lesson['example'])
        variants.append({'name': lesson['id'], 'code': example['code'], 'output': example['output']})
        edit = lesson['edit']
        if example['code'].count(edit['find']) != 1:
            raise ValueError('A lesson edit must match exactly once')
        variants.append({'name': lesson['id'] + '-edit', 'code': example['code'].replace(edit['find'], edit['replace']), 'output': edit['output']})
        replacements.update({'__LESSON_EDIT__': html.escape(edit['replace']), '__LESSON_EDIT_OUTPUT__': html.escape(edit['output'])})
    for step in lesson.get('steps', []):
        example = examples[step['example']]
        variants.append({'name': lesson['id'] + '-' + step['id'], 'code': example['code'], 'output': step['output']})
        for edit_index, edit in enumerate(step.get('edits', [])):
            if example['code'].count(edit['find']) != 1:
                raise ValueError('A step edit must match exactly once')
            variants.append({'name': lesson['id'] + '-' + step['id'] + f'-edit-{edit_index}',
                             'code': example['code'].replace(edit['find'], edit['replace']),
                             'output': edit['output']})
        code_id = f'{lesson["id"]}-{step["id"]}-code'
        panel = (f'<figure class="module-panel lesson-example"><figcaption>{html.escape(step["title"])}</figcaption>'
                 f'<pre class="codeblock" id="{code_id}" tabindex="0">{html.escape(example["code"].strip())}</pre>'
                 f'<div class="lesson-example-actions"><button class="tool-btn" data-copy="{code_id}" hidden>Copy code</button>'
                 f'<a class="btn btn-primary" aria-label="Run example: {html.escape(step["title"])}" '
                 f'href="__ROOT__playground.html?example={step["example"]}#playground">Run this example →</a></div>'
                 '<p role="status"></p></figure>'
                 f'<div class="lesson-result"><span>Expected output</span><pre>{html.escape(step["output"])}</pre></div>')
        replacements[f'__STEP_{step["id"].upper()}__'] = panel
        replacements[f'__STEP_{step["id"].upper()}_OUTPUT__'] = html.escape(step['output'])
    replacements.update({f'__LESSON_{name}__': html.escape(assets[path].strip())
                         for name, path in lesson.get('snippets', {}).items()})
    result = (site_dir / 'lesson.html').read_text()
    for key, value in replacements.items():
        result = result.replace(key, value)
    return result, variants
