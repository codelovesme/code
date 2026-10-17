#!/usr/bin/env python3
"""Serial, CPU-pinned Length bridge measurements against frozen artifact dirs.

Each variant directory contains code, arrays.so/strings.so and arrays.a/strings.a.
The CLI/native totals include startup, parsing and assertions; compiled-wasm
samples time a warm handler only. Zero-call controls isolate setup. Requires
the native/wasm compiler toolchains and Node; no application or live data used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import statistics
import subprocess
import time


def checked(command, **kwargs):
    return subprocess.run(command, check=True, capture_output=True, text=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("variants", nargs="+", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--samples", type=int, default=9)
    parser.add_argument("--calls", type=int, default=2000)
    args = parser.parse_args()
    args.out = args.out.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    cpu = min(os.sched_getaffinity(0))
    os.sched_setaffinity(0, {cpu})
    def array_setup(n, item):
        # Keep the generated IR small: large unrolled literals mostly measure
        # the compiler optimizer's basic-block size, not module dispatch.
        return (f"seed = {item}\na = []\nj = 0\nloop\n    if j = {n}, break\n"
                "    a = a + [seed]\n    j = j + 1\n")
    cases = [(f"array-{n}", "arrays", array_setup(n, "1"), n, "Length")
             for n in (10, 1000, 10000)]
    cases += [("unicode-10000", "strings", "a = " + json.dumps("é😀x" * 10000, ensure_ascii=False) + "\n", 30000, "Length"),
              ("nested-1000", "arrays", array_setup(1000, '{ text = "' + "x" * 100 + '", children = [1, 2, 3] }'), 1000, "Length"),
              ("trim-small", "strings", 'a = " foo "\n', '"foo"', "Trim"),
              ("reverse-small", "arrays", 'a = [1, 2, 3]\n', '[3, 2, 1]', "Reverse")]
    records, artifacts, programs, jobs = [], {}, [], []
    for variant in args.variants:
        variant = variant.resolve()
        label = variant.name
        artifacts[label] = {name: hashlib.sha256((variant / name).read_bytes()).hexdigest()
                            for name in ("code", "arrays.so", "strings.so", "arrays.a", "strings.a")}
        for name, module, setup, count, handler in cases:
            for calls in (0, args.calls):
                directory = args.out / label / f"{name}-{calls}"
                directory.mkdir(parents=True, exist_ok=True)
                body = (setup + "Bench =>\n    i = 0\n    loop\n"
                        f"        if i = {calls}, break\n"
                        f"        emit {handler} {{ value = a }} to m get result\n"
                        f"        assert result.value = {count}\n        i = i + 1\n"
                        "    return Done\nemit Bench to this get done\nassert done = Done\n")
                source = directory / "main.code"
                source.write_text(f'link "{variant / (module + ".so")}" as m\n' + body)
                checked([str(variant / "code"), "build", str(source), "--release", "-o", str(directory / "native")])
                wasm_source = directory / "wasm.code"
                wasm_source.write_text(f'link "{variant / (module + ".a")}" as m\n' + body)
                checked([str(variant / "code"), "build", str(wasm_source), "--target", "wasm", "--release", "-o", str(directory / "main.wasm")])
                runner = directory / "bench.mjs"
                runner.write_text("""import {readFileSync} from 'node:fs';
import {createHost} from './host.mjs';
const host=createHost({log:()=>{}});
const {instance}=await WebAssembly.instantiate(readFileSync(new URL('./main.wasm',import.meta.url)),{env:host.env});
if(host.start(instance)!==0)throw Error('start failed');
const samples=[];
for(let i=0;i<14;i++){
  const start=performance.now();const r=host.ask({_class:'Bench'});
  const ms=performance.now()-start;
  if(r?._class!=='Done')throw Error(JSON.stringify(r));
  if(i>=5)samples.push(ms);
}
console.log(JSON.stringify(samples));
""".replace("i<14", f"i<{args.samples + 5}"))
                programs.append(dict(variant=label, case=name, calls=calls,
                                     sha256=hashlib.sha256(source.read_bytes()).hexdigest()))
                for mode in ("interpret", "native"):
                    command = [str(variant / "code"), "run", str(source), "--interpret"] if mode == "interpret" else [str(directory / "native")]
                    checked(command)  # Warm OS caches before shuffled fresh launches.
                    for _ in range(args.samples):
                        jobs.append((label, name, calls, mode, command))
                jobs.append((label, name, calls, "wasm", ["node", str(runner)]))
    random.Random(131).shuffle(jobs)
    for label, name, calls, mode, command in jobs:
        start = time.perf_counter_ns()
        result = checked(command)
        elapsed = (time.perf_counter_ns() - start) / 1e6
        samples = json.loads(result.stdout) if mode == "wasm" else [elapsed]
        records.extend(dict(variant=label, case=name, calls=calls, mode=mode, ms=sample) for sample in samples)
    result = dict(cpu=cpu, seed=131, calls=args.calls, artifacts=artifacts,
                  programs=programs, samples=records)
    (args.out / "results.json").write_text(json.dumps(result, indent=2) + "\n")
    for label in artifacts:
        for name, _, _, _, _ in cases:
            for mode in ("interpret", "native", "wasm"):
                medians = [statistics.median(r["ms"] for r in records if
                           (r["variant"],r["case"],r["mode"],r["calls"]) == (label,name,mode,c))
                           for c in (0,args.calls)]
                print(label, name, mode, "zero/total/delta ms:", *[round(x,4) for x in (*medians,medians[1]-medians[0])], flush=True)


if __name__ == "__main__":
    main()
