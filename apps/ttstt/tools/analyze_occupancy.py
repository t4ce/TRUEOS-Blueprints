#!/usr/bin/env python3
"""Integrate exact kernel overlap; keep wall occupancy and CPU time separate."""
import argparse
from bisect import bisect_right
from collections import defaultdict
from itertools import groupby
import json
from pathlib import Path
import statistics


def histogram(intervals, start, end, limit):
    edges = [(start, 0), (end, 0)]
    total = 0
    for lo, hi in intervals:
        assert start <= lo <= hi <= end
        edges.extend(((lo, 1), (hi, -1)))
        total += hi-lo
    result = [0]*(limit+1)
    active, last = 0, start
    for stamp, entries in groupby(sorted(edges), key=lambda x: x[0]):
        assert 0 <= active <= limit
        result[active] += stamp-last
        active += sum(delta for _, delta in entries)
        last = stamp
    assert active == 0 and sum(result) == end-start
    assert sum(n*value for n, value in enumerate(result)) == total
    return result


def union_length(intervals):
    total, end = 0, -1
    for lo, hi in sorted(intervals):
        if lo >= end:
            total += hi-lo
        else:
            total += max(0, hi-end)
        end = max(end, hi)
    return total


def overview(events, begin, end, actors, bins=1800):
    # Binned display only. All numeric overlap statistics use exact endpoints.
    values = {cpu: [0.0]*bins for cpu in actors}
    width = (end-begin)/bins
    for lo, hi, _, _, kind, _, cpu in events:
        if kind not in (0, 2) or cpu not in values:
            continue
        first = min(bins-1, int((lo-begin)/width))
        last = min(bins-1, int((hi-begin)/width))
        for slot in range(first, last+1):
            amount = max(0, min(hi, begin+(slot+1)*width)-max(lo, begin+slot*width))
            values[cpu][slot] += amount/width
    return {str(cpu): [round(min(1, n), 3) for n in row] for cpu, row in values.items()}


def analyze_case(trace, run, metadata):
    begin, end, events = trace['start_ns'], trace['end_ns'], trace['events']
    kinds = trace['kind_names']
    workers = run['workers']
    numeric = 0 if workers else 1
    kernel = [e for e in events if e[4] == numeric]
    by_cpu = defaultdict(list)
    by_kind = defaultdict(lambda: {'wall_ns_sum': 0, 'cpu_ns': 0, 'calls': 0})
    by_op = defaultdict(lambda: defaultdict(lambda: [0, 0, 0]))
    for lo, hi, cpu_start, cpu_end, kind, op, cpu in events:
        assert begin <= lo <= hi <= end
        assert cpu_end >= cpu_start
        name = kinds[kind]
        wall, elapsed_cpu = hi-lo, cpu_end-cpu_start
        by_kind[name]['wall_ns_sum'] += wall
        by_kind[name]['cpu_ns'] += elapsed_cpu
        by_kind[name]['calls'] += 1
        by_op[op][name][0] += wall
        by_op[op][name][1] += elapsed_cpu
        by_op[op][name][2] += 1
        if kind == numeric:
            by_cpu[cpu].append((lo, hi))
    for intervals in by_cpu.values():
        intervals.sort()
        assert all(a[1] <= b[0] for a, b in zip(intervals, intervals[1:])), 'one CPU has overlapping kernels'
    intervals = [(e[0], e[1]) for e in kernel]
    limit = max(1, workers)
    hist = histogram(intervals, begin, end, limit)
    awaits = sorted((e[0], e[1]) for e in events if kinds[e[4]] == 'await_operation')
    await_ns = union_length(awaits)
    assert sum(hi-lo for lo, hi in awaits) == await_ns, 'graph has concurrent operations unexpectedly'
    if workers:
        starts = [lo for lo, _ in awaits]
        for lo, hi in intervals:
            position = bisect_right(starts, lo)-1
            assert position >= 0 and hi <= awaits[position][1], 'kernel outside its awaited operation'
        assert len(awaits) == (87 if trace['case'] == 'short' else 174 if trace['case'] == 'medium' else 435)
    kernel_wall = sum(hi-lo for lo, hi in intervals)
    kernel_cpu = sum(e[3]-e[2] for e in kernel)
    elapsed = end-begin
    conv_ops = {event[5] for event in events if event[4] in (0, 1)}
    other_dispatch = sum(parts.get('coordinator_dispatch', [0, 0, 0])[1]
                         for op, parts in by_op.items() if op not in conv_ops)
    groups = defaultdict(lambda: {'coordinator_cpu_ns': 0, 'worker_kernel_cpu_ns': 0})
    for op, parts in by_op.items():
        name = metadata.get(op, {}).get('source_name', '')
        group = 'other'
        for b in range(6):
            if f'/generator/resblocks.{b}/' in name:
                group = f'mrf_branch_{b}'
        for b in range(2):
            if f'/generator/noise_res.{b}/' in name:
                group = f'noise_branch_{b}'
        if 1639 <= op <= 1645 or 1911 <= op <= 1917:
            group = 'mrf_shared_input'
        elif metadata.get(op, {}).get('phase') == 0:
            group = 'phase0'
        groups[group]['coordinator_cpu_ns'] += parts.get('coordinator_dispatch', [0, 0])[1]
        groups[group]['worker_kernel_cpu_ns'] += parts.get('worker_kernel', [0, 0])[1]
    worker_thread_cpu = 0
    for cpu in run['compute_cpus']:
        records = [e for e in events if e[6] == cpu and e[4] != 10]
        if records:
            worker_thread_cpu += max(e[3] for e in records)-min(e[2] for e in records)
    result = {
        'case': trace['case'], 'workers': workers, 'repeat': run['repeat'],
        'elapsed_ns': elapsed, 'kernel_overlap_ns': hist, 'await_ns': await_ns,
        'outside_await_ns': elapsed-await_ns if workers else None,
        'await_with_no_kernel_ns': hist[0]-(elapsed-await_ns) if workers else None,
        'kernel_wall_ns_sum': kernel_wall, 'kernel_cpu_ns': kernel_cpu,
        'average_kernel_workers_whole_case': kernel_wall/elapsed,
        'average_kernel_workers_during_await': kernel_wall/await_ns if await_ns else None,
        'average_kernel_cpu_during_await': kernel_cpu/await_ns if await_ns else None,
        'all_workers_in_kernel_fraction': hist[limit]/elapsed,
        'kernel_cpu_to_wall_ratio': kernel_cpu/kernel_wall,
        'coordinator_cpu_ns': trace['coordinator_cpu_ns'],
        'coordinator_other_dispatch_cpu_ns': other_dispatch,
        'worker_thread_cpu_between_first_last_events_ns': worker_thread_cpu,
        'by_kind': dict(by_kind), 'by_op': dict(by_op), 'groups': dict(groups),
        'queue_average_ns': (by_kind['queue']['wall_ns_sum']/by_kind['queue']['calls'] if by_kind['queue']['calls'] else 0),
        'event_count': len(events),
    }
    assert result['await_with_no_kernel_ns'] is None or result['await_with_no_kernel_ns'] >= 0
    actors = [run['coordinator_cpu']] + run['compute_cpus']
    display = {'case': trace['case'], 'workers': workers, 'repeat': run['repeat'],
               'duration_s': elapsed/1e9, 'overlap_s': [n/1e9 for n in hist],
               'await_s': await_ns/1e9, 'actors': actors, 'bins': overview(events, begin, end, actors)}
    return result, display


