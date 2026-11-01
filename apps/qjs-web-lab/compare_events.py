#!/usr/bin/env python3
"""Compare fresh QuickJS startup runs with explicitly synthetic lifecycle events.

This replays saved sources offline. Chrome may use a frozen capture or a live
navigation; corpus identity and metric definitions are checked explicitly.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path

from run import ROOT, assess, execute, location

DEFAULT_EVENTS = [
    {'name':'pageshow', 'type':'pageshow', 'target':'window', 'init':{'persisted':False}},
    {'name':'focus', 'type':'focus', 'target':'window'},
    {'name':'resize', 'type':'resize', 'target':'window'},
    {'name':'scroll', 'type':'scroll', 'target':'window'},
    {'name':'visibility-hidden', 'type':'visibilitychange', 'target':'document', 'state':{'visibilityState':'hidden'}},
    {'name':'visibility-visible', 'type':'visibilitychange', 'target':'document', 'state':{'visibilityState':'visible'}},
    {'name':'readystatechange', 'type':'readystatechange', 'target':'document'},
]
LIFECYCLE_TYPES = {'readystatechange','DOMContentLoaded','load','pageshow','pagehide','focus','blur','visibilitychange','resize','scroll'}
NUMERIC_FIELDS = ('api_calls','allocated_nodes','connected_nodes','text_characters','text_draw_calls','text_draw_characters','errors','host_errors','unhandled_rejections','pending_timers')
STATE_FIELDS = ('allocated_nodes','connected_nodes','text_characters','text_draw_calls','text_draw_characters')
TEXT_MEASUREMENTS = {
    'text_characters':'raw connected text-node Unicode scalars, including SCRIPT/STYLE text; not rendered text',
    'chrome_content_text_unicode_scalars':'connected text-node Unicode scalars excluding SCRIPT, STYLE, NOSCRIPT and TEMPLATE ancestors; not visibility or rendered text',
    'text_draw_characters':'Unicode scalars in recorded canvas text arguments; no rasterization is measured',
    'content_text_comparison':'unavailable: QuickJS checkpoint has no equivalent content-only filter',
}


def source_identity(capture):
    # Hash actual source strings, not possibly stale advertised source hashes.
    sources={'scripts':[(entry['url'],hashlib.sha256(entry['source'].encode()).hexdigest()) for entry in capture['scripts']],
             'modules':[(url,hashlib.sha256(entry['source'].encode()).hexdigest()) for url,entry in sorted(capture.get('modules',{}).items())],
             'import_map':capture.get('import_map',{})}
    return hashlib.sha256(json.dumps(sources,sort_keys=True).encode()).hexdigest()


def attach_capture_identity(result, capture_path):
    try:
        raw=capture_path.read_bytes();current=json.loads(raw)
        verified=source_identity(current)==result['source'].get('identity_sha256')
        result['source']['capture_file_sha256']=hashlib.sha256(raw).hexdigest() if verified else None
        result['source']['capture_file_identity_verified']=verified
        result['source']['capture_file_identity_note']='script/module source identity matches the current capture file' if verified else 'current capture sources differ from the saved QuickJS execution corpus; current file hash was not attributed to that run'
    except (OSError,ValueError,KeyError) as error:
        result['source']['capture_file_sha256']=None
        result['source']['capture_file_identity_verified']=False
        result['source']['capture_file_identity_note']=str(error)



def baseline_payload(capture):
    return {
        'page':{'location':location(capture.get('final_url',capture['url']))},
        'scripts':capture['scripts'], 'modules':capture.get('modules',{}),
        'module_resolutions':capture.get('module_resolutions',{}),
        'import_map':capture.get('import_map',{}),
    }


def delta(before, after):
    return {key:after[key]-before[key] for key in NUMERIC_FIELDS
            if isinstance(before.get(key),(int,float)) and isinstance(after.get(key),(int,float))}


def extract_chrome_probe(report):
    """Accept either a snapshot itself or a CDP runner's baseline wrapper."""
    candidates = [report.get('baseline'), report.get('snapshot'), report.get('probe'), report]
    for candidate in candidates:
        if not isinstance(candidate,dict): continue
        for item in (candidate, candidate.get('snapshot'), candidate.get('probe')):
            if isinstance(item,dict) and ('lifecycle' in item or 'dom' in item): return item
    return None


