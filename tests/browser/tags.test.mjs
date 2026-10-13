// #508, #511, #512: the control-flow, scope and script tags in a real browser
// under CSP=strict, where an inline script or style runs only with the page's
// nonce: `<rx-script>` and `<rx-style>` (also pushed to a stack), `<template
// rx-if / rx-elif / rx-else / rx-for>` and `<rx-let>` render what they say.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

let browser;
let app;

before(async () => {
  app = await fixture({ env: { CSP: 'strict' } });
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await app?.stop();
});

test('rx-script and rx-style run under CSP=strict, in place and pushed', () => browser.with(async (page) => {
  await page.goto(`${app.url}/tags`);
  await page.waitFor(() => document.getElementById('out').textContent === 'inline ran', {
    message: 'the inline <rx-script> ran',
  });
  await page.waitFor(() => window.rxPushed === 'pushed ran', { message: 'the pushed module <rx-script> ran' });
  const state = await page.eval(() => ({
    color: getComputedStyle(document.getElementById('mark')).color,
    nonces: [...document.querySelectorAll('script[nonce], style[nonce]')].map((e) => e.getAttribute('nonce')),
    // htmx writes its own indicator <style> at run time, without a nonce; it is not ours.
    bare: [...document.querySelectorAll('script:not([nonce]):not([src]), style:not([nonce])')]
      .filter((e) => !e.textContent.startsWith('.htmx-indicator'))
      .map((e) => e.outerHTML.slice(0, 120)),
  }));
  assert.equal(state.color, 'rgb(255, 0, 0)', 'the <rx-style> rule applied');
  assert.ok(state.nonces.length >= 3, `three tags carry a nonce: ${state.nonces}`);
  assert.equal(new Set(state.nonces).size, 1, 'one nonce for the page');
  assert.deepEqual(state.bare, [], 'no inline code without a nonce');
  page.assertClean();
}));

test('template groups, elif, loops and rx-let render without wrappers', () => browser.with(async (page) => {
  await page.goto(`${app.url}/tags`);
  const box = await page.eval(() => ({
    text: document.getElementById('tags-box').textContent.replace(/\s+/g, ' ').trim(),
    two: document.getElementById('two')?.textContent,
    templates: document.querySelectorAll('#tags-box template').length,
    items: [...document.querySelectorAll('#tags-box .item')].map((e) => e.textContent),
  }));
  // `<template rx-for>` writes its children one after the other, with no text between.
  assert.equal(box.text, '20 two and text 1a2b');
  assert.equal(box.two, 'two');
  assert.equal(box.templates, 0, 'no <template> element is written');
  assert.deepEqual(box.items, ['1a', '2b']);
  page.assertClean();
}));
