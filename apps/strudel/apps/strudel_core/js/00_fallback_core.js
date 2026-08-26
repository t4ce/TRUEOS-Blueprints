/*
 * Independently written emergency temporal kernel.
 *
 * This is NOT copied from Strudel and is NOT represented as @strudel/core.
 * It only covers the tiny ABI needed to prove:
 *   sequence -> queryArc -> timed haps -> TRUEOS PCM.
 *
 * The upstream vendor tool installs the real Strudel pattern slice as
 * globalThis.StrudelCore; in that case this file does nothing.
 */
(function installFallback(G) {
  "use strict";
  if (G.StrudelCore) return;

  function span(begin, end) {
    return { begin: Number(begin), end: Number(end) };
  }

  function intersect(begin, end, queryBegin, queryEnd) {
    const clippedBegin = Math.max(begin, queryBegin);
    const clippedEnd = Math.min(end, queryEnd);
    return clippedEnd > clippedBegin ? span(clippedBegin, clippedEnd) : null;
  }

  function mapSpan(input, fn) {
    return input ? span(fn(input.begin), fn(input.end)) : undefined;
  }

  class Pattern {
    constructor(query) {
      this._query = query;
      this._Pattern = true;
    }

    queryArc(begin, end) {
      begin = Number(begin);
      end = Number(end);
      if (!Number.isFinite(begin) || !Number.isFinite(end) || end <= begin) return [];
      return this._query(begin, end);
    }

    withValue(fn) {
      const source = this;
      return new Pattern((begin, end) =>
        source.queryArc(begin, end).map((hap) => ({
          whole: hap.whole,
          part: hap.part,
          value: fn(hap.value),
        })),
      );
    }

    map(fn) {
      return this.withValue(fn);
    }

    fast(factor) {
      factor = Number(factor);
      if (!(factor > 0)) throw new RangeError("fast factor must be positive");
      const source = this;
      return new Pattern((begin, end) =>
        source.queryArc(begin * factor, end * factor).map((hap) => ({
          whole: mapSpan(hap.whole, (time) => time / factor),
          part: mapSpan(hap.part, (time) => time / factor),
          value: hap.value,
        })),
      );
    }

    slow(factor) {
      factor = Number(factor);
      if (!(factor > 0)) throw new RangeError("slow factor must be positive");
      return this.fast(1 / factor);
    }

    stack(...others) {
      return stack(this, ...others);
    }
  }

  const silence = new Pattern(() => []);

  function isPattern(value) {
    return Boolean(value && value._Pattern && typeof value.queryArc === "function");
  }

  function pure(value) {
    return sequence(value);
  }

  function emitItem(item, slotBegin, slotEnd, queryBegin, queryEnd, output) {
    if (item === null || item === undefined) return;

    if (Array.isArray(item)) {
      if (item.length === 0) return;
      const width = (slotEnd - slotBegin) / item.length;
      for (let index = 0; index < item.length; index += 1) {
        emitItem(
          item[index],
          slotBegin + width * index,
          slotBegin + width * (index + 1),
          queryBegin,
          queryEnd,
          output,
        );
      }
      return;
    }

    if (isPattern(item)) {
      const width = slotEnd - slotBegin;
      if (!(width > 0)) return;
      const innerBegin = (Math.max(queryBegin, slotBegin) - slotBegin) / width;
      const innerEnd = (Math.min(queryEnd, slotEnd) - slotBegin) / width;
      for (const hap of item.queryArc(innerBegin, innerEnd)) {
        const whole = hap.whole
          ? span(slotBegin + hap.whole.begin * width, slotBegin + hap.whole.end * width)
          : undefined;
        const part = hap.part
          ? span(slotBegin + hap.part.begin * width, slotBegin + hap.part.end * width)
          : undefined;
        if (part && part.end > part.begin) output.push({ whole, part, value: hap.value });
      }
      return;
    }

    const part = intersect(slotBegin, slotEnd, queryBegin, queryEnd);
    if (part) {
      output.push({
        whole: span(slotBegin, slotEnd),
        part,
        value: item,
      });
    }
  }

  function sequence(...items) {
    if (items.length === 0) return silence;
    return new Pattern((queryBegin, queryEnd) => {
      const output = [];
      const firstCycle = Math.floor(queryBegin);
      const finalCycle = Math.ceil(queryEnd);
      const topWidth = 1 / items.length;

      for (let cycle = firstCycle; cycle < finalCycle; cycle += 1) {
        for (let index = 0; index < items.length; index += 1) {
          emitItem(
            items[index],
            cycle + topWidth * index,
            cycle + topWidth * (index + 1),
            queryBegin,
            queryEnd,
            output,
          );
        }
      }
      return output;
    });
  }

  function stack(...patterns) {
    const sources = patterns.flat().map((value) => (isPattern(value) ? value : sequence(value)));
    return new Pattern((begin, end) => sources.flatMap((pattern) => pattern.queryArc(begin, end)));
  }

  G.StrudelCoreFallback = Object.freeze({
    Pattern,
    pure,
    silence,
    sequence,
    seq: sequence,
    fastcat: sequence,
    stack,
  });
})(globalThis);
