/*
 * Deliberately import the pattern module directly, not @strudel/core's package
 * root. This keeps editor/repl/UI/browser exports out of the bare-metal bundle.
 */
import {
  Pattern,
  pure,
  silence,
  sequence,
  seq,
  fastcat,
  slowcat,
  stack,
} from '@strudel/core/pattern.mjs';

globalThis.StrudelCore = Object.freeze({
  Pattern,
  pure,
  silence,
  sequence,
  seq,
  fastcat,
  slowcat,
  stack,
});
globalThis.__TRUEOS_UPSTREAM_STRUDEL_PRESENT = true;
