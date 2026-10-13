// #422: dialogs announce what happened as `rx:<component>:<event>`
// (opened, closed, saved, failed, confirmed, cancelled), bubbling to document.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

let browser;
let app;
let strict;

before(async () => {
  app = await fixture();
  strict = await fixture({ env: { CSP: 'strict' } });
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await app?.stop();
  await strict?.stop();
});

const NAMES = ['opened', 'closed', 'saved', 'failed'].map((n) => `rx:action-sheet:${n}`)
  .concat(['opened', 'closed', 'confirmed', 'cancelled'].map((n) => `rx:confirm:${n}`));

const listen = (page) =>
  page.eval((names) => {
    window.__ev = [];
    for (const n of names) document.addEventListener(n, (e) => window.__ev.push([n, e.target.id, e.detail]));
  }, NAMES);

const events = (page) => page.eval(() => window.__ev);

test('an action sheet says opened, failed (422, stays open), saved then closed', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listen(page);
    await page.click('[data-rx-open="ev-sheet"]');
    await page.waitFor(() => document.querySelector('#ev-sheet').open);
    assert.deepEqual((await events(page))[0], ['rx:action-sheet:opened', 'ev-sheet', { id: 'ev-sheet' }]);

    await page.click('#ev-sheet button[type=submit]');
    await page.waitFor(() => window.__ev.some((e) => e[0] === 'rx:action-sheet:failed'));
    const failed = (await events(page)).find((e) => e[0] === 'rx:action-sheet:failed');
    assert.deepEqual(failed[2], { id: 'ev-sheet', status: 422 });
    assert.ok(await page.eval(() => document.querySelector('#ev-sheet').open), 'stays open');

    await page.type('#rx-from', 'Bandung');
    await page.type('#rx-to', 'Jakarta');
    await page.click('#ev-sheet button[type=submit]');
    await page.waitFor(() => window.__ev.some((e) => e[0] === 'rx:action-sheet:closed'));
    const names = (await events(page)).map((e) => e[0]);
    const tail = names.slice(names.indexOf('rx:action-sheet:saved'));
    assert.deepEqual(tail, ['rx:action-sheet:saved', 'rx:action-sheet:closed']);
    page.assertClean({ allow: [/422/] });
  }));

test('a confirm says cancelled then closed on Cancel', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listen(page);
    await page.click('[data-rx-open="ev-confirm"]');
    await page.waitFor(() => document.querySelector('#ev-confirm').open);
    await page.click('#ev-confirm [data-rx-close]');
    await page.waitFor(() => window.__ev.some((e) => e[0] === 'rx:confirm:closed'));
    assert.deepEqual((await events(page)).map((e) => e[0]), ['rx:confirm:opened', 'rx:confirm:cancelled', 'rx:confirm:closed']);
    page.assertClean({ allow: [/422/] });
  }));

test('a confirm says confirmed when Delete is pressed', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await page.eval(() => {
      sessionStorage.removeItem('confirmed');
      document.addEventListener('rx:confirm:confirmed', (e) =>
        sessionStorage.setItem('confirmed', JSON.stringify([e.target.id, e.detail])));
    });
    await page.click('[data-rx-open="ev-confirm"]');
    await page.waitFor(() => document.querySelector('#ev-confirm').open);
    await page.click('#ev-confirm button[type=submit]');
    await page.waitFor(() => location.pathname === '/confirm-done');
    assert.deepEqual(JSON.parse(await page.eval(() => sessionStorage.getItem('confirmed'))), ['ev-confirm', { id: 'ev-confirm' }]);
    page.assertClean({ allow: [/422/] });
  }));

// #423: tabs and wizards say `changed`, only when the selection really changes.
const listenChanged = (page) =>
  page.eval(() => {
    window.__ch = [];
    for (const n of ['rx:tabs:changed', 'rx:wizard:changed'])
      document.addEventListener(n, (e) => window.__ch.push([n, e.detail]));
  });
const changes = (page) => page.eval(() => window.__ch);

test('tabs say changed on a click and on ArrowRight, never for the selected tab', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listenChanged(page);
    await page.click('#evt-tab-a');
    assert.deepEqual(await changes(page), [], 'already selected');
    await page.click('#evt-tab-b');
    assert.deepEqual(await changes(page), [['rx:tabs:changed', { name: 'b' }]]);
    await page.eval(() => document.querySelector('#evt-tab-b').focus());
    await page.press('ArrowLeft');
    assert.deepEqual((await changes(page)).length, 2);
    await page.press('ArrowRight');
    assert.deepEqual((await changes(page)).length, 3);
    page.assertClean({ allow: [/422/] });
  }));

test('a wizard says changed on Next, not when its sheet opens', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listenChanged(page);
    await page.click('[data-rx-open="wizard-sheet"]');
    await page.waitFor(() => document.querySelector('#wizard-sheet').open);
    await page.waitFor(() => document.querySelector('[data-rx-wizard][data-rx-ready]'));
    assert.deepEqual(await changes(page), []);
    await page.type('#ws-from', 'Bandung');
    await page.click('#wizard-sheet [data-rx-wizard-next]');
    await page.waitFor(() => window.__ch.length > 0);
    assert.deepEqual(await changes(page), [['rx:wizard:changed', { name: 'end', index: 1 }]]);
    page.assertClean({ allow: [/422/] });
  }));

// #424: fields say `changed` with their value; repeaters say added/removed.
const FIELD_EVENTS = ['select', 'tags-input', 'date-picker'].map((c) => `rx:${c}:changed`)
  .concat(['added', 'removed'].map((n) => `rx:repeater:${n}`));
