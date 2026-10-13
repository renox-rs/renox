// A check on the JavaScript Renox ships, without a browser: two function
// declarations with one name in the same file silently replace each other.
// renox-grid.js had two `save`s, so the column menu called the inline
// editor's and threw (found by grid.test.mjs).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ROOT } from './lib/app.mjs';

const FILES = [
  'crates/renox-core/assets/renox-ui.js',
  'crates/renox-core/assets/renox-ui-chart.js',
  'crates/renox-core/assets/renox-ui-wizard.js',
  'crates/renox-core/assets/renox-ui-repeater.js',
  'crates/renox-core/assets/renox-ui-tags.js',
  'crates/renox-core/assets/renox-grid.js',
  'crates/renox-core/assets/renox-live.js',
  'crates/renox-editors/assets/editors.js',
];

/** renox.js lives in a Rust string (crates/renox-core/src/assets.rs). */
function renoxJs() {
  const rust = readFileSync(join(ROOT, 'crates/renox-core/src/assets.rs'), 'utf8');
  const start = rust.indexOf('const RENOX: &str = r#"');
  return rust.slice(start, rust.indexOf('"#;', start));
}

/**
 * Names declared twice at the file's top level (the shallowest indent: the
 * body of each file's one IIFE, or a module's top). Functions nested in
 * other functions are their own scopes and may reuse a name.
 */
function duplicates(source) {
  const declared = [...source.matchAll(/^([ \t]*)function ([A-Za-z_$][\w$]*)\s*\(/gm)].map(
    ([, indent, name]) => ({ depth: indent.length, name }),
  );
  const top = Math.min(...declared.map((d) => d.depth));
  const seen = new Map();
  for (const { depth, name } of declared) {
    if (depth === top) seen.set(name, (seen.get(name) || 0) + 1);
  }
  return [...seen].filter(([, n]) => n > 1).map(([name]) => name);
}

for (const file of FILES) {
  test(`${file} declares each function once`, () => {
    assert.deepEqual(duplicates(readFileSync(join(ROOT, file), 'utf8')), []);
  });
}

test('renox.js declares each function once', () => {
  assert.deepEqual(duplicates(renoxJs()), []);
});