def load_chrome(directory):
    for filename in ('chrome-events.json','chrome-report.json','chrome.json'):
        path=directory/filename
        if path.exists():
            try: return json.loads(path.read_text()), str(path)
            except (OSError,json.JSONDecodeError) as error: return {'read_error':str(error)},str(path)
    return None,None


def observed_candidates(chrome):
    if not isinstance(chrome,dict): return []
    probe=extract_chrome_probe(chrome)
    if not probe: return []
    events=[];seen=set()
    for registration in probe.get('lifecycle',{}).get('registrations',[]):
        kind=registration.get('type');target=registration.get('target',{})
        if kind not in LIFECYCLE_TYPES or not isinstance(target,dict): continue
        if target.get('kind') in ('window','document'): selector=target['kind']
        elif target.get('kind')=='element' and target.get('selector'): selector=target['selector']
        else: continue
        identity=(kind,selector)
        if identity in seen: continue
        seen.add(identity)
        events.append({'name':f'observed-{kind}-{selector}', 'type':kind,'target':selector,
                       'evidence':{'kind':'chrome-site-listener-registration','registration':registration}})
    return events


def run_case(payload, capture, events):
    try:
        report=execute(dict(payload,events=events))
        return {'status':'completed','summary':assess(capture,report),'report':report}
    except Exception as error:
        return {'status':'runner-failed','runner_error':str(error)}


def classify_case(case, separate_baseline):
    report=case.get('report',{});checkpoints=report.get('checkpoints',[])
    before=next((c for c in checkpoints if c.get('kind')=='baseline'),None)
    after=[c for c in checkpoints if c.get('kind')=='event']
    if case['status']!='completed': return
    if before is None or not after:
        case['event_comparison']={'available':False,'reason':'runner did not report an event checkpoint'};return
    last=after[-1]
    difference=delta(before,last)
    calls_before=before.get('api_counts',{});calls_after=last.get('api_counts',{})
    api_delta={key:value-calls_before.get(key,0) for key,value in calls_after.items() if value!=calls_before.get(key,0)}
    state_progress=any(difference.get(key,0)!=0 for key in STATE_FIELDS)
    separate_checkpoint=next((c for c in separate_baseline.get('report',{}).get('checkpoints',[]) if c.get('kind')=='baseline'),None)
    case['event_comparison']={
        'available':True,'before':before,'after':last,'within_runtime_delta':difference,
        'api_call_delta':api_delta, 'state_progress':state_progress,
        'api_activity':difference.get('api_calls',0)>0,
        'listener_callbacks':api_delta.get('EventTarget.listener',0),
        'progress_policy':'stored node/raw-text/recorded-draw changes, not rendered text; dispatch or API calls alone are not state progress',
        'separate_baseline_delta':delta(separate_checkpoint,last) if separate_checkpoint else None,
        'separate_baseline_caveat':'independent runtime; time/randomness can change startup paths',
    }



def chrome_metrics(probe):
    if not isinstance(probe,dict): return None
    dom=probe.get('dom',{});lifecycle=probe.get('lifecycle',{})
    callbacks=lifecycle.get('callbacks',{});text=dom.get('text',{})
    unicode_units=text.get('units')=='unicode-scalars'
    raw=text.get('charactersUnicodeIncludingScripts',text.get('characters')) if unicode_units else None
    content=text.get('contentCharactersUnicode',text.get('contentCharacters')) if unicode_units else None
    return {
        'connected_nodes':dom.get('nodeCount'),
        'text_characters':raw, 'raw_text_unicode_scalars':raw,
        'chrome_content_text_unicode_scalars':content,
        'raw_text_utf16_units':text.get('charactersUTF16IncludingScripts',text.get('characters') if probe.get('version')==1 and not unicode_units else None),
        'legacy_untyped_text_count':text.get('characters') if not unicode_units else None,
        'text_units_verified':unicode_units, 'text_measurements':TEXT_MEASUREMENTS,
        'connected_scope_verified':dom.get('scope')=='connected-document-and-shadow-roots',
        'listener_callbacks':sum(value for value in callbacks.values() if isinstance(value,(int,float))),
        'listener_callbacks_by_type':callbacks,
        'dom_truncated':bool(dom.get('truncated') or dom.get('textTruncated')),
    }


