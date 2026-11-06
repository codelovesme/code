#!/usr/bin/env python3
"""Independently audit both current-site datasets and optional downloaded artifacts."""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import statistics

REPO = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def audit(folder, count, groups_count, artifacts=None):
    report = json.loads((folder / 'report.json').read_text())
    raw = list(csv.DictReader((folder / 'samples.csv').open()))
    assert report['status'] == 'complete'
    assert len(raw) == len(report['samples']) == count
    groups = collections.defaultdict(list)
    for saved, row in zip(report['samples'], raw):
        assert set(saved) == set(row)
        for key in saved:
            value = float(row[key]) if key == 'elapsed_seconds' else int(row[key]) if key in ['sample', 'peak_rss_kib'] else row[key]
            assert saved[key] == value, (key, saved[key], value)
        groups[row['workload'], row['group'], row['mode']].append(row)
    assert len(groups) == len(report['summary']) == groups_count
    assert {(r['workload'], r['group'], r['mode']) for r in report['summary']} == set(groups)
    for summary in report['summary']:
        rows = groups[summary['workload'], summary['group'], summary['mode']]
        n = 5 if summary['group'] == 'compile' else 15
        assert len(rows) == summary['n'] == n
        assert sorted(int(r['sample']) for r in rows) == list(range(1, n + 1))
        times = [float(r['elapsed_seconds']) for r in rows]
        rss = [int(r['peak_rss_kib']) for r in rows]
        assert min(times) > 0 and min(rss) >= 0
        for name, aggregate in [('median', statistics.median), ('min', min), ('max', max)]:
            assert summary[name + '_seconds'] == aggregate(times)
            assert summary[name + '_peak_rss_kib'] == aggregate(rss)
        assert summary['mad_seconds'] == statistics.median(abs(x - statistics.median(times)) for x in times)
        assert summary['zero_rss_samples'] == rss.count(0)
    for name, item in report['sources'].items():
        source = REPO / name
        assert source.stat().st_size == item['bytes'] and digest(source) == item['sha256'], name
    if artifacts:
        def verify(item, subdir):
            path = artifacts / subdir / Path(item['path']).name
            assert path.stat().st_size == item['bytes'] and digest(path) == item['sha256'], path
        for build in report['builds']:
            verify(build['artifact'], 'binaries')
            if 'source' in build:
                verify(build['source'], 'sources')
        wrapper = report['measurement_wrapper']['artifact'] if 'measurement_wrapper' in report else report['wrapper']
        verify(wrapper, 'binaries')
        if 'measurement_wrapper' in report:
            assert digest(REPO / 'benchmarks/language-comparison/peak.c') == report['measurement_wrapper']['source']['sha256']
        for name, item in report.get('warm_native_artifact', {}).items():
            assert digest(artifacts / 'warm-cache/run' / name) == item['sha256']
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('results', type=Path)
    parser.add_argument('--artifacts', type=Path)
    args = parser.parse_args()
    a = audit(args.results / 'comparison', 485, 35, args.artifacts / 'comparison' if args.artifacts else None)
    b = audit(args.results / 'history', 300, 20, args.artifacts / 'history' if args.artifacts else None)
    release, history = a['public_release'], b['release']
    spec = json.loads((REPO / 'benchmarks/website-evidence/releases/v2.23.2.json').read_text())
    assert release['tag'] == history['tag'] == spec['tag']
    assert release['source_commit'] == history['source'] == spec['source_commit']
    for mode, field in [('launcher', 'launcher_sha256'), ('driver', 'driver_sha256')]:
        assert release[mode]['sha256'] == history[mode]['sha256'] == spec[field]
    assert a['source_revision'] == b['source_revision']
    assert a['environment']['github_run_id'] == b['environment']['github_run_id'] != None
    assert a['environment']['pinned_logical_cpu'] == b['environment']['pinned_cpu']
    assert a['environment']['os_release'] == b['environment']['os_release']
    assert a['release_manifest']['sha256'] == b['release_manifest']['sha256'] == digest(REPO / 'benchmarks/website-evidence/releases/v2.23.2.json')
    print('785 CSV/JSON rows, all 55 summaries/ranges/MAD, source hashes and shared host/CPU/public release verified' + ('; native/source/wrapper/warm-cache artifact hashes verified' if args.artifacts else '; artifact bytes not requested'))


if __name__ == '__main__':
    main()
