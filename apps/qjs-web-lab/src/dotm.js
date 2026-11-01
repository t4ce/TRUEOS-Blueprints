// DistanceOnTheMonitor keeps physical monitor distance separate from glyph ratios.
// Lab reference policy: one numeric DotM is one centimetre. This is not a device
// dot, a CSS pixel, a raster cell, or a DPI estimate. Display conversion belongs
// to the existing kernel's monitor-calibrated presentation boundary.
(() => {
    'use strict';
    const scalar = value => {
        if (typeof value !== 'number' || !Number.isFinite(value)) throw new TypeError('DotM requires a finite number');
        return value;
    };
    class DistanceOnTheMonitor {
        #centimeters;
        constructor(value) { this.#centimeters = scalar(value); Object.freeze(this); }
        static fromCentimeters(value) { return new DistanceOnTheMonitor(value); }
        static fromMillimeters(value) { return new DistanceOnTheMonitor(scalar(value) / 10); }
        static fromNormalized(value, referenceDistance) { return required(referenceDistance).scale(value); }
        static isDistance(value) { return value instanceof DistanceOnTheMonitor; }
        toCentimeters() { return this.#centimeters; }
        toMillimeters() { return this.#centimeters * 10; }
        // Explicit unwrap at numeric browser API boundaries; no implicit unit loss.
        toNumber() { return this.#centimeters; }
        add(other) { return new DistanceOnTheMonitor(this.#centimeters + required(other).#centimeters); }
        subtract(other) { return new DistanceOnTheMonitor(this.#centimeters - required(other).#centimeters); }
        scale(factor) { return new DistanceOnTheMonitor(this.#centimeters * scalar(factor)); }
        ratio(referenceDistance) {
            const divisor = required(referenceDistance).#centimeters;
            if (divisor === 0) throw new RangeError('DotM ratio requires a nonzero reference distance');
            return this.#centimeters / divisor;
        }
        equals(other) { return other instanceof DistanceOnTheMonitor && this.#centimeters === other.#centimeters; }
        toJSON() { return {value:this.#centimeters, unit:'DotM'}; }
        toString() { return this.#centimeters + ' DotM'; }
        valueOf() { throw new TypeError('Use DotM.toNumber() explicitly at numeric API boundaries'); }
        [Symbol.toPrimitive](hint) { if (hint === 'string') return this.toString(); return this.valueOf(); }
    }
    function required(value) {
        if (!(value instanceof DistanceOnTheMonitor)) throw new TypeError('Expected DistanceOnTheMonitor');
        return value;
    }
    // Source geometry: TRUEOS/vendor/microfont/src/lib.rs FWIDTH/FHEIGHT;
    // regular text: TRUEOS-Buildins/hull/src/layout.rs estimate_text_width;
    // baseline: TRUEOS/src/r/services/hull_font_service.rs prepare_blocking.
    const normalized = Object.freeze({glyphAdvance:6/11, lineHeight:1, ascent:9/11, descent:2/11});
    // 1.1 cm reference em gives the user's stated 0.6 cm natural advance. This
    // reference does not force every requested font size to that physical size.
    const referenceEm = DistanceOnTheMonitor.fromCentimeters(1.1);
    const controlNames = ['NUL','SOH','STX','ETX','EOT','ENQ','ACK','BEL','BS','HT','LF','VT','FF','CR','SO','SI','DLE','DC1','DC2','DC3','DC4','NAK','SYN','ETB','CAN','EM','SUB','ESC','FS','GS','RS','US','SP'];
    // Optional representation expansion mirrors microfont::special_cells. Hull's
    // normal text bridge uses one cell per Unicode scalar instead, so this is
    // opt-in and must not turn ordinary page spaces into visible "SP" labels.
    const specialCellCount = character => {
        const code = character.codePointAt(0);
        if (code <= 32) return controlNames[code].length;
        if (code === 127) return 3;
        if (character === '\u2328') return 2;
        if (character === '\u{1f5fa}') return 3;
        return 1;
    };
    function countCells(text, expandSpecialCells = false) {
        return Array.from(String(text)).reduce((sum, character) => sum + (expandSpecialCells ? specialCellCount(character) : 1), 0);
    }
    function measure(text, em = referenceEm, options = {}) {
        em = required(em);
        if (em.toNumber() <= 0) throw new RangeError('Microfont em must be positive');
        const lineHeight = options.lineHeight === undefined ? em : required(options.lineHeight);
        if (lineHeight.toNumber() < 0) throw new RangeError('Microfont line height must be nonnegative');
        const lines = String(text).replace(/\r\n?/g,'\n').split('\n');
        const columns = lines.map(line => countCells(line, options.expandSpecialCells === true));
        const glyphAdvance = em.scale(normalized.glyphAdvance);
        return Object.freeze({
            width:glyphAdvance.scale(columns.reduce((maximum, count) => Math.max(maximum, count), 0)),
            height:lineHeight.scale(lines.length),
            lineHeight,
            glyphAdvance,
            advance:glyphAdvance,
            ascent:em.scale(normalized.ascent),
            descent:em.scale(normalized.descent),
            em,
            lineCount:lines.length,
            columns:Object.freeze(columns),
        });
    }
    globalThis.DistanceOnTheMonitor = DistanceOnTheMonitor;
    globalThis.DotM = DistanceOnTheMonitor;
    globalThis.MicrofontMetrics = Object.freeze({normalized, referenceEm, countCells, specialCellCount, measure});
})();
