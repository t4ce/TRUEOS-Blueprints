#!/usr/bin/env python3
"""Capture public scripts, then execute offline in a fresh isolated QuickJS process."""
import argparse
import concurrent.futures
import datetime
import hashlib
from html.parser import HTMLParser
import json
import os
import shutil
from pathlib import Path
import resource
import subprocess
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent
BINARY = REPO / 'target/x86_64-unknown-linux-gnu/debug/qjs-web-lab'
MAX_RESOURCE = 16 * 1024 * 1024
MAX_SCRIPTS = 256
MAX_TOTAL_SOURCES = 64 * 1024 * 1024
MAX_MODULES = 256
MAX_MODULE_DEPTH = 12

class Scripts(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=False)
        self.scripts, self.current, self.base = [], None, None
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'base' and self.base is None:
            self.base = attrs.get('href')
        if tag == 'script':
            self.current = {'attrs': attrs, 'text': ''}
    def handle_data(self, data):
        if self.current is not None:
            self.current['text'] += data
    def handle_endtag(self, tag):
        if tag == 'script' and self.current is not None:
            self.scripts.append(self.current)
            self.current = None

def download(url):
    if urllib.parse.urlsplit(url).scheme not in ('https', 'http'):
        raise ValueError('only HTTP(S) capture is supported')
    req = urllib.request.Request(url, headers={'User-Agent': 'TRUEOS-QJS-Web-Lab/0.1 (public-page compatibility testing)', 'Accept': '*/*'})
    with urllib.request.urlopen(req, timeout=20) as response:
        body = response.read(MAX_RESOURCE + 1)
        if len(body) > MAX_RESOURCE:
            raise ValueError('resource exceeds 16 MiB capture limit')
        return body.decode(response.headers.get_content_charset() or 'utf-8', errors='replace'), response.url, response.status

