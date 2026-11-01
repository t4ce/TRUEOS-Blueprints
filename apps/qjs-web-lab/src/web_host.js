// Explicit initial host surface. Missing browser APIs remain missing.
(() => {
    const logs = [], timers = new Map();
    let nextId = 1, droppedLogs = 0, virtualTime = 0;
    const stringify = value => { try { return typeof value === 'string' ? value : String(value); } catch { return '<unprintable>'; } };
    globalThis.console = Object.fromEntries(['log', 'info', 'debug', 'warn', 'error', 'trace', 'assert'].map(level => [level, (...args) => {
        if (level === 'assert' && args.shift()) return;
        if (logs.length >= 1000) { droppedLogs++; return; }
        logs.push({level, message: args.map(stringify).join(' ')});
    }]));
    globalThis.window = globalThis;
    globalThis.self = globalThis;
    globalThis.navigator = Object.freeze({userAgent:'TRUEOS-QJS-Web-Lab/0.1', platform:'Linux x86_64', language:'en-US', languages:['en-US'], cookieEnabled:false, hardwareConcurrency:1, onLine:false, maxTouchPoints:0});
    class Storage {
        constructor() { this._data = new Map(); }
        get length() { return this._data.size; }
        key(index) { return [...this._data.keys()][Number(index)] ?? null; }
        getItem(key) { return this._data.get(String(key)) ?? null; }
        setItem(key,value) { this._data.set(String(key),String(value)); }
        removeItem(key) { this._data.delete(String(key)); }
        clear() { this._data.clear(); }
    }
    globalThis.localStorage = new Storage();
    globalThis.sessionStorage = new Storage();
    globalThis.location = Object.freeze({...__page.location, toString() { return this.href; }});
    globalThis.setTimeout = (callback, delay = 0, ...args) => {
        if (typeof callback !== 'function') throw new TypeError('String timers are not implemented');
        const id = nextId++; timers.set(id, {callback, args, at: virtualTime + Math.max(0, Number(delay) || 0)}); return id;
    };
    globalThis.clearTimeout = id => timers.delete(id);
    globalThis.queueMicrotask = callback => { if (typeof callback !== 'function') throw new TypeError('Expected function'); Promise.resolve().then(callback); };
    globalThis.__pumpTimers = limit => {
        // Virtual time: never wait on a website's real delays. Intervals are not implemented.
        for (let i = 0; i < limit && timers.size; i++) {
            const [id, timer] = [...timers].sort((a,b) => a[1].at - b[1].at || a[0] - b[0])[0];
            timers.delete(id); virtualTime = timer.at; timer.callback(...timer.args);
        }
    };
    globalThis.__hostReport = () => ({logs, droppedLogs, pendingTimers: timers.size, virtualTime, dom: globalThis.__domReport?.()});
})();
