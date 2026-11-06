#!/usr/bin/env python3
"""Check lesson 3's two local files and error semantics with public v2.24.1.

Usage: python3 site/check_messages.py /path/to/public/code /path/to/built-site
Needs Linux x86_64, curl/cc/network. Uses a temporary project and isolated cache.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

SITE = Path(__file__).resolve().parent
RELEASE = 'https://github.com/codelovesme/code/releases/tag/v2.24.1'


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    cli = Path(sys.argv[1]).resolve(strict=True)
    built = Path(sys.argv[2]).resolve(strict=True)
    files = built / 'lesson-examples/messages'
    plan = json.loads((SITE / 'lessons.json').read_text())
    lesson = next(item for item in plan if item['id'] == 'messages')
    with tempfile.TemporaryDirectory(prefix='code-lesson-messages space-') as directory:
        root = Path(directory)
        env = dict(os.environ)
        for key in ['CODE_RUN', 'CODE_LINK', 'CODE_MODULE_PATH', 'CODE_MODULE_RELEASE', 'CODE_CACHE_DIR']:
            env.pop(key, None)
        env.update(CODE_MODULE_RELEASE=RELEASE, CODE_CACHE_DIR=str(root / 'cache'), CODE_CACHE='1')

        def run(args, cwd, expected=None, allowed=True):
            result = subprocess.run([str(cli), *args], cwd=cwd, env=env, text=True, capture_output=True)
            if allowed and result.returncode != 0:
                raise AssertionError(f'{args}: {result.stdout}\n{result.stderr}')
            if expected is not None:
                assert result.stdout.strip() == expected, result.stdout
            return result

        run(['--version'], root, 'Code v2.24.1')
        run(['init', 'hello'], root)
        project = root / 'hello'
        run(['install', 'console'], project)
        console = json.loads((project / '.code/lock.json').read_text())['modules']['console']
        assert console['version'] == '2.24.1' and console['source'] == RELEASE
        installed = project / '.code/modules/console' / console['version'] / console['asset']
        assert hashlib.sha256(installed.read_bytes()).hexdigest() == console['sha256']
        for name in ['greeter.code', 'main.code']:
            shutil.copyfile(files / name, project / name)
        script_env = dict(env, code_cli=str(cli))
        result = subprocess.run(['sh', '-eu', str(files / 'run.sh')], cwd=project, env=script_env,
                                text=True, capture_output=True, check=True)
        expected = 'hello, Ada\nhello, Lin\ncaller: from main'
        assert result.stdout.count(expected) == 2, result.stdout
        assert json.loads(run(['check'], project).stdout) == {'schema_version': 1, 'diagnostics': []}
        (project / 'greeter.code').write_text((files / 'greeter.code').read_text().replace('"hello"', '"welcome"'))
        changed = 'welcome, Ada\nwelcome, Lin\ncaller: from main'
        run(['run', '--interpret'], project, changed)
        run(['build', '--release'], project)
        answer = subprocess.run([str(project / 'build/hello')], cwd=project, env=env,
                                text=True, capture_output=True, check=True)
        assert answer.stdout.strip() == changed
        (project / 'greeter.code').write_bytes((files / 'greeter.code').read_bytes())
        invalid = project / 'empty-name.code'
        invalid.write_text('link "greeter"\nlink "console" as out\n'
                           'emit Greet { name = "" } to this get reply\n'
                           'assert reply ∈ Exception\nassert reply.source = "greeter"\n'
                           'emit Print { value = reply.message } to out\n')
        run(['run', invalid.name, '--interpret'], project, 'Choose a name.')
        run(['build', invalid.name, '--release', '-o', 'build/empty-name'], project)
        failed_request = subprocess.run([str(project / 'build/empty-name')], cwd=project, env=env,
                                        text=True, capture_output=True, check=True)
        assert failed_request.stdout.strip() == 'Choose a name.'
        invalid.unlink()
        hidden = project / 'private.code'
        hidden.write_text('link "greeter"\nleaked = greeting\n')
        for args in [['run', hidden.name, '--interpret'], ['build', hidden.name]]:
            answer = run(args, project, allowed=False)
            assert answer.returncode != 0 and 'undefined variable' in answer.stdout + answer.stderr
        hidden.unlink()

        for index, case in enumerate(lesson['checks']):
            probe = project / f'probe-{index}.code'
            probe.write_bytes((SITE / 'lesson-checks' / case['source']).read_bytes())
            interpreted = run(['run', probe.name, '--interpret'], project, allowed=False)
            error = case.get('error_prefix')
            if error:
                assert interpreted.returncode != 0 and error in interpreted.stdout + interpreted.stderr
            else:
                assert interpreted.returncode == 0, interpreted.stderr
            assert interpreted.stdout.strip() == case['output'], interpreted.stdout
            compiled = run(['build', probe.name, '--release', '-o', f'build/probe-{index}'], project, allowed=False)
            if case['id'] == 'pre-run':
                assert compiled.returncode != 0 and 'undefined variable' in compiled.stdout + compiled.stderr
            else:
                assert compiled.returncode == 0, compiled.stderr
                native = subprocess.run([str(project / f'build/probe-{index}')], cwd=project, env=env,
                                        text=True, capture_output=True)
                assert (native.returncode != 0) == bool(error), native.stderr
                assert native.stdout.strip() == case['output'], native.stdout
                if error:
                    assert 'assertion failed' in native.stderr
            probe.unlink()
    print('Public v2.24.1: exact local files/commands, console provenance/hash, private-name refusals, '
          'independent file bindings, edited/empty greetings and all four error-boundary probes passed interpreted/compiled')


if __name__ == '__main__':
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or '', file=sys.stderr)
        print(error.stderr or '', file=sys.stderr)
        raise SystemExit(f'Command failed: {error.cmd}')