def capture(name, url, directory):
    directory.mkdir(parents=True, exist_ok=True)
    issues, scripts, skipped = [], [], []
    result = {'name': name, 'url': url, 'captured_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'issues': issues, 'skipped': skipped, 'scripts': scripts}
    try:
        html, final, status = download(url)
        (directory / 'page.html').write_text(html)
        result.update(final_url=final, status=status)
        parsed = Scripts(); parsed.feed(html)
        base = urllib.parse.urljoin(final, parsed.base or final)
        def script(item):
            index, entry = item; attrs = entry['attrs']; kind = attrs.get('type', '').strip().lower()
            if kind not in ('', 'text/javascript', 'application/javascript', 'module'):
                return {'skip': {'index': index, 'type': kind}}
            if 'nomodule' in attrs:
                return {'skip': {'index': index, 'type': 'nomodule'}}
            source, script_url = entry['text'], final + '#inline-' + str(index)
            source_base = base
            try:
                if attrs.get('src'):
                    script_url = urllib.parse.urljoin(base, attrs['src']); source, source_base, _ = download(script_url)
                return {'url': script_url, 'final_url':source_base, 'source': source, 'module': kind == 'module', 'index': index, 'async': 'async' in attrs, 'defer': 'defer' in attrs, 'sha256': hashlib.sha256(source.encode()).hexdigest()}
            except Exception as error:
                return {'issue': {'phase': 'script-fetch', 'url': script_url, 'error': str(error)}}
        if len(parsed.scripts) > MAX_SCRIPTS:
            issues.append({'phase': 'capture-limit', 'error': f'more than {MAX_SCRIPTS} script tags'})
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            total = 0
            for entry in pool.map(script, enumerate(parsed.scripts[:MAX_SCRIPTS])):
                if 'issue' in entry: issues.append(entry['issue'])
                elif 'skip' in entry: skipped.append(entry['skip'])
                else:
                    total += len(entry['source'].encode())
                    if total > MAX_TOTAL_SOURCES:
                        issues.append({'phase': 'capture-limit', 'error': 'scripts exceed 64 MiB'}); break
                    scripts.append(entry)
    except Exception as error:
        issues.append({'phase': 'page-fetch', 'error': str(error)})
    if not any(issue['phase'] == 'page-fetch' for issue in issues):
        capture_module_graph(result, parsed, base)
    (directory / 'capture.json').write_text(json.dumps(result, indent=2))
    return result


# The controller uses a real JavaScript parser. No JavaScript from a website is
# executed here; Acorn produces an AST and we inspect import/export syntax.
ACORN_IMPORTS = r"""
const fs = require('fs'), acorn = require('acorn');
const inputs = JSON.parse(fs.readFileSync(0, 'utf8'));
const results = inputs.map(({url,source,module}) => {
    try {
        const ast = acorn.parse(source, {ecmaVersion:'latest', sourceType:module?'module':'script', allowHashBang:true});
        const imports = new Set(), pending = [ast]; let computed = 0;
        while (pending.length) {
            const node = pending.pop();
            if (!node || typeof node !== 'object') continue;
            if (node.type === 'ImportDeclaration' || node.type === 'ExportAllDeclaration' || node.type === 'ExportNamedDeclaration') {
                if (node.source) imports.add(node.source.value);
            } else if (node.type === 'ImportExpression') {
                if (node.source?.type === 'Literal' && typeof node.source.value === 'string') imports.add(node.source.value);
                else computed++;
            }
            for (const [key,value] of Object.entries(node)) {
                if (key === 'start' || key === 'end') continue;
                if (Array.isArray(value)) { for (const child of value) if (child && typeof child === 'object') pending.push(child); }
                else if (value && typeof value === 'object') pending.push(value);
            }
        }
        return {url, imports:[...imports], computed_imports:computed};
    } catch (error) { return {url, error:String(error)}; }
});
process.stdout.write(JSON.stringify({parser:'acorn',version:acorn.version,results}));
"""

def parse_imports(sources):
    if not shutil.which('node'):
        raise RuntimeError('module capture requires Node.js and the Acorn parser')
    result = subprocess.run(['node', '--max-old-space-size=512', '-e', ACORN_IMPORTS],
        input=json.dumps(sources), text=True, capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError('Acorn module parser failed: ' + result.stderr[:1000])
    return json.loads(result.stdout)

def is_url_specifier(value):
    return value.startswith(('/', './', '../', 'http://', 'https://'))

def import_mapping(raw, base, issues):
    def table(values):
        mapped = {}
        for key, target in values.items() if isinstance(values, dict) else []:
            if not isinstance(target, str):
                issues.append({'phase':'import-map', 'error':f'blocked/invalid import-map target for {key}'}); continue
            normalized = urllib.parse.urljoin(base,key) if is_url_specifier(key) else key
            destination = urllib.parse.urljoin(base,target)
            if key.endswith('/') and not destination.endswith('/'):
                issues.append({'phase':'import-map','error':f'prefix target must end in / for {key}'}); continue
            mapped[normalized] = destination
        return mapped
    scopes = raw.get('scopes',{})
    return {'imports':table(raw.get('imports',{})), 'scopes':{
        urllib.parse.urljoin(base,scope):table(values) for scope,values in scopes.items()
    } if isinstance(scopes,dict) else {}}

def resolve_import(name, base, mapping):
    normalized = urllib.parse.urljoin(base,name) if is_url_specifier(name) else name
    tables = [table for scope,table in sorted(mapping['scopes'].items(),key=lambda x:len(x[0]),reverse=True) if base.startswith(scope)]
    tables.append(mapping['imports'])
    for table in tables:
        if normalized in table: return table[normalized]
        prefixes = [key for key in table if key.endswith('/') and normalized.startswith(key)]
        if prefixes:
            key=max(prefixes,key=len); return table[key]+normalized[len(key):]
    if is_url_specifier(name): return normalized
    raise ValueError(f'unmapped bare module specifier: {name}')

def capture_module_graph(result, parsed, base):
    issues = result['issues']
    mapping = {'imports':{}, 'scopes':{}}
    for entry in parsed.scripts:
        if entry['attrs'].get('type','').lower().strip() != 'importmap': continue
        try:
            update=import_mapping(json.loads(entry['text']),base,issues)
            # Earlier mappings win, matching browser import-map merge behavior.
            for key,value in update['imports'].items(): mapping['imports'].setdefault(key,value)
            for scope,values in update['scopes'].items():
                scoped=mapping['scopes'].setdefault(scope,{})
                for key,value in values.items(): scoped.setdefault(key,value)
        except Exception as error: issues.append({'phase':'import-map','error':str(error)})
    result['import_map']=mapping
    modules, resolutions = {}, {}
    result['modules'], result['module_resolutions'] = modules, resolutions
    seen = {entry['url'] for entry in result['scripts']}
    wave = [dict(entry, depth=0) for entry in result['scripts']]
    total = sum(len(entry['source'].encode()) for entry in wave)
    graph = result['module_graph'] = {'status':'captured', 'parser':'acorn', 'computed_imports':0, 'computed_imports_policy':'literal dynamic imports captured; computed targets require a captured runtime resolution or mapped/absolute module identity', 'module_count':0, 'limits':{'modules':MAX_MODULES,'depth':MAX_MODULE_DEPTH,'total_source_bytes':MAX_TOTAL_SOURCES}}
    while wave:
        try: parsed_wave=parse_imports(wave)
        except Exception as error:
            issues.append({'phase':'module-parse','error':str(error)}); graph['status']='incomplete'; break
        graph['parser_version']=parsed_wave['version']
        originals = {entry['url']:entry for entry in wave}
        wanted = {}
        for entry in parsed_wave['results']:
            source=originals[entry['url']]
            if 'error' in entry:
                # Classic scripts can contain syntax intentionally unsupported by
                # Acorn; never claim their dynamic dependencies were inspected.
                issues.append({'phase':'module-parse','url':entry['url'],'error':entry['error']}); graph['status']='incomplete'; continue
            graph['computed_imports'] += entry['computed_imports']
            resolved = resolutions.setdefault(entry['url'],{})
            for specifier in entry['imports']:
                try:
                    url=resolve_import(specifier,source.get('final_url',entry['url']),mapping)
                    if urllib.parse.urlsplit(url).scheme not in ('http','https'):
                        raise ValueError(f'unsupported module URL scheme: {url}')
                    resolved[specifier]=url
                    if url in seen: continue
                    seen.add(url)
                    if source['depth'] >= MAX_MODULE_DEPTH: raise ValueError('module dependency depth limit reached')
                    if len(modules)+len(wanted) >= MAX_MODULES: raise ValueError('module count limit reached')
                    wanted[url]=source['depth']+1
                except Exception as error:
                    issues.append({'phase':'module-resolution','url':entry['url'],'specifier':specifier,'error':str(error)}); graph['status']='incomplete'
        if not wanted: break
        def fetch(item):
            url,depth=item
            try:
                source,final,status=download(url)
                return {'url':url,'final_url':final,'source':source,'module':True,'depth':depth,'status':status,'sha256':hashlib.sha256(source.encode()).hexdigest()}
            except Exception as error: return {'issue':{'phase':'module-fetch','url':url,'error':str(error)}}
        wave=[]
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            for entry in pool.map(fetch,wanted.items()):
                if 'issue' in entry:
                    issues.append(entry['issue']);graph['status']='incomplete';continue
                total += len(entry['source'].encode())
                if total > MAX_TOTAL_SOURCES:
                    issues.append({'phase':'capture-limit','error':'module graph exceeds 64 MiB source limit'});graph['status']='incomplete';break
                modules[entry['url']]=entry;wave.append(entry)
    graph['module_count']=len(modules)
    graph['total_source_bytes']=total


def limits():
    resource.setrlimit(resource.RLIMIT_CPU, (12, 12))
    resource.setrlimit(resource.RLIMIT_AS, (768 * 1024 * 1024,) * 2)
    resource.setrlimit(resource.RLIMIT_FSIZE, (1024 * 1024,) * 2)
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

def execute(payload):
    command = ['bwrap', '--unshare-all', '--die-with-parent', '--new-session', '--clearenv', '--setenv', 'TZ', 'UTC']
    for path in ['/usr', '/lib', '/lib64']:
        if Path(path).exists(): command += ['--ro-bind', path, path]
    command += ['--ro-bind', str(BINARY), '/vm', '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp', '--chdir', '/tmp', '/vm']
    run = subprocess.run(command, input=json.dumps(payload), text=True, capture_output=True, timeout=15, preexec_fn=limits)
    if run.returncode:
        raise RuntimeError(f'isolated VM exited {run.returncode}: {run.stderr[:4000]}')
    return json.loads(run.stdout)

def location(url):
    p = urllib.parse.urlsplit(url)
    return dict(href=url, origin=f'{p.scheme}://{p.netloc}', protocol=p.scheme+':', host=p.netloc, hostname=p.hostname, port=str(p.port or ''), pathname=p.path or '/', search=('?'+p.query if p.query else ''), hash=('#'+p.fragment if p.fragment else ''))

def assess(captured, report):
    host = report.get('host', {})
    diagnostics = len(report.get('errors', [])) + len(host.get('errors', [])) + len(report.get('unhandled_rejections', [])) + sum(x['level'] in ('warn','error','assert') for x in host.get('logs', []))
    graph_missing = bool(captured['scripts']) and 'module_graph' not in captured
    incomplete = bool(captured['issues'] or graph_missing or report.get('modules', {}).get('missing') or report.get('pending_jobs') or host.get('pendingTimers') or host.get('droppedLogs'))
    # A run with no exceptions is never proof of website functionality.
    return {'execution': 'incomplete' if incomplete else 'diagnostics' if diagnostics else 'clean', 'diagnostics': diagnostics, 'website_functionality': 'not-validated', 'dom_mode': 'lazy-synthetic', 'module_dependencies': captured.get('module_graph', {}).get('status', 'not-captured'), 'dynamic_scripts': 'unsupported', 'ordering': 'HTML order; async/defer not modelled'}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--site', action='append', help='site key; repeat to select several')
    parser.add_argument('--refresh-modules', action='store_true', help='add a fresh dependency capture to saved page/script sources')
    parser.add_argument('--replay', action='store_true', help='use saved captures without network')
    args = parser.parse_args()
    sites = json.loads((ROOT / 'sites.json').read_text())
    selected = args.site or list(sites)
    if args.replay and args.refresh_modules: parser.error('--replay and --refresh-modules are mutually exclusive')
    if any(name not in sites for name in selected): parser.error('unknown site key')
    results = []
    for name in selected:
        directory = ROOT / 'artifacts' / name
        captured = json.loads((directory / 'capture.json').read_text()) if args.replay or args.refresh_modules else capture(name, sites[name], directory)
        if args.refresh_modules:
            parsed=Scripts();parsed.feed((directory/'page.html').read_text())
            base=urllib.parse.urljoin(captured.get('final_url',sites[name]),parsed.base or captured.get('final_url',sites[name]))
            captured['issues']=[issue for issue in captured['issues'] if issue['phase'] not in ('import-map','module-parse','module-resolution','module-fetch')]
            capture_module_graph(captured,parsed,base)
            (directory/'capture.json').write_text(json.dumps(captured,indent=2))
        try:
            report = execute({'page': {'location': location(captured.get('final_url', sites[name]))}, 'scripts': captured['scripts'], 'modules':captured.get('modules',{}), 'module_resolutions':captured.get('module_resolutions',{}), 'import_map':captured.get('import_map',{})})
            summary = assess(captured, report)
        except Exception as error:
            report, summary = {'runner_error': str(error)}, {'execution': 'runner-failed', 'website_functionality': 'not-validated'}
        result = {'name': name, 'capture_issues': captured['issues'], 'summary': summary, 'report': report}
        (directory / 'report.json').write_text(json.dumps(result, indent=2))
        results.append(result)
        print(name, summary['execution'], len(captured['scripts']), 'scripts', flush=True)
    (ROOT / 'artifacts' / 'summary.json').write_text(json.dumps(results, indent=2))
    return int(any(r['summary']['execution'] != 'clean' for r in results))

if __name__ == '__main__':
    raise SystemExit(main())
