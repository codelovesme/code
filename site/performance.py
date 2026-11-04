"""Render the verified public-release evidence without duplicating its numbers."""
import html
import json
from pathlib import Path

EVIDENCE = Path('benchmarks/website-evidence/results/2026-11-04-v2.23.0/report.json')


def render(repo_root):
    report = json.loads((repo_root / EVIDENCE).read_text())
    if report['status'] != 'complete' or report['public_release']['tag'] != 'v2.23.0':
        raise ValueError('Performance page requires the reviewed public v2.23.0 dataset')
    rows = {(r['group'], r['workload'], r['mode']): r for r in report['summary']}

    def number(group, workload, mode, metric='time'):
        row = rows[group, workload, mode]
        value = row['median_seconds'] * 1000 if metric == 'time' else row['median_peak_rss_kib'] / 1024
        key = html.escape('|'.join((group, workload, mode, metric)), quote=True)
        return f'<span data-evidence="{key}">{value:.3f}</span>'

    def table_row(group, workload, mode, label):
        return (f'<tr><th scope="row">{html.escape(label)}</th>'
                f'<td>{number(group, workload, mode)} ms</td>'
                f'<td>{number(group, workload, mode, "rss")} MiB</td></tr>')

    bars = []
    for mode, label, color in [('Code release', 'Code', 'code'), ('Rust O3', 'Rust O3', 'rust'), ('C O2', 'C O2', 'c')]:
        width = rows['prebuilt', 'sum_squares', mode]['median_seconds'] * 1000 / 15 * 100
        bars.append(f'<div class="perf-bar-row"><span>{label}</span>'
                    f'<span class="perf-track" aria-hidden="true"><i class="perf-bar {color}" style="width:{width:.3f}%"></i></span>'
                    f'<strong>{number("prebuilt", "sum_squares", mode)} ms</strong></div>')

    broad = []
    for workload, label in [('binary_trees', 'Binary trees'), ('dictionary', 'Dictionary'), ('points', 'Point records'), ('spectral_norm', 'Spectral norm'), ('string_build', 'Build text')]:
        cells = ''.join(f'<td>{number("prebuilt", workload, mode)}</td>' for mode in ['Code release', 'C O2', 'Rust O3'])
        broad.append(f'<tr><th scope="row">{label}</th>{cells}</tr>')

    paths = []
    for group, mode, label, description in [
        ('prebuilt', 'Code release', 'Prebuilt', 'Launch the native executable. Build time excluded.'),
        ('source', 'Code warm run', 'Cached run', 'Run source with a valid native build already cached.'),
        ('source', 'Code cold run', 'Cold run', 'Compile and run source with an empty Code cache.'),
        ('source', 'Code interpret', 'Interpret', 'Run source through the bytecode interpreter.')]:
        paths.append(f'<article class="module-panel"><h3>{label}</h3>'
                     f'<p class="perf-value">{number(group, "sum_squares", mode)} <small>ms</small></p>'
                     f'<p>{description}</p><p class="perf-rss">Peak RSS: {number(group, "sum_squares", mode, "rss")} MiB</p></article>')

    snapshots = [table_row('prebuilt', 'snapshots', mode, mode) for mode in [
        'Code automatic COW', 'C eager clone', 'Rust Vec clone', 'C explicit COW', 'Rust Rc COW']]
    compilers = [table_row('compile', 'sum_squares', mode, mode) for mode in ['Code release', 'C O2', 'C O3', 'Rust O3']]
    return {
        '__PERF_NATIVE__': '\n'.join(bars),
        '__PERF_BROADER__': '\n'.join(broad),
        '__PERF_PATHS__': '\n'.join(paths),
        '__PERF_SNAPSHOTS__': '\n'.join(snapshots),
        '__PERF_COMPILERS__': '\n'.join(compilers),
    }
