"""Render same-page comparison charts from the verified public-release report."""
import html
import json
from pathlib import Path

EVIDENCE = Path('benchmarks/website-evidence/results/2026-11-04-v2.23.0/report.json')


def render(repo_root):
    report = json.loads((repo_root / EVIDENCE).read_text())
    if report['status'] != 'complete' or report['public_release']['tag'] != 'v2.23.0':
        raise ValueError('Comparison requires the reviewed public v2.23.0 dataset')
    rows = {(r['group'], r['workload'], r['mode']): r for r in report['summary']}

    def value(row, metric, statistic='median'):
        return row[f'{statistic}_seconds'] * 1000 if metric == 'time' else row[f'{statistic}_peak_rss_kib'] / 1024

    def number(group, workload, mode, metric='time', statistic='median'):
        field = metric if statistic == 'median' else metric + '-' + statistic
        key = html.escape('|'.join((group, workload, mode, field)), quote=True)
        return f'<span data-evidence="{key}">{value(rows[group, workload, mode], metric, statistic):.3f}</span>'

    def chart(name, caption, entries, metric, maximum):
        unit = 'ms' if metric == 'time' else 'MiB'
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
    return {
        '__PERF_NATIVE__': chart('compiled-runtime', 'Prebuilt program runtime', native, 'time', 15),
        '__PERF_SOURCE__': chart('source-runtime', 'Source-run program runtime', source, 'time', 1200),
        '__PERF_COMPILE__': chart('compile', 'Compile and link time', compilers, 'time', 80),
        '__PERF_MEMORY__': chart('memory', 'Application peak resident memory', memory, 'rss', 55),
        '__PERF_SNAPSHOTS__': chart('snapshot-memory', 'Retained snapshot memory', snapshots, 'rss', 280),
        '__PERF_BROADER__': ''.join(broad),
        '__PERF_ALL__': ''.join(all_results),
    }
