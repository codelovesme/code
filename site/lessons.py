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
        items.append(f'<li><span class="lesson-number">{index if index else "Start"}</span>'
                     f'<div><h3>{heading}</h3><p>{html.escape(lesson["goal"])}</p>'
                     f'<p class="lesson-outcome">{html.escape(lesson["outcome"])}</p></div>'
                     f'<span class="lesson-state">{state}</span></li>')
    return '<ol class="learning-path">' + ''.join(items) + '</ol>'


def render_lesson(site_dir, plan, lesson, previews):
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
            raise ValueError('The quick-start edit must match exactly once')
        variants.append({'name': lesson['id'] + '-edit', 'code': example['code'].replace(edit['find'], edit['replace']), 'output': edit['output']})
        replacements.update({'__LESSON_EDIT__': html.escape(edit['replace']), '__LESSON_EDIT_OUTPUT__': html.escape(edit['output'])})
    result = (site_dir / 'lesson.html').read_text()
    for key, value in replacements.items():
        result = result.replace(key, value)
    return result, variants
