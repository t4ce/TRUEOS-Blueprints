(() => {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const S = { schema:null, groups:[], group:0, fs:0, form:null, selected:null, drawer:false };
  const E = Object.fromEntries([
    "app","capture-state","top-tabs","search","search-results","group-heading","form-nav",
    "breadcrumbs","form-title","form-help","page-flags","form-content","help-title","help-text",
    "help-meta","footer-state","details-button","details-drawer","drawer-backdrop","drawer-close",
    "drawer-title","drawer-body","toast"
  ].map(id => [id.replaceAll("-","_"), $(id)]));

  const qops = new Set(["one-of","checkbox","numeric","string","password","action"]);
  const esc = (v) => String(v ?? "").replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;").replaceAll('"',"&quot;").replaceAll("'","&#039;");
  const pick = (o,a,b) => o ? (o[a] ?? o[b]) : undefined;
  const h16 = (v) => Number.isFinite(Number(v)) ? `0x${Number(v).toString(16).toUpperCase().padStart(4,"0")}` : "—";
  const hoff = (v) => Number.isFinite(Number(v)) ? `0x${Number(v).toString(16).toUpperCase()}` : "—";
  const formsets = () => Array.isArray(S.schema?.formsets) ? S.schema.formsets : [];
  const fsIndex = (fs,i=0) => Number.isFinite(Number(pick(fs,"index","formset_index"))) ? Number(pick(fs,"index","formset_index")) : i;
  const forms = (fs) => Array.isArray(fs?.forms) ? fs.forms : [];
  const formId = (f) => Number(pick(f,"formId","form_id"));
  const questions = (f) => Array.isArray(f?.questions) ? f.questions : [];
  const nodes = () => Array.isArray(S.schema?.presentation?.nodes) ? S.schema.presentation.nodes : [];
  const currentRecords = () => Array.isArray(S.schema?.current?.questions) ? S.schema.current.questions : [];
  const formNodes = (fi,id) => nodes().filter(n =>
    Number(pick(n,"formsetIndex","formset_index"))===Number(fi) &&
    Number(pick(n,"formId","form_id"))===Number(id)
  ).sort((a,b)=>Number(pick(a,"sourceOffset","source_offset")||0)-Number(pick(b,"sourceOffset","source_offset")||0));

  function mac(text) {
    const m=String(text??"").match(/MAC\s*[:=]?\s*([0-9A-Fa-f:-]{12,20})/);
    if(!m) return null;
    const x=m[1].replace(/[^0-9A-Fa-f]/g,"").slice(0,12).toUpperCase();
    return x.length===12 ? x.match(/.{2}/g).join(":") : null;
  }

  function section(fs) {
    const t=String(fs?.title??"");
    if(/IPv4/i.test(t)) return "IPv4";
    if(/IPv6/i.test(t)) return "IPv6";
    if(/family controller/i.test(t)) return "Controller";
    if(/tls/i.test(t)) return "TLS";
    if(/acoustic/i.test(t)) return "Acoustic";
    return t||"Firmware";
  }

  function pageTitle(fs,f) {
    const t=String(f?.title??"").trim();
    return (!t||/^Form 0x/i.test(t)) ? `${section(fs)} Configuration` : t;
  }

  function buildGroups() {
    const out=[], byMac=new Map(), used=new Set();
    formsets().forEach((fs,i)=>{
      const t=String(fs.title??"");
      if(!/family controller/i.test(t)) return;
      const m=mac(t);
      if(!m) return;
      const ix=fsIndex(fs,i);
      const g={key:`nic:${m}`,title:/2\.5/i.test(t)?"2.5GbE":"GbE",sub:m,icon:"↔",sets:[fs]};
      out.push(g); byMac.set(m,g); used.add(ix);
    });
    formsets().forEach((fs,i)=>{
      const ix=fsIndex(fs,i);
      if(used.has(ix)) return;
      const t=String(fs.title??""), m=mac(t);
      if(m&&byMac.has(m)){byMac.get(m).sets.push(fs);used.add(ix);return;}
      let g;
      if(/acoustic/i.test(t)) g={key:`fs:${ix}`,title:"Acoustic",sub:"Drive policy",icon:"◉",sets:[fs]};
      else if(/tls|auth/i.test(t)) g={key:`fs:${ix}`,title:"TLS",sub:"Authentication",icon:"⬡",sets:[fs]};
      else g={key:`fs:${ix}`,title:t.replace(/configuration/ig,"").trim()||`Form set ${ix}`,sub:"Firmware",icon:"◆",sets:[fs]};
      out.push(g); used.add(ix);
    });
    return out;
  }

  const group = () => S.groups[S.group]??null;
  const activeFs = () => formsets().find((f,i)=>fsIndex(f,i)===Number(S.fs))??null;
  const activeForm = () => forms(activeFs()).find(f=>formId(f)===Number(S.form))??null;
  const contains = (g,ix) => g?.sets?.some((fs,i)=>fsIndex(fs,i)===Number(ix));

  function init() {
    S.groups=buildGroups();
    if(!S.groups.length) return;
    const fs=S.groups[0].sets[0];
    S.group=0;
    S.fs=fsIndex(fs,0);
    S.form=forms(fs)[0]?formId(forms(fs)[0]):null;
  }

  function chooseGroup(i) {
    S.group=i;
    const g=group();
    if(!contains(g,S.fs)){
      const fs=g.sets[0];
      S.fs=fsIndex(fs,0);
      S.form=forms(fs)[0]?formId(forms(fs)[0]):null;
    }
    S.selected=null;
    render();
  }

  function chooseForm(fi,id) {
    S.fs=Number(fi);
    S.form=Number(id);
    const gi=S.groups.findIndex(g=>contains(g,fi));
    if(gi>=0) S.group=gi;
    S.selected=null;
    render();
  }

  function toast(msg){
    E.toast.textContent=msg;
    E.toast.hidden=false;
    clearTimeout(toast.t);
    toast.t=setTimeout(()=>E.toast.hidden=true,2400);
  }

  function renderTabs(){
    E.top_tabs.innerHTML="";
    S.groups.forEach((g,i)=>{
      const b=document.createElement("button");
      b.type="button";
      b.className=`top-tab${i===S.group?" active":""}`;
      b.innerHTML=`<span class="tab-icon">${esc(g.icon)}</span><span><span class="tab-label">${esc(g.title)}</span><span class="tab-sub">${esc(g.sub)}</span></span>`;
      b.onclick=()=>chooseGroup(i);
      E.top_tabs.appendChild(b);
    });
  }

  function renderNav(){
    const g=group();
    E.form_nav.innerHTML="";
    E.group_heading.textContent=g?`${g.title} pages`:"Firmware pages";
    if(!g)return;
    g.sets.forEach((fs,i)=>{
      const fi=fsIndex(fs,i),sec=document.createElement("section");
      sec.className="form-section";
      sec.innerHTML=`<div class="form-section-title">${esc(section(fs))}</div>`;
      const counts={};
      forms(fs).forEach(f=>counts[pageTitle(fs,f)]=(counts[pageTitle(fs,f)]||0)+1);
      forms(fs).forEach(f=>{
        const id=formId(f),label=pageTitle(fs,f),b=document.createElement("button");
        b.type="button";
        b.className=`form-button${fi===S.fs&&id===S.form?" active":""}`;
        b.innerHTML=`${esc(label)}${counts[label]>1?` <span class="form-id">${h16(id)}</span>`:""}`;
        b.onclick=()=>chooseForm(fi,id);
        sec.appendChild(b);
      });
      E.form_nav.appendChild(sec);
    });
  }

  function canonical(form,node){
    const d=node?.details||{},qid=Number(pick(d,"questionId","question_id")),off=Number(pick(node,"sourceOffset","source_offset"));
    return questions(form).find(q=>Number(pick(q,"sourceOffset","source_offset"))===off&&Number(pick(q,"questionId","question_id"))===qid)||
      questions(form).find(q=>Number(pick(q,"questionId","question_id"))===qid)||null;
  }

  const qkey = q => q?.recordKey??`${S.fs}:${S.form}:${pick(q,"questionId","question_id")}:${pick(q,"sourceOffset","source_offset")}`;
  const currentFor = q => {
    const key=qkey(q);
    return currentRecords().find(record=>record.recordKey===key)??null;
  };
  const visibilityFor = q => currentFor(q)?.visibility??"visible";

  function pseudo(n){
    const d=n?.details||{};
    return {
      prompt:d.prompt,help:d.help,questionId:pick(d,"questionId","question_id"),
      sourceOffset:pick(n,"sourceOffset","source_offset"),kind:pick(n,"opcodeName","opcode_name"),
      options:[],defaults:[],storage:{backend:"presentation-only",validated:false},
      policy:{trueosWrite:"locked"},currentValue:"presentation-only"
    };
  }

  function value(q){
    const k=String(q.kind??"unknown"),o=Array.isArray(q.options)?q.options:[],num=q.numericRange??q.numeric,lim=q.stringLimits??q.string_limits,c=currentFor(q);
    if(k==="action") return ["Unavailable","Firmware callback/action is not invoked"];
    if(k==="password") return ["Not exposed","Firmware secret is never rendered"];
    if(c?.status==="decoded"){
      const shown=c.optionLabel??c.display??(c.boolean===true?"Enabled":c.boolean===false?"Disabled":c.unsigned??"Decoded");
      if(k==="one-of") return [shown,`Captured preboot · ${o.length} allowed option${o.length===1?"":"s"}`];
      if(k==="checkbox") return [shown,"Captured preboot boolean"];
      if(k==="numeric") return [shown,num?`Captured preboot · range ${num.minimum??"?"}–${num.maximum??"?"}${num.step?` · step ${num.step}`:""}`:"Captured preboot numeric value"];
      if(k==="string") return [shown,lim?.maximumChars?`Captured preboot · maximum ${lim.maximumChars} characters`:"Captured preboot string"];
      return [shown,"Captured preboot current value"];
    }
    if(k==="one-of") return ["Not exposed",`${o.length} allowed option${o.length===1?"":"s"}`];
    if(k==="checkbox") return ["Not exposed","Boolean firmware value"];
    if(k==="numeric") return ["Not exposed",num?`Range ${num.minimum??"?"}–${num.maximum??"?"}${num.step?` · step ${num.step}`:""}`:"Numeric firmware value"];
    if(k==="string") return ["Not exposed",lim?.maximumChars?`Maximum ${lim.maximumChars} characters`:"String firmware value"];
    return ["Read only",k];
  }

  function select(item){
    S.selected=item;
    renderHelp();
    renderDrawer();
    document.querySelectorAll(".setting-row.selected").forEach(x=>x.classList.remove("selected"));
    if(item?.key){
      const row=[...document.querySelectorAll("[data-key]")].find(x=>x.dataset.key===item.key);
      row?.classList.add("selected");
    }
  }

  function questionRow(q){
    const row=document.createElement("article"),key=qkey(q),[v,s0]=value(q),kind=String(q.kind??"unknown"),action=kind==="action",visibility=visibilityFor(q);
    const state=visibility!=="visible"&&visibility!=="unknown"?visibility:null;
    const s=state?`${s0} · ${state}`:s0;
    row.className=`setting-row selectable${S.selected?.key===key?" selected":""}${state?` ${state}`:""}${visibility==="unknown"?" visibility-unknown":""}`;
    row.dataset.key=key;
    row.dataset.visibility=visibility;
    row.tabIndex=0;
    const stateBadge=state?` <span class="badge state">${esc(state)}</span>`:visibility==="unknown"?' <span class="badge state">condition?</span>':"";
    row.innerHTML=`<div class="setting-main"><div class="setting-title">${esc(q.prompt||`Unnamed ${kind}`)} <span class="badge${action?" warn":""}">${esc(kind)}</span>${stateBadge}</div>${q.help?`<div class="setting-desc">${esc(q.help)}</div>`:""}</div><div class="setting-value"><span class="value-primary ${action?"action-state":currentFor(q)?.status==="decoded"?"current":"redacted"}">${esc(v)}</span><span class="value-secondary">${esc(s)}</span></div>`;
    const item={type:"question",key,question:q};
    row.onclick=()=>select(item);
    row.onkeydown=e=>{if(e.key==="Enter"||e.key===" "){e.preventDefault();select(item);}};
    return row;
  }

  function textRow(n){
    const d=n.details||{},row=document.createElement("div");
    row.className="text-row";
    row.innerHTML=`${d.prompt?`<strong>${esc(d.prompt)}</strong>`:""}${(d.text_two??d.textTwo)?`<span>${esc(d.text_two??d.textTwo)}</span>`:""}${d.help&&d.help!==(d.text_two??d.textTwo)?`<span>${esc(d.help)}</span>`:""}`;
    return row;
  }

  function refRow(n,fs){
    const d=n.details||{},target=Number(pick(d,"targetFormId","target_form_id")),guid=pick(d,"targetFormsetGuid","target_formset_guid"),b=document.createElement("button");
    b.type="button";
    b.className="ref-row";
    b.innerHTML=`<span><strong>${esc(d.prompt||"Open firmware page")}</strong><span>${esc(d.help||`Navigate to ${h16(target)}`)}</span></span><span class="ref-arrow">→</span>`;
    b.onclick=()=>{
      let tfs=fs;
      if(guid)tfs=formsets().find(x=>x.guid===guid)||fs;
      const fi=fsIndex(tfs,S.fs);
      if(forms(tfs).some(f=>formId(f)===target))chooseForm(fi,target);
      else toast(`Reference target ${h16(target)} is not present in this capture.`);
    };
    return b;
  }

  function empty(title,detail){
    const d=document.createElement("div");
    d.className="empty-form";
    d.innerHTML=`<strong>${esc(title)}</strong>${esc(detail)}`;
    return d;
  }

  function appendQuestion(q){
    if(visibilityFor(q)==="suppressed") return false;
    E.form_content.appendChild(questionRow(q));
    return true;
  }

  function renderPage(){
    const fs=activeFs(),form=activeForm();
    E.form_content.innerHTML="";
    if(!fs||!form){
      E.form_title.textContent="No captured firmware page";
      E.form_help.textContent="";
      E.breadcrumbs.textContent="";
      E.form_content.appendChild(empty("No form available","The captured HII does not expose a form here."));
      return;
    }
    const g=group(),ns=formNodes(S.fs,S.form),qs=questions(form);
    const decoded=qs.filter(q=>currentFor(q)?.status==="decoded").length;
    const suppressed=qs.filter(q=>visibilityFor(q)==="suppressed").length;
    E.breadcrumbs.textContent=`${g?.title||"Firmware"} / ${section(fs)} / ${h16(S.form)}`;
    E.form_title.textContent=pageTitle(fs,form);
    E.form_help.textContent=form.help||fs.help||"";
    E.page_flags.innerHTML=`<span class="page-flag${ns.length?" safe":""}">${ns.length?`${ns.length} IFR nodes`:"schema v1"}</span><span class="page-flag">${qs.length} questions</span>${decoded?`<span class="page-flag safe">${decoded} current</span>`:""}${suppressed?`<span class="page-flag">${suppressed} suppressed</span>`:""}`;
    let visible=0;
    if(ns.length){
      ns.forEach(n=>{
        const name=String(pick(n,"opcodeName","opcode_name")||"");
        if(name==="subtitle"&&n.details?.prompt){
          const h=document.createElement("h2");h.className="section-title";h.textContent=n.details.prompt;E.form_content.appendChild(h);visible++;
        }else if(name==="text"){
          E.form_content.appendChild(textRow(n));visible++;
        }else if(name==="ref"){
          E.form_content.appendChild(refRow(n,fs));visible++;
        }else if(qops.has(name)){
          const q=canonical(form,n)||pseudo(n);
          if(appendQuestion(q)) visible++;
        }
      });
    }else{
      qs.forEach(q=>{if(appendQuestion(q))visible++;});
    }
    if(!visible)E.form_content.appendChild(empty(ns.length?"No visible statements":"Presentation stream unavailable",ns.length?"Firmware conditions suppress all captured statements on this page, or the form contains only structural metadata.":"The semantic snapshot exposes no visible questions here."));
    renderHelp();
  }

  function metaRows(rows){
    E.help_meta.innerHTML="";
    rows.forEach(([a,b])=>{
      const d=document.createElement("div");
      d.className="help-meta-row";
      d.innerHTML=`<span>${esc(a)}</span><span>${esc(b)}</span>`;
      E.help_meta.appendChild(d);
    });
  }

  function renderHelp(){
    const fs=activeFs(),form=activeForm(),sel=S.selected;
    if(sel?.type==="question"){
      const q=sel.question,c=currentFor(q),shown=c?.status==="decoded"?(c.optionLabel??c.display??"decoded"):(c?.detail??"not exposed");
      E.help_title.textContent=q.prompt||"Firmware question";
      E.help_text.textContent=q.help||"No help text supplied by firmware.";
      metaRows([["Question",h16(pick(q,"questionId","question_id"))],["Kind",q.kind||"unknown"],["Current",shown],["Visibility",c?.visibility??"not evaluated"]]);
    }else{
      E.help_title.textContent=pageTitle(fs,form)||"Firmware explorer";
      E.help_text.textContent=form?.help||fs?.help||"Select a firmware item to see help supplied by the firmware.";
      metaRows([["Form",form?h16(S.form):"—"],["Presentation",formNodes(S.fs,S.form).length?"source ordered":"schema v1"],["Current values",S.schema?.current?.state==="ready"?"captured preboot":"unavailable"]]);
    }
  }

  function renderStatus(){
    const st=S.schema?.stats||{},ps=S.schema?.presentation?.stats||{},cs=S.schema?.current||{};
    E.capture_state.textContent=(S.schema?.state||"unknown").toUpperCase();
    const current=cs.state==="ready"?`${cs.questionsDecoded??0} current`:`current ${cs.state||"unavailable"}`;
    E.footer_state.textContent=`${st.formsets??formsets().length} formsets · ${st.forms??"?"} forms · ${st.questions??"?"} questions · ${ps.nodes??nodes().length} presentation nodes · ${current} · ${ps.semanticallyUnresolvedOpcodes??"?"} unresolved`;
  }

  function drows(rows){
    return `<dl class="detail-grid">${rows.map(([a,b,c])=>`<dt>${esc(a)}</dt><dd${c?' class="code"':""}>${esc(b)}</dd>`).join("")}</dl>`;
  }

  function renderDrawer(){
    if(!S.drawer)return;
    E.drawer_body.innerHTML="";
    const fs=activeFs(),form=activeForm(),sel=S.selected,s=S.schema||{},cs=s.current||{};
    const sum=document.createElement("section");
    sum.className="detail-section";
    sum.innerHTML=`<h3>Page</h3>${drows([["API",s.api||"—",true],["Formset",fs?.guid||"—",true],["Form",form?h16(S.form):"—",true],["Write path",s.activeWritePath||"none",true]])}`;
    E.drawer_body.appendChild(sum);
    if(sel?.type==="question"){
      const q=sel.question,st=q.storage||{},p=q.policy||{},c=currentFor(q),shown=c?.status==="decoded"?(c.optionLabel??c.display??"decoded"):(c?.detail??"unavailable");
      E.drawer_title.textContent=q.prompt||"Firmware question";
      const a=document.createElement("section");
      a.className="detail-section";
      a.innerHTML=`<h3>Question</h3>${drows([["ID",h16(pick(q,"questionId","question_id")),true],["Kind",q.kind||"unknown"],["Source",hoff(pick(q,"sourceOffset","source_offset")),true],["Current",shown],["Current source",c?.status==="decoded"?"preboot ExportConfig":"—"],["Visibility",c?.visibility??"not evaluated"],["Callback",p.callback?"yes, not invoked":"no"],["Reset",(p.requiresReset??p.requires_reset)?"required":"no"]])}`;
      E.drawer_body.appendChild(a);
      const b=document.createElement("section");
      b.className="detail-section";
      b.innerHTML=`<h3>Storage</h3>${drows([["Backend",st.backend||"none"],["Variable",st.variable||"—",true],["Varstore",h16(st.varstoreId??st.varstore_id),true],["Offset",st.offset??"—",true],["Width",st.width??"—"],["Validated",st.validated===true?"yes":st.validated===false?"no":"—"]])}`;
      E.drawer_body.appendChild(b);
      if(Array.isArray(c?.conditions)&&c.conditions.length){
        const d=document.createElement("section");
        d.className="detail-section";
        d.innerHTML=`<h3>Condition evaluation</h3>${drows(c.conditions.map(x=>[x.kind,x.result]))}`;
        E.drawer_body.appendChild(d);
      }
    }else{
      E.drawer_title.textContent="Firmware metadata";
      const p=s.presentation||{},ps=p.stats||{},a=document.createElement("section");
      a.className="detail-section";
      a.innerHTML=`<h3>Capture</h3>${drows([["Presentation",p.api||"not available",true],["Ordered",p.ordered?"yes":"no"],["Captured HII",p.completeForCapturedHii?"complete":"not claimed"],["Motherboard setup",p.completeMotherboardSetupSurface||"not claimed"],["Presentation nodes",ps.nodes??nodes().length],["Unresolved",ps.semanticallyUnresolvedOpcodes??"—"],["Current source",cs.source||"not available",true],["Current timing",cs.captureTiming||"—"],["Current decoded",cs.questionsDecoded??"—"],["Conditions",cs.conditionsEvaluated??"—"],["Raw config",cs.rawConfig||"hidden"]])}`;
      E.drawer_body.appendChild(a);
    }
  }

  function drawer(open){
    S.drawer=open;
    E.details_drawer.classList.toggle("open",open);
    E.details_drawer.setAttribute("aria-hidden",open?"false":"true");
    E.details_button.setAttribute("aria-expanded",open?"true":"false");
    E.drawer_backdrop.hidden=!open;
    if(open)renderDrawer();
  }

  function entries(){
    const out=[];
    S.groups.forEach((g,gi)=>g.sets.forEach((fs,i)=>{
      const fi=fsIndex(fs,i);
      forms(fs).forEach(f=>{
        const id=formId(f),ctx=`${g.title} / ${section(fs)} / ${pageTitle(fs,f)}`;
        out.push({gi,fi,id,label:pageTitle(fs,f),ctx,type:"page"});
        questions(f).forEach(q=>{
          if(visibilityFor(q)==="suppressed") return;
          const label=q.prompt||q.help;
          if(label)out.push({gi,fi,id,label,ctx,type:q.kind||"question",q});
        });
        formNodes(fi,id).forEach(n=>{
          const name=String(pick(n,"opcodeName","opcode_name")||"");
          if(!["subtitle","text","ref"].includes(name))return;
          const d=n.details||{},label=d.prompt||d.text_two||d.textTwo||d.help;
          if(label)out.push({gi,fi,id,label,ctx,type:name});
        });
      });
    }));
    return out;
  }

  function search(){
    const q=E.search.value.trim().toLowerCase();
    if(!q){E.search_results.hidden=true;E.search_results.innerHTML="";return;}
    const rs=entries().filter(x=>`${x.label} ${x.ctx} ${x.type}`.toLowerCase().includes(q)).slice(0,32);
    E.search_results.innerHTML="";
    E.search_results.hidden=false;
    if(!rs.length){E.search_results.innerHTML='<div class="search-none">question_match=none</div>';return;}
    rs.forEach(x=>{
      const b=document.createElement("button");
      b.type="button";
      b.className="search-result";
      b.innerHTML=`<strong>${esc(x.label)}</strong><span>${esc(x.type)} · ${esc(x.ctx)}</span>`;
      b.onclick=()=>{
        E.search.value="";E.search_results.hidden=true;S.group=x.gi;S.fs=x.fi;S.form=x.id;
        S.selected=x.q?{type:"question",key:qkey(x.q),question:x.q}:null;
        render();
        if(S.selected)select(S.selected);
      };
      E.search_results.appendChild(b);
    });
  }

  function render(){renderTabs();renderNav();renderPage();renderStatus();renderDrawer();}

  async function load(){
    try{
      const r=await fetch("/api/bios/schema",{method:"GET",headers:{Accept:"application/json"},cache:"no-store"});
      if(!r.ok)throw new Error(`HTTP ${r.status}`);
      const s=await r.json();
      if(s.readOnly!==true||s.activeWritePath!=="none")throw new Error("kernel BIOS snapshot did not advertise the locked read-only boundary");
      S.schema=s;
      init();
      E.app.setAttribute("aria-busy","false");
      render();
    }catch(e){
      E.capture_state.textContent="UNAVAILABLE";
      E.form_title.textContent="BIOS schema unavailable";
      E.form_help.textContent=String(e);
      E.footer_state.textContent="Axum is running, but the kernel BIOS snapshot could not be read.";
      E.form_content.innerHTML="";
      E.form_content.appendChild(empty("No firmware data","The UI remains read-only. Retry when the TRUEOS BIOS snapshot ABI is available."));
      E.app.setAttribute("aria-busy","false");
    }
  }

  E.search.addEventListener("input",search);
  E.search.addEventListener("keydown",e=>{if(e.key==="Escape"){E.search.value="";search();}});
  document.addEventListener("click",e=>{if(!E.search_results.hidden&&!e.target.closest(".firmware-search")&&!e.target.closest(".search-results"))E.search_results.hidden=true;});
  E.details_button.onclick=()=>drawer(!S.drawer);
  E.drawer_close.onclick=()=>drawer(false);
  E.drawer_backdrop.onclick=()=>drawer(false);
  window.addEventListener("keydown",e=>{
    if(e.key==="/"&&!/input|textarea/i.test(document.activeElement?.tagName||"")){e.preventDefault();E.search.focus();return;}
    if(e.key==="Escape"&&S.drawer){drawer(false);return;}
    const save=e.key==="F10"||((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==="s");
    if(save){e.preventDefault();e.stopPropagation();toast("Save is unavailable. TRUEOS exposes no BIOS write path.");}
  }, {capture:true});

  load();
})();