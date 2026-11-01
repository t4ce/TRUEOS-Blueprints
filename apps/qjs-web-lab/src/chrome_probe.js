// Chrome preload for startup comparison. Wrap only known host surfaces; never
// synthesize events or alter page state. All buffers are bounded.
(() => {
    'use strict';
    const nativeNow = performance.now.bind(performance);
    const start = nativeNow();
    const MAX_RECENT = 200, MAX_REGISTRATIONS = 500, MAX_STAGES = 64;
    const counts = Object.create(null), recent = [], registrations = [], removals = [];
    const registrationCounts = Object.create(null), deliveryCounts = Object.create(null), listenerCounts = Object.create(null);
    const lifecycleRecent = [], stages = [], shadowRoots = new Set();
    const lifecycleTypes = new Set(['readystatechange','DOMContentLoaded','load','pageshow','pagehide','focus','blur','visibilitychange','resize','scroll']);
    const callbacks = Object.fromEntries(['timeout','interval','animationFrame','idle','microtask','mutationObserver','intersectionObserver','resizeObserver'].map(key=>[key,{scheduled:0,invoked:0,items:0}]));
    const createdByAPI={returnedRoots:0,byAPI:Object.create(null)};
    const creationMethods=new Set(['createElement','createElementNS','createTextNode','createComment','createDocumentFragment','createCDATASection','createProcessingInstruction']);
    const mutations = {batches:0,records:0,addedNodes:0,removedNodes:0,attributes:0,characterData:0};
    const limitations = ['Instrumentation wraps native host functions and is observable through function identity/toString.', 'Registration counts are add/remove calls, including native duplicate registrations.', 'String timer callbacks retain native behavior; invocation counts cover function callbacks only.', 'DOM totals cover the connected document and connected shadow roots tracked after preload; iframe documents report separately.', 'Mutation totals include observed detached shadow roots, while DOM/text snapshot totals exclude them.', 'createdByAPI counts successful returned factory roots, not descendants cloned/parsed internally.', 'Microtask callback counters cover queueMicrotask, not native Promise reaction jobs.'];
    let stage = 'initial', total = 0, droppedRegistrations = 0, droppedStages = 0;
    const time = () => nativeNow() - start;
    const bounded = (array,value,limit=MAX_RECENT) => { array.push(value); if(array.length>limit)array.shift(); };
    const bump = (map,key) => {if(!Object.hasOwn(map,key)&&Object.keys(map).length>=1024)key='<other>';map[key]=(map[key]||0)+1;};
    const brief = value => {
        const type=typeof value;
        return value===null?null:type==='string'?value.slice(0,120):type==='number'||type==='boolean'?value:type==='undefined'?'undefined':'['+type+']';
    };
    const record = (api,args=[]) => {
        total++; bump(counts,api);
        bounded(recent,{sequence:total,stage,time:time(),api,args:Array.from(args).slice(0,4).map(brief)});
    };
    const originalAdd = EventTarget.prototype.addEventListener;
    const originalRemove = EventTarget.prototype.removeEventListener;
    const nodeName = Object.getOwnPropertyDescriptor(Node.prototype,'nodeName').get;
    const nodeType = Object.getOwnPropertyDescriptor(Node.prototype,'nodeType').get;
    const nodeValue=Object.getOwnPropertyDescriptor(Node.prototype,'nodeValue').get;
    const parentNode=Object.getOwnPropertyDescriptor(Node.prototype,'parentNode').get;
    const isConnected=Object.getOwnPropertyDescriptor(Node.prototype,'isConnected').get;
    const shadowHost=Object.getOwnPropertyDescriptor(ShadowRoot.prototype,'host').get;
    const elementId = Object.getOwnPropertyDescriptor(Element.prototype,'id').get;
    const nativeQueryAll = Document.prototype.querySelectorAll;
    const nativeCreateWalker = Document.prototype.createTreeWalker;
    const nativeWalkerNext = TreeWalker.prototype.nextNode;
    const NativeMutationObserver = globalThis.MutationObserver;
    const nativeObserve = NativeMutationObserver.prototype.observe;
    function targetInfo(target) {
        if(target===globalThis)return {kind:'window'};
        if(target===document)return {kind:'document'};
        try {
            if(Reflect.apply(nodeType,target,[])===1){
                const id=Reflect.apply(elementId,target,[]), tag=Reflect.apply(nodeName,target,[]);
                return {kind:'element',tag,id:id.slice(0,160),selector:id&&id.length<=160?'#'+CSS.escape(id):null,...(id.length>160?{truncatedId:true}:{})};
            }
            return {kind:'node',tag:Reflect.apply(nodeName,target,[])};
        } catch {return {kind:'other'};}
    }
    const targetKey = target => target.kind+(target.id?'#'+target.id:target.tag?':'+target.tag:'');
    function lifecycleRegistration(kind,target,type) {
        if(typeof type!=='string'||!lifecycleTypes.has(type))return;
        const entry={type,target:targetInfo(target),stage,time:time()};
        if(kind==='add') {
            bump(registrationCounts,type+'@'+targetKey(entry.target));
            if(registrations.length<MAX_REGISTRATIONS)registrations.push(entry);else droppedRegistrations++;
        } else bounded(removals,entry);
    }
    const observedEvents=new WeakSet();
    function observeLifecycle(event) {
        if(observedEvents.has(event))return;
        observedEvents.add(event);bump(deliveryCounts,event.type);
        bounded(lifecycleRecent,{type:event.type,target:targetInfo(event.target),stage,time:time(),trusted:event.isTrusted,visibility:document.visibilityState,readyState:document.readyState});
    }
    // Install probe listeners through the originals before page instrumentation.
    for(const type of lifecycleTypes){
        Reflect.apply(originalAdd,globalThis,[type,observeLifecycle,true]);
        Reflect.apply(originalAdd,document,[type,observeLifecycle,true]);
    }
    // One stable wrapper per target/listener, reused across types and capture
    // modes. Native add/remove reads the original options object exactly once.
    const listenerWrappers = new WeakMap();
    const weakKey=value=>value!==null&&(typeof value==='object'||typeof value==='function');
    // Native Window event methods accept an absent/null receiver as Window.
    // Keep that receiver for callback identity even when a strict wrapper is called bare.
    const listenerTarget=target=>target===undefined||target===null?globalThis:target;
    function mappedListener(target,listener,create) {
        target=listenerTarget(target);
        if(!weakKey(target)||listener===null||!['function','object'].includes(typeof listener))return listener;
        let wrappers=listenerWrappers.get(target);
        if(!wrappers){if(!create)return listener;wrappers=new WeakMap();listenerWrappers.set(target,wrappers);}
        let wrapper=wrappers.get(listener);
        if(!wrapper&&create){
            wrapper=function(event){
                if(lifecycleTypes.has(event.type))bump(listenerCounts,event.type);
                record('EventListener.callback',[event.type]);
                return typeof listener==='function'?Reflect.apply(listener,this,[event]):listener.handleEvent(event);
            };
            wrappers.set(listener,wrapper);
        }
        return wrapper||listener;
    }
    EventTarget.prototype.addEventListener=function(type,listener,...options){
        record('EventTarget.addEventListener',[type]);lifecycleRegistration('add',listenerTarget(this),type);
        return Reflect.apply(originalAdd,this,[type,mappedListener(this,listener,true),...options]);
    };
    EventTarget.prototype.removeEventListener=function(type,listener,...options){
        record('EventTarget.removeEventListener',[type]);lifecycleRegistration('remove',listenerTarget(this),type);
        return Reflect.apply(originalRemove,this,[type,mappedListener(this,listener,false),...options]);
    };
    // Preserve property-handler getter identity as well as native setter rules.
    for(const owner of [globalThis,Document.prototype,HTMLElement.prototype])for(const type of lifecycleTypes){
        const key='on'+type,descriptor=Object.getOwnPropertyDescriptor(owner,key);
        if(!descriptor?.configurable||typeof descriptor.set!=='function'||typeof descriptor.get!=='function')continue;
        const nativeSet=descriptor.set,nativeGet=descriptor.get,originalHandlers=new WeakMap();
        descriptor.set=function(handler){
            record('EventHandler.'+key+'.set',[handler]);
            if(typeof handler!=='function')return Reflect.apply(nativeSet,this,[handler]);
            const wrapped=function(event){bump(listenerCounts,type);record('EventHandler.callback',[type]);return Reflect.apply(handler,this,[event]);};
            originalHandlers.set(wrapped,handler);lifecycleRegistration('add',this,type);
            return Reflect.apply(nativeSet,this,[wrapped]);
        };
        descriptor.get=function(){const value=Reflect.apply(nativeGet,this,[]);return weakKey(value)?originalHandlers.get(value)||value:value;};
        Object.defineProperty(owner,key,descriptor);
    }
    function wrapTimer(name,key) {
        const native=globalThis[name];if(typeof native!=='function')return;
        globalThis[name]=function(callback,...args){
            record(name,args);callbacks[key].scheduled++;
            const wrapped=typeof callback==='function'?function(...values){callbacks[key].invoked++;record(name+'.callback',values);return Reflect.apply(callback,this,values);}:callback;
            return Reflect.apply(native,this,[wrapped,...args]);
        };
    }
    wrapTimer('setTimeout','timeout');wrapTimer('setInterval','interval');
    wrapTimer('requestAnimationFrame','animationFrame');wrapTimer('requestIdleCallback','idle');wrapTimer('queueMicrotask','microtask');
    function metadata(wrapper,native){for(const key of ['name','length'])try{Object.defineProperty(wrapper,key,{value:native[key],configurable:true});}catch{}return wrapper;}
    function instrument(object,prefix,names) {
        if(!object)return;
        for(const name of names){
            const descriptor=Object.getOwnPropertyDescriptor(object,name);
            if(!descriptor||!descriptor.configurable||typeof descriptor.value!=='function')continue;
            const native=descriptor.value;
            descriptor.value=metadata(function(...args){record(prefix+'.'+name,args);const value=Reflect.apply(native,this,args);if(prefix==='Document'&&creationMethods.has(name)){createdByAPI.returnedRoots++;bump(createdByAPI.byAPI,name);}return value;},native);
            Object.defineProperty(object,name,descriptor);
        }
    }
    instrument(Node.prototype,'Node',['appendChild','insertBefore','removeChild','replaceChild','cloneNode','contains','getRootNode','compareDocumentPosition']);
    instrument(Element.prototype,'Element',['getAttribute','getAttributeNS','setAttribute','setAttributeNS','removeAttribute','removeAttributeNS','toggleAttribute','hasAttribute','getAttributeNames','querySelector','querySelectorAll','getElementsByTagName','getElementsByClassName','matches','closest','getBoundingClientRect','getClientRects','insertAdjacentElement','insertAdjacentHTML','insertAdjacentText','append','prepend','replaceChildren','remove','before','after','replaceWith','animate','getAnimations']);
    instrument(Document.prototype,'Document',['createElement','createElementNS','createTextNode','createComment','createDocumentFragment','getElementById','querySelector','querySelectorAll','getElementsByTagName','getElementsByClassName','createEvent','createRange','createTreeWalker','importNode','adoptNode','append','prepend','replaceChildren']);
    instrument(HTMLElement.prototype,'HTMLElement',['focus','blur','click']);
    instrument(CanvasRenderingContext2D.prototype,'CanvasRenderingContext2D',['measureText','fillText','strokeText']);
    instrument(globalThis.FontFace?.prototype,'FontFace',['load']);
    instrument(globalThis.FontFaceSet?.prototype,'FontFaceSet',['check','load','add','delete','clear']);
    instrument(globalThis.Storage?.prototype,'Storage',['getItem','setItem','removeItem','clear','key']);
    for(const name of ['getComputedStyle','matchMedia','cancelAnimationFrame','cancelIdleCallback','clearTimeout','clearInterval']) {
        const native=globalThis[name];if(typeof native!=='function')continue;
        globalThis[name]=function(...args){record(name,args);return Reflect.apply(native,this,args);};
    }
    function wrapObserver(name,key) {
        const Native=globalThis[name];if(typeof Native!=='function')return;
        let ProxyConstructor;
        ProxyConstructor=new Proxy(Native,{construct(target,args,newTarget){
            callbacks[key].scheduled++;record(name+'.constructor',[]);
            if(typeof args[0]!=='function')return Reflect.construct(target,args,newTarget);
            const callback=args[0];
            const wrapped=function(...values){callbacks[key].invoked++;callbacks[key].items+=values[0]?.length||0;record(name+'.callback',[values[0]?.length||0]);return Reflect.apply(callback,this,values);};
            return Reflect.construct(target,[wrapped,...args.slice(1)],newTarget===ProxyConstructor?target:newTarget);
        }});
        globalThis[name]=ProxyConstructor;
        const constructorDescriptor=Object.getOwnPropertyDescriptor(Native.prototype,'constructor');
        if(constructorDescriptor?.configurable)Object.defineProperty(Native.prototype,'constructor',{...constructorDescriptor,value:ProxyConstructor});
        instrument(Native.prototype,name,['observe','unobserve','disconnect','takeRecords']);
    }
    wrapObserver('MutationObserver','mutationObserver');wrapObserver('IntersectionObserver','intersectionObserver');wrapObserver('ResizeObserver','resizeObserver');
    const ownObserver=new NativeMutationObserver(records=>{
        mutations.batches++;mutations.records+=records.length;
        for(const item of records){mutations.addedNodes+=item.addedNodes.length;mutations.removedNodes+=item.removedNodes.length;if(item.type==='attributes')mutations.attributes++;if(item.type==='characterData')mutations.characterData++;}
    });
    const observeRoot=root=>Reflect.apply(nativeObserve,ownObserver,[root,{subtree:true,childList:true,attributes:true,characterData:true}]);
    observeRoot(document);
    const nativeAttachShadow=Element.prototype.attachShadow;
    Element.prototype.attachShadow=function(...args){record('Element.attachShadow',args);const root=Reflect.apply(nativeAttachShadow,this,args);shadowRoots.add(root);observeRoot(root);return root;};
    function domSnapshot() {
        let nodeCount=0,elementCount=0,truncated=false;
        const text={units:'unicode-scalars',counting:'Array.from(string).length',nodes:0,characters:0,charactersUnicodeIncludingScripts:0,charactersUTF16IncludingScripts:0,nonWhitespaceCharacters:0,contentNodes:0,contentCharacters:0,contentCharactersUnicode:0,contentCharactersUTF16:0};
        const connectedRoots=[];for(const root of shadowRoots){const host=Reflect.apply(shadowHost,root,[]);if(Reflect.apply(isConnected,host,[]))connectedRoots.push(root);}
        const countNode=node=>{
            const type=Reflect.apply(nodeType,node,[]);if(type===1)elementCount++;
            if(type!==3)return;
            const value=Reflect.apply(nodeValue,node,[])||'',unicodeCharacters=Array.from(value).length;text.nodes++;text.characters+=unicodeCharacters;text.charactersUnicodeIncludingScripts+=unicodeCharacters;text.charactersUTF16IncludingScripts+=value.length;text.nonWhitespaceCharacters+=Array.from(value.replace(/\s/g,'')).length;
            let content=true;for(let parent=Reflect.apply(parentNode,node,[]);parent;parent=Reflect.apply(parentNode,parent,[]))if(['SCRIPT','STYLE','NOSCRIPT','TEMPLATE'].includes(Reflect.apply(nodeName,parent,[]))){content=false;break;}
            if(content){text.contentNodes++;text.contentCharacters+=unicodeCharacters;text.contentCharactersUnicode+=unicodeCharacters;text.contentCharactersUTF16+=value.length;}
        };
        for(const root of [document,...connectedRoots]){
            const walker=Reflect.apply(nativeCreateWalker,document,[root,NodeFilter.SHOW_ALL]);
            nodeCount++;countNode(root);
            let node;while((node=Reflect.apply(nativeWalkerNext,walker,[]))){nodeCount++;countNode(node);if(nodeCount>=500000){truncated=true;break;}}
            if(truncated)break;
        }
        return {nodeCount,elementCount,lightElementCount:Reflect.apply(nativeQueryAll,document,['*']).length,scope:'connected-document-and-shadow-roots',shadowRoots:connectedRoots.length,trackedShadowRoots:shadowRoots.size,connectedShadowRoots:connectedRoots.length,detachedShadowRoots:shadowRoots.size-connectedRoots.length,truncated,text,createdByAPI:{returnedRoots:createdByAPI.returnedRoots,byAPI:{...createdByAPI.byAPI}},mutations:{...mutations}};
    }
    function setStage(name) {
        stage=String(name).slice(0,120);
        const entry={name:stage,time:time(),readyState:document.readyState,visibility:document.visibilityState,dom:domSnapshot()};
        if(stages.length<MAX_STAGES)stages.push(entry);else droppedStages++;
        return entry;
    }
    function snapshot() {
        return {version:2,stage,time:time(),api:{total,counts:{...counts},recent:recent.slice()},lifecycle:{registrations:registrations.slice(),removals:removals.slice(),registrationCounts:{...registrationCounts},delivered:{...deliveryCounts},callbacks:{...listenerCounts},recent:lifecycleRecent.slice(),droppedRegistrations},callbacks:structuredClone(callbacks),dom:domSnapshot(),stages:stages.slice(),droppedStages,limitations:limitations.slice()};
    }
    Object.defineProperty(globalThis,'__chromeProbe',{value:Object.freeze({snapshot}),configurable:false});
    Object.defineProperty(globalThis,'__setProbeStage',{value:setStage,configurable:false});
    setStage('preload');
})();