const listenFields = (page) =>
  page.eval((names) => {
    window.__fe = [];
    for (const n of names) document.addEventListener(n, (e) => window.__fe.push([n, e.target.id, e.detail]));
  }, FIELD_EVENTS);
const fieldEvents = (page) => page.eval(() => window.__fe);

test('a searchable select says changed with its value', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listenFields(page);
    await page.waitFor(() => document.querySelector('#rx-flavour-search'));
    await page.type('#rx-flavour-search', 'min');
    await page.press('Enter');
    await page.waitFor(() => window.__fe.length > 0);
    assert.deepEqual(await fieldEvents(page), [['rx:select:changed', 'rx-flavour', { value: 'mint' }]]);
    page.assertClean({ allow: [/422/] });
  }));

test('a plain select says changed with its value', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listenFields(page);
    await page.eval(() => {
      const s = document.querySelector('#rx-size');
      s.value = 'm';
      s.dispatchEvent(new Event('change', { bubbles: true }));
    });
    assert.deepEqual(await fieldEvents(page), [['rx:select:changed', 'rx-size', { value: 'm' }]]);
  }));

test('tags say changed once with the array; a blur with an empty box adds nothing', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listenFields(page);
    await page.click('#rx-labels');
    await page.type('#rx-labels', 'sale');
    await page.press('Enter');
    await page.waitFor(() => window.__fe.length > 0);
    await page.eval(() => document.querySelector('#rx-labels').blur());
    assert.deepEqual(await fieldEvents(page), [['rx:tags-input:changed', 'rx-labels', { value: ['new', 'sale'] }]]);
    page.assertClean({ allow: [/422/] });
  }));

test('a date picker says changed when a day is picked', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listenFields(page);
    await page.click('[popovertarget="rx-on-calendar"]');
    await page.waitFor(() => document.querySelector('#rx-on-calendar').matches(':popover-open'));
    await page.waitFor(() => !!document.querySelector('calendar-date'));
    await new Promise((r) => setTimeout(r, 200));
    await page.eval(() => document.querySelector('calendar-date').focus());
    await page.press('PageDown');
    await page.press('Enter');
    await page.waitFor(() => window.__fe.length > 0);
    assert.deepEqual(await fieldEvents(page), [['rx:date-picker:changed', 'rx-on', { value: '2026-11-02' }]]);
    page.assertClean({ allow: [/422/] });
  }));

test('a repeater says added then removed with the row index', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/events`);
    await listenFields(page);
    await page.waitFor(() => document.querySelector('#rx-lines [data-rx-row-add]'));
    await page.click('#rx-lines [data-rx-row-add]');
    await page.waitFor(() => document.querySelector('#rx-lines [data-rx-row]'));
    await page.click('#rx-lines [data-rx-row-remove]');
    await page.waitFor(() => window.__fe.length > 1);
    assert.deepEqual(await fieldEvents(page), [
      ['rx:repeater:added', 'rx-lines', { index: 0 }],
      ['rx:repeater:removed', 'rx-lines', { index: 0 }],
    ]);
    page.assertClean({ allow: [/422/] });
  }));

// #427: `@event` on a tag becomes `x-on:rx:<component>:<event>.self`, under CSP=strict.

test('@saved on an action sheet and @changed on tabs reach an Alpine.data component', () =>
  browser.with(async (page) => {
    await page.goto(`${strict.url}/events-tags`);
    await page.waitFor(() => document.querySelector('#saved'));
    assert.equal(await page.eval(() => document.querySelector('#saved').textContent), 'false');

    await page.click('[data-rx-open="tag-trip"]');
    await page.waitFor(() => document.querySelector('#tag-trip').open);
    await page.type('#rx-from', 'Bandung');
    await page.type('#rx-to', 'Jakarta');
    await page.click('#tag-trip button[type=submit]');
    await page.waitFor(() => document.querySelector('#saved').textContent === 'true');

    await page.click('#tag-tabs-tab-b');
    await page.waitFor(() => document.querySelector('#tab').textContent === 'b');
    page.assertClean();
  }));

test('a nested component\'s event does not fire the outer handler', () =>
  browser.with(async (page) => {
    await page.goto(`${strict.url}/events-tags`);
    await page.waitFor(() => document.querySelector('#inner-tabs-tab-y'));
    await page.eval(() => {
      window.__inner = 0;
      document.addEventListener('rx:tabs:changed', (e) => { if (e.target.querySelector('#inner-tabs-tab-y')) window.__inner++; });
    });
    await page.click('#inner-tabs-tab-y');
    await page.waitFor(() => window.__inner > 0);
    assert.equal(await page.eval(() => document.querySelector('#outer').textContent), '0');
    page.assertClean();
  }));

test('hx-trigger can listen for rx:action-sheet:saved from the body', () =>
  browser.with(async (page) => {
    await page.goto(`${strict.url}/events-tags`);
    await page.waitFor(() => /loads: \d+/.test(document.querySelector('#stamp').textContent));
    const first = await page.eval(() => document.querySelector('#stamp').textContent);
    await page.click('[data-rx-open="tag-trip"]');
    await page.waitFor(() => document.querySelector('#tag-trip').open);
    await page.type('#rx-from', 'Bandung');
    await page.type('#rx-to', 'Jakarta');
    await page.click('#tag-trip button[type=submit]');
    await page.waitFor((was) => document.querySelector('#stamp').textContent !== was, {}, first);
    page.assertClean();
  }));