def cross_engine_case(case, chrome):
    if not isinstance(chrome,dict) or not isinstance(case.get('event'),dict): return
    comparison=case.get('event_comparison',{})
    if not comparison.get('available'): return
    baseline=extract_chrome_probe(chrome)
    event=case['event']
    candidates=chrome.get('events',[])
    matching=next((item for item in candidates if item.get('name')==case['name']),None)
    if matching is None:
        matching=next((item for item in candidates if item.get('event',{}).get('type')==event['type'] and item.get('event',{}).get('target')==event['target'] and item.get('event',{}).get('state',{})==event.get('state',{})),None)
    if matching is None:
        case['chrome_comparison']={'available':False,'reason':'Chrome did not run a matching candidate'};return
    after=extract_chrome_probe(matching)
    if baseline is None or after is None:
        case['chrome_comparison']={'available':False,'reason':'Chrome candidate lacks usable snapshots'};return
    before_metrics=chrome_metrics(extract_chrome_probe(matching.get('before',{})) or baseline);after_metrics=chrome_metrics(after)
    chrome_delta={key:after_metrics[key]-before_metrics[key] for key in ('connected_nodes','text_characters','listener_callbacks')
                  if isinstance(before_metrics.get(key),(int,float)) and isinstance(after_metrics.get(key),(int,float))}
    quickjs_delta=comparison['within_runtime_delta']
    shared_fields=[]
    if before_metrics['connected_scope_verified'] and after_metrics['connected_scope_verified']:
        shared_fields.append('connected_nodes')
        if before_metrics['text_units_verified'] and after_metrics['text_units_verified']:shared_fields.append('text_characters')
    case['chrome_comparison']={'available':True,'chrome_before':before_metrics,'chrome_after':after_metrics,'chrome_delta':chrome_delta,
        'quickjs_delta':{key:quickjs_delta.get(key) for key in ('connected_nodes','text_characters')},
        'quickjs_listener_callbacks':comparison['listener_callbacks'],
        'state_progress':{'chrome':any(chrome_delta.get(key,0)!=0 for key in shared_fields) if shared_fields else None, 'quickjs':any(quickjs_delta.get(key,0)!=0 for key in shared_fields) if shared_fields else None},
        'shared_progress_fields':shared_fields,
        'quickjs_broad_state_progress':comparison['state_progress'],
        'chrome_content_text_delta':(after_metrics['chrome_content_text_unicode_scalars']-before_metrics['chrome_content_text_unicode_scalars'] if isinstance(after_metrics.get('chrome_content_text_unicode_scalars'),(int,float)) and isinstance(before_metrics.get('chrome_content_text_unicode_scalars'),(int,float)) else None),
        'content_text_comparison':'unavailable: no equivalent content-only QuickJS checkpoint',
        'text_measurements':TEXT_MEASUREMENTS,
        'caveat':'Chrome event windows may include scheduled work and earlier scenario state; QuickJS candidates use fresh runtimes. DOM/environment differ; dispatch alone is not progress and matching counts do not establish functional equivalence'}


def chrome_reference(chrome, path, source=None):
    if chrome is None: return {'available':False,'reason':'no Chrome report was available'}
    probe=extract_chrome_probe(chrome)
    if not probe: return {'available':False,'path':path,'reason':'Chrome report has no usable probe snapshot','report':chrome}
    source=source or {}
    matching_capture=chrome.get('mode')=='captured-offline' and bool(source.get('capture_file_sha256')) and source['capture_file_sha256']==chrome.get('capture_sha256')
    return {
        'available':True,'path':path,'baseline_probe':probe,
        'comparison_scope':('verified same capture snapshot provenance; Chrome loads full saved HTML and admitted sources while QuickJS starts with a synthetic lazy DOM, so executed-program parity is not established' if matching_capture else 'capture snapshot provenance is unverified or differs; observations are not an identical-input comparison'),
        'same_capture_snapshot_verified':matching_capture,
        'capture_hash_scope':'capture.json metadata and script/module sources; the separate initial HTML file is not part of this hash',
        'same_script_corpus_verified':False,
        'executed_program_parity':chrome.get('execution_source_parity',chrome.get('script_parity','not-established')),
        'chrome_source_scope':chrome.get('source_scope','full document parsing differs from QuickJS lazy DOM'),
        'capture_issues':chrome.get('capture_issues',source.get('capture_issues',[])),
        'chrome_capture_sha256':chrome.get('capture_sha256'),
        'chrome_capture_timestamp':chrome.get('capture_timestamp'),
        'quickjs_capture_sha256':source.get('capture_file_sha256'),
        'same_initial_dom':False,
        'chrome_mode':chrome.get('mode','unknown'),
        'synthetic_events_are_trusted':False,'text_measurements':TEXT_MEASUREMENTS,
        'idle_controls':chrome.get('controls',[]),
    }


