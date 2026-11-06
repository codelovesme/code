#!/usr/bin/env python3
"""Recompute retained diagnostic counts and identities without rerunning profiles."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('document_profile', HERE / 'run.py')
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)


def check(path, identity):
    data = path.read_bytes()
    assert len(data) == identity['bytes'], path
    assert hashlib.sha256(data).hexdigest() == identity['sha256'], path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('results', type=Path)
    parser.add_argument('--artifacts', type=Path, help='Optional original directory including compiled executables')
    args = parser.parse_args()
    report = json.loads((args.results / 'report.json').read_text())
    assert report['status'] == 'complete'
    check(HERE / 'run.py', report['script'])
    check(HERE.parent / 'history-pilot/history.code', report['template'])
    manifest = json.loads((HERE.parent / 'website-evidence/releases/v2.23.2.json').read_text())
    assert report['release'] == manifest
    assert report['launcher']['sha256'] == manifest['launcher_sha256']
    assert report['driver']['sha256'] == manifest['driver_sha256']
    expected = {f'{size}-{phase}': text for size in [65536, 1048576]
                for phase, text in profile.probes(size).items()}
    assert set(report['profiles']) == set(expected)
    published = json.loads((HERE.parent / 'website-evidence/results/2026-11-06-v2.23.2/history/report.json').read_text())
    for name, entry in report['profiles'].items():
        source = args.results / f'{name}.code'
        check(source, entry['source'])
        assert source.read_text() == expected[name], name
        for key, suffix in [('raw', 'callgrind'), ('annotated', 'annotated')]:
            check(args.results / f'{name}.{suffix}', entry[key])
        raw = (args.results / f'{name}.callgrind').read_text()
        assert re.findall(r'^events: (.+)$', raw, re.M) == ['Ir']
        assert re.findall(r'^summary: (\d+)$', raw, re.M) == [str(entry['instructions'])]
        annotation = (args.results / f'{name}.annotated').read_text()
        total = re.search(r'^([\d,]+)\s+\(100\.0%\)\s+PROGRAM TOTALS$', annotation, re.M)
        assert total and int(total[1].replace(',', '')) == entry['instructions']
        if name.endswith('-full'):
            workload = 'document_64k' if name.startswith('65536-') else 'document_1m'
            original = [x for x in published['builds'] if x['workload'] == workload and x['mode'] == 'Code automatic']
            assert len(original) == 1 and original[0]['source']['sha256'] == entry['source']['sha256']
        if args.artifacts:
            check(args.artifacts / name, entry['binary'])
        print(f"{name}: {entry['instructions']:,} instructions verified")
    print('Eight complete profiles, exact original full programs, all source/raw/annotation identities and public release pair verified.')


if __name__ == '__main__':
    main()
