# Learn page authoring

`../lessons.json` is the ordered publication plan. Quick start is a short
browser warm-up; the four full lessons are separate tickets 219–222. A planned
lesson has a goal and outcome on the hub but no link or generated page. Do not
mark one ready until its content and commands pass that ticket's checks.

To publish a lesson:

1. Write the content fragment at its `source` path under `site/learn/`.
2. Use short `<section>` steps with IDs, headings, explanations, code and
   expected results. The wrapper in `../lesson.html` provides the goal,
   location in the path, previous/next and back-to-Learn navigation.
3. Author site links as `__ROOT__playground.html`, `__ROOT__docs.html#cli`, etc.
   The build inserts `../` on nested routes. Local `#step` anchors stay local;
   do not use a `<base>` tag or root-absolute `/...` links.
4. Copyable complete programs use a focusable `<pre id="unique-id"
   tabindex="0">`, a button with `data-copy="unique-id"`, and the single
   `lesson-copy-status` live region. `../lesson.mjs` handles clipboard failure
   by selecting the code for manual copying. It does not load a language engine.
5. Reuse canonical program files. The current optional `example`/`edit` fields
   generate Quick start's original and edited program checks from the featured
   greeting; `lesson-examples.json` joins the pinned-engine validation gate.
   Extend the checked-source manifest for future lesson examples rather than
   copying unchecked snippets into HTML. Native/module commands need their
   lesson's published-CLI checks too; browser success does not prove those.
6. Set `ready` only after verification. The shared hub/outline and neighbors
   then link the route automatically. Withdrawing readiness also removes a
   formerly generated page on a reused output directory.

No login or progress database is needed. The current step uses `aria-current`;
planned neighbors are plain text. Ready lessons provide native anchor links,
including direct URLs under the GitHub Pages `/code/` prefix. Only ready
content ships. Missing ready source, duplicate routes and unsafe routes fail
the build. Static tests exercise publishing/withdrawing the next lesson in a
temporary fixture, without putting a draft lesson on the public site.


Lesson 1 adds optional `snippets` to its record. Those paths refer to source
command templates under `site/lesson-examples/`; the builder renders the
canonical greeting into them, displays those exact strings and generates the
downloadable combined commands. Only ready lessons' assets are emitted.
Copy controls can sit in individual `figure` panels with their own status
region; `data-copy-type="shell"` gives terminal-specific feedback. Keep copy
buttons hidden in source markup; `lesson.mjs` enables them when available.
The lesson 1 native checker verifies the assembled scripts with the actual
public release rather than maintaining a second command implementation.


Lesson 2 uses `steps`: each entry has an `id`, canonical `example` name, title
and expected `output`. `__STEP_ID__` renders its full copy/run/output panel;
`__STEP_ID_OUTPUT__` renders only the same expected output (e.g. an exercise
target). Optional `edits` contain a unique `find`, `replace` and expected
`output`; each edited full program joins the published-engine gate. Programs
come from the validated `site/examples/` collection, not unchecked HTML text.
A solution panel can live inside native HTML details and still be checked.
Use the lesson's current engine syntax; separate-line array items need no
commas, and comparisons use the language's canonical ≥/≤ symbols.


Lesson 3 also declares optional `checks` for documented error boundaries. Each
references a canonical file under `site/lesson-checks/`, expected printed output
and an optional `error_prefix`. The built manifest feeds those probes into the
published-engine gate; it does not turn errors in ordinary examples into
expected successes. Keep browser programs in the console-only host, and use
explicitly local snippets for source-file imports. `data-copy-type="file"`
provides save-to-file feedback. The messages native checker verifies those
exact emitted files, binding privacy and error boundaries with public v2.23.0.


Lesson 4 completes the course with a cart receipt. Optional `downloads` maps
relative lesson-asset paths to canonical example names, e.g.
`"cart/receipt.code": "learn_cart_final"`. The builder copies the complete
program byte-for-byte and withdraws it with lesson readiness. Use snippets
for explicitly local multi-file versions, and verify their emitted run script
with the published CLI. Finish the course with Modules/Docs links rather than
an empty next lesson.
