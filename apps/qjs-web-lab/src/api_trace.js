// Count implemented host API calls, not arbitrary JavaScript function calls.
(() => {
    const counts = Object.create(null), recent = []; let total = 0, phase = 'bootstrap';
    const brief = x => x === null ? null : ['string','number','boolean','undefined'].includes(typeof x) ? String(x).slice(0,120) : '[' + typeof x + ']';
    globalThis.__traceCall = (api, args = []) => { total++; counts[api] = (counts[api] || 0) + 1; recent.push({sequence:total, phase, api, args:Array.from(args).slice(0,4).map(brief)}); if (recent.length > 200) recent.shift(); };
    globalThis.__tracePhase = value => { phase = value; };
    globalThis.__traceSnapshot = () => ({total, counts:{...counts}, recent:[...recent]});
    globalThis.__traceObject = (object, prefix) => {
        if (!object) return;
        for (const key of Object.getOwnPropertyNames(object)) {
            if (key === 'constructor' || key.startsWith('_')) continue;
            const d = Object.getOwnPropertyDescriptor(object,key); if (!d.configurable) continue;
            const wrap = (fn, label) => function(...args) { __traceCall(label,args); return Reflect.apply(fn,this,args); };
            if (typeof d.value === 'function') d.value = wrap(d.value, prefix+'.'+key);
            if (d.get) d.get = wrap(d.get, prefix+'.'+key+'.get');
            if (d.set) d.set = wrap(d.set, prefix+'.'+key+'.set');
            Object.defineProperty(object,key,d);
        }
    };
})();
