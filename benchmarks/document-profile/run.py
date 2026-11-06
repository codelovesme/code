#!/usr/bin/env python3
"""Local instruction diagnostics, never publication timings or a speed ranking."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
spec = importlib.util.spec_from_file_location('evidence', HERE.parent / 'website-evidence/run.py')
evidence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evidence)


def probes(size):
    full = (HERE.parent / 'history-pilot/history.code').read_text()
    full = full.replace('__PAYLOAD__', str(size)).replace('__SECTIONS__', '1').replace('__DOCUMENT__', 'true')
    construction = full.split('snapshots = []\n')[0]
    edits = full.split('expected = payload\n')[0]
    return {'construction': construction, 'edits': edits,
            'edits-expected': full.replace('        assert item.text = expected\n', ''), 'full': full}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--code', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--annotate', required=True, type=Path)
    args = parser.parse_args()
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    code = args.code.resolve(strict=True)
    annotate = args.annotate.resolve(strict=True)
    valgrind = Path(shutil.which('valgrind')).resolve(strict=True)
    env = dict(os.environ, CODE_CACHE='0')
    for key in ['CODE_RUN', 'CODE_LINK', 'CODE_MODULE_PATH', 'CODE_CHECK_LEAKS', 'CODE_TRACE', 'CODE_OBSERVE']:
        env.pop(key, None)

    def run(command, **kwargs):
        result = subprocess.run(list(map(str, command)), cwd=REPO, env=env,
                                text=True, capture_output=True, **kwargs)
        if result.returncode:
            raise RuntimeError(f'{command}\n{result.stdout}\n{result.stderr}')
        return result

    manifest = HERE.parent / 'website-evidence/releases/v2.23.2.json'
    release = evidence.release_spec(manifest)
    evidence.verify_release(code, release, run([code, '--version']).stdout.strip())
    cpu = min(os.sched_getaffinity(0))
    os.sched_setaffinity(0, {cpu})
    report = {'status': 'preparing', 'source_revision': run(['git', 'rev-parse', 'HEAD']).stdout.strip(),
              'started': evidence.utc(), 'release': release,
              'launcher': evidence.identity(code), 'driver': evidence.identity(code.with_name('code-driver')),
              'script': evidence.identity(Path(__file__)),
              'template': evidence.identity(HERE.parent / 'history-pilot/history.code'),
              'environment': {'platform': platform.platform(), 'lscpu': run(['lscpu']).stdout,
                              'cpu': cpu, 'valgrind': run([valgrind, '--version']).stdout.strip(),
                              'valgrind_binary': evidence.identity(valgrind),
                              'annotate': evidence.identity(annotate)},
              'boundary': 'Local Callgrind Ir counts, startup/construction/checks/cleanup included. '
                          'Cumulative probes differ in checks and code generation; do not add or '
                          'subtract them as exact isolated phase times. No wall-clock ranking.',
              'profiles': {}}
    try:
        jobs = []
        for size in [65536, 1048576]:
            for phase, source in probes(size).items():
                name = f'{size}-{phase}'
                path = out / f'{name}.code'
                path.write_text(source)
                binary = out / name
                command = [code, 'build', path, '--release', '-o', binary]
                run(command)
                result = run([binary])
                if result.stdout or result.stderr:
                    raise RuntimeError(f'Expected silent success: {name}: {result}')
                # Leak checks are functional validation, outside instruction profiling.
                old = env.get('CODE_CHECK_LEAKS')
                env['CODE_CHECK_LEAKS'] = '1'
                try:
                    leak = run([binary])
                    (out / f'{name}.leaks.log').write_text(leak.stdout + leak.stderr)
                finally:
                    if old is None:
                        env.pop('CODE_CHECK_LEAKS', None)
                    else:
                        env['CODE_CHECK_LEAKS'] = old
                report['profiles'][name] = {'source': evidence.identity(path),
                                            'binary': evidence.identity(binary),
                                            'build_command': list(map(str, command))}
                jobs.append((name, binary))
                print(f'{name}: compiled and checked', flush=True)
        report['status'] = 'profiling'
        for name, binary in jobs:
            raw = out / f'{name}.callgrind'
            command = [valgrind, '--tool=callgrind', '--error-exitcode=99',
                       '--cache-sim=no', '--branch-sim=no', f'--callgrind-out-file={raw}', binary]
            result = run(command)
            (out / f'{name}.profile.log').write_text(result.stdout + result.stderr)
            annotated = out / f'{name}.annotated'
            annotated.write_text(run([annotate, '--auto=no', '--inclusive=no', '--threshold=99.9', raw]).stdout)
            counts = re.findall(r'^summary: (\d+)$', raw.read_text(), re.M)
            if len(counts) != 1:
                raise RuntimeError(f'Expected one instruction summary in {raw}')
            report['profiles'][name].update(instructions=int(counts[0]), raw=evidence.identity(raw),
                                             annotated=evidence.identity(annotated),
                                             profile_command=list(map(str, command)))
            print(f'{name}: {counts[0]} instructions', flush=True)
        report['finished'] = evidence.utc()
        report['status'] = 'complete'
    finally:
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
