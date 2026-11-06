#!/usr/bin/env python3
"""Controlled before/after bounded ASCII slice-seek study. Local smoke is untimed; sample on a fresh hosted VM."""
import argparse
import csv
import importlib.util
import json
import os
from pathlib import Path
import random
import re
import shutil
import subprocess

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
spec = importlib.util.spec_from_file_location('website_evidence', HERE.parent / 'website-evidence/run.py')
evidence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evidence)
BASELINE = 'af3430d26dc7d577feffd756c3e9c444f57eec0b'
WORKLOADS = ['matrix_64', 'matrix_128', 'sieve_100000', 'sieve_1000000',
             'merge_16384', 'merge_65536', 'binary_trees', 'dictionary', 'points',
             'spectral_norm', 'string_build', 'sum_squares', 'snapshots',
             'config_4k', 'config_64k', 'document_64k', 'document_1m',
             'document_64k_unicode', 'document_1m_unicode']


def inputs():
    spec = importlib.util.spec_from_file_location('algorithm_inputs', HERE.parent / 'compiled-algorithms/run.py')
    algorithms = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(algorithms)
    for algorithm, size in algorithms.CASES:
        count, total = algorithms.prime_oracle(size) if algorithm == 'sieve' else (0, 0)
        text = (HERE.parent / f'compiled-algorithms/{algorithm}.code').read_text()
        yield f'{algorithm}_{size}', text.replace('__SIZE__', str(size)).replace('__COUNT__', str(count)).replace('__SUM__', str(total))
    for name in ['binary_trees', 'dictionary', 'points', 'spectral_norm', 'string_build']:
        yield name, (HERE.parent / f'workloads/{name}.code').read_text()
    yield 'sum_squares', (HERE.parent / 'language-comparison/sum_squares.code').read_text()
    yield 'snapshots', (HERE.parent / 'website-evidence/snapshots.code').read_text()
    for name, payload, sections, document in [('config_4k', 4096, 64, False), ('config_64k', 65536, 64, False),
                                               ('document_64k', 65536, 1, True), ('document_1m', 1048576, 1, True)]:
        text = (HERE.parent / 'history-pilot/history.code').read_text()
        yield name, text.replace('__PAYLOAD__', str(payload)).replace('__SECTIONS__', str(sections)).replace('__DOCUMENT__', str(document).lower())


    for name, payload in [('document_64k_unicode', 65536), ('document_1m_unicode', 1048576)]:
        text = (HERE.parent / 'history-pilot/history.code').read_text()
        yield name, text.replace('__PAYLOAD__', str(payload)).replace('__SECTIONS__', '1').replace('__DOCUMENT__', 'true').replace('payload = "a"', 'payload = "é"')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', required=True, type=Path)
    parser.add_argument('--after', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--runs', type=int, default=15)
    parser.add_argument('--smoke-only', action='store_true')
    args = parser.parse_args()
    if args.runs < 3:
        parser.error('at least three runs required')
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    compilers = {'Before': args.before.resolve(strict=True), 'After': args.after.resolve(strict=True)}
    env = dict(os.environ, CODE_CACHE='0')
    for key in ['CODE_RUN', 'CODE_LINK', 'CODE_MODULE_PATH', 'CODE_CHECK_LEAKS', 'CODE_TRACE', 'CODE_OBSERVE']:
        env.pop(key, None)
    def run(command):
        result = subprocess.run(list(map(str, command)), cwd=REPO, env=env, text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(f'{command}\n{result.stdout}\n{result.stderr}')
        return result
    for code in compilers.values():
        if run([code, '--version']).stdout.strip() != 'Code v2.24.0':
            raise RuntimeError('Use the fixed source before/after Code v2.24.0 builds')
    if evidence.sha(compilers['Before']) == evidence.sha(compilers['After']):
        raise RuntimeError('Before/after compiler identities must differ')
    tools = {name: shutil.which(name) for name in ['gcc', 'rustc']}
    if not all(tools.values()):
        raise RuntimeError(f'Missing tools: {tools}')
    report = {'status': 'preparing', 'source_revision': run(['git', 'rev-parse', 'HEAD']).stdout.strip(),
              'compilers': {mode: evidence.identity(path) for mode, path in compilers.items()},
              'baseline_revision': run(['git', 'rev-parse', BASELINE]).stdout.strip(),
              'environment': {'platform': evidence.platform.platform(), 'lscpu': run(['lscpu']).stdout,
                              'github_run_id': os.environ.get('GITHUB_RUN_ID'), 'runner_image': os.environ.get('ImageVersion'),
                              'tools': {k: run([v, '--version']).stdout.splitlines()[0] for k, v in tools.items()},
                              'rust_verbose': run([tools['rustc'], '-vV']).stdout,
                              'load_before_preparation': Path('/proc/loadavg').read_text(),
                              'os_release': Path('/etc/os-release').read_text(),
                              'tool_executables': {k: evidence.identity(Path(v).resolve()) for k, v in tools.items()},
                              'actual_rustc': evidence.identity(Path(run([tools['rustc'], '--print', 'sysroot']).stdout.strip())/'bin/rustc')},
              'protocol': {'runs': args.runs, 'seed': 20261106, 'experiment': 'bounded ASCII skipping for finite native string slice offsets',
                           'cases': WORKLOADS, 'includes': 'startup, input construction, algorithm, result checks and cleanup',
                           'rss': 'Linux wait4 largest process resident peak; not live heap or allocation count',
                           'boundary': 'matched source before/after; separate fresh host, not the public-release ranking'},
              'sources': {str(p.relative_to(REPO)): evidence.identity(p) for p in [HERE/'run.py', HERE.parent/'website-evidence/run.py', HERE.parent/'compiled-algorithms/run.py', HERE.parent/'language-comparison/peak.c', REPO/'src/runtime.c', REPO/'crates/code-native/vendor/runtime.c']},
              'builds': [], 'samples': [], 'summary': []}
    try:
        bins = out/'binaries'; bins.mkdir()
        generated = out/'sources'; generated.mkdir()
        jobs = []
        for name, source in inputs():
            path = generated / (name + '.code'); path.write_text(source)
            for mode, code in compilers.items():
                binary = bins / (name + '-' + mode)
                command = [code, 'build', path, '--release', '-o', binary]
                run(command)
                result = run([binary])
                if result.stdout:
                    raise RuntimeError('All variants must remain silent on success')
                report['builds'].append({'workload': name, 'mode': mode, 'source': evidence.identity(path),
                                        'command': list(map(str, command)), 'artifact': evidence.identity(binary)})
                jobs.append({'workload': name, 'group': 'prebuilt', 'mode': mode, 'binary': str(binary)})
            print(name + ' prepared and checked (both source builds)', flush=True)
        if args.smoke_only:
            report['status'] = 'smoke-only; no timing samples'
            return
        if run(['git', 'status', '--porcelain', '--untracked-files=no']).stdout.strip():
            raise RuntimeError('Sampling requires a clean tracked tree')
        if not os.environ.get('GITHUB_ACTIONS'):
            raise RuntimeError('Publication sampling is restricted to the fresh hosted workflow; use --smoke-only locally')
        allowed = sorted(os.sched_getaffinity(0)); os.sched_setaffinity(0, {allowed[0]})
        report['environment'].update(allowed_cpus=allowed, pinned_cpu=allowed[0], pressure_before=Path('/proc/pressure/cpu').read_text())
        peak = bins/'peak'
        run([tools['gcc'], '-std=c17', '-O2', HERE.parent/'language-comparison/peak.c', '-o', peak])
        report['wrapper'] = evidence.identity(peak)
        report['sampling_started'] = evidence.utc()
        rng = random.Random(20261106)
        with (out/'samples.csv').open('w', newline='') as stream:
            fields = ['workload', 'group', 'mode', 'sample', 'elapsed_seconds', 'peak_rss_kib']
            writer = csv.DictWriter(stream, fieldnames=fields); writer.writeheader()
            for index in range(args.runs):
                order = list(jobs); rng.shuffle(order)
                for job in order:
                    active = evidence.interference()
                    if active:
                        raise RuntimeError(f'Concurrent compiler/test/profiler processes: {active}')
                    result = run([peak, job['binary']])
                    usage = re.findall(r'CODE_BENCH_USAGE ([0-9.]+) ([0-9]+)', result.stderr)
                    if len(usage) != 1 or result.stdout:
                        raise RuntimeError(f'Unexpected result: {result.stdout} {result.stderr}')
                    seconds, rss = usage[0]
                    row = {k: job[k] for k in ['workload', 'group', 'mode']}
                    row.update(sample=index+1, elapsed_seconds=float(seconds), peak_rss_kib=int(rss))
                    report['samples'].append(row); writer.writerow(row); stream.flush()
                print(f'repetition {index+1}/{args.runs}', flush=True)
        for build in report['builds']:
            if evidence.sha(build['artifact']['path']) != build['artifact']['sha256']:
                raise RuntimeError('Prepared binary changed during sampling')
        report['summary'] = evidence.summary(report['samples'])
        report['environment']['pressure_after'] = Path('/proc/pressure/cpu').read_text()
        report['sampling_finished'] = evidence.utc()
        report['status'] = 'complete'
    finally:
        (out/'report.json').write_text(json.dumps(report, indent=2)+'\n')


if __name__ == '__main__':
    main()
