// Headless approximation: persistent tree, lazy selector placeholders, no rendering.
(() => {
    const notes = [], synthetic = new Map(); let allocated = 0;
    const note = (kind, detail) => { if (notes.length < 500) notes.push({kind, detail}); };
    class Event {
        constructor(type, init = {}) { this.type = String(type); Object.assign(this, init); this.defaultPrevented = false; }
        preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
        stopPropagation() { this._stopped = true; }
        stopImmediatePropagation() { this._immediate = this._stopped = true; }
    }
    class CustomEvent extends Event { constructor(type, init = {}) { super(type, init); this.detail = init.detail ?? null; } }
    class EventTarget {
        constructor() { this._listeners = new Map(); }
        addEventListener(type, fn, options = {}) {
            if (!fn) return; type = String(type);
            const list = this._listeners.get(type) || [];
            if (!list.some(x => x.fn === fn)) list.push({fn, once: !!options?.once});
            this._listeners.set(type, list);
        }
        removeEventListener(type, fn) { this._listeners.set(String(type), (this._listeners.get(String(type)) || []).filter(x => x.fn !== fn)); }
        dispatchEvent(event) {
            event.target ??= this; event.currentTarget = this;
            for (const entry of [...(this._listeners.get(event.type) || [])]) {
                if (entry.once) this.removeEventListener(event.type, entry.fn);
                if (typeof entry.fn === 'function') entry.fn.call(this, event); else entry.fn.handleEvent(event);
                if (event._immediate) break;
            }
            this['on' + event.type]?.call(this, event);
            if (event.bubbles && !event._stopped && this.parentNode) this.parentNode.dispatchEvent(event);
            return !event.defaultPrevented;
        }
    }
    class Node extends EventTarget {
        constructor(name, type = 1, owner = null) { super(); allocated++; this.nodeName = name; this.nodeType = type; this.ownerDocument = owner; this.parentNode = null; this.childNodes = []; this.nodeValue = type === 3 || type === 8 ? '' : null; }
        get firstChild() { return this.childNodes[0] || null; }
        get lastChild() { return this.childNodes.at(-1) || null; }
        get nextSibling() { return this.parentNode?.childNodes[this.parentNode.childNodes.indexOf(this) + 1] || null; }
        get previousSibling() { return this.parentNode?.childNodes[this.parentNode.childNodes.indexOf(this) - 1] || null; }
        get parentElement() { return this.parentNode?.nodeType === 1 ? this.parentNode : null; }
        get isConnected() { return this.nodeType === 9 || !!this.parentNode?.isConnected; }
        get textContent() { return this.nodeValue ?? this.childNodes.map(x => x.textContent).join(''); }
        set textContent(value) { if (this.nodeType === 3 || this.nodeType === 8) this.nodeValue = String(value ?? ''); else { this.replaceChildren(); if (value != null && value !== '') this.appendChild(this.ownerDocument.createTextNode(String(value))); } }
        appendChild(node) { return this.insertBefore(node, null); }
        insertBefore(node, before) {
            if (node === this || node.contains(this)) throw new Error('HierarchyRequestError');
            if (before !== null && before.parentNode !== this) throw new Error('NotFoundError');
            if (node === before) return node;
            if (node.nodeType === 11) { for (const child of [...node.childNodes]) this.insertBefore(child, before); return node; }
            node.remove(); const index = before === null ? this.childNodes.length : this.childNodes.indexOf(before);
            this.childNodes.splice(index, 0, node); node.parentNode = this; return node;
        }
        removeChild(node) { const i = this.childNodes.indexOf(node); if (i < 0) throw new Error('NotFoundError'); this.childNodes.splice(i, 1); node.parentNode = null; return node; }
        replaceChild(node, old) { this.insertBefore(node, old); this.removeChild(old); return old; }
        remove() { this.parentNode?.removeChild(this); }
        contains(node) { return node === this || this.childNodes.some(x => x.contains(node)); }
        hasChildNodes() { return !!this.childNodes.length; }
        append(...nodes) { for (const n of nodes) this.appendChild(typeof n === 'string' ? this.ownerDocument.createTextNode(n) : n); }
        prepend(...nodes) { for (const n of nodes.reverse()) this.insertBefore(typeof n === 'string' ? this.ownerDocument.createTextNode(n) : n, this.firstChild); }
        replaceChildren(...nodes) { for (const child of [...this.childNodes]) this.removeChild(child); this.append(...nodes); }
        cloneNode(deep = false) { const n = this.nodeType === 1 ? this.ownerDocument.createElement(this.localName) : new Node(this.nodeName, this.nodeType, this.ownerDocument); n.nodeValue = this.nodeValue; if (this.attributes) for (const [k,v] of Object.entries(this._attrs)) n.setAttribute(k,v); if (deep) for (const c of this.childNodes) n.appendChild(c.cloneNode(true)); return n; }
    }
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
        selector = String(selector); const found = descendants(root).find(n => matches(n, selector)); if (found || !lazy) return found || null;
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
        get attributes() { return Object.entries(this._attrs).map(([name,value]) => ({name,value})); }
        setAttribute(name, value) { this._attrs[String(name)] = String(value); }
        getAttribute(name) { return this._attrs[String(name)] ?? null; }
        hasAttribute(name) { return Object.hasOwn(this._attrs, String(name)); }
        removeAttribute(name) { delete this._attrs[String(name)]; }
        get id() { return this.getAttribute('id') || ''; } set id(v) { this.setAttribute('id',v); }
        get className() { return this.getAttribute('class') || ''; } set className(v) { this.setAttribute('class',v); }
        get classList() { const el = this; return { contains: x => el.className.split(/\s+/).includes(x), add(...xs) { el.className = [...new Set([...el.className.split(/\s+/).filter(Boolean), ...xs])].join(' '); }, remove(...xs) { el.className = el.className.split(/\s+/).filter(x => !xs.includes(x)).join(' '); }, toggle(x, force) { const on = force ?? !this.contains(x); if (on) this.add(x); else this.remove(x); return on; } }; }
        get style() { return this._style ??= {cssText:'', setProperty(k,v) { this[k] = String(v); }, getPropertyValue(k) { return this[k] || ''; }, removeProperty(k) { const v = this[k] || ''; delete this[k]; return v; }}; }
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
        attachShadow() { return this.shadowRoot = this.ownerDocument.createDocumentFragment(); }
    }
    for (const prop of ['clientWidth','clientHeight','offsetWidth','offsetHeight','offsetTop','offsetLeft','scrollWidth','scrollHeight','scrollTop','scrollLeft']) Object.defineProperty(Element.prototype, prop, {get() { return this['_' + prop] || 0; }, set(v) { this['_' + prop] = Number(v); }});
    class Document extends Node {
        constructor() { super('#document',9); this.ownerDocument = null; this._root = null; this.readyState = 'loading'; this.visibilityState = 'visible'; this.hidden = false; this.currentScript = null; this.activeElement = null; }
        get documentElement() { if (!this._root) this.appendChild(this._root = this.createElement('html')); return this._root; }
        get head() { return this._head ??= this.documentElement.appendChild(this.createElement('head')); }
        get body() { return this._body ??= this.documentElement.appendChild(this.createElement('body')); }
        createElement(tag) { return new Element(tag, this); }
        createElementNS(ns, tag) { const n = this.createElement(tag); n.namespaceURI = ns; return n; }
        createTextNode(text) { const n = new Node('#text',3,this); n.nodeValue = String(text); return n; }
        createComment(text) { const n = new Node('#comment',8,this); n.nodeValue = String(text); return n; }
        createDocumentFragment() { return new Node('#document-fragment',11,this); }
        getElementById(id) { return this.querySelector('#' + id); }
        querySelector(selector) { return query(this, selector, true); }
        querySelectorAll(selector) { this.documentElement; return Element.prototype.querySelectorAll.call(this,selector); }
        getElementsByTagName(tag) { if (tag === 'head') this.head; if (tag === 'body') this.body; return this.querySelectorAll(tag); }
        getElementsByClassName(cls) { return this.querySelectorAll('.' + cls); }
        get URL() { return location.href; } get baseURI() { return location.href; }
    }
    for (const prop of ['src','href','type','name','value','rel','nonce']) Object.defineProperty(Element.prototype, prop, {
        get() { return this.getAttribute(prop) || ''; }, set(value) { this.setAttribute(prop, value); }
    });
    Object.defineProperty(Element.prototype, 'options', {get() { return this.children.filter(n => n.localName === 'option'); }});
    Object.defineProperty(Element.prototype, 'selectedIndex', {get() { return this.options.length ? 0 : -1; }});
    class Image extends Element {
        constructor(width = 0, height = 0) { super('img', document); this.width = width; this.height = height; this.complete = false; this.naturalWidth = 0; this.naturalHeight = 0; }
        set src(value) { this.setAttribute('src', value); note('image-not-fetched', String(value)); }
        get src() { return this.getAttribute('src') || ''; }
    }
    const document = new Document();
    Object.assign(globalThis, {Event, CustomEvent, EventTarget, Node, Element, HTMLElement:Element, Document, Image, document});
    const windowEvents = new EventTarget();
    for (const method of ['addEventListener','removeEventListener','dispatchEvent']) globalThis[method] = windowEvents[method].bind(windowEvents);
    globalThis.getComputedStyle = node => new Proxy(node.style, {get(target,key) { if (key === 'then') return undefined; return target[key] ?? ''; }});
    globalThis.__domReport = () => ({allocatedNodes:allocated, notes});
    globalThis.__domReady = () => { document.readyState = 'interactive'; document.dispatchEvent(new Event('DOMContentLoaded', {bubbles:true})); document.readyState = 'complete'; dispatchEvent(new Event('load')); };
})();
