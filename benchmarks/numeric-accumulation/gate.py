from pathlib import Path
import json,sys
root=Path(sys.argv[1]);d=json.loads((root/'report.json').read_text());assert d['status']=='complete'
r={(x['workload'],x['mode']):x for x in d['summary']};changes=[];bad=[];targets=[]
for name in sorted({k[0] for k in r}):
 a=r[name,'Before'];b=r[name,'After'];delta=b['median_seconds']-a['median_seconds'];pct=delta/a['median_seconds']*100
 material=delta >= .1*a['median_seconds'] and delta > max(.0002,2*(a['mad_seconds']+b['mad_seconds']))
 row=dict(workload=name,before_ms=a['median_seconds']*1000,after_ms=b['median_seconds']*1000,change_percent=pct,material_regression=material,before_mad_ms=a['mad_seconds']*1000,after_mad_ms=b['mad_seconds']*1000)
 changes.append(row)
 if material:bad.append(row)
 if name == 'threshold_262144' and pct<=-10:targets.append(row)
accepted=bool(targets) and not bad
report=dict(accepted=accepted,large_targets_with_at_least_10_percent_gain=targets,material_regressions=bad,all_changes=changes,gate='>=10% large threshold reduction; reject any increase >=10% and >max(0.2ms,2*(MAD_before+MAD_after))')
(root/'gate.json').write_text(json.dumps(report,indent=2)+'\n')
print('ACCEPT' if accepted else 'REJECT')
for x in changes:print(f"{x['workload']}: {x['before_ms']:.3f} -> {x['after_ms']:.3f} ms ({x['change_percent']:+.2f}%)"+(' MATERIAL REGRESSION' if x['material_regression'] else ''))
