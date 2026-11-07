#!/usr/bin/env python3
"""Extract helper call counts from complete untimed Callgrind programs."""
import argparse
import collections
import json
from pathlib import Path
import re


def arcs(path):
    lines = path.read_text().splitlines()
    names = {}
    for line in lines:
        m = re.match(r'c?fn=\((\d+)\) (.+)', line)
        if m:
            names[m[1]] = m[2]
    calls = collections.Counter()
    caller = callee = None
    for line in lines:
        m = re.match(r'(c?fn)=\((\d+)\)', line)
        if m:
            if m[1] == 'fn':
                caller = names[m[2]]
            else:
                callee = names[m[2]]
        m = re.match(r'calls=(\d+)', line)
        if m and caller and callee:
            calls[caller, callee] += int(m[1])
    return [{'caller': a, 'callee': b, 'calls': n}
            for (a, b), n in sorted(calls.items())
            if a.startswith('code_add') or b.startswith('code_add')]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('profiles', type=Path)
    args = p.parse_args()
    result = {'boundary': 'Dynamic call counts of complete local programs, not wall-time shares or exact phase subtraction.',
              'probes': {path.stem: arcs(path) for path in sorted(args.profiles.glob('*.callgrind'))}}
    (args.profiles / 'calls.json').write_text(json.dumps(result, indent=2) + '\n')
    for name, calls in result['probes'].items():
        if name.startswith('threshold-'):
            print(name, calls)


if __name__ == '__main__':
    main()
