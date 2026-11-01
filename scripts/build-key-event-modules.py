#!/usr/bin/env python3
"""Build source window/pty/tty modules for a development terminal bundle.

--abi 1 uses the v2.14.5 code-native/runtime snapshot; --abi 2 uses current
source. This does not publish or change the installed compiler/module pins.
"""
import argparse
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import io

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--abi', type=int, choices=(1, 2), required=True)
parser.add_argument('--out', type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
args.out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='key-event-modules-') as temporary:
    stage = Path(temporary)
    if args.abi == 1:
        archive = subprocess.check_output(['git', '-C', str(root), 'archive', 'v2.14.5', 'crates/code-native'])
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            tar.extractall(stage, filter='data')
        # ABI 1 exposes length as a field; current modules use the ABI 2
        # accessor spelling. This adapter preserves the ABI 1 layout/runtime.
        native = stage/'crates/code-native/src/lib.rs'
        native.write_text(native.read_text()+'\nimpl CodeValue { pub fn len(&self) -> i64 { self.len } }\n')
    else:
        shutil.copytree(root/'crates/code-native', stage/'crates/code-native')
    for name in ('window', 'pty', 'tty'):
        module = stage/'crates/modules'/name
        shutil.copytree(root/'crates/modules'/name, module)
        subprocess.run(['cargo', 'build', '--release', '--manifest-path', str(module/'Cargo.toml'),
                        '--target-dir', str(root/'target'/f'key-events-abi{args.abi}')], check=True)
        asset = root/'target'/f'key-events-abi{args.abi}'/'release'/f'lib{name}.so'
        destination = args.out/f'{name}.so'
        installing = args.out/f'.{name}.install'
        shutil.copy2(asset, installing)
        installing.replace(destination)
        print(f'installed source {name} (ABI {args.abi}) in {args.out}', flush=True)
