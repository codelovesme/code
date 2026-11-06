#!/usr/bin/env python3
"""Pre-registered compiled-workload study. Local smoke is untimed; sample on a fresh hosted VM."""
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
CASES = [(name, size) for name in ['weighted', 'threshold', 'nearest'] for size in [32768, 262144]]


def oracle(kind, n):
    # Inputs traverse all 1024 integers exactly n/1024 times. Independent exact
    # integer arithmetic; no shared target implementation or floating tolerance.
    if kind == 'weighted':
        return sum(sum(x*(r+1)+(1024-x)*3-x*4 for x in range(1024)) * (n//1024) for r in range(16))
    if kind == 'threshold':
        return sum(sum(range(513+r,1024)) * (n//1024) for r in range(16))
    return sum(min((x-r*17)**2+(1024-x-r*11)**2 for x in range(1024)) for r in range(16))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--code', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--runs', type=int, default=15)
    parser.add_argument('--smoke-only', action='store_true')
    args = parser.parse_args()
    if args.runs < 3:
        parser.error('at least three runs required')
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    code = args.code.resolve(strict=True)
    env = dict(os.environ, CODE_CACHE='0')
    for key in ['CODE_RUN', 'CODE_LINK', 'CODE_MODULE_PATH', 'CODE_CHECK_LEAKS', 'CODE_TRACE', 'CODE_OBSERVE']:
        env.pop(key, None)
    def run(command):
        result = subprocess.run(list(map(str, command)), cwd=REPO, env=env, text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(f'{command}\n{result.stdout}\n{result.stderr}')
        return result
    manifest = REPO/'benchmarks/website-evidence/releases/v2.24.1.json'
    release = evidence.release_spec(manifest)
    evidence.verify_release(code, release, run([code, '--version']).stdout.strip())
    tools = {name: shutil.which(name) for name in ['gcc', 'rustc']}
    if not all(tools.values()):
        raise RuntimeError(f'Missing tools: {tools}')
    report = {'status': 'preparing', 'source_revision': run(['git', 'rev-parse', 'HEAD']).stdout.strip(),
              'release': {'tag': release['tag'], 'source': release['source_commit'], 'launcher': evidence.identity(code),
                          'driver': evidence.identity(code.with_name('code-driver'))},
              'environment': {'platform': evidence.platform.platform(), 'lscpu': run(['lscpu']).stdout,
                              'github_run_id': os.environ.get('GITHUB_RUN_ID'), 'runner_image': os.environ.get('ImageVersion'),
                              'tools': {k: run([v, '--version']).stdout.splitlines()[0] for k, v in tools.items()},
                              'rust_verbose': run([tools['rustc'], '-vV']).stdout,
                              'load_before_preparation': Path('/proc/loadavg').read_text(),
                              'os_release': Path('/etc/os-release').read_text(),
                              'tool_executables': {k: evidence.identity(Path(v).resolve()) for k, v in tools.items()},
                              'actual_rustc': evidence.identity(Path(run([tools['rustc'], '--print', 'sysroot']).stdout.strip())/'bin/rustc')},
              'protocol': {'runs': args.runs, 'seed': 20261106, 'algorithms': 'weighted record scoring; threshold aggregation; nearest point; 16 queries each',
                           'cases': CASES, 'includes': 'startup, input construction, algorithm, result checks and cleanup',
                           'rss': 'Linux wait4 largest process resident peak; not live heap or allocation count',
                           'boundary': 'separate fresh-host pilot; never merge with the old host report'},
              'sources': {str(p.relative_to(REPO)): evidence.identity(p) for p in [*[HERE/f'{algorithm}.{suffix}' for algorithm in ['weighted', 'threshold', 'nearest'] for suffix in ['code', 'c', 'rs']], HERE/'run.py', HERE/'README.md', HERE/'verify.py', manifest, REPO/'.github/workflows/compiled-workloads.yml', HERE.parent/'website-evidence/run.py', HERE.parent/'language-comparison/peak.c']},
              'builds': [], 'samples': [], 'summary': []}
    try:
        bins = out/'binaries'; bins.mkdir()
        generated = out/'sources'; generated.mkdir()
        jobs = []
        for algorithm, size in CASES:
            name = f'{algorithm}_{size}'
            expected = oracle(algorithm, size)
            for suffix, mode in [('code', 'Code release'), ('c', 'C O2'), ('c', 'C O3'), ('rs', 'Rust O3')]:
                source = (HERE/f'{algorithm}.{suffix}').read_text()
                source = source.replace('__SIZE__', str(size)).replace('__EXPECTED__', str(expected))
                path = generated/f'{name}.{suffix}'; path.write_text(source)
                binary = bins/(name+'-'+mode.replace(' ', '-'))
                command = ([code, 'build', path, '--release'] if suffix == 'code' else
                           [tools['gcc'], '-std=c17', '-'+mode.split()[1], '-march=native', '-ffp-contract=off', path] if suffix == 'c' else
                           [tools['rustc'], '--edition', '2024', '-C', 'opt-level=3', '-C', 'target-cpu=native', path])
                command = [*command, '-o', binary]
                run(command)
                result = run([binary])
                if result.stdout:
                    raise RuntimeError('All variants must remain silent on success')
                report['builds'].append({'workload': name, 'mode': mode, 'source': evidence.identity(path),
                                         'command': list(map(str, command)), 'artifact': evidence.identity(binary)})
                jobs.append({'workload': name, 'group': 'prebuilt', 'mode': mode, 'binary': str(binary)})
            print(name+' prepared and checked (Code, C O2/O3, Rust O3)', flush=True)
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
