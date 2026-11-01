// Deliberate headless compatibility policy: normalized microfont metrics (height 1, advance 6/11), no rasterization.
(() => {
    const notes = [], draws = [], note = (kind, detail) => { if (notes.length < 500) notes.push({kind,detail}); };
    const metric = (text, em = MicrofontMetrics.referenceEm) => {
        const m = MicrofontMetrics.measure(text, em);
        // Existing browser-facing APIs require numbers; unwrap only here.
        return {width:m.width.toNumber(),height:m.height.toNumber(),ascent:m.ascent.toNumber(),descent:m.descent.toNumber(),em:m.em.toNumber()};
    };
    globalThis.__microfontMetrics = metric;
    globalThis.__textReport = () => ({units:'DotM', unitName:'DistanceOnTheMonitor', referenceEm:MicrofontMetrics.referenceEm, glyphAdvance:MicrofontMetrics.measure('').glyphAdvance, lineHeight:MicrofontMetrics.referenceEm, normalized:MicrofontMetrics.normalized, monitor:globalThis.__monitorDistanceReport?.(), draws, notes});
    const now = __hostNow;
    globalThis.performance = {timeOrigin:Date.now()-now(), now:() => now(), mark(name) { note('performance-mark',String(name)); }, measure(name) { note('performance-measure',String(name)); }, clearMarks() {}, clearMeasures() {}, getEntries:() => [], getEntriesByName:() => [], getEntriesByType:() => [], timing:{navigationStart:Date.now()}};
    globalThis.crypto = {
        getRandomValues(array) {
            if (!ArrayBuffer.isView(array) || array instanceof DataView || array instanceof Float32Array || array instanceof Float64Array) throw new TypeError('Expected integer typed array');
            if (array.byteLength > 65536) throw new RangeError('QuotaExceededError');
            new Uint8Array(array.buffer,array.byteOffset,array.byteLength).set(new Uint8Array(__hostRandom(array.byteLength))); return array;
        },
        randomUUID() { const b = this.getRandomValues(new Uint8Array(16)); b[6] = (b[6]&15)|64; b[8] = (b[8]&63)|128; const h = Array.from(b,x => x.toString(16).padStart(2,'0')); return h.slice(0,4).join('')+'-'+h.slice(4,6).join('')+'-'+h.slice(6,8).join('')+'-'+h.slice(8,10).join('')+'-'+h.slice(10).join(''); }
    };
    globalThis.requestAnimationFrame = callback => setTimeout(() => callback(performance.now()),16);
    globalThis.cancelAnimationFrame = id => clearTimeout(id);
    globalThis.requestIdleCallback = callback => setTimeout(() => callback({didTimeout:false,timeRemaining:() => 0}),1);
    globalThis.cancelIdleCallback = id => clearTimeout(id);
    globalThis.matchMedia = query => { const target = new EventTarget(); return Object.assign(target,{media:String(query),matches:false,onchange:null,addListener(fn) { this.addEventListener('change',fn); },removeListener(fn) { this.removeEventListener('change',fn); }}); };
    globalThis.CSS = {supports:() => false, escape:value => String(value).replace(/[^a-zA-Z0-9_-]/g,c => '\\'+c.codePointAt(0).toString(16)+' ')};
    // A declared virtual monitor for headless runs, never inferred from raster DPI.
    const monitor = {width:DotM.fromCentimeters(__page.monitor?.widthCm ?? 32),height:DotM.fromCentimeters(__page.monitor?.heightCm ?? 18),source:__page.monitor ? 'configured' : 'synthetic-headless'};
    globalThis.__monitorDistanceReport = () => monitor;
    globalThis.devicePixelRatio = 1; // Dimensionless compatibility value, not calibration.
    globalThis.innerWidth = monitor.width.toNumber(); globalThis.innerHeight = monitor.height.toNumber();
    globalThis.screen = {width:innerWidth,height:innerHeight,availWidth:innerWidth,availHeight:innerHeight,colorDepth:24,pixelDepth:24};
    document.defaultView = globalThis; document.referrer = ''; document.characterSet = 'UTF-8'; document.compatMode = 'CSS1Compat';
    const cookies = new Map();
    Object.defineProperty(document,'cookie',{configurable:true,get() { return [...cookies].map(([k,v]) => k+'='+v).join('; '); },set(value) { const pair = String(value).split(';')[0], index = pair.indexOf('='); if (index >= 0) cookies.set(pair.slice(0,index).trim(),pair.slice(index+1)); note('memory-cookie',pair.split('=')[0]); }});
    Event.prototype.initEvent = function(type,bubbles=false,cancelable=false) { this.type=String(type);this.bubbles=!!bubbles;this.cancelable=!!cancelable; };
    CustomEvent.prototype.initCustomEvent = function(type,bubbles,cancelable,detail) { this.initEvent(type,bubbles,cancelable);this.detail=detail; };
    Document.prototype.createAttribute = function(name) { return {name:String(name),value:'',nodeType:2}; };
    Element.prototype.setAttributeNode = function(attr) { const old=this.hasAttribute(attr.name)?{name:attr.name,value:this.getAttribute(attr.name)}:null;this.setAttribute(attr.name,attr.value);return old; };
    Node.prototype.getRootNode = function() { let n=this;while(n.parentNode)n=n.parentNode;return n; };
    Node.prototype.compareDocumentPosition = function(other) { if (other===this)return 0;if(this.contains(other))return 20;if(other.contains(this))return 10;return 1; };
    Document.prototype.importNode = function(node,deep=false) { const clone=node.cloneNode(deep);const adopt=n=>{n.ownerDocument=this;for(const c of n.childNodes)adopt(c);};adopt(clone);return clone; };
    Document.prototype.adoptNode = function(node) { node.remove();const adopt=n=>{n.ownerDocument=this;for(const c of n.childNodes)adopt(c);};adopt(node);return node; };
    Object.defineProperty(Element.prototype,'innerText',{configurable:true,get() {return this.textContent;},set(value) {this.textContent=value;}});
    for(const property of ['lang','dir','title']) Object.defineProperty(Element.prototype,property,{configurable:true,get() {return this.getAttribute(property)||'';},set(v) {this.setAttribute(property,v);}});
    class FontFace {
        constructor(family,source,descriptors={}) {this.family=String(family);this.source=source;Object.assign(this,descriptors);this.status='unloaded';this.loaded=Promise.resolve(this);note('font-mapped-to-microfont',this.family);}
        load() {this.status='loaded';return this.loaded;}
    }
    class FontFaceSet extends EventTarget {
        constructor(){super();this._faces=new Set();this.status='loaded';this.ready=Promise.resolve(this);}
        add(face){this._faces.add(face);return this;}delete(face){return this._faces.delete(face);}clear(){this._faces.clear();}has(face){return this._faces.has(face);}
        get size(){return this._faces.size;}values(){return this._faces.values();}keys(){return this.values();}[Symbol.iterator](){return this.values();}
        check(font,text=' '){note('font-check',String(font));return true;}
        load(font,text=' '){note('font-load-mapped',String(font));return Promise.resolve([...this._faces]);}
    }
    globalThis.FontFace=FontFace;globalThis.FontFaceSet=FontFaceSet;document.fonts=new FontFaceSet();
    class Animation extends EventTarget {
        constructor(effect=null,timeline=null){super();this.effect=effect;this.timeline=timeline;this.currentTime=0;this.playbackRate=1;this.playState='finished';this.pending=false;this.ready=Promise.resolve(this);this.finished=Promise.resolve(this);note('animation-inactive','Animation');}
        play(){this.playState='finished';}pause(){this.playState='paused';}cancel(){this.playState='idle';}finish(){this.playState='finished';}reverse(){this.playState='finished';}updatePlaybackRate(rate){this.playbackRate=Number(rate);}
    }
    Element.prototype.animate=function(keyframes,options){return new Animation({target:this,keyframes,options});};
    Element.prototype.getAnimations=()=>[]; Document.prototype.getAnimations=()=>[];
    globalThis.Animation=Animation;document.timeline={currentTime:0};
    class CanvasRenderingContext2D {
        constructor(canvas){this.canvas=canvas;this.font='microfont';this.monitorEm=MicrofontMetrics.referenceEm;this.fillStyle='#000000';this.strokeStyle='#000000';this.globalAlpha=1;this.textAlign='start';this.textBaseline='alphabetic';this._stack=[];}
        measureText(text){const m=metric(String(text).replace(/[\r\n]/g,' '),this.monitorEm);return {width:m.width,actualBoundingBoxLeft:0,actualBoundingBoxRight:m.width,actualBoundingBoxAscent:m.ascent,actualBoundingBoxDescent:m.descent,fontBoundingBoxAscent:m.ascent,fontBoundingBoxDescent:m.descent,emHeightAscent:m.ascent,emHeightDescent:m.descent,alphabeticBaseline:0,hangingBaseline:m.ascent,ideographicBaseline:-m.descent};}
        fillText(text,x,y,maxWidth){const content=String(text),m=metric(content,this.monitorEm);if(draws.length<500)draws.push({method:'fillText',text:content.slice(0,4096),x:Number(x),y:Number(y),coordinates:'site-supplied; not display-calibrated',width:m.width,height:m.em,units:'DotM',maxWidth:maxWidth??null});}
        strokeText(...args){this.fillText(...args);}
        save(){this._stack.push({font:this.font,fillStyle:this.fillStyle,strokeStyle:this.strokeStyle,globalAlpha:this.globalAlpha,textAlign:this.textAlign,textBaseline:this.textBaseline});}
        restore(){if(this._stack.length)Object.assign(this,this._stack.pop());}
        createLinearGradient(){return {addColorStop(){}};}createRadialGradient(){return this.createLinearGradient();}createPattern(){return null;}
        getLineDash(){return [];}setLineDash(){}isPointInPath(){return false;}isPointInStroke(){return false;}
    }
    for(const name of ['fillRect','strokeRect','clearRect','beginPath','closePath','moveTo','lineTo','bezierCurveTo','quadraticCurveTo','arc','arcTo','ellipse','rect','fill','stroke','clip','translate','rotate','scale','transform','setTransform','resetTransform','drawImage']) CanvasRenderingContext2D.prototype[name]=function(){note('canvas-rasterization-skipped',name);};
    Element.prototype.getContext=function(kind){if(this.localName!=='canvas')return null;if(String(kind)!=='2d'){note('unsupported-canvas-context',String(kind));return null;}return this._context2d??=new CanvasRenderingContext2D(this);};
    globalThis.CanvasRenderingContext2D=CanvasRenderingContext2D;
    Element.prototype.getBoundingClientRect=function(){const m=metric(this.textContent,this.monitorEm??MicrofontMetrics.referenceEm);return {x:0,y:0,top:0,left:0,right:m.width,bottom:m.height,width:m.width,height:this.textContent?m.height:0,toJSON(){return {x:this.x,y:this.y,width:this.width,height:this.height};}};};
    for(const key of ['clientWidth','offsetWidth','scrollWidth','clientHeight','offsetHeight','scrollHeight']) {
        const old=Object.getOwnPropertyDescriptor(Element.prototype,key);
        if(old?.configurable)Object.defineProperty(Element.prototype,key,{configurable:true,get(){return this['_'+key]??this.getBoundingClientRect()[key.endsWith('Width')?'width':'height'];},set(v){this['_'+key]=Number(v);}});
    }
    const computed = globalThis.getComputedStyle;
    globalThis.getComputedStyle=node=>new Proxy(computed(node),{get(target,key){if(key==='fontSize')return node.style.fontSize || 'medium';if(key==='lineHeight')return node.style.lineHeight || 'normal';if(key==='fontFamily')return node.style.fontFamily || 'microfont';return target[key];}});
    // Same-runtime message ports, with delivery scheduled as tasks; no workers.
    const clone = (value, seen = new Map()) => {
        if (typeof value === 'function' || typeof value === 'symbol') throw new TypeError('DataCloneError');
        if (value === null || typeof value !== 'object') return value;
        if (seen.has(value)) return seen.get(value);
        if (value instanceof ArrayBuffer) { const out=value.slice(0);seen.set(value,out);return out; }
        if (ArrayBuffer.isView(value)) {const buffer=clone(value.buffer,seen);const out=value instanceof DataView?new DataView(buffer,value.byteOffset,value.byteLength):new value.constructor(buffer,value.byteOffset,value.length);seen.set(value,out);return out;}
        if (value instanceof Date) return new Date(value.getTime());
        if (value instanceof RegExp) return new RegExp(value.source,value.flags);
        const proto=Object.getPrototypeOf(value);
        if (!(Array.isArray(value) || value instanceof Map || value instanceof Set || proto===Object.prototype || proto===null))throw new TypeError('DataCloneError: unsupported object');
        const out=Array.isArray(value)?new Array(value.length):value instanceof Map?new Map():value instanceof Set?new Set():{};seen.set(value,out);
        if(value instanceof Map)for(const [key,item]of value)out.set(clone(key,seen),clone(item,seen));
        else if(value instanceof Set)for(const item of value)out.add(clone(item,seen));
        else for(const key of Object.keys(value))Object.defineProperty(out,key,{value:clone(value[key],seen),writable:true,enumerable:true,configurable:true});
        return out;
    };
    globalThis.structuredClone=(value,options={})=>{if(options.transfer?.length)throw new TypeError('Transfer is not implemented');return clone(value);};
    class MessagePort extends EventTarget {
        constructor(){super();this._peer=null;this._closed=false;this._started=false;this._messages=[];this._onmessage=null;}
        get onmessage(){return this._onmessage;}set onmessage(fn){this._onmessage=fn;if(typeof fn==='function')this.start();}
        start(){this._started=true;this._schedule();}
        close(){this._closed=true;this._messages.length=0;}
        postMessage(value,transfer=[]){if(transfer.length)throw new TypeError('Transfer is not implemented');const copied=structuredClone(value);if(!this._closed&&this._peer&&!this._peer._closed){this._peer._messages.push(copied);this._peer._schedule();}}
        _schedule(){if(!this._started||this._closed||this._scheduled||!this._messages.length)return;this._scheduled=true;setTimeout(()=>{this._scheduled=false;if(this._closed)return;const data=this._messages.shift();this.dispatchEvent(new MessageEvent('message',{data,ports:[]}));this._schedule();},0);}
    }
    const historyEntries=[{state:null,url:location.href}];let historyIndex=0;
    globalThis.history={
        get length(){return historyEntries.length;},get state(){return structuredClone(historyEntries[historyIndex].state);},scrollRestoration:'auto',
        pushState(state,title,url){historyEntries.splice(historyIndex+1);historyEntries.push({state:structuredClone(state),url:url==null?location.href:String(url)});historyIndex++;if(url!=null)note('history-url-not-navigated',String(url));},
        replaceState(state,title,url){historyEntries[historyIndex]={state:structuredClone(state),url:url==null?location.href:String(url)};if(url!=null)note('history-url-not-navigated',String(url));},
        go(delta=0){const target=historyIndex+Math.trunc(Number(delta)||0);if(target===historyIndex){note('history-reload-inactive','go(0)');return;}if(target<0||target>=historyEntries.length)return;historyIndex=target;const state=this.state;setTimeout(()=>dispatchEvent(Object.assign(new Event('popstate'),{state})),0);},
        back(){this.go(-1);},forward(){this.go(1);}
    };
    class MessageEvent extends Event {constructor(type,init={}){super(type,init);this.data=init.data??null;this.origin=init.origin??'';this.ports=init.ports??[];}}
    class MessageChannel {constructor(){this.port1=new MessagePort();this.port2=new MessagePort();this.port1._peer=this.port2;this.port2._peer=this.port1;}}
    Object.assign(globalThis,{MessageChannel,MessagePort,MessageEvent});
    // Geometry-driven callbacks need a layout engine. Retain targets and report inactivity.
    class InactiveObserver {
        constructor(callback,options={}){if(typeof callback!=='function')throw new TypeError('Expected callback');this.callback=callback;this.options=options;this.targets=new Set();note('observer-inactive',this.constructor.name);}
        observe(target){this.targets.add(target);}unobserve(target){this.targets.delete(target);}disconnect(){this.targets.clear();}takeRecords(){return [];}
    }
    class IntersectionObserver extends InactiveObserver {get root(){return this.options.root??null;}get rootMargin(){return this.options.rootMargin??'0';}get thresholds(){const t=this.options.threshold??0;return Array.isArray(t)?[...t]:[t];}}
    class ResizeObserver extends InactiveObserver {}
    globalThis.IntersectionObserver=IntersectionObserver;globalThis.ResizeObserver=ResizeObserver;
    for(const [name,object] of Object.entries({Node:Node.prototype,Element:Element.prototype,Document:Document.prototype,Event:Event.prototype,EventTarget:EventTarget.prototype,FontFace:FontFace.prototype,FontFaceSet:FontFaceSet.prototype,Animation:Animation.prototype,CanvasRenderingContext2D:CanvasRenderingContext2D.prototype,MessagePort:MessagePort.prototype,IntersectionObserver:InactiveObserver.prototype,performance,crypto,history,localStorage:Object.getPrototypeOf(localStorage)}))__traceObject(object,name);
    __traceObject(document,'document');
    for(const key of ['setTimeout','clearTimeout','setInterval','clearInterval','queueMicrotask','requestAnimationFrame','cancelAnimationFrame','requestIdleCallback','getComputedStyle','matchMedia','structuredClone']){const fn=globalThis[key];globalThis[key]=function(...args){__traceCall(key,args);return Reflect.apply(fn,this,args);};}
})();
