#!/usr/bin/env python3
"""Independently verify a completed algorithm study's CSV/JSON/statistics; optionally its artifact bundle."""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import statistics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('results', type=Path)
    parser.add_argument('--artifacts', type=Path, help='downloaded bundle with binaries/ and sources/')
    args = parser.parse_args()
    report = json.loads((args.results / 'report.json').read_text())
    raw = list(csv.DictReader((args.results / 'samples.csv').open()))
    assert report['status'] == 'complete'
    assert len(raw) == len(report['samples']) == 270
    assert len(report['builds']) == len(report['summary']) == 18
    groups = collections.defaultdict(list)
    for recorded, row in zip(report['samples'], raw):
        for key in ['workload', 'group', 'mode']:
            assert recorded[key] == row[key]
        for key in ['sample', 'peak_rss_kib']:
            assert recorded[key] == int(row[key])
        assert recorded['elapsed_seconds'] == float(row['elapsed_seconds'])
        groups[row['workload'], row['group'], row['mode']].append(row)
    expected = {(f'{algorithm}_{size}', 'prebuilt', mode)
                for algorithm, size in [('matrix', 64), ('matrix', 128), ('sieve', 100000),
                                        ('sieve', 1000000), ('merge', 16384), ('merge', 65536)]
                for mode in ['Code release', 'C O2', 'Rust O3']}
    assert set(groups) == expected
    assert {(r['workload'], r['group'], r['mode']) for r in report['summary']} == expected
    assert {(r['workload'], 'prebuilt', r['mode']) for r in report['builds']} == expected
    for summary in report['summary']:
        rows = groups[summary['workload'], summary['group'], summary['mode']]
        assert len(rows) == summary['n'] == 15
        assert sorted(int(r['sample']) for r in rows) == list(range(1, 16))
        times = [float(r['elapsed_seconds']) for r in rows]
        rss = [int(r['peak_rss_kib']) for r in rows]
        assert min(times) > 0 and min(rss) >= 0
        for name, aggregate in [('median', statistics.median), ('min', min), ('max', max)]:
            assert summary[name + '_seconds'] == aggregate(times)
            assert summary[name + '_peak_rss_kib'] == aggregate(rss)
        assert summary['mad_seconds'] == statistics.median(abs(x - statistics.median(times)) for x in times)
        assert summary['zero_rss_samples'] == rss.count(0)
    if args.artifacts:
        def verify(item, directory):
            file = args.artifacts / directory / Path(item['path']).name
            assert file.stat().st_size == item['bytes']
            assert hashlib.sha256(file.read_bytes()).hexdigest() == item['sha256']
        for build in report['builds']:
            verify(build['source'], 'sources')
            verify(build['artifact'], 'binaries')
        verify(report['wrapper'], 'binaries')
    print('270 CSV/JSON samples and all 18 medians/ranges/MAD/zero-RSS counts verified' +
          ('; generated-source/native-artifact hashes verified' if args.artifacts else '; artifact bytes not requested'))


if __name__ == '__main__':
    main()
