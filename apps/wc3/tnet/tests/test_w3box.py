"""Local tests use a FAKE PvPGN process, not the Warcraft/Battle.net protocol."""
from __future__ import annotations
import copy
import http.client
import importlib.util
import json
import os
import signal
import socket
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import unittest
from pathlib import Path

APP = Path(__file__).resolve().parents[1] / "app"
sys.path.insert(0, str(APP))
from common import (DEFAULTS, PIN, Layout, atomic_write, check_password, initialize,
                    load_settings, only_121b, password_record, read_json,
                    render_config, set_directives, valid_host, validate_settings, write_json)
from server import Application, Manager, WebServer, gateway_script

PASSWORD = 'correct-horse-private-realm'
VERSIONS = {"WAR3": {"IX86": {"0x15": {
    "checkRevisionFile": "IX86ver1.mpq",
    "equation": "A=3845581634 B=880823580 C=1363937103 4 A=A-S B=B-C C=C-A A=A-B",
    "entries": [
        {"title":"Warcraft III - ROC 1.21b","version":"1.21.1.156","hash":"0x1b735294","fileMetadata":"war3.exe 07/19/07 18:41:12 409660","versionTag":"WAR3_121B"},
        {"versionTag":"WAR3_121A"}
    ]
}}}, "W3XP":{"IX86":{}}}

FAKE = r'''#!/usr/bin/env python3
# Test fixture only. Does not implement any game protocol.
import argparse, pathlib, re, signal, socket, threading
p=argparse.ArgumentParser();p.add_argument('-f',action='store_true');p.add_argument('-c');a=p.parse_args()
root=pathlib.Path(a.c).parent.parent
if (root/'fail-start').exists(): raise SystemExit(7)
sockets=[]
for port,kind in ((6112,socket.SOCK_STREAM),(6112,socket.SOCK_DGRAM),(6200,socket.SOCK_STREAM)):
    s=socket.socket(socket.AF_INET,kind);s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
    s.bind(('127.0.0.1',port))
    if kind==socket.SOCK_STREAM:s.listen()
    sockets.append(s)
(root/'var/bnetd.log').write_text('TEST FIXTURE: NOT A REAL PVPGN SERVER\nFake listener ready\n')
print('Test fixture started',flush=True)
e=threading.Event()
for sig in (signal.SIGTERM,signal.SIGINT):signal.signal(sig,lambda *_:e.set())
e.wait()
(root/'var/users/flushed-account').write_text('state flushed on graceful shutdown\n')
for s in sockets:s.close()
'''

def wait_for(fn, seconds=5):
    deadline=time.monotonic()+seconds
    while time.monotonic()<deadline:
        result=fn()
        if result:return result
        time.sleep(.05)
    raise AssertionError('Condition did not become true')


def fixture(base: Path):
    layout=Layout(base/'data',base/'opt',base/'run')
    for p in (layout.prefix/'templates',layout.binary.parent,layout.conf,
              layout.var/'files',layout.var/'users',layout.runtime):p.mkdir(parents=True,exist_ok=True)
    (layout.prefix/'templates/bnetd.base.conf').write_text(
        f'# Test template\nstorage_path = "file:dir={layout.var}/users"\n'
        f'filedir = "{layout.var}/files"\nlogfile = "{layout.var}/bnetd.log"\n'
        'allowed_clients = all\ntrack = 60\nservaddrs = ":"\n'
        'w3routeaddr = "0.0.0.0:6200"\nnew_accounts = true\n')
    write_json(layout.prefix/'templates/versioncheck.upstream.json',VERSIONS)
    layout.binary.write_text(FAKE);layout.binary.chmod(0o755)
    for name in ('IX86ver1.mpq','icons-WAR3.bni'):
        (layout.var/'files'/name).write_text('TEST PLACEHOLDER; NOT A REAL SUPPORT FILE')
    initialize(layout,'127.0.0.1',PASSWORD)
    return layout

class ConfigTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.layout=fixture(Path(self.temp.name))
    def tearDown(self):self.temp.cleanup()
    def test_valid_hosts(self):
        for host in ['192.168.1.10','realm.example.org','localhost','10.10.0.2']:self.assertTrue(valid_host(host))
    def test_reject_bad_hosts(self):
        for host in ['x\ntrack=1','http://foo','x:6112','999.1.1.1','0.0.0.0','evil;sh','a..b','::1']:self.assertFalse(valid_host(host),host)
    def test_reject_injected_server_name(self):
        for name in ['x"\ntrack=1','x;whoami','a#comment',"x'",'x'*49]:
            with self.assertRaises(ValueError):validate_settings({**DEFAULTS,'server_name':name})
    def test_reject_wrong_types(self):
        for key,val in [('new_accounts','false'),('max_users',True),('max_users',0),('advertise_ip',None)]:
            with self.assertRaises(ValueError):validate_settings({**DEFAULTS,key:val})
    def test_reject_unknown_settings(self):
        with self.assertRaises(ValueError):validate_settings({**DEFAULTS,'command':'/bin/sh'})
    def test_password_good_bad(self):
        record=read_json(self.layout.root/'auth.json')
        self.assertTrue(check_password(PASSWORD,record));self.assertFalse(check_password('bad',record))
        self.assertNotIn(PASSWORD,json.dumps(record))
    def test_password_minimum(self):
        with self.assertRaises(ValueError):password_record('short')
    def test_initialization_preserves_password_and_accounts(self):
        auth=(self.layout.root/'auth.json').read_bytes()
        saved=self.layout.var/'users/test';saved.write_text('keep')
        self.assertIsNone(initialize(self.layout,'10.1.1.1','another-long-password'))
        self.assertEqual(auth,(self.layout.root/'auth.json').read_bytes());self.assertEqual(saved.read_text(),'keep')
        self.assertEqual(load_settings(self.layout)['server_address'],'127.0.0.1')
    def test_password_file_permissions(self):
        self.assertEqual((self.layout.root/'auth.json').stat().st_mode & 0o777,0o600)
    def test_exact_version_only(self):
        v=only_121b(VERSIONS)
        self.assertEqual(list(v),['WAR3'])
        self.assertEqual([e['versionTag'] for e in v['WAR3']['IX86']['0x15']['entries']],['WAR3_121B'])
        self.assertEqual(v['WAR3']['IX86']['0x15']['equation'],VERSIONS['WAR3']['IX86']['0x15']['equation'])
    def test_missing_version_fails(self):
        with self.assertRaises(ValueError):only_121b({'WAR3':{'IX86':{}}})
    def test_duplicate_config_keys_removed(self):
        result=set_directives('track = 60\ntrack = 99\n# track = 1\n',{'track':'0'})
        self.assertEqual(result.count('track = 0'),1);self.assertNotIn('track = 99',result)
    def test_config_policy(self):
        text=(self.layout.conf/'bnetd.conf').read_text()
        for expected in ['allowed_clients = war3','allow_bad_version = false','allow_unknown_version = false','track = 0','shutdown_delay = 0']:
            self.assertIn(expected,text)
        self.assertNotIn('skip_versioncheck',text)
    def test_manual_nat_rules_survive(self):
        p=self.layout.conf/'address_translation.conf'
        p.write_text(p.read_text()+'\n192.168.1.5:6112 203.0.113.1:6115 NONE ANY\n')
        render_config(self.layout,load_settings(self.layout));render_config(self.layout,load_settings(self.layout))
        self.assertIn('192.168.1.5:6112',p.read_text());self.assertEqual(p.read_text().count('# W3BOX ROUTE BEGIN'),1)
    def test_gateway_script_is_personalized(self):
        text=gateway_script({**DEFAULTS,'server_address':'192.168.2.30'})
        self.assertNotIn('__SERVER_ADDRESS__',text);self.assertIn('192.168.2.30',text)
        self.assertIn('CurrentUser',text);self.assertIn('Registry32',text);self.assertIn('Restore',text)
    def test_atomic_write(self):
        p=self.layout.root/'atomic';atomic_write(p,'before');atomic_write(p,'after')
        self.assertEqual(p.read_text(),'after');self.assertFalse(list(p.parent.glob('.w3box-*')))