def compare_site(name, capture, output, chrome=None, chrome_path=None, combined=False, include_observed=True):
    payload=baseline_payload(capture)
    unavailable=not capture['scripts'] and bool(capture['issues'])
    baseline=({'status':'unavailable','runner_error':'saved capture has no scripts and contains capture failures; no VM startup was treated as a website baseline'} if unavailable else run_case(payload,capture,[]))
    baseline_checkpoint=next((c for c in baseline.get('report',{}).get('checkpoints',[]) if c.get('kind')=='baseline'),{})
    event_budget_unavailable=baseline['status']=='runner-failed' or baseline_checkpoint.get('deadline_exceeded',False)
    candidates=[dict(item,evidence={'kind':'generic-lifecycle-probe'}) for item in DEFAULT_EVENTS]
    if unavailable or event_budget_unavailable:candidates=[]
    if include_observed and not unavailable and not event_budget_unavailable:
        seen={(item['type'],item['target']) for item in candidates}
        for event in observed_candidates(chrome):
            if (event['type'],event['target']) not in seen:
                candidates.append(event);seen.add((event['type'],event['target']))
            if len(candidates)>=16:break
    cases=[]
    for event in candidates:
        case={'name':event['name'],'event':event,'synthetic':True,'trusted':False,'fresh_runtime':True,
              **run_case(payload,capture,[event])}
        classify_case(case,baseline);cross_engine_case(case,chrome);cases.append(case)
    combined_case=None
    if combined and not unavailable and not event_budget_unavailable:
        combined_case={'name':'combined-sequence','events':candidates,'synthetic':True,'trusted':False,'fresh_runtime':True,
                       **run_case(payload,capture,candidates)}
        classify_case(combined_case,baseline)
    result={
        'site':name,'generated_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'source':{'url':capture['url'],'captured_at':capture.get('captured_at'),'scripts':len(capture['scripts']),
                  'capture_issues':capture['issues'],
                  'identity_sha256':source_identity(capture)},
        'policy':{'text_measurements':TEXT_MEASUREMENTS,'headless':True,'guest_network':False,'synthetic_events':True,'trusted_events':False,
                  'fresh_runtime_per_case':True,'event_cases_skipped':('saved capture unavailable' if unavailable else 'startup failed or exhausted execution deadline' if event_budget_unavailable else None),'interactivity_or_site_functionality':'not-validated',
                  'untrusted_event_caveat':'handlers that require trusted input or real document/network state may ignore these probes'},
        'chrome_reference':{},'baseline':baseline,'cases':cases,'combined_sequence':combined_case,
    }
    attach_capture_identity(result,ROOT/'artifacts'/name/'capture.json')
    result['chrome_reference']=chrome_reference(chrome,chrome_path,result['source'])
    return write_result(result,output)


