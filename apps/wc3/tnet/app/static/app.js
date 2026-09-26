'use strict';
const $ = (q) => document.querySelector(q);
let csrf = '', tab = 'overview', active = false, busy = false;
async function api(path, data) {
  const options = {credentials:'same-origin', cache:'no-store'};
  if (data !== undefined) {
    options.method = 'POST'; options.headers = {'Content-Type':'application/json','X-CSRF-Token':csrf};
    options.body = JSON.stringify(data);
  }
  const response = await fetch(path, options);
  const result = await response.json();
  if (!response.ok) {
    if (response.status === 401) showLogin();
    throw new Error(result.error || `HTTP ${response.status}`);
  }
  return result;
}
function showLogin() {active=false; csrf=''; $('#dashboard').hidden=true; $('#logout').hidden=true; $('#login').hidden=false; $('#password').focus();}
function message(text, error=false) {const n=$('#notice'); n.textContent=text; n.classList.toggle('error', error); n.hidden=false;}
function duration(total) {const d=Math.floor(total/86400), h=Math.floor(total%86400/3600), m=Math.floor(total%3600/60); return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : m ? `${m}m ${total%60}s` : `${total}s`;}
async function status() {
  if (!active) return;
  const s=await api('/api/status');
  $('#realm-name').textContent=s.server_name; $('#realm-address').textContent=s.server_address + ':6112';
  $('#state').textContent=s.ready?'Online':s.process_running?'Starting':'Stopped';
  $('#state').className='pill '+(s.ready?'good':s.process_running?'':'bad');
  $('#login-state').textContent=s.ready?'Listening':s.process_running?'Starting':'Offline';
  $('#uptime').textContent=duration(s.uptime_seconds); $('#accounts').textContent=s.account_files;
  $('#policy').textContent=s.strict_version?'Strict':'Relaxed hash'; $('#commit').textContent=s.source_commit.slice(0,12);
  $('#start').disabled=busy||s.process_running; $('#stop').disabled=busy||!s.process_running; $('#restart').disabled=busy; $('#backup').disabled=busy;
  $('#server-error').textContent=s.last_error; $('#server-error').hidden=!s.last_error;
  $('#ssh-command').textContent=`ssh -N -L 8787:127.0.0.1:8787 YOUR_USER@${s.server_address}`;
}
async function checks() {
  const result=await api('/api/doctor'); const box=$('#checks'); box.replaceChildren();
  for (const c of result.checks) {
    const row=document.createElement('div'); row.className='check';
    const icon=document.createElement('span'); icon.className='check-icon'+(c.ok?'':' fail'); icon.textContent=c.ok?'✓':'!';
    const body=document.createElement('div'), title=document.createElement('strong'), detail=document.createElement('p');
    title.textContent=c.name; detail.textContent=c.detail; body.append(title,detail); row.append(icon,body); box.append(row);
  }
}
async function logs() {const s=await api('/api/logs'); $('#server-log').textContent=s.server; $('#startup-log').textContent=s.startup;}
async function settings() {const s=await api('/api/settings'); const f=$('#settings-form'); for(const [k,v] of Object.entries(s)){if(typeof v==='boolean')f.elements[k].checked=v;else f.elements[k].value=v;}}
async function enter() {active=true; $('#login').hidden=true; $('#dashboard').hidden=false; $('#logout').hidden=false; await status(); await checks(); await settings();}
async function action(name) {
  if (busy) return;
  if(['stop','restart','backup'].includes(name)&&!confirm(name==='backup'?'Create a private backup? The realm briefly stops and existing sessions disconnect.':`${name==='stop'?'Stop':'Restart'} the realm? Existing realm sessions will disconnect.`)) return;
  busy=true; document.querySelectorAll('.actions button').forEach(b=>b.disabled=true);
  try {const r=await api('/api/'+name,{}); message(name==='backup'?`Backup saved: ${r.filename}${r.graceful_stop?'':' — Warning: the server required a forced stop.'}`:`${name[0].toUpperCase()+name.slice(1)} requested.`); await status(); await checks();}
  catch(e){message(e.message,true);}finally{busy=false; await status().catch(e=>message(e.message,true));}
}
$('#login-form').addEventListener('submit',async e=>{e.preventDefault(); const b=e.target.querySelector('button'); b.disabled=true; $('#login-error').textContent=''; try{const r=await api('/api/login',{password:$('#password').value}); csrf=r.csrf; $('#password').value=''; await enter();}catch(err){$('#login-error').textContent=err.message;}finally{b.disabled=false;}});
$('#logout').addEventListener('click',async()=>{try{await api('/api/logout',{});}finally{showLogin();}});
for(const n of ['start','stop','restart','backup'])$('#'+n).addEventListener('click',()=>action(n));
$('#check').addEventListener('click',()=>checks().catch(e=>message(e.message,true)));
$('#refresh-logs').addEventListener('click',()=>logs().catch(e=>message(e.message,true)));
document.querySelectorAll('[data-tab]').forEach(b=>b.addEventListener('click',async()=>{tab=b.dataset.tab; document.querySelectorAll('[data-tab]').forEach(x=>x.classList.toggle('selected',x===b)); document.querySelectorAll('.tab-panel').forEach(x=>x.hidden=x.id!=='tab-'+tab); try{if(tab==='logs')await logs();}catch(e){message(e.message,true);}}));
$('#settings-form').addEventListener('submit',async e=>{
  e.preventDefault(); if(busy||!confirm('Save settings? A running realm will restart, disconnecting its sessions.'))return;
  const f=e.target, data={}; for(const k of ['server_name','server_address','bind_address','advertise_ip'])data[k]=f.elements[k].value.trim();
  data.max_users=Number(f.elements.max_users.value); for(const k of ['new_accounts','strict_version'])data[k]=f.elements[k].checked;
  busy=true; const b=f.querySelector('button[type=submit]'); b.disabled=true;
  try{await api('/api/settings',data); message('Configuration saved.'); await status(); await checks();}catch(e){message(e.message,true);}finally{busy=false;b.disabled=false;await status().catch(e=>message(e.message,true));}
});
(async()=>{try{const s=await api('/api/session');csrf=s.csrf;await enter();}catch{showLogin();}})();
setInterval(async()=>{if(!active||busy)return;try{await status();if(tab==='logs')await logs();}catch(e){message(e.message,true);}},5000);