class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.layout=fixture(Path(self.temp.name));self.manager=Manager(self.layout)
    def tearDown(self):self.manager.close();self.temp.cleanup()
    def start(self):self.manager.start();wait_for(lambda:self.manager.status()['ready'])
    def test_start_and_child_owned_ports(self):
        self.start();s=self.manager.status();self.assertTrue(s['ready']);self.assertEqual({p['port'] for p in s['listeners']},{6112,6200})
    def test_stop_persists_and_flushes(self):
        self.start();self.assertTrue(self.manager.stop()['graceful_stop'])
        self.assertFalse(self.manager.status()['process_running']);self.assertFalse(read_json(self.layout.root/'state.json')['desired_running'])
        self.assertTrue((self.layout.var/'users/flushed-account').exists())
    def test_restart_replaces_child(self):
        self.start();pid=self.manager.status()['pid'];self.manager.restart();wait_for(lambda:self.manager.status()['ready'])
        self.assertNotEqual(pid,self.manager.status()['pid'])
    def test_configuration_update_restarts_running_server(self):
        self.start();pid=self.manager.status()['pid']
        result=self.manager.update_settings({**self.manager.settings,'server_name':'Test Realm','max_users':12})
        wait_for(lambda:self.manager.status()['ready']);self.assertEqual(result['max_users'],12)
        self.assertNotEqual(pid,self.manager.status()['pid']);self.assertIn('max_concurrent_logins = 12',(self.layout.conf/'bnetd.conf').read_text())
    def test_stopped_server_stays_stopped_on_config_change(self):
        self.manager.stop();self.manager.update_settings({**self.manager.settings,'max_users':12})
        self.assertFalse(self.manager.status()['process_running'])
    def test_bad_configuration_does_not_stop_running_server(self):
        self.start();pid=self.manager.status()['pid']
        with self.assertRaises(ValueError):self.manager.update_settings({**self.manager.settings,'max_users':-1})
        self.assertEqual(pid,self.manager.status()['pid'])
    def test_backup_flushes_and_restarts(self):
        self.start();result=self.manager.backup();wait_for(lambda:self.manager.status()['ready'])
        self.assertTrue(result['graceful_stop']);p=Path(result['path']);self.assertEqual(p.stat().st_mode & 0o777,0o600)
        with tarfile.open(p) as archive:
            names=archive.getnames();self.assertIn('var/users/flushed-account',names);self.assertIn('auth.json',names)
            self.assertFalse(any(n.startswith('backups') for n in names))
    def test_local_doctor_passes_fixture_checks(self):
        self.start();checks=self.manager.doctor()['checks'];self.assertTrue(all(c['ok'] for c in checks),checks)
    def test_crash_is_restarted(self):
        self.start();self.manager.launch();pid=self.manager.status()['pid'];os.kill(pid,signal.SIGKILL)
        wait_for(lambda:self.manager.status()['ready'] and self.manager.status()['pid']!=pid,seconds=8)
    def test_repeated_failure_stops_restart_loop(self):
        self.manager.failures.extend([time.monotonic()]*5);self.manager.launch()
        wait_for(lambda:not self.manager.desired)
        self.assertIn('five failures',self.manager.last_error)
    def test_arbitrary_command_rejected(self):
        app=Application(self.manager,8787)
        with self.assertRaises(ValueError):app.dispatch('exec',{'command':'id'},local=True)
    def test_password_change_revokes_sessions(self):
        app=Application(self.manager,8787);token,_=app.login(PASSWORD)
        app.dispatch('password',{'password':'new-private-realm-password'},local=True)
        self.assertEqual(app.session('w3box_session='+token),(None,None))
        with self.assertRaises(PermissionError):app.login(PASSWORD)

class HTTPTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.layout=fixture(Path(self.temp.name));self.manager=Manager(self.layout)
        self.app=Application(self.manager,0);self.server=WebServer(('127.0.0.1',0),self.app)
        self.port=self.server.server_address[1];self.app.port=self.port
        self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start()
    def tearDown(self):
        self.server.shutdown();self.server.server_close();self.thread.join();self.manager.close();self.temp.cleanup()
    def call(self,path,method='GET',data=None,headers=None):
        h={'Host':f'127.0.0.1:{self.port}',**(headers or {})};body=None
        if data is not None:body=json.dumps(data);h.setdefault('Content-Type','application/json')
        c=http.client.HTTPConnection('127.0.0.1',self.port,timeout=15)
        c.request(method,path,body=body,headers=h);r=c.getresponse();content=r.read();code=r.status;hs=dict(r.getheaders());c.close()
        return code,hs,content
    def login(self):
        code,h,b=self.call('/api/login','POST',{'password':PASSWORD});self.assertEqual(code,200)
        return {'Cookie':h['Set-Cookie'].split(';',1)[0],'X-CSRF-Token':json.loads(b)['csrf']}
    def test_static_page_and_security_headers(self):
        code,h,b=self.call('/');self.assertEqual(code,200);self.assertIn(b'W3Box',b)
        self.assertEqual(h['X-Frame-Options'],'DENY');self.assertIn("frame-ancestors 'none'",h['Content-Security-Policy'])
    def test_unauthenticated_status_denied(self):self.assertEqual(self.call('/api/status')[0],401)
    def test_dns_rebinding_host_denied(self):self.assertEqual(self.call('/',headers={'Host':'attacker.example'})[0],403)
    def test_cross_origin_login_denied(self):self.assertEqual(self.call('/api/login','POST',{'password':PASSWORD},{'Origin':'https://evil.example'})[0],403)
    def test_successful_login_secure_cookie(self):
        c,h,b=self.call('/api/login','POST',{'password':PASSWORD});self.assertEqual(c,200)
        self.assertIn('HttpOnly',h['Set-Cookie']);self.assertIn('SameSite=Strict',h['Set-Cookie'])
    def test_missing_csrf_denied(self):
        h=self.login();del h['X-CSRF-Token'];self.assertEqual(self.call('/api/start','POST',{},h)[0],403)
    def test_authenticated_status(self):
        code,_,body=self.call('/api/status',headers=self.login());self.assertEqual(code,200);self.assertEqual(json.loads(body)['source_commit'],PIN)
    def test_get_cannot_mutate(self):self.assertEqual(self.call('/api/start',headers=self.login())[0],404)
    def test_invalid_settings_rejected(self):self.assertEqual(self.call('/api/settings','POST',{'cmd':'sh'},self.login())[0],400)
    def test_gateway_download(self):
        code,h,b=self.call('/client/add-gateway.ps1',headers=self.login());self.assertEqual(code,200)
        self.assertIn('attachment',h['Content-Disposition']);self.assertIn(b'127.0.0.1',b)
    def test_logout_revokes_cookie(self):
        h=self.login();self.assertEqual(self.call('/api/logout','POST',{},h)[0],200)
        self.assertEqual(self.call('/api/status',headers=h)[0],401)
    def test_login_throttle(self):
        for _ in range(8):self.assertEqual(self.call('/api/login','POST',{'password':'bad'})[0],403)
        code,_,body=self.call('/api/login','POST',{'password':PASSWORD});self.assertEqual(code,403);self.assertIn(b'Too many',body)
    def test_web_cannot_reset_password(self):self.assertEqual(self.call('/api/password','POST',{'password':'x'*16},self.login())[0],404)

if __name__=='__main__':unittest.main(verbosity=2)
