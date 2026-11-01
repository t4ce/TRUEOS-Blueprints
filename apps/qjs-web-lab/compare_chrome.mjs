#!/usr/bin/env node
// Chrome DevTools reference execution, always with fresh ephemeral browser contexts.
import {createRequire} from 'node:module';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import {createHash} from 'node:crypto';
const require=createRequire(import.meta.url), WebSocket=require('ws');
const root=path.dirname(fileURLToPath(import.meta.url));
const args=process.argv.slice(2), option=(name,def)=>{const i=args.indexOf(name);return i<0?def:args[i+1];};
const mode=option('--mode','captured-offline'), requested=option('--site','all'), instrumented=!args.includes('--no-probe');
if(!['captured-offline','live'].includes(mode))throw Error('mode must be captured-offline or live');
const reportFile=instrumented?(mode==='live'?'chrome-live.json':'chrome-events.json'):(mode==='live'?'chrome-live-control.json':'chrome-control.json');
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
class CDP {
    constructor(ws){this.ws=ws;this.next=0;this.pending=new Map();this.handlers=new Map();ws.on('message',data=>{const message=JSON.parse(data);if(message.id){const p=this.pending.get(message.id);if(p){this.pending.delete(message.id);clearTimeout(p.timer);message.error?p.reject(Error(message.error.message)):p.resolve(message.result);}}else for(const fn of this.handlers.get(message.method)||[])Promise.resolve(fn(message.params)).catch(()=>{});});}
    static async open(url){const ws=new WebSocket(url,{maxPayload:200*1024*1024});await new Promise((ok,bad)=>{ws.once('open',ok);ws.once('error',bad);});return new CDP(ws);}
    on(method,fn){const all=this.handlers.get(method)||[];all.push(fn);this.handlers.set(method,all);}
    send(method,params={}){const id=++this.next;return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{this.pending.delete(id);reject(Error(method+' timed out'));},12000);this.pending.set(id,{resolve,reject,timer});this.ws.send(JSON.stringify({id,method,params}));});}
    close(){this.ws.terminate();for(const p of this.pending.values()){clearTimeout(p.timer);p.reject(Error('CDP closed'));}this.pending.clear();}
}
const port=Number((await readFile(option('--port-file','/tmp/trueos-qjs-chrome/profile/DevToolsActivePort'),'utf8')).split('\n')[0]);
const version=await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
const browser=await CDP.open(version.webSocketDebuggerUrl);
const probeSource=await readFile(path.join(root,'src/chrome_probe.js'),'utf8');
const sites=JSON.parse(await readFile(path.join(root,'sites.json'),'utf8'));
const names=requested==='all'?Object.keys(sites):requested.split(',');
const output=path.join(root,'artifacts/chrome-comparison');await mkdir(output,{recursive:true});
const candidates=[{name:'hidden',type:'visibilitychange',target:'document',state:{visibilityState:'hidden'}},{name:'pageshow',type:'pageshow',target:'window'},{name:'focus',type:'focus',target:'window'},{name:'resize',type:'resize',target:'window'},{name:'scroll',type:'scroll',target:'window'},{name:'visible',type:'visibilitychange',target:'document',state:{visibilityState:'visible'}},{name:'readystatechange',type:'readystatechange',target:'document'}];
const canonical=url=>{try{const u=new URL(url);u.hash='';return u.href;}catch{return url;}};
async function createPage(name){
 const context=(await browser.send('Target.createBrowserContext')).browserContextId;let cdp;
 try{
 const target=(await browser.send('Target.createTarget',{url:'about:blank',browserContextId:context})).targetId;
 let info;for(let n=0;n<30;n++){info=(await(await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(x=>x.id===target);if(info)break;await wait(30);}
 if(!info)throw Error('Chrome target unavailable');
 cdp=await CDP.open(info.webSocketDebuggerUrl);
 const errors=[],consoleMessages=[],browserLogs=[],blocked=[],responses=[],parsedScripts=new Set(),lifecycle=[];let currentStage='navigation';
 cdp.on('Runtime.exceptionThrown',({exceptionDetails:e})=>{if(errors.length<100)errors.push({stage:currentStage,text:e.text,error:e.exception?.description||e.exception?.value||'',url:e.url,line:e.lineNumber});});
 cdp.on('Runtime.consoleAPICalled',e=>{if(consoleMessages.length<100&&['error','warning','assert'].includes(e.type))consoleMessages.push({stage:currentStage,type:e.type,message:e.args.map(a=>String(a.value??a.description??a.type)).join(' ').slice(0,3000)});});
 cdp.on('Log.entryAdded',({entry})=>{if(browserLogs.length<200&&['warning','error'].includes(entry.level))browserLogs.push({stage:currentStage,level:entry.level,source:entry.source,text:entry.text,url:entry.url});});
 cdp.on('Debugger.scriptParsed',e=>{if(e.url)parsedScripts.add(e.url);});
 cdp.on('Page.lifecycleEvent',e=>{if(lifecycle.length<100)lifecycle.push({name:e.name,timestamp:e.timestamp});});
 cdp.on('Network.responseReceived',e=>{if(responses.length<300)responses.push({url:e.response.url,status:e.response.status,type:e.type});});
 await cdp.send('Page.enable');await cdp.send('Runtime.enable');await cdp.send('Log.enable');await cdp.send('Network.enable');await cdp.send('Debugger.enable');await cdp.send('Page.setLifecycleEventsEnabled',{enabled:true});
 if(instrumented)await cdp.send('Page.addScriptToEvaluateOnNewDocument',{source:probeSource});
 const captureBytes=await readFile(path.join(root,'artifacts',name,'capture.json'));
 const capture=JSON.parse(captureBytes.toString('utf8'));const captureHash=createHash('sha256').update(captureBytes).digest('hex');
 const url=capture.final_url||sites[name];
 if(mode==='captured-offline'){
  const html=await readFile(path.join(root,'artifacts',name,'page.html'),'utf8');
  const resources=new Map();
  for(const script of capture.scripts){if(!script.url.includes('#inline-')){resources.set(canonical(script.url),script.source);if(script.final_url)resources.set(canonical(script.final_url),script.source);}}
  for(const [moduleURL,module]of Object.entries(capture.modules||{}))resources.set(canonical(moduleURL),module.source);
  await cdp.send('Fetch.enable',{patterns:[{urlPattern:'*',requestStage:'Request'}]});
  cdp.on('Fetch.requestPaused',async e=>{
   const req=e.request,urlKey=canonical(req.url);let body,type;
   if(e.resourceType==='Document'&&urlKey===canonical(url)){body=html;type='text/html; charset=utf-8';}
   else if(resources.has(urlKey)){body=resources.get(urlKey);type='text/javascript; charset=utf-8';}
   if(body!==undefined)await cdp.send('Fetch.fulfillRequest',{requestId:e.requestId,responseCode:200,responseHeaders:[{name:'Content-Type',value:type},{name:'Access-Control-Allow-Origin',value:'*'}],body:Buffer.from(body).toString('base64')});
   else{if(blocked.length<300)blocked.push({url:req.url,type:e.resourceType});await cdp.send('Fetch.failRequest',{requestId:e.requestId,errorReason:'Aborted'});}
  });
 }
 const nav=await cdp.send('Page.navigate',{url});
 const started=Date.now();let snapshot;
 while(Date.now()-started<9000){await wait(250);try{const state=await evaluate(cdp,'document.readyState');if(state==='complete'||(state==='interactive'&&Date.now()-started>3500))break;}catch{}}
 await wait(700);
 const snapshotNow=async stage=>{
  currentStage=stage;
  await evaluate(cdp,`globalThis.__setProbeStage?.(${JSON.stringify(stage)})`);
  return evaluate(cdp,`globalThis.__chromeProbe?.snapshot()||(()=>{const walker=document.createTreeWalker(document,NodeFilter.SHOW_ALL);let nodeCount=1;while(walker.nextNode())nodeCount++;return {probeMissing:true,url:location.href,readyState:document.readyState,title:document.title,dom:{nodeCount,elementCount:document.querySelectorAll('*').length,scope:'light-DOM-only; uninstrumented control'}};})()`);
 };
 snapshot=await snapshotNow('baseline');
 const pageInfo=await evaluate(cdp,"({url:location.href,title:document.title,readyState:document.readyState})");
 return {cdp,context,snapshotNow,setStage:value=>{currentStage=value;},report:{name,url,mode,instrumented,capture_sha256:captureHash,capture_timestamp:capture.captured_at,capture_issues:capture.issues,source_scope:'full saved HTML including inline handlers + admitted external/module bodies',execution_source_parity:'not-guaranteed; original HTML may execute inline handlers or capped inline scripts not run by QuickJS',chrome_version:version.Browser,navigation:nav,page_info:pageInfo,baseline:snapshot,baseline_errors:[...errors],baseline_console:[...consoleMessages],baseline_browser_logs:[...browserLogs],events:[],controls:[],errors,console:consoleMessages,browser_logs:browserLogs,limitations:['Captured replay uses synthetic content-type/CORS headers and blocks uncaptured resources; live report preserves network behavior.','Sequential synthetic events may include scheduled work; idle control is observational, not a causal proof.'],blocked_requests:blocked,responses,parsed_scripts:[],lifecycle},finish:async()=>{const r={parsed_scripts:[...parsedScripts]};cdp.close();await browser.send('Target.disposeBrowserContext',{browserContextId:context});return r;}};
 }catch(error){cdp?.close();await browser.send('Target.disposeBrowserContext',{browserContextId:context}).catch(()=>{});throw error;}
}
async function evaluate(cdp,expression){const r=await cdp.send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:false});if(r.exceptionDetails)throw Error(r.exceptionDetails.exception?.description||r.exceptionDetails.text);return r.result.value;}
function dispatchSource(event){return `(()=>{const spec=${JSON.stringify(event)};if(spec.state?.visibilityState){Object.defineProperty(document,'visibilityState',{configurable:true,get:()=>spec.state.visibilityState});Object.defineProperty(document,'hidden',{configurable:true,get:()=>spec.state.visibilityState==='hidden'});}const target=spec.target==='document'?document:spec.target==='window'?window:document.querySelector(spec.target);if(!target)return {missingTarget:true,synthetic:true};const value=spec.type==='pageshow'?new PageTransitionEvent(spec.type,{persisted:false}):new Event(spec.type);target.dispatchEvent(value);return {synthetic:true,trusted:value.isTrusted};})()`;}
const summaries=[];
try{
 for(const name of names){
  const directory=path.join(output,name);await mkdir(directory,{recursive:true});
  let page;
  try{
   page=await createPage(name);
   const controlBefore=await page.snapshotNow('idle-control-before');await wait(300);const controlAfter=await page.snapshotNow('idle-control-after');page.report.controls.push({name:'idle-300ms',before:controlBefore,after:controlAfter});
   const extraEvents=[];const seen=new Set(candidates.map(e=>e.type+'@'+e.target));
   for(const registration of page.report.baseline.lifecycle?.registrations||[]){const t=registration.target;const target=t?.kind==='window'?'window':t?.kind==='document'?'document':t?.selector;const key=registration.type+'@'+target;if(target&&!seen.has(key)){seen.add(key);extraEvents.push({name:'observed-'+registration.type+'-'+target,type:registration.type,target});}}
   for(const event of (instrumented?[...candidates,...extraEvents].slice(0,24):[])){const before=await page.snapshotNow('before:'+event.name);page.setStage('event:'+event.name);const errorOffset=page.report.errors.length;let dispatch,error;try{dispatch=await evaluate(page.cdp,dispatchSource(event));}catch(e){error=String(e);}await wait(300);const snapshot=await page.snapshotNow('event:'+event.name);page.report.events.push({name:event.name,event,before,snapshot,dispatch,error,errors:page.report.errors.slice(errorOffset)});}
   const report=page.report;Object.assign(report,await page.finish());page=undefined;
   await writeFile(path.join(directory,reportFile),JSON.stringify(report,null,2));summaries.push(report);
   console.log(name,mode,'baseline-nodes',report.baseline.dom?.nodeCount??report.baseline.nodeCount??'unknown','errors',report.errors.length);
  }catch(error){
   if(page){await page.finish().catch(()=>{});}
   const report={name,mode,chrome_version:version.Browser,error:String(error),unavailable:true};await writeFile(path.join(directory,reportFile),JSON.stringify(report,null,2));summaries.push(report);console.log(name,mode,'failed',String(error));continue;
  }
 }
 await writeFile(path.join(output,instrumented?(mode==='live'?'chrome-live-summary.json':'chrome-summary.json'):(mode==='live'?'chrome-live-control-summary.json':'chrome-control-summary.json')),JSON.stringify(summaries,null,2));
}finally{browser.close();}