def write_page(path, displays):
    page = r'''<!doctype html><html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>TTS compute occupancy</title><style>body{font:16px/1.5 system-ui;max-width:1150px;margin:30px auto;padding:0 20px;background:#111922;color:#e6edf3}select{padding:7px;background:#253444;color:inherit}canvas{width:100%;background:#192632;border:1px solid #384b5d;margin:20px 0}a{color:#9bd1ff}table{border-collapse:collapse}td,th{padding:8px 15px;border-bottom:1px solid #384b5d;text-align:right}p{max-width:950px;color:#b8c9d6}#controls{display:flex;gap:20px;flex-wrap:wrap}.key{color:#6ce4b3}input{width:100%}</style><h1>Where do the compute workers run?</h1><p>Exact kernel-region overlap is in the table. The timeline is a binned overview: green shows coordinator dispatch, cyan shows each compute CPU inside a numerical convolution call. These intervals include gathering, quantization and memory stalls; they do not measure VNNI instruction occupancy. Thread CPU clocks separately exclude descheduling.</p><div id="controls"><label>Passage <select id="case"><option>short</option><option>medium</option><option selected>long</option></select></label><label>Compute workers <select id="workers"><option value="0">Serial</option><option>1</option><option>2</option><option selected>3</option></select></label><label>Pass <select id="repeat"><option>0</option><option>1</option></select></label><label>View <select id="zoom"><option value="0">Full passage</option><option value="2">2 seconds</option><option value="0.5">0.5 seconds</option></select></label></div><input id="offset" type="range" min="0" max="1000" value="0"><canvas id="plot" width="1100" height="310"></canvas><div id="readout"></div><p>Zero compute workers in a numerical convolution call usually means the coordinator is executing another operator or preparing/committing data. It does not mean the entire system is idle. Native TRUEOS was not changed.</p><a href="analysis.json">Detailed CPU and per-operation measurements</a><script>
const data=DATA;const $=id=>document.getElementById(id);function draw(){const r=data.find(x=>x.case==$('case').value&&x.workers==+$('workers').value&&x.repeat==+$('repeat').value);if(!r)return;const c=$('plot'),x=c.getContext('2d');x.clearRect(0,0,c.width,c.height);const zoom=+$('zoom').value,span=zoom?Math.min(zoom,r.duration_s):r.duration_s,start=+$('offset').value/1000*(r.duration_s-span),left=110,w=960;let row=0;for(const cpu of r.actors){const bins=r.bins[String(cpu)];let y=35+row*55;x.fillStyle='#c6d8e6';x.fillText((row?'Compute':'Coordinator')+' CPU '+cpu,5,y+16);for(let i=0;i<bins.length;i++){let t=i/bins.length*r.duration_s,dt=r.duration_s/bins.length;if(t+dt<start||t>start+span)continue;let a=Math.max(t,start),b=Math.min(t+dt,start+span);x.fillStyle=row?`rgba(80,204,239,${bins[i]})`:`rgba(91,226,163,${bins[i]})`;x.fillRect(left+(a-start)/span*w,y,Math.max(1,(b-a)/span*w),28)}row++}x.fillStyle='#c6d8e6';for(let i=0;i<=8;i++){let t=start+i*span/8;x.fillText(t.toFixed(2)+'s',left+i*w/8,285)}let html=`<b>Passage time: ${r.duration_s.toFixed(3)} s. Awaited convolution windows: ${r.await_s.toFixed(3)} s.</b><table><tr><th>Simultaneous convolution calls</th><th>Seconds</th><th>Whole passage</th></tr>`;r.overlap_s.forEach((v,i)=>html+=`<tr><td>${i}</td><td>${v.toFixed(3)}</td><td>${(100*v/r.duration_s).toFixed(1)}%</td></tr>`);$('readout').innerHTML=html+'</table>'}for(const id of ['case','workers','repeat','zoom','offset'])$(id).addEventListener('input',draw);draw();</script></html>'''
    path.write_text(page.replace('DATA', json.dumps(displays, separators=(',', ':'))))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--plan', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    benchmark = json.loads(args.input.read_text())
    metadata = {}
    if args.plan:
        metadata = {o['op_index']: o for o in json.loads(args.plan.read_text())['operations']}
    results, displays = [], []
    for run in benchmark['runs']:
        for case in run['cases']:
            file = Path(case['wav']).parent/'trace'/(case['case']+'.json')
            row, display = analyze_case(json.loads(file.read_text()), run, metadata)
            row['trace'] = str(file)
            results.append(row)
            displays.append(display)
    summary = {}
    for name in ['short', 'medium', 'long']:
        summary[name] = {}
        for workers in range(4):
            rows = [r for r in results if r['case']==name and r['workers']==workers]
            target = {}
            for key in ['elapsed_ns', 'await_ns', 'outside_await_ns', 'await_with_no_kernel_ns',
                        'kernel_wall_ns_sum', 'kernel_cpu_ns', 'coordinator_cpu_ns',
                        'coordinator_other_dispatch_cpu_ns', 'average_kernel_workers_whole_case',
                        'average_kernel_workers_during_await', 'all_workers_in_kernel_fraction']:
                values = [r[key] for r in rows if r[key] is not None]
                target[key] = statistics.mean(values) if values else None
            target['overlap_ns'] = [statistics.mean(r['kernel_overlap_ns'][n] for r in rows)
                                    for n in range(max(1, workers)+1)]
            target['mean_cpu_by_kind_ns'] = {kind: statistics.mean(r['by_kind'].get(kind, {}).get('cpu_ns', 0) for r in rows)
                                            for kind in set().union(*(r['by_kind'] for r in rows))}
            summary[name][str(workers)] = target
    comparisons = {}
    for name, modes in summary.items():
        base = modes['0']['kernel_cpu_ns']
        comparisons[name] = {mode: {'conv_cpu_work_vs_serial':
            (row['kernel_cpu_ns']+row['mean_cpu_by_kind_ns'].get('quant_prepare',0))/base}
            for mode, row in modes.items()}
    result = {'metric_notes': ['Kernel intervals include all work within the numerical API, including gathers and cache/memory stalls.',
              'Thread CPU clocks exclude descheduling but do not measure arithmetic-unit or VNNI utilization.',
              'Coordinator dispatch spans contain nested setup/kernel spans; do not add these categories.',
              'Histogram uses exact nanosecond endpoints, not timeline bins.'],
              'host_cpu': benchmark['host_cpu'], 'binary_sha256': benchmark['binary_sha256'],
              'runs': results, 'summary': summary, 'comparisons': comparisons}
    (args.output/'analysis.json').write_text(json.dumps(result, indent=2)+'\n')
    write_page(args.output/'index.html', displays)
    for name,modes in summary.items():
        for n,row in modes.items():
            print(json.dumps({'case':name,'workers':n,'wall_s':row['elapsed_ns']/1e9,
                  'await_s':row['await_ns']/1e9,'overlap_s':[v/1e9 for v in row['overlap_ns']],
                  'cpu_math_s':row['kernel_cpu_ns']/1e9,
                  'work_inflation':comparisons[name][n]['conv_cpu_work_vs_serial']}))


if __name__ == '__main__':
    main()
