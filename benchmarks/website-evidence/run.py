#!/usr/bin/env python3
"""Public-release website evidence. Prepare first, then sample serially.

Use a fresh GitHub runner for publication measurements. --smoke-only validates
programs locally without publishing timing data. Historical results are read,
never rewritten. See README.md for boundaries and representation differences.
"""
import argparse
import collections
import csv
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import re
import shutil
import statistics
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
NUMERIC = REPO / "benchmarks/language-comparison"
WORKLOADS = REPO / "benchmarks/workloads"
PUBLIC_COMMIT = "e7436b70ba337f706a9761a9380dd49118818c71"


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def checked(command, env=None):
    p = subprocess.run(list(map(str, command)), cwd=REPO, env=env,
                       capture_output=True, text=True, check=True)
    return p.stdout


def identity(path):
    path = Path(path).resolve()
    return {"path": str(path), "sha256": sha(path), "bytes": path.stat().st_size}


def release_spec(path=None):
    spec = json.loads(Path(path).read_text()) if path else {
        'tag': 'v2.23.0', 'source_commit': PUBLIC_COMMIT,
        'artifact_url': 'https://github.com/codelovesme/code/releases/download/v2.23.0/code-v2.23.0-x86_64-linux.tar.gz',
        'launcher_sha256': '531adc3dbdf413f1f625ee7df2f2271cdfa3ee19f082c00ef4cf3ef49e1d698d',
        'driver_sha256': '784519bd5dffe7ed8c448ad73c9c96c000c7a16bb45f4068d84b5324ac594363',
    }
    if not re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+', spec['tag']):
        raise ValueError('Expected an exact public-release tag')
    for key, size in [('source_commit', 40), ('launcher_sha256', 64), ('driver_sha256', 64)]:
        if not re.fullmatch(r'[0-9a-f]{' + str(size) + '}', spec[key]):
            raise ValueError('Invalid release identity: ' + key)
    expected_url = f"https://github.com/codelovesme/code/releases/download/{spec['tag']}/code-{spec['tag']}-x86_64-linux.tar.gz"
    if spec['artifact_url'] != expected_url:
        raise ValueError('Release tag and artifact URL must match')
    return spec


def verify_release(code, spec, reported_version):
    if reported_version != f"Code {spec['tag']}":
        raise RuntimeError('Use the exact published release bundle')
    if not code.is_file() or not code.with_name('code-driver').is_file():
        raise RuntimeError('Keep the public launcher and code-driver together')
    if [sha(code), sha(code.with_name('code-driver'))] != [spec['launcher_sha256'], spec['driver_sha256']]:
        raise RuntimeError('Public release binary identity mismatch')


def interference():
    found = []
    own = os.getpid()
    for entry in Path('/proc').glob('[0-9]*'):
        if int(entry.name) == own:
            continue
        try:
            name = (entry / 'comm').read_text().strip()
            command = (entry / 'cmdline').read_bytes().replace(b'\0', b' ').decode(errors='replace')
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
        if (name in {'cargo', 'rustc', 'cc1', 'cc1plus', 'gcc', 'g++', 'ld', 'perf', 'valgrind', 'pytest'}
                or name.startswith(('clang', 'wasm-ld'))
                or any(x in command for x in ('load-compare.sh', 'pytest ', 'unittest discover', 'tools/browser/tests.mjs'))):
            found.append({"pid": int(entry.name), "comm": name})
    return found


def historical_inventory():
    inventory = []
    for folder in (NUMERIC, WORKLOADS):
        for path in sorted(folder.rglob('*.json')):
            data = json.loads(path.read_text())
            raw_keys = {'samples_seconds', 'samples_peak_rss_kib', 'compile_samples_seconds',
                        'summary', 'results', 'samples', 'rows', 'measurements', 'raw_samples'}
            inventory.append({"path": str(path.relative_to(REPO)), "sha256": sha(path),
                              "metadata": {k: v for k, v in data.items() if k not in raw_keys},
                              "top_level_keys": list(data)})
        for path in sorted(folder.rglob('*.csv')):
            with path.open(newline='') as stream:
                reader = csv.DictReader(stream)
                rows = sum(1 for _ in reader)
                fields = reader.fieldnames
            paired = path.with_suffix('.json')
            inventory.append({"path": str(path.relative_to(REPO)), "sha256": sha(path),
                              "rows": rows, "fields": fields,
                              "paired_report": str(paired.relative_to(REPO)) if paired.exists() else None})
    return inventory


