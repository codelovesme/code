"""Render same-page comparison charts from the verified public-release report."""
import html
import json
import datetime as dt
import math
import re
from pathlib import Path

CONFIG = json.loads(Path(__file__).with_name('comparison-data.json').read_text())
EVIDENCE = Path(CONFIG['comparison'])
HISTORY_EVIDENCE = Path(CONFIG['history'])


def render(repo_root):
    report = json.loads((repo_root / EVIDENCE).read_text())
    if report['status'] != 'complete' or report['public_release']['tag'] != CONFIG['release']:
        raise ValueError('Comparison requires the reviewed public-release dataset')
    rows = {(r['group'], r['workload'], r['mode']): r for r in report['summary']}

    pilot = json.loads((repo_root / HISTORY_EVIDENCE).read_text())
    if pilot['status'] != 'complete' or pilot['release']['tag'] != CONFIG['release']:
        raise ValueError('Sharing comparison requires the reviewed public-release dataset')
    if (report['public_release']['source_commit'] != CONFIG['source_commit']
            or pilot['release']['source'] != CONFIG['source_commit']
            or report['environment']['github_run_id'] != pilot['environment']['github_run_id']
            or not report['environment']['github_run_id']
            or report['environment']['pinned_logical_cpu'] != pilot['environment']['pinned_cpu']):
        raise ValueError('Current comparison reports must share one reviewed release/host/CPU')
    for kind in ['launcher', 'driver']:
        if report['public_release'][kind]['sha256'] != pilot['release'][kind]['sha256']:
            raise ValueError('Current comparison reports must use identical public binaries')
    rows.update({('history', r['workload'], r['mode']): r for r in pilot['summary']})

    def value(row, metric, statistic='median'):
        return row[f'{statistic}_seconds'] * 1000 if metric == 'time' else row[f'{statistic}_peak_rss_kib'] / 1024

    def number(group, workload, mode, metric='time', statistic='median'):
        field = metric if statistic == 'median' else metric + '-' + statistic
        key = html.escape('|'.join((group, workload, mode, field)), quote=True)
        return f'<span data-evidence="{key}">{value(rows[group, workload, mode], metric, statistic):.3f}</span>'

    def chart(name, caption, entries, metric, maximum):
        unit = 'ms' if metric == 'time' else 'MiB'
        largest = max(value(rows[e[0], e[1], e[2]], metric) for e in entries)
        if largest > maximum:
            magnitude = 10 ** math.floor(math.log10(largest * 1.05))
            maximum = next(step * magnitude for step in [1, 2, 2.5, 5, 10]
                           if step * magnitude >= largest * 1.05)
        ranked = sorted(entries, key=lambda e: value(rows[e[0], e[1], e[2]], metric))
        bars, exact = [], []
        for group, workload, mode, label, detail in ranked:
            row = rows[group, workload, mode]
            amount = value(row, metric)
            if amount > maximum:
                raise ValueError(f'{name}: chart scale clips {mode}')
            color = ('code' if mode.startswith('Code') else 'rust' if mode.startswith('Rust')
                     else 'c' if mode.startswith('C ') else 'node' if mode == 'Node/V8' else 'python')
            bars.append(f'<li class="comparison-bar-row"><span class="comparison-label">{html.escape(label)}'
                        f'<small>{html.escape(detail)}</small></span>'
                        f'<span class="perf-track" aria-hidden="true"><i class="perf-bar {color}" style="width:{amount / maximum * 100:.6f}%"></i></span>'
                        f'<strong>{number(group, workload, mode, metric)} {unit}</strong></li>')
            exact.append(f'<tr><th scope="row">{html.escape(label)}<small>{html.escape(detail)}</small></th>'
                         f'<td>{number(group, workload, mode, metric)}</td>'
                         f'<td>{number(group, workload, mode, metric, "min")}–{number(group, workload, mode, metric, "max")}</td></tr>')
        return (f'<figure class="module-panel perf-chart comparison-chart" data-chart="{name}" data-scale-max="{maximum}" data-metric="{metric}" aria-labelledby="{name}-chart-title">'
                f'<figcaption id="{name}-chart-title">{html.escape(caption)} · median {unit} · lower is better</figcaption>'
                f'<div class="comparison-scale" aria-hidden="true"><span>0</span><span>{maximum} {unit} · linear scale</span></div>'
                '<ol class="comparison-bars">' + ''.join(bars) + '</ol></figure>'
                '<details class="module-details comparison-exact"><summary>Exact results and measured ranges</summary>'
                f'<div class="module-table-wrap" tabindex="0" role="region" aria-label="{html.escape(caption)} exact results">'
                f'<table class="module-table perf-table comparison-table"><caption>{unit} · minimum–maximum · {rows[ranked[0][:3]]["n"]} launches per entry</caption>'
                '<thead><tr><th scope="col">Entry</th><th scope="col">Median</th><th scope="col">Range</th></tr></thead><tbody>'
                + ''.join(exact) + '</tbody></table></div></details>')

    native = [('prebuilt', 'sum_squares', mode, label, detail) for mode, label, detail in [
        ('Code release', 'Code', 'Native · release'), ('Rust O3', 'Rust', 'Native · O3'),
        ('C O2', 'C · O2', 'Native'), ('C O3', 'C · O3', 'Native')]]
    source = [('source', 'sum_squares', mode, label, detail) for mode, label, detail in [
        ('Code warm run', 'Code · cached', 'Reuses a native build'),
        ('Code cold run', 'Code · cold', 'Compilation + native run'),
        ('Code interpret', 'Code · interpreted', 'Forced bytecode interpreter'),
        ('Node/V8', 'JavaScript', 'Node / V8 · JIT'), ('Python', 'Python', 'CPython')]]
    compilers = [('compile', 'sum_squares', mode, label, detail) for mode, label, detail in [
        ('Code release', 'Code', 'Release build'), ('C O2', 'C · O2', 'Compile + link'),
        ('C O3', 'C · O3', 'Compile + link'), ('Rust O3', 'Rust', 'O3 · rustc')]]
    # Cold Code runs include a compiler, so they do not belong in application RSS.
    memory = native[:2] + [native[2]] + [entry for entry in source if entry[2] != 'Code cold run']
    snapshots = [('prebuilt', 'snapshots', mode, label, detail) for mode, label, detail in [
        ('Code automatic COW', 'Code', 'Automatic sharing'), ('C eager clone', 'C · full copies', 'Eager clone'),
        ('Rust Vec clone', 'Rust · full copies', 'Vec clone'), ('C explicit COW', 'C · shared', 'Explicit COW'),
        ('Rust Rc COW', 'Rust · shared', 'Rc copy-on-write')]]
    broad = []
    workloads = [('binary_trees', 'Binary trees'), ('dictionary', 'Dictionary'), ('points', 'Point records'), ('spectral_norm', 'Spectral norm'), ('string_build', 'Build text')]
    # Normalize against the faster competitor: different workloads have different sizes.
    def relative_result(entry):
        workload, label = entry
        code = rows['prebuilt', workload, 'Code release']['median_seconds']
        fastest = min(rows['prebuilt', workload, mode]['median_seconds']
                      for mode in ['C O2', 'Rust O3'])
        return code / fastest, label

    for workload, label in sorted(workloads, key=relative_result):
        cells = ''.join(f'<td>{number("prebuilt", workload, mode)}</td>' for mode in ['Code release', 'C O2', 'Rust O3'])
        broad.append(f'<tr><th scope="row">{label}</th>{cells}</tr>')
    all_results = []
    contexts = {'prebuilt': 'Prebuilt application', 'source': 'Source launch', 'compile': 'Compiler process'}
    for row in report['summary']:
        group, workload, mode = row['group'], row['workload'], row['mode']
        all_results.append(f'<tr><th scope="row">{html.escape(workload.replace("_", " "))} · {html.escape(mode)}'
                           f'<small>{contexts[group]} · n={row["n"]}</small></th>'
                           f'<td>{number(group, workload, mode)}<small class="comparison-range">{number(group, workload, mode, "time", "min")}–{number(group, workload, mode, "time", "max")}</small></td>'
                           f'<td>{number(group, workload, mode, "rss")}<small class="comparison-range">{number(group, workload, mode, "rss", "min")}–{number(group, workload, mode, "rss", "max")}</small></td></tr>')
    sharing = [('history', 'config_64k', mode, label, detail) for mode, label, detail in [
        ('Code automatic', 'Code', 'Automatic sharing'),
        ('C eager', 'C · full copies', 'Deep copy every version'),
        ('Rust eager', 'Rust · full copies', 'Deep clone every version'),
        ('C shared', 'C · shared', 'Explicit reference counting'),
        ('Rust shared', 'Rust · shared', 'Explicit Rc sharing')]]
    history_results = []
    cases = [('config_64k', 'Configuration · 64 × 64 KiB'),
             ('config_4k', 'Configuration · 64 × 4 KiB'),
             ('document_64k', 'Text document · 64 KiB'),
             ('document_1m', 'Text document · 1 MiB')]
    for workload, label in cases:
        for entry in sharing:
            mode = entry[2]
            history_results.append(f'<tr><th scope="row">{label} · {html.escape(entry[3])}'
                                   f'<small>{html.escape(entry[4])} · n=15</small></th>'
                                   f'<td>{number("history", workload, mode)}<small class="comparison-range">{number("history", workload, mode, "time", "min")}–{number("history", workload, mode, "time", "max")}</small></td>'
                                   f'<td>{number("history", workload, mode, "rss")}<small class="comparison-range">{number("history", workload, mode, "rss", "min")}–{number("history", workload, mode, "rss", "max")}</small></td></tr>')
    env = report['environment']
    date = dt.datetime.fromisoformat(report['sampling_started_at_utc'])
    measured_date = f"{date.day} {date.strftime('%B %Y')}"
    def span(stamp):
        return dt.datetime.fromisoformat(stamp).strftime('%H:%M:%S')
    windows = (f"Comparison: {span(report['sampling_started_at_utc'])}–{span(report['sampling_finished_at_utc'])} UTC. "
               f"Sharing: {span(pilot['sampling_started'])}–{span(pilot['sampling_finished'])} UTC.")
    os_name = next(line.split('=', 1)[1].strip('"') for line in env['os_release'].splitlines() if line.startswith('PRETTY_NAME='))
    cpu = re.search(r'^Model name:\s*(.+)$', env['lscpu'], re.M).group(1)
    machine = f"{os_name}, {cpu}, {len(env['initial_allowed_cpus'])} allowed logical CPUs; pinned to CPU {env['pinned_logical_cpu']}."
    tools = env['toolchains']
    versions = (f"Public Code {CONFIG['release']} (release source {CONFIG['source_commit'][:12]}). "
                f"{tools['gcc']}; {tools['rustc']}; {tools['python3']}; Node {tools['node']} / V8 {env['v8']}. "
                "C uses O2/O3 and Rust O3 with native CPU targeting; Code uses its public release pipeline.")
    snapshot = rows['prebuilt', 'snapshots', 'Code automatic COW']
    eager = rows['prebuilt', 'snapshots', 'Rust Vec clone']
    saved_rss = (1 - value(snapshot, 'rss') / value(eager, 'rss')) * 100
    snapshot_claim = (f"Code uses {abs(saved_rss):.1f}% {'less' if saved_rss >= 0 else 'more'} peak RSS than Rust Vec clones on this program, "
                      f"but takes {value(snapshot, 'time') / value(eager, 'time'):.2f}× the time.")
    fast_shared = all(value(rows['prebuilt', 'snapshots', mode], metric) < value(snapshot, metric)
                      for mode in ['C explicit COW', 'Rust Rc COW'] for metric in ['time', 'rss'])
    snapshot_claim += ' Explicit C/Rust copy-on-write is faster and leaner here.' if fast_shared else ' Explicit C/Rust copy-on-write results are shown alongside.'
    snapshot_claim += ' Complete-program times are in All results.'
    automatic = rows['history', 'config_64k', 'Code automatic']
    deep = rows['history', 'config_64k', 'Rust eager']
    shared = rows['history', 'config_64k', 'Rust shared']
    speed = value(deep, 'time') / value(automatic, 'time')
    memory_saved = (1 - value(automatic, 'rss') / value(deep, 'rss')) * 100
    sharing_claim = (f"On this program, Code is <strong>{speed:.2f}× as fast and uses {memory_saved:.2f}% less peak RSS</strong> than Rust deep clones. "
                     f"Explicitly shared Rust takes {value(shared, 'time'):.3f} ms versus Code’s {value(automatic, 'time'):.3f} ms; "
                     "explicitly shared C is shown too. Code provides sharing automatically rather than requiring a shared representation in application code.")
    document = rows['history', 'document_1m', 'Code automatic']
    rust_document = rows['history', 'document_1m', 'Rust shared']
    document_claim = ("The smaller configuration history also benefits from automatic sharing versus full copies. "
                      f"Editing the text itself is different: for 1 MiB document revisions, Code takes {value(document, 'time'):.3f} ms "
                      f"versus {value(rust_document, 'time'):.3f} ms for explicitly shared Rust—Code takes {value(document, 'time') / value(rust_document, 'time'):.2f}× the time. "
                      "Both document sizes and every baseline remain below.")
    base = f"https://github.com/codelovesme/code/blob/{CONFIG['evidence_commit']}/"
    links = [('Full report ↗', CONFIG['readme']), ('Comparison CSV ↗', str(EVIDENCE.parent / 'samples.csv')),
             ('Comparison JSON ↗', str(EVIDENCE)), ('Sharing CSV ↗', str(HISTORY_EVIDENCE.parent / 'samples.csv')),
             ('Sharing JSON ↗', str(HISTORY_EVIDENCE))]
    evidence_links = ''.join(f'<a href="{html.escape(base + path, quote=True)}">{label}</a>' for label, path in links)
    total = len(report['samples']) + len(pilot['samples'])
    return {
        '__CURRENT_RELEASE__': html.escape(CONFIG['release']),
        '__MEASURED_DATE__': html.escape(measured_date),
        '__COMPARISON_WINDOWS__': html.escape(windows),
        '__COMPARISON_MACHINE__': html.escape(machine),
        '__COMPARISON_VERSIONS__': html.escape(versions),
        '__SNAPSHOT_CLAIM__': html.escape(snapshot_claim),
        '__SHARING_CLAIM__': sharing_claim,
        '__DOCUMENT_CLAIM__': html.escape(document_claim),
        '__CURRENT_EVIDENCE_LINKS__': evidence_links,
        '__MEASUREMENT_TOTAL__': str(total),
        '__HISTORY_TIME__': chart('configuration-runtime', 'Configuration history runtime', sharing, 'time', 170),
        '__HISTORY_MEMORY__': chart('configuration-memory', 'Configuration history peak resident memory', sharing, 'rss', 280),
        '__HISTORY_ALL__': ''.join(history_results),
        '__PERF_NATIVE__': chart('compiled-runtime', 'Prebuilt program runtime', native, 'time', 15),
        '__PERF_SOURCE__': chart('source-runtime', 'Source-run program runtime', source, 'time', 1200),
        '__PERF_COMPILE__': chart('compile', 'Compile and link time', compilers, 'time', 80),
        '__PERF_MEMORY__': chart('memory', 'Application peak resident memory', memory, 'rss', 55),
        '__PERF_SNAPSHOTS__': chart('snapshot-memory', 'Retained snapshot memory', snapshots, 'rss', 280),
        '__PERF_BROADER__': ''.join(broad),
        '__PERF_ALL__': ''.join(all_results),
    }
