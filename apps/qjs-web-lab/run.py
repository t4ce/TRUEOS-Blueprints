#!/usr/bin/env python3
"""Capture public scripts, then execute offline in a fresh isolated QuickJS process."""
import argparse
import concurrent.futures
import datetime
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import resource
import subprocess
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent
BINARY = REPO / 'target/x86_64-unknown-linux-gnu/debug/qjs-web-lab'
MAX_RESOURCE = 4 * 1024 * 1024

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
            raise ValueError('resource exceeds 4 MiB capture limit')
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
            try:
                if attrs.get('src'):
                    script_url = urllib.parse.urljoin(base, attrs['src']); source, _, _ = download(script_url)
                return {'url': script_url, 'source': source, 'module': kind == 'module', 'index': index, 'async': 'async' in attrs, 'defer': 'defer' in attrs, 'sha256': hashlib.sha256(source.encode()).hexdigest()}
            except Exception as error:
                return {'issue': {'phase': 'script-fetch', 'url': script_url, 'error': str(error)}}
        if len(parsed.scripts) > 80:
            issues.append({'phase': 'capture-limit', 'error': 'more than 80 script tags'})
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            total = 0
            for entry in pool.map(script, enumerate(parsed.scripts[:80])):
                if 'issue' in entry: issues.append(entry['issue'])
                elif 'skip' in entry: skipped.append(entry['skip'])
                else:
                    total += len(entry['source'].encode())
                    if total > 24 * 1024 * 1024:
                        issues.append({'phase': 'capture-limit', 'error': 'scripts exceed 24 MiB'}); break
                    scripts.append(entry)
    except Exception as error:
        issues.append({'phase': 'page-fetch', 'error': str(error)})
    (directory / 'capture.json').write_text(json.dumps(result, indent=2))
    return result

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
    diagnostics = len(report.get('errors', [])) + len(report.get('unhandled_rejections', [])) + sum(x['level'] in ('warn','error','assert') for x in host.get('logs', []))
    incomplete = bool(captured['issues'] or report.get('pending_jobs') or host.get('pendingTimers') or host.get('droppedLogs'))
    # A run with no exceptions is never proof of website functionality.
    return {'execution': 'incomplete' if incomplete else 'diagnostics' if diagnostics else 'clean', 'diagnostics': diagnostics, 'website_functionality': 'not-validated', 'dom_mode': 'lazy-synthetic', 'module_dependencies': 'unsupported', 'dynamic_scripts': 'unsupported', 'ordering': 'HTML order; async/defer not modelled'}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--site', action='append', help='site key; repeat to select several')
    parser.add_argument('--replay', action='store_true', help='use saved captures without network')
    args = parser.parse_args()
    sites = json.loads((ROOT / 'sites.json').read_text())
    selected = args.site or list(sites)
    if any(name not in sites for name in selected): parser.error('unknown site key')
    results = []
    for name in selected:
        directory = ROOT / 'artifacts' / name
        captured = json.loads((directory / 'capture.json').read_text()) if args.replay else capture(name, sites[name], directory)
        try:
            report = execute({'page': {'location': location(captured.get('final_url', sites[name]))}, 'scripts': captured['scripts']})
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