def summary(samples):
    groups = collections.defaultdict(list)
    for row in samples:
        groups[row['workload'], row['group'], row['mode']].append(row)
    result = []
    for (workload, group, mode), rows in sorted(groups.items()):
        times = sorted(r['elapsed_seconds'] for r in rows)
        rss = sorted(r['peak_rss_kib'] for r in rows)
        median = statistics.median(times)
        result.append({"workload": workload, "group": group, "mode": mode, "n": len(rows),
                       "median_seconds": median, "min_seconds": times[0], "max_seconds": times[-1],
                       "mad_seconds": statistics.median(abs(t - median) for t in times),
                       "median_peak_rss_kib": statistics.median(rss),
                       "min_peak_rss_kib": rss[0], "max_peak_rss_kib": rss[-1],
                       "zero_rss_samples": rss.count(0)})
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--code', type=Path, required=True)
    ap.add_argument('--release-manifest', type=Path)
    ap.add_argument('--output-dir', type=Path, required=True)
    ap.add_argument('--runs', type=int, default=15)
    ap.add_argument('--compile-runs', type=int, default=5)
    ap.add_argument('--cpu', type=int)
    ap.add_argument('--smoke-only', action='store_true')
    args = ap.parse_args()
    if args.runs < 3 or args.compile_runs < 1:
        ap.error('use at least three launches and one compiler sample')
    if platform.system() != 'Linux':
        ap.error('peak RSS requires Linux wait4 accounting')
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    code = args.code.resolve(strict=True)
    base = dict(os.environ, CODE_CACHE='0')
    for key in ('CODE_RUN', 'CODE_MODULE_PATH', 'CODE_LINK', 'CODE_CHECK_LEAKS', 'CODE_TRACE', 'CODE_OBSERVE', 'NODE_COMPILE_CACHE'):
        base.pop(key, None)
    release = release_spec(args.release_manifest)
    verify_release(code, release, checked([code, '--version'], base).strip())
    allowed = sorted(os.sched_getaffinity(0))
    cpu = args.cpu if args.cpu is not None else allowed[0]
    if cpu not in allowed:
        raise RuntimeError('CPU is outside the allowed affinity set')
    os.sched_setaffinity(0, {cpu})
    # Preserve proxy names: resolving the rustc symlink would invoke rustup
    # itself with compiler flags instead of selecting the Rust compiler.
    tools = {name: shutil.which(name) for name in ('gcc', 'cc', 'ld', 'rustc', 'python3', 'node')}
    if not all(tools.values()):
        raise RuntimeError(f'Missing required tool: {tools}')
    report = {"schema": 1, "status": "preparing", "started_at_utc": utc(),
              "source_revision": checked(['git', 'rev-parse', 'HEAD']).strip(),
              "release_manifest": identity(args.release_manifest) if args.release_manifest else None,
              "public_release": {"tag": release["tag"], "source_commit": release["source_commit"],
                                 "artifact_url": release["artifact_url"],
                                 "launcher": identity(code), "driver": identity(code.with_name('code-driver'))},
              "environment": {"platform": platform.platform(), "machine": platform.machine(),
                              "lscpu": checked(['lscpu']), "os_release": Path('/etc/os-release').read_text(),
                              "initial_allowed_cpus": allowed, "pinned_logical_cpu": cpu,
                              "initial_loadavg": Path('/proc/loadavg').read_text().strip(),
                              "cpu_pressure_before": Path('/proc/pressure/cpu').read_text(),
                              "github_run_id": os.environ.get('GITHUB_RUN_ID'),
                              "github_run_attempt": os.environ.get('GITHUB_RUN_ATTEMPT'),
                              "runner_image": os.environ.get('ImageVersion'),
                              "toolchains": {n: checked([p, '--version']).splitlines()[0] for n, p in tools.items()}},
              "sampling": {"runs_per_runtime_mode": args.runs, "compile_runs_per_compiler": args.compile_runs,
                           "shuffle_seed": 20261104},
              "historical_inventory": historical_inventory(), "sources": {}, "builds": [], "cases": [],
              "notes": ["Single fresh hosted VM/job; serial samples pinned to one logical CPU. Physical-host isolation is not guaranteed.",
                        "Peak wrapper times fork/exec/wait and reports Linux wait4 ru_maxrss in KiB. It excludes Python harness/wrapper startup.",
                        "RSS is the largest process peak including waited descendants, not simultaneous process-tree sum or allocated heap.",
                        "Prebuilt runtime excludes preparation. Compile and cold-run RSS include the compiler; never rank them as application RSS.",
                        "Cold native caches are new per sample; warm cache is primed once outside timing. OS file caches are not cleared.",
                        "Existing broader sources are unchanged. Typed layout, allocation/lookup algorithms, validation and checksum output differ; see source audit.",
                        "Snapshots retain all 65 matrices until full checksums complete. C/Rust offer eager deep-clone and equivalent non-atomic COW variants.",
                        "Startup dominates short programs; zero/low RSS samples can hit OS accounting resolution. No universal language ranking."]}
    samples = []
    bins = out / 'binaries'; bins.mkdir()
    compile_outputs = out / 'compiler-outputs'; compile_outputs.mkdir()
    peak = bins / 'peak'
    jobs = []
    numeric_builds = []

    def build(workload, mode, source, command, binary):
        report['sources'][str(source.relative_to(REPO))] = identity(source)
        checked(command, base)
        report['builds'].append({"workload": workload, "mode": mode, "command": list(map(str, command)),
                                 "artifact": identity(binary)})
        jobs.append({"workload": workload, "group": "prebuilt", "mode": mode,
                     "command": [str(binary)], "env": base})

    try:
        checked([tools['gcc'], '-std=c17', '-O2', NUMERIC / 'peak.c', '-o', peak])
        report['measurement_wrapper'] = {"source": identity(NUMERIC / 'peak.c'), "artifact": identity(peak)}
        specs = [('sum_squares', NUMERIC)] + [(w, WORKLOADS) for w in ('spectral_norm', 'binary_trees', 'string_build', 'dictionary', 'points')]
        for w, folder in specs:
            commands = [
                ('Code release', folder / f'{w}.code', [str(code), 'build', str(folder / f'{w}.code'), '--release']),
                ('C O2', folder / f'{w}.c', [tools['gcc'], '-std=c17', '-O2', '-march=native', str(folder / f'{w}.c')]),
                ('Rust O3', folder / f'{w}.rs', [tools['rustc'], '--edition', '2024', '-C', 'opt-level=3', '-C', 'target-cpu=native', str(folder / f'{w}.rs')])]
            if w == 'sum_squares':
                commands.append(('C O3', folder / f'{w}.c', [tools['gcc'], '-std=c17', '-O3', '-march=native', str(folder / f'{w}.c')]))
            for mode, source, cmd in commands:
                binary = bins / (w + '-' + mode.replace(' ', '-'))
                command = [*cmd, '-o', str(binary)] + (['-lm'] if mode.startswith('C ') else [])
                build(w, mode, source, command, binary)
                if w == 'sum_squares':
                    compile_binary = compile_outputs / binary.name
                    compile_command = list(command)
                    compile_command[compile_command.index('-o') + 1] = str(compile_binary)
                    numeric_builds.append({"workload": w, "group": "compile", "mode": mode,
                                           "command": compile_command, "env": base,
                                           "artifact_output": str(compile_binary)})
        for mode, suffix, flags in [('Code automatic COW', 'code', []), ('C eager clone', 'c', []),
                                    ('C explicit COW', 'c', ['-DCOW=1']), ('Rust Vec clone', 'rs', []),
                                    ('Rust Rc COW', 'rs', ['--cfg', 'cow'])]:
            source = HERE / f'snapshots.{suffix}'
            binary = bins / ('snapshots-' + mode.replace(' ', '-'))
            cmd = ([str(code), 'build', str(source), '--release'] if suffix == 'code' else
                   [tools['gcc'], '-std=c17', '-O2', '-march=native', *flags, str(source)] if suffix == 'c' else
                   [tools['rustc'], '--edition', '2024', '-C', 'opt-level=3', '-C', 'target-cpu=native', *flags, str(source)])
            build('snapshots', mode, source, [*cmd, '-o', str(binary)], binary)
        source = NUMERIC / 'sum_squares.code'
        warm = out / 'warm-cache'
        warm_env = dict(base, CODE_CACHE='1', CODE_CACHE_DIR=str(warm))
        checked([code, 'run', source], warm_env)

        def warm_identity():
            return {p.name: {"inode": p.stat().st_ino, "sha256": sha(p)}
                    for p in (warm / 'run').glob('*') if p.is_file() and not p.suffix}
        warm_before = warm_identity()
        if len(warm_before) != 1:
            raise RuntimeError('Warm-up did not populate exactly one native executable')
        report['warm_native_artifact'] = warm_before
        jobs.extend([
            {"workload": 'sum_squares', "group": 'source', "mode": 'Code warm run', "command": [str(code), 'run', str(source)], "env": warm_env},
            {"workload": 'sum_squares', "group": 'source', "mode": 'Code cold run', "command": [str(code), 'run', str(source)], "env": base, "cold": True},
            {"workload": 'sum_squares', "group": 'source', "mode": 'Python', "command": [tools['python3'], str(NUMERIC / 'sum_squares.py')], "env": base},
            {"workload": 'sum_squares', "group": 'source', "mode": 'Node/V8', "command": [tools['node'], str(NUMERIC / 'sum_squares.js')], "env": base}])
        for w, folder in [('sum_squares', NUMERIC), ('binary_trees', WORKLOADS), ('dictionary', WORKLOADS)]:
            jobs.append({"workload": w, "group": 'source', "mode": 'Code interpret',
                         "command": [str(code), 'run', '--interpret', str(folder / f'{w}.code')], "env": base})
        for source in (NUMERIC / 'sum_squares.py', NUMERIC / 'sum_squares.js', HERE / 'run.py'):
            report['sources'][str(source.relative_to(REPO))] = identity(source)
        # Preflight/warm filesystem pages outside timing, then compile samples,
        # then runtime samples. Nothing is built concurrently with a runtime.
        for job in jobs:
            if not job.get('cold'):
                checked(job['command'], job['env'])
        report['environment']['v8'] = checked([tools['node'], '-p', 'process.versions.v8']).strip()
        report['environment']['tool_executables'] = {n: identity(p) for n, p in tools.items()}
        rust_sysroot = checked([tools['rustc'], '--print', 'sysroot']).strip()
        report['environment']['actual_rustc'] = identity(Path(rust_sysroot) / 'bin/rustc')
        report['environment']['rustc_verbose'] = checked([tools['rustc'], '-vV'])
        governor = Path(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor')
        report['environment']['cpu_governor'] = governor.read_text().strip() if governor.exists() else 'unavailable'
        report['cases'] = [{k: v for k, v in j.items() if k not in ('env',)} for j in jobs + numeric_builds]
        if args.smoke_only:
            report['status'] = 'smoke-only; no publication timing'
            print('All public-release programs and warm-cache preparation validated; no timings published')
            return
        if checked(['git', 'status', '--porcelain', '--untracked-files=no']).strip():
            raise RuntimeError('Publication measurements require a clean tracked source tree')
        report['sampling_started_at_utc'] = utc()
        rng = random.Random(20261104)
        fields = ['workload', 'group', 'mode', 'sample', 'started_at_utc', 'elapsed_seconds', 'peak_rss_kib', 'stdout', 'native_artifact_sha256']
        with (out / 'samples.csv').open('w', newline='') as stream:
            writer = csv.DictWriter(stream, fieldnames=fields); writer.writeheader()
            for group_jobs, count in [(numeric_builds, args.compile_runs), (jobs, args.runs)]:
                for index in range(count):
                    order = list(group_jobs); rng.shuffle(order)
                    for job in order:
                        active = interference()
                        if active:
                            raise RuntimeError(f'Unrelated build/test/profile processes present: {active}')
                        with tempfile.TemporaryDirectory(prefix='code-evidence-cold-') as cold:
                            env = dict(job['env'], CODE_CACHE='1', CODE_CACHE_DIR=cold) if job.get('cold') else job['env']
                            started = utc()
                            p = subprocess.run([str(peak), *job['command']], cwd=REPO, env=env,
                                               capture_output=True, text=True, check=True)
                            usage = re.findall(r'CODE_BENCH_USAGE ([0-9.]+) ([0-9]+)', p.stderr)
                            if len(usage) != 1:
                                raise RuntimeError(f'Invalid measurement trailer: {p.stderr}')
                            seconds, rss = usage[0]
                            native_hash = ''
                            if job.get('cold'):
                                native = [p for p in (Path(cold) / 'run').glob('*') if p.is_file() and not p.suffix]
                                if len(native) != 1:
                                    raise RuntimeError('Cold native run fell back or reused an invalid cache')
                                native_hash = sha(native[0])
                            elif job.get('artifact_output'):
                                native_hash = sha(job['artifact_output'])
                            row = {k: job[k] for k in ('workload', 'group', 'mode')}
                            row.update(sample=index + 1, started_at_utc=started, elapsed_seconds=float(seconds), peak_rss_kib=int(rss),
                                       stdout=p.stdout.strip(), native_artifact_sha256=native_hash)
                            samples.append(row); writer.writerow(row); stream.flush()
                    print(f'{group_jobs[0]["group"]}: repetition {index + 1}/{count}', flush=True)
        if warm_identity() != warm_before:
            raise RuntimeError('Warm native artifact changed during sampling')
        for build_record in report['builds']:
            if sha(build_record['artifact']['path']) != build_record['artifact']['sha256']:
                raise RuntimeError('Prepared runtime artifact changed during sampling')
        report['sampling_finished_at_utc'] = utc()
        report['environment']['cpu_pressure_after'] = Path('/proc/pressure/cpu').read_text()
        report['status'] = 'complete'
    except Exception as error:
        report['status'] = 'failed'
        report['error'] = str(error)
        if isinstance(error, subprocess.CalledProcessError):
            report['failure_output'] = {"stdout": error.stdout, "stderr": error.stderr}
            print(error.stdout or '', file=sys.stderr)
            print(error.stderr or '', file=sys.stderr)
        raise
    finally:
        report['finished_at_utc'] = utc()
        report['samples'] = samples
        report['summary'] = summary(samples)
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
