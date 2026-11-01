// Headless approximation: persistent tree, lazy selector placeholders, no rendering.
(() => {
    const notes = [], synthetic = new Map(); let allocated = 0;
    const note = (kind, detail) => { if (notes.length < 500) notes.push({kind, detail}); };
    const observers=new Set();
    function notifyMutation(record) {
        for(const observer of observers){
            const match=observer._targets.some(({target,options})=>{
                let contains=target===record.target;
                if(!contains&&options.subtree)for(let n=record.target.parentNode;n;n=n.parentNode)if(n===target){contains=true;break;}
                return contains&&options[record.type]&&(!options.attributeFilter||record.type!=='attributes'||options.attributeFilter.includes(record.attributeName));
            });
            if(!match)continue;
            observer._records.push(record);
            if(!observer._scheduled){observer._scheduled=true;queueMicrotask(()=>{observer._scheduled=false;const records=observer.takeRecords();if(records.length)try{observer._callback(records,observer);}catch(error){if(globalThis.__recordHostError)__recordHostError('mutation-observer',error);else throw error;}});}
        }
    }
    class MutationObserver {
        constructor(callback){if(typeof callback!=='function')throw TypeError('Expected observer callback');this._callback=callback;this._targets=[];this._records=[];this._scheduled=false;}
        observe(target,options={}){if(!(target instanceof Node))throw TypeError('Expected Node');options={...options};if(options.attributeOldValue||options.attributeFilter)options.attributes??=true;if(options.characterDataOldValue)options.characterData??=true;if(!options.childList&&!options.attributes&&!options.characterData)throw TypeError('Expected mutation type');this._targets=this._targets.filter(x=>x.target!==target);this._targets.push({target,options});observers.add(this);}
        disconnect(){this._targets=[];this._records=[];observers.delete(this);}takeRecords(){return this._records.splice(0);}
    }
    class Event {
        constructor(type, init = {}) { this.type = String(type); Object.assign(this, init); this.defaultPrevented = false; }
        initEvent(type,bubbles=false,cancelable=false) { this.type=String(type);this.bubbles=!!bubbles;this.cancelable=!!cancelable;this.defaultPrevented=false; }
        composedPath() { const result=[];for(let node=this.target;node;node=node.parentNode||node.host)result.push(node);return result; }
        preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
        stopPropagation() { this._stopped = true; }
        stopImmediatePropagation() { this._immediate = this._stopped = true; }
    }
    class CustomEvent extends Event { constructor(type, init = {}) { super(type, init); this.detail = init.detail ?? null; } }
    class MouseEvent extends Event {
        constructor(type, init={}) { super(type, init);for(const key of ['screenX','screenY','clientX','clientY','button','buttons','detail'])this[key]=Number(init[key]||0);for(const key of ['ctrlKey','altKey','shiftKey','metaKey'])this[key]=!!init[key];this.relatedTarget=init.relatedTarget||null;this.view=init.view||null; }
        initMouseEvent(type,bubbles,cancelable,view,detail,screenX,screenY,clientX,clientY,ctrlKey,altKey,shiftKey,metaKey,button,relatedTarget) { this.initEvent(type,bubbles,cancelable);Object.assign(this,{view,detail,screenX,screenY,clientX,clientY,ctrlKey,altKey,shiftKey,metaKey,button,relatedTarget}); }
        getModifierState(key) { return !!this[{Control:'ctrlKey',Alt:'altKey',Shift:'shiftKey',Meta:'metaKey'}[key]]; }
    }
    class KeyboardEvent extends Event { constructor(type,init={}) { super(type,init);this.key=init.key||'';this.code=init.code||''; } }
    class NodeList extends Array { item(index) { return this[Number(index)] || null; } }
    class HTMLCollection extends NodeList { namedItem(name) { return this.find(n=>n.id===name||n.getAttribute?.('name')===name)||null; } }
    class EventTarget {
        constructor() { this._listeners = new Map(); }
        addEventListener(type, fn, options = {}) {
            if (!fn) return; type = String(type);
            const capture=typeof options==='boolean'?options:!!options?.capture, once=!!options?.once;
            if(options?.signal?.aborted)return;
            const list = this._listeners.get(type) || [];
            if (!list.some(x => x.fn === fn && x.capture===capture)) list.push({fn, once, capture});
            this._listeners.set(type, list);
        }
        removeEventListener(type, fn, options={}) { const capture=typeof options==='boolean'?options:!!options?.capture;this._listeners.set(String(type), (this._listeners.get(String(type)) || []).filter(x => x.fn !== fn || x.capture!==capture)); }
        dispatchEvent(event) {
            event.target ??= this; event.currentTarget = this;
            for (const entry of [...(this._listeners.get(event.type) || [])]) {
                if (entry.once) this.removeEventListener(event.type, entry.fn,entry.capture);
                try {if (typeof entry.fn === 'function') {globalThis.__traceCall?.('EventTarget.listener',[event.type]);entry.fn.call(this, event);} else {const handle=entry.fn.handleEvent;if(typeof handle==='function')globalThis.__traceCall?.('EventTarget.listener',[event.type]);handle.call(entry.fn,event);}}catch(error){if(globalThis.__recordHostError)__recordHostError('event:'+event.type,error);else throw error;}
                if (event._immediate) break;
            }
            try {const handler=this['on'+event.type];if(typeof handler==='function'){globalThis.__traceCall?.('EventTarget.listener',[event.type]);handler.call(this,event);}}catch(error){if(globalThis.__recordHostError)__recordHostError('event:'+event.type,error);else throw error;}
            if (event.bubbles && !event._stopped && this.parentNode) this.parentNode.dispatchEvent(event);
            return !event.defaultPrevented;
        }
    }
    class Node extends EventTarget {
        constructor(name, type = 1, owner = null) { super(); allocated++; this.nodeName = name; this.nodeType = type; this.ownerDocument = owner; this.parentNode = null; this.childNodes = new NodeList(); this.nodeValue = type === 3 || type === 8 ? '' : null; }
        get firstChild() { return this.childNodes[0] || null; }
        get lastChild() { return this.childNodes.at(-1) || null; }
        get nextSibling() { return this.parentNode?.childNodes[this.parentNode.childNodes.indexOf(this) + 1] || null; }
        get previousSibling() { return this.parentNode?.childNodes[this.parentNode.childNodes.indexOf(this) - 1] || null; }
        get parentElement() { return this.parentNode?.nodeType === 1 ? this.parentNode : null; }
        get isConnected() { return this.nodeType === 9 || !!this.parentNode?.isConnected; }
        get textContent() { return this.nodeValue ?? this.childNodes.map(x => x.textContent).join(''); }
        set textContent(value) { if (this.nodeType === 3 || this.nodeType === 8) this.data = String(value ?? ''); else { this.replaceChildren(); if (value != null && value !== '') this.appendChild((this.nodeType===9?this:this.ownerDocument).createTextNode(String(value))); } }
        appendChild(node) { return this.insertBefore(node, null); }
        insertBefore(node, before) {
            if (node === this || node.contains(this)) throw new Error('HierarchyRequestError');
            if (before !== null && before.parentNode !== this) throw new Error('NotFoundError');
            if (node === before) return node;
            if (node.nodeType === 11) { for (const child of [...node.childNodes]) this.insertBefore(child, before); return node; }
            node.remove(); const index = before === null ? this.childNodes.length : this.childNodes.indexOf(before);
            this.childNodes.splice(index, 0, node); node.parentNode = this;notifyMutation({type:'childList',target:this,addedNodes:new NodeList(node),removedNodes:new NodeList(),previousSibling:node.previousSibling,nextSibling:node.nextSibling});return node;
        }
        removeChild(node) { const i = this.childNodes.indexOf(node); if (i < 0) throw new Error('NotFoundError'); const previousSibling=node.previousSibling,nextSibling=node.nextSibling;this.childNodes.splice(i,1);node.parentNode=null;notifyMutation({type:'childList',target:this,addedNodes:new NodeList(),removedNodes:new NodeList(node),previousSibling,nextSibling});return node; }
        replaceChild(node, old) { this.insertBefore(node, old); this.removeChild(old); return old; }
        remove() { this.parentNode?.removeChild(this); }
        contains(node) { return node === this || this.childNodes.some(x => x.contains(node)); }
        hasChildNodes() { return !!this.childNodes.length; }
        append(...nodes) { for (const n of nodes) this.appendChild(typeof n === 'string' ? (this.nodeType===9?this:this.ownerDocument).createTextNode(n) : n); }
        prepend(...nodes) { for (const n of nodes.reverse()) this.insertBefore(typeof n === 'string' ? (this.nodeType===9?this:this.ownerDocument).createTextNode(n) : n, this.firstChild); }
        replaceChildren(...nodes) { for (const child of [...this.childNodes]) this.removeChild(child); this.append(...nodes); }
        before(...nodes) { if(!this.parentNode)return;for(const node of nodes)this.parentNode.insertBefore(typeof node==='string'?this.ownerDocument.createTextNode(node):node,this); }
        after(...nodes) { if(!this.parentNode)return;const before=this.nextSibling;for(const node of nodes)this.parentNode.insertBefore(typeof node==='string'?this.ownerDocument.createTextNode(node):node,before); }
        replaceWith(...nodes) { this.before(...nodes);this.remove(); }
        cloneNode(deep = false) {const n=this.nodeType===1?this.ownerDocument.createElementNS(this.namespaceURI,this.localName):this.nodeType===3?new Text(this.nodeValue,this.ownerDocument):this.nodeType===8?new Comment(this.nodeValue,this.ownerDocument):this.nodeType===11?new DocumentFragment(this.ownerDocument):new Node(this.nodeName,this.nodeType,this.ownerDocument); n.nodeValue = this.nodeValue; if (this.attributes) for (const [k,v] of Object.entries(this._attrs)) n.setAttribute(k,v); if (deep) for (const c of this.childNodes) n.appendChild(c.cloneNode(true)); return n; }
    }
    class CharacterData extends Node {
        get data() { return this.nodeValue; }set data(value) {const oldValue=this.nodeValue;this.nodeValue=String(value);if(oldValue!==this.nodeValue)notifyMutation({type:'characterData',target:this,oldValue});}
        get length() { return this.data.length; }
        appendData(data) {this.data+=String(data);} deleteData(offset,count) {this.replaceData(offset,count,'');}
        insertData(offset,data) {this.replaceData(offset,0,data);} substringData(offset,count) {return this.data.slice(offset,offset+count);}
        replaceData(offset,count,data) {this.data=this.data.slice(0,offset)+String(data)+this.data.slice(offset+count);}
    }
    class Text extends CharacterData { constructor(data='',owner=null) {super('#text',3,owner||globalThis.document);this.data=data;}splitText(offset) {const tail=new Text(this.data.slice(offset),this.ownerDocument);this.data=this.data.slice(0,offset);if(this.parentNode)this.parentNode.insertBefore(tail,this.nextSibling);return tail;} }
    class CDATASection extends CharacterData {constructor(data='',owner=null){super('#cdata-section',4,owner||globalThis.document);this.data=data;}}
    class ProcessingInstruction extends CharacterData {constructor(target='',data='',owner=null){super(String(target),7,owner||globalThis.document);this.target=String(target);this.data=data;}}
    class Comment extends CharacterData { constructor(data='',owner=null) {super('#comment',8,owner||globalThis.document);this.data=data;} }
    class DocumentFragment extends Node {
        constructor(owner=null) {super('#document-fragment',11,owner||globalThis.document);}
        get children() {return this.childNodes.filter(n=>n.nodeType===1);}get childElementCount(){return this.children.length;}get firstElementChild(){return this.children[0]||null;}get lastElementChild(){return this.children.at(-1)||null;}
        querySelector(selector) {return query(this,selector,true);}querySelectorAll(selector) {return Element.prototype.querySelectorAll.call(this,selector);}
        getElementById(id) {return this.querySelector('#'+id);}
    }
    class ShadowRoot extends DocumentFragment { constructor(host,init={}) {super(host.ownerDocument);this.host=host;this.mode=init.mode||'open';this.delegatesFocus=!!init.delegatesFocus;note('shadow-root-in-memory',this.mode);}get isConnected(){return this.host.isConnected;} }
    class CSSStyleDeclaration {
        constructor() {this._values=new Map();this._priority=new Map();}
        get cssText() {return [...this._values].map(([k,v])=>k+': '+v+(this._priority.get(k)?' !important':'')+';').join(' ');}
        set cssText(text) {this._values.clear();this._priority.clear();for(const declaration of String(text).split(';')){const i=declaration.indexOf(':');if(i>=0)this.setProperty(declaration.slice(0,i).trim(),declaration.slice(i+1).trim());}}
        get length(){return this._values.size;}item(index){return [...this._values.keys()][index]||'';}
        setProperty(key,value,priority='') {key=String(key).replace(/[A-Z]/g,c=>'-'+c.toLowerCase());this._values.set(key,String(value));this._priority.set(key,String(priority));}
        getPropertyValue(key) {return this._values.get(String(key).replace(/[A-Z]/g,c=>'-'+c.toLowerCase()))||'';}
        getPropertyPriority(key){return this._priority.get(String(key))||'';}
        removeProperty(key) {const old=this.getPropertyValue(key);this._values.delete(String(key));this._priority.delete(String(key));return old;}
    }
    function styleDeclaration(){const style=new CSSStyleDeclaration();return new Proxy(style,{get(target,key,receiver){if(typeof key==='symbol'||Reflect.has(target,key))return Reflect.get(target,key,receiver);if(key==='then')return undefined;return target.getPropertyValue(key);},set(target,key,value,receiver){if(typeof key==='symbol'||Reflect.has(target,key))return Reflect.set(target,key,value,receiver);target.setProperty(key,value);return true;}});}
    Object.assign(Node, {ELEMENT_NODE:1, TEXT_NODE:3, COMMENT_NODE:8, DOCUMENT_NODE:9, DOCUMENT_FRAGMENT_NODE:11});
    const descendants = node => node.childNodes.flatMap(c => [c, ...descendants(c)]);
    const matches = (node, selector) => {
        if (node.nodeType !== 1) return false;
        if (selector === '*') return true;
        if (/^#[\w-]+$/.test(selector)) return node.id === selector.slice(1);
        if (/^\.[\w-]+$/.test(selector)) return node.classList.contains(selector.slice(1));
        if (/^[\w-]+$/.test(selector)) return node.localName === selector.toLowerCase();
        const attr = selector.match(/^\[([\w-]+)(?:=["']?([^"'\]]+)["']?)?\]$/);
        if (attr) return node.hasAttribute(attr[1]) && (attr[2] === undefined || node.getAttribute(attr[1]) === attr[2]);
        note('unsupported-selector', selector); return false;
    };
    function query(root, selector, lazy) {
        selector = String(selector);
        if(root.nodeType===9 && ['html','head','body'].includes(selector))return selector==='html'?root.documentElement:root[selector];
        const found = descendants(root).find(n => matches(n, selector)); if (found || !lazy) return found || null;
        // Explicitly non-browser behavior requested for the compatibility lab.
        let map = synthetic.get(root); if (!map) synthetic.set(root, map = new Map());
        if (map.has(selector)) return map.get(selector);
        const doc = root.nodeType === 9 ? root : root.ownerDocument;
        const node = doc.createElement(/^[\w-]+$/.test(selector) ? selector : 'div');
        if (selector[0] === '#') node.id = selector.slice(1); else if (selector[0] === '.') node.className = selector.slice(1);
        map.set(selector, node); note('synthetic-node', selector);
        (root.nodeType === 9 ? root.body : root).appendChild(node); return node;
    }
    class Element extends Node {
        constructor(tag, owner) { super(String(tag).toUpperCase(), 1, owner); this.localName = String(tag).toLowerCase(); this._attrs = Object.create(null); this.dataset = Object.create(null); this._style = null; }
        get tagName() { return this.nodeName; }
        get children() { return this.childNodes.filter(n => n.nodeType === 1); }
        get childElementCount() { return this.children.length; }
        get firstElementChild() { return this.children[0] || null; }
        get lastElementChild() { return this.children.at(-1) || null; }
        get attributes() { const attrs=Object.entries(this._attrs).map(([name,value]) => ({name,value,nodeName:name,nodeValue:value,specified:true}));attrs.item=i=>attrs[i]||null;attrs.getNamedItem=name=>attrs.find(a=>a.name===name)||null;return attrs; }
        setAttribute(name,value){name=String(name);value=String(value);const oldValue=this.getAttribute(name);this._attrs[name]=value;if(oldValue!==value)notifyMutation({type:'attributes',target:this,attributeName:name,attributeNamespace:null,oldValue});}
        getAttribute(name) { return this._attrs[String(name)] ?? null; }
        getAttributeNames() {return Object.keys(this._attrs);}
        getAttributeNS(namespace,name) {return this.getAttribute(name);}
        setAttributeNS(namespace,name,value) {this.setAttribute(name,value);}
        removeAttributeNS(namespace,name) {this.removeAttribute(name);}
        hasAttributeNS(namespace,name) {return this.hasAttribute(name);}
        toggleAttribute(name,force) {const next=force===undefined?!this.hasAttribute(name):!!force;if(next)this.setAttribute(name,'');else this.removeAttribute(name);return next;}
        hasAttribute(name) { return Object.hasOwn(this._attrs, String(name)); }
        removeAttribute(name){name=String(name);const oldValue=this.getAttribute(name);delete this._attrs[name];if(oldValue!==null)notifyMutation({type:'attributes',target:this,attributeName:name,attributeNamespace:null,oldValue});}
        get id() { return this.getAttribute('id') || ''; } set id(v) { this.setAttribute('id',v); }
        get className() { return this.getAttribute('class') || ''; } set className(v) { this.setAttribute('class',v); }
        get classList() { const el = this; return { contains: x => el.className.split(/\s+/).includes(x), add(...xs) { el.className = [...new Set([...el.className.split(/\s+/).filter(Boolean), ...xs])].join(' '); }, remove(...xs) { el.className = el.className.split(/\s+/).filter(x => !xs.includes(x)).join(' '); }, toggle(x, force) { const on = force ?? !this.contains(x); if (on) this.add(x); else this.remove(x); return on; } }; }
        get style() {return this._style??=styleDeclaration();}set style(value){this.style.cssText=String(value);}
        querySelector(selector) { return query(this, selector, true); }
        querySelectorAll(selector) { const result = descendants(this).filter(n => matches(n, String(selector))); result.item = i => result[i] || null; return result; }
        getElementsByTagName(tag) { return this.querySelectorAll(tag); }
        getElementsByClassName(cls) { return this.querySelectorAll('.' + cls); }
        matches(selector) { return matches(this, selector); }
        closest(selector) { return this.matches(selector) ? this : this.parentElement?.closest(selector) || null; }
        getBoundingClientRect() { return {x:0,y:0,top:0,left:0,right:0,bottom:0,width:0,height:0,toJSON() { return {...this}; }}; }
        getClientRects() { return []; }
        get innerHTML() { return this._html ?? ''; }
        set innerHTML(html) { this.replaceChildren(); this._html = String(html); note('unparsed-html', this.localName); }
        focus() { this.ownerDocument.activeElement = this; } blur() { if (this.ownerDocument.activeElement === this) this.ownerDocument.activeElement = this.ownerDocument.body; }
        click() { this.dispatchEvent(new Event('click', {bubbles:true, cancelable:true})); }
        attachShadow(init={}) {if(this._shadowRoot)throw Error('NotSupportedError: shadow root already attached');this._shadowRoot=new ShadowRoot(this,init);if(this._shadowRoot.mode==='open')this.shadowRoot=this._shadowRoot;return this._shadowRoot;}
        insertAdjacentElement(position,element) {position=String(position).toLowerCase();if(position==='beforebegin'){if(!this.parentNode)return null;this.before(element);}else if(position==='afterend'){if(!this.parentNode)return null;this.after(element);}else if(position==='afterbegin')this.insertBefore(element,this.firstChild);else if(position==='beforeend')this.appendChild(element);else throw Error('SyntaxError');return element;}
        insertAdjacentText(position,text){this.insertAdjacentElement(position,this.ownerDocument.createTextNode(text));}
        insertAdjacentHTML(position,html){note('unparsed-html','insertAdjacentHTML');this.insertAdjacentText(position,String(html));}
    }
    for (const prop of ['clientWidth','clientHeight','offsetWidth','offsetHeight','offsetTop','offsetLeft','scrollWidth','scrollHeight','scrollTop','scrollLeft']) Object.defineProperty(Element.prototype, prop, {configurable:true,get() { return this['_' + prop] || 0; }, set(v) { this['_' + prop] = Number(v); }});
    // Keep constructor identity per element family: a span is never a div.
    const htmlConstructors=Object.create(null), htmlTags=new Map();
    for(const [name,tags]of Object.entries({
        HTMLAnchorElement:'a',HTMLAreaElement:'area',HTMLAudioElement:'audio',HTMLBaseElement:'base',HTMLBodyElement:'body',HTMLBRElement:'br',HTMLButtonElement:'button',HTMLCanvasElement:'canvas',HTMLDataElement:'data',HTMLDataListElement:'datalist',HTMLDetailsElement:'details',HTMLDialogElement:'dialog',HTMLDivElement:'div',HTMLDListElement:'dl',HTMLEmbedElement:'embed',HTMLFieldSetElement:'fieldset',HTMLFormElement:'form',HTMLHeadElement:'head',HTMLHeadingElement:'h1 h2 h3 h4 h5 h6',HTMLHRElement:'hr',HTMLHtmlElement:'html',HTMLIFrameElement:'iframe',HTMLImageElement:'img',HTMLInputElement:'input',HTMLLabelElement:'label',HTMLLegendElement:'legend',HTMLLIElement:'li',HTMLLinkElement:'link',HTMLMapElement:'map',HTMLMediaElement:'',HTMLMenuElement:'menu',HTMLMetaElement:'meta',HTMLMeterElement:'meter',HTMLModElement:'del ins',HTMLObjectElement:'object',HTMLOListElement:'ol',HTMLOptGroupElement:'optgroup',HTMLOptionElement:'option',HTMLOutputElement:'output',HTMLParagraphElement:'p',HTMLParamElement:'param',HTMLPictureElement:'picture',HTMLPreElement:'pre',HTMLProgressElement:'progress',HTMLQuoteElement:'q blockquote',HTMLScriptElement:'script',HTMLSelectElement:'select',HTMLSourceElement:'source',HTMLSpanElement:'span',HTMLStyleElement:'style',HTMLTableCaptionElement:'caption',HTMLTableCellElement:'td th',HTMLTableColElement:'col colgroup',HTMLTableElement:'table',HTMLTableRowElement:'tr',HTMLTableSectionElement:'thead tbody tfoot',HTMLTextAreaElement:'textarea',HTMLTimeElement:'time',HTMLTitleElement:'title',HTMLTrackElement:'track',HTMLUListElement:'ul',HTMLVideoElement:'video'
    })){
        const first=tags.split(' ')[0]||'media';
        const Constructor=class extends Element {constructor(owner=null,tag=first){super(tag,owner||globalThis.document);}};
        Object.defineProperty(Constructor,'name',{value:name});htmlConstructors[name]=Constructor;
        for(const tag of tags.split(' ').filter(Boolean))htmlTags.set(tag,Constructor);
    }
    for(const name of ['HTMLAudioElement','HTMLVideoElement'])Object.setPrototypeOf(htmlConstructors[name].prototype,htmlConstructors.HTMLMediaElement.prototype);
    class SVGElement extends Element {}
    class HTMLTemplateElement extends Element {constructor(owner){super('template',owner||globalThis.document);this.content=new DocumentFragment(this.ownerDocument);}get innerHTML(){return this._html||'';}set innerHTML(value){this._html=String(value);this.content.replaceChildren();note('unparsed-html','template');}cloneNode(deep=false){const node=new HTMLTemplateElement(this.ownerDocument);node._html=this._html;for(const [k,v]of Object.entries(this._attrs))node.setAttribute(k,v);if(deep)for(const c of this.content.childNodes)node.content.appendChild(c.cloneNode(true));return node;} }
    class HTMLSlotElement extends Element {constructor(owner){super('slot',owner||globalThis.document);}assignedNodes(){return [];}assignedElements(){return [];} }
    class CustomElementRegistry {
        constructor(){this._definitions=new Map();this._pending=new Map();}
        define(name,constructor,options={}){name=String(name);if(!name.includes('-')||this._definitions.has(name))throw Error('NotSupportedError');if(typeof constructor!=='function')throw TypeError('Expected constructor');this._definitions.set(name,constructor);note('custom-element-registered',name);this._pending.get(name)?.forEach(resolve=>resolve(constructor));this._pending.delete(name);}
        get(name){return this._definitions.get(String(name));}getName(constructor){for(const [name,value]of this._definitions)if(value===constructor)return name;return null;}
        whenDefined(name){name=String(name);if(this._definitions.has(name))return Promise.resolve(this._definitions.get(name));return new Promise(resolve=>{const pending=this._pending.get(name)||[];pending.push(resolve);this._pending.set(name,pending);});}
        upgrade(root){note('custom-element-upgrade-skipped',root?.nodeName||'unknown');}
    }
    const NodeFilter={FILTER_ACCEPT:1,FILTER_REJECT:2,FILTER_SKIP:3,SHOW_ALL:0xffffffff,SHOW_ELEMENT:1,SHOW_ATTRIBUTE:2,SHOW_TEXT:4,SHOW_CDATA_SECTION:8,SHOW_COMMENT:128,SHOW_DOCUMENT:256,SHOW_DOCUMENT_TYPE:512,SHOW_DOCUMENT_FRAGMENT:1024};
    class TreeWalker {
        constructor(root,whatToShow=0xffffffff,filter=null){this.root=root;this.whatToShow=whatToShow>>>0;this.filter=filter;this.currentNode=root;}
        _test(node){if(!(this.whatToShow&(1<<(node.nodeType-1))))return 3;return this.filter?(typeof this.filter==='function'?this.filter(node):this.filter.acceptNode(node)):1;}
        _nodes(){const out=[];const walk=node=>{for(const child of node.childNodes){const status=this._test(child);if(status===1)out.push(child);if(status!==2)walk(child);}};walk(this.root);return out;}
        nextNode(){const nodes=this._nodes();const index=this.currentNode===this.root?-1:nodes.indexOf(this.currentNode);const next=nodes[index+1];if(next)this.currentNode=next;return next||null;}
        previousNode(){const nodes=this._nodes(),index=nodes.indexOf(this.currentNode);const previous=index>0?nodes[index-1]:null;if(previous)this.currentNode=previous;return previous;}
        parentNode(){for(let n=this.currentNode.parentNode;n;n=n.parentNode){if(this._test(n)===1){this.currentNode=n;return n;}if(n===this.root)break;}return null;}
        firstChild(){const parent=this.currentNode;const child=this._nodes().find(n=>n.parentNode===parent);if(child)this.currentNode=child;return child||null;}
        lastChild(){const parent=this.currentNode;const child=this._nodes().filter(n=>n.parentNode===parent).at(-1);if(child)this.currentNode=child;return child||null;}
        nextSibling(){let n=this.currentNode.nextSibling;while(n&&this._test(n)!==1)n=n.nextSibling;if(n)this.currentNode=n;return n||null;}
        previousSibling(){let n=this.currentNode.previousSibling;while(n&&this._test(n)!==1)n=n.previousSibling;if(n)this.currentNode=n;return n||null;}
    }
    class Document extends Node {
        constructor() { super('#document',9); this.ownerDocument = null; this._root = null; this.readyState = 'loading'; this.visibilityState = 'visible'; this.hidden = false; this.currentScript = null; this.activeElement = null; }
        get documentElement() { if (!this._root) this.appendChild(this._root = this.createElement('html')); return this._root; }
        get head() { return this._head ??= this.documentElement.appendChild(this.createElement('head')); }
        get body() { return this._body ??= this.documentElement.appendChild(this.createElement('body')); }
        get implementation(){return this._implementation??={hasFeature:()=>false,createHTMLDocument:title=>{const d=new Document();if(title)d.head.appendChild(d.createElement('title')).textContent=String(title);return d;},createDocument:(namespace,name)=>{const d=new Document();if(name)d.appendChild(d._root=d.createElementNS(namespace,name));return d;}};}
        createTreeWalker(root,whatToShow,filter){return new TreeWalker(root,whatToShow,filter);}
        createEvent(type) {type=String(type).toLowerCase();return type==='customevent'?new CustomEvent(''):type==='mouseevent'||type==='mouseevents'?new MouseEvent(''):type==='keyboardevent'?new KeyboardEvent(''):new Event('');}
        createElement(tag) {tag=String(tag).toLowerCase();if(tag==='template')return new HTMLTemplateElement(this);if(tag==='slot')return new HTMLSlotElement(this);const Constructor=htmlTags.get(tag);return Constructor?new Constructor(this,tag):new Element(tag,this);}
        createElementNS(ns, tag) {const n=ns==='http://www.w3.org/2000/svg'?new SVGElement(tag,this):this.createElement(tag);n.namespaceURI=ns;return n;}
        createTextNode(text) {return new Text(text,this);}
        createCDATASection(text){return new CDATASection(text,this);}
        createProcessingInstruction(target,data){return new ProcessingInstruction(target,data,this);}
        createComment(text) {return new Comment(text,this);}
        createDocumentFragment() {return new DocumentFragment(this);}
        getElementById(id) { return this.querySelector('#' + id); }
        querySelector(selector) { return query(this, selector, true); }
        querySelectorAll(selector) { this.documentElement; return Element.prototype.querySelectorAll.call(this,selector); }
        getElementsByTagName(tag) { if (tag === 'head') this.head; if (tag === 'body') this.body; return this.querySelectorAll(tag); }
        getElementsByClassName(cls) { return this.querySelectorAll('.' + cls); }
        get URL() { return location.href; } get baseURI() { return location.href; }
    }
    for (const prop of ['src','href','type','name','value','rel','nonce']) Object.defineProperty(Element.prototype, prop, {
        configurable:true, get() { return this.getAttribute(prop) || ''; }, set(value) { this.setAttribute(prop, value); }
    });
    Object.defineProperty(Element.prototype, 'options', {get() { return this.children.filter(n => n.localName === 'option'); }});
    Object.defineProperty(Element.prototype, 'selectedIndex', {get() { return this.options.length ? 0 : -1; }});
    class Image extends htmlConstructors.HTMLImageElement {
        constructor(width = 0, height = 0) { super(document,'img'); this.width = width; this.height = height; this.complete = false; this.naturalWidth = 0; this.naturalHeight = 0; }
        set src(value) { this.setAttribute('src', value); note('image-not-fetched', String(value)); }
        get src() { return this.getAttribute('src') || ''; }
    }
    class Window {}
    const document = new Document();
    Object.assign(globalThis, {Window,NodeFilter,TreeWalker,MutationObserver,Event, CustomEvent, MouseEvent, KeyboardEvent, EventTarget, Node, CharacterData, Text, Comment, CDATASection, ProcessingInstruction, NodeList, HTMLCollection, DocumentFragment, ShadowRoot, Element, HTMLElement:Element, SVGElement, HTMLTemplateElement, HTMLSlotElement, CustomElementRegistry, CSSStyleDeclaration, Document, HTMLDocument:Document, Image, document});
    Object.assign(globalThis,htmlConstructors);
    // Polyfills patch Window.prototype; those methods must be visible on window.
    Object.setPrototypeOf(Window.prototype,Object.getPrototypeOf(globalThis));
    Object.setPrototypeOf(globalThis,Window.prototype);
    globalThis.customElements=new CustomElementRegistry();
    const windowEvents = new EventTarget();
    for (const method of ['addEventListener','removeEventListener','dispatchEvent']) {Window.prototype[method]=function(...args){return windowEvents[method](...args);};globalThis[method] = windowEvents[method].bind(windowEvents);}
    globalThis.getComputedStyle = node => new Proxy(node.style, {get(target,key) { if (key === 'then') return undefined; return target[key] ?? ''; }});
    let lifecycleMode='legacy';
    globalThis.__domReport = () => ({allocatedNodes:allocated, notes, lifecycleMode});
    globalThis.__domReady = (mode='legacy') => {
        lifecycleMode=mode;
        document.readyState='interactive';
        if(mode!=='chrome') {
            document.dispatchEvent(new Event('DOMContentLoaded',{bubbles:true}));
            document.readyState='complete';dispatchEvent(new Event('load'));return;
        }
        // Experimental Chrome-like ordering. Each task permits promise jobs to run.
        document.dispatchEvent(new Event('readystatechange'));
        setTimeout(()=>{
            const event=new Event('DOMContentLoaded',{bubbles:true});
            document.dispatchEvent(event);
            if(!event._stopped)windowEvents.dispatchEvent(event);
            setTimeout(()=>{
                document.readyState='complete';document.dispatchEvent(new Event('readystatechange'));
                dispatchEvent(new Event('load'));
                dispatchEvent(new Event('pageshow',{persisted:false}));
            },0);
        },0);
    };
})();
