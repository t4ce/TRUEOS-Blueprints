// Private build-time alias for packages that import the @strudel/core barrel.
// Re-export only the non-audio modules needed by mini and tonal.  In
// particular, this prevents their barrel import from pulling UI/clock code.
export * from '@strudel/core/pattern.mjs';
export { default as Fraction } from '@strudel/core/fraction.mjs';
export { errorLogger, logger } from '@strudel/core/logger.mjs';
export * from '@strudel/core/util.mjs';