def write_result(result,output):
    name=result['site'];baseline=result['baseline'];cases=result['cases']
    unavailable=result['policy'].get('event_cases_skipped')=='saved capture unavailable'
    event_budget_unavailable=result['policy'].get('event_cases_skipped')=='startup failed or exhausted execution deadline'
    output.mkdir(parents=True,exist_ok=True)
    (output/'quickjs-events.json').write_text(json.dumps(result,indent=2))
    before=next((c for c in baseline.get('report',{}).get('checkpoints',[]) if c.get('kind')=='baseline'),{})
    summary={
        'site':name,'baseline':{key:before.get(key) for key in NUMERIC_FIELDS},
        'baseline_execution':baseline.get('summary',{}).get('execution',baseline['status']),
        'baseline_listener_callbacks':before.get('api_counts',{}).get('EventTarget.listener',0),
        'chrome_available':result['chrome_reference']['available'],
        'chrome_baseline':chrome_metrics(result.get('chrome_reference',{}).get('baseline_probe')),
        'fresh_event_cases':len(cases),
        'cases_with_api_activity':[c['name'] for c in cases if c.get('event_comparison',{}).get('api_activity')],
        'cases_with_state_progress':[c['name'] for c in cases if c.get('event_comparison',{}).get('state_progress')],
        'runner_failures':[c['name'] for c in cases if c['status']=='runner-failed'],
        'event_checkpoint_failures':[c['name'] for c in cases if c['status']=='completed' and not c.get('event_comparison',{}).get('available')],
        'capture_issues':len(result['source']['capture_issues']),
        'capture_unavailable':unavailable,
        'event_budget_unavailable':bool(event_budget_unavailable),
        'text_measurements':TEXT_MEASUREMENTS,
        'same_script_corpus_verified':False,
        'same_capture_snapshot_verified':result.get('chrome_reference',{}).get('same_capture_snapshot_verified',False),
        'executed_program_parity':result.get('chrome_reference',{}).get('executed_program_parity','not-established'),
        'event_deltas':[{'name':case['name'],'listener_callbacks':case.get('event_comparison',{}).get('listener_callbacks'),'state_progress':case.get('event_comparison',{}).get('state_progress'),'quickjs':case.get('event_comparison',{}).get('within_runtime_delta'),'chrome':case.get('chrome_comparison',{}).get('chrome_delta')} for case in cases],
    }
    (output/'quickjs-events-summary.json').write_text(json.dumps(summary,indent=2))
    return summary


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--site',action='append',help='saved site key; repeat to select several')
    parser.add_argument('--refresh-chrome-only',action='store_true',help='refresh Chrome references/deltas on saved QuickJS cases without executing the VM')
    parser.add_argument('--combined',action='store_true',help='also test all candidate events in one separate runtime')
    parser.add_argument('--generic-only',action='store_true',help='do not add candidates from observed Chrome registrations')
    parser.add_argument('--output',type=Path,default=ROOT/'artifacts/chrome-comparison')
    parser.add_argument('--chrome-dir',type=Path,help='directory with per-site Chrome reports; defaults to output')
    args=parser.parse_args();sites=json.loads((ROOT/'sites.json').read_text());selected=args.site or list(sites)
    if any(name not in sites for name in selected):parser.error('unknown site key')
    if args.refresh_chrome_only and (args.combined or args.generic_only):parser.error('--refresh-chrome-only retains saved scenarios; it cannot change event selection')
    summaries=[]
    for name in selected:
        if args.refresh_chrome_only:
            saved=args.output/name/'quickjs-events.json'
            if not saved.exists():
                print(f'{name}: no saved QuickJS event comparison',flush=True);summaries.append({'site':name,'error':'no saved QuickJS event comparison'});continue
            result=json.loads(saved.read_text())
            chrome,path=load_chrome((args.chrome_dir or args.output)/name)
            attach_capture_identity(result,ROOT/'artifacts'/name/'capture.json')
            result['chrome_reference']=chrome_reference(chrome,path,result['source'])
            result['chrome_refreshed_at']=datetime.datetime.now(datetime.timezone.utc).isoformat()
            result['policy']['text_measurements']=TEXT_MEASUREMENTS
            result['policy']['chrome_refresh_only']=True
            result['policy']['observed_candidate_refresh']='saved QuickJS scenarios retained; newly observed Chrome registrations are not replayed'
            for case in result['cases']:
                case.pop('chrome_comparison',None)
                classify_case(case,result['baseline']);cross_engine_case(case,chrome)
            summary=write_result(result,args.output/name);summaries.append(summary)
            print(f'{name}: refreshed Chrome reference; capture snapshot verified={summary["same_capture_snapshot_verified"]}; executed-program parity={summary["executed_program_parity"]}',flush=True)
            continue
        capture_path=ROOT/'artifacts'/name/'capture.json'
        if not capture_path.exists():
            print(f'{name}: no saved capture',flush=True);summaries.append({'site':name,'error':'no saved capture'});continue
        capture=json.loads(capture_path.read_text())
        chrome,path=load_chrome((args.chrome_dir or args.output)/name)
        summary=compare_site(name,capture,args.output/name,chrome,path,args.combined,not args.generic_only)
        summaries.append(summary)
        print(f"{name}: baseline {summary['baseline_execution']}; API activity {len(summary['cases_with_api_activity'])}/{summary['fresh_event_cases']}; state progress {summary['cases_with_state_progress']}",flush=True)
    args.output.mkdir(parents=True,exist_ok=True)
    (args.output/'quickjs-events-summary.json').write_text(json.dumps(summaries,indent=2))
    return int(any(s.get('error') or s.get('capture_unavailable') or s.get('runner_failures') or s.get('event_budget_unavailable') or s.get('event_checkpoint_failures') for s in summaries))

if __name__=='__main__':raise SystemExit(main())
