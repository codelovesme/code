#!/usr/bin/env python3
"""Untimed cumulative construction/query probes; not wall-time phase subtraction."""
import argparse,hashlib,importlib.util,json,os,re,subprocess,shutil
from pathlib import Path
HERE=Path(__file__).resolve().parent
REPO=HERE.parents[1]
spec=importlib.util.spec_from_file_location('workloads',HERE.parent/'compiled-workloads/run.py')
w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w)
def identity(p):
 p=Path(p).resolve();return dict(path=str(p),sha256=hashlib.sha256(p.read_bytes()).hexdigest(),bytes=p.stat().st_size)
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--code',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--annotate',type=Path,default=Path(shutil.which('callgrind_annotate') or Path.home()/'.local/opt/gdb-root/bin/callgrind_annotate'));a=p.parse_args()
 out=a.output_dir.resolve();out.mkdir(exist_ok=False)
 env=dict(os.environ,CODE_CACHE='0')
 for k in ['CODE_RUN','CODE_LINK','CODE_MODULE_PATH','CODE_TRACE','CODE_OBSERVE']:env.pop(k,None)
 def run(cmd,**kw):
  r=subprocess.run(list(map(str,cmd)),env=env,capture_output=True,text=True,check=True,**kw);return r.stdout
 report={'source_revision':run(['git','rev-parse','HEAD']).strip(),'compiler':identity(a.code),'valgrind':run(['valgrind','--version']).strip(),'boundary':'Local instruction counts of cumulative complete programs; probes differ in checks/code generation. No wall-time ranking or exact phase subtraction. direct-read changes source, not compiler.','probes':{}}
 for kind in ['weighted','threshold','nearest']:
  text=(HERE.parent/f'compiled-workloads/{kind}.code').read_text().replace('__SIZE__','32768').replace('__EXPECTED__',str(w.oracle(kind,32768)))
  construction=text[:text.index('result = 0')]
  construction+='assert values[0] = 17\nassert values[length - 1] = 0\n' if kind=='threshold' else 'assert values[0].x = 17\nassert values[length - 1].x = 0\n'
  direct=text.replace('        v = values[i]\n','').replace('v.x','values[i].x').replace('v.y','values[i].y').replace('v.z','values[i].z')
  if kind=='threshold':direct=direct.replace('if v >','if values[i] >').replace('answer + v','answer + values[i]')
  for phase,source in [('construct',construction),('full',text),('direct-read',direct)]:
   name=kind+'-'+phase;path=out/(name+'.code');path.write_text(source);binary=out/name
   run([a.code,'build',path,'--release','-o',binary]);run([binary],cwd=out)
   leak=subprocess.run([str(binary)],env=dict(env,CODE_CHECK_LEAKS='1'),text=True,capture_output=True,check=True);assert not leak.stdout+leak.stderr
   raw=out/(name+'.callgrind');command=['valgrind','--tool=callgrind','--error-exitcode=99','--cache-sim=no','--branch-sim=no',f'--callgrind-out-file={raw}',binary]
   proc=subprocess.run(list(map(str,command)),env=env,capture_output=True,text=True,check=True);(out/(name+'.log')).write_text(proc.stdout+proc.stderr)
   annotate=a.annotate.resolve(strict=True)
   rendered=run([annotate,'--auto=no','--inclusive=no','--threshold=99.9',raw]);(out/(name+'.annotated')).write_text(rendered)
   total=int(re.search(r'^summary: (\d+)$',raw.read_text(),re.M)[1]);report['probes'][name]={'source':identity(path),'binary':identity(binary),'raw':identity(raw),'instructions':total}
   print(name,total,flush=True)
 (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
if __name__=='__main__':main()
