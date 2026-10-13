// /about/fields in a browser under CSP=strict (#351; the fields example's round
// trip in examples-flows.test.mjs and crud's live validation in
// examples.test.mjs until then): a field checked live as it is left, every
// kind of field sent, saved and back in the edit form as it was, and the
// pages at a phone's and a desktop's width, light and dark. The editors
// themselves are editors.test.mjs. Screenshots go to BIKESHOP_SCREENS.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser, sleep } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PHONE = { width: 390, height: 844 };
let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { CSP: 'strict' } });
  // Logged in from a tab of its own: in headless Chrome the tab that sent
  // the bike shop's login form gets no key presses afterwards (the cookie
  // is the browser's, so the tests' tabs are logged in too).
  await browser.with((page) => logIn(page));
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

async function submitAndLoad(page, button) {
  const loaded = page.once('Page.loadEventFired');
  await page.click(button);
  await loaded;
  await page.settle();
}

/** Logs the seeded customer in, unless this browser already is. */
async function logIn(page) {
  await page.goto(`${app.url}/login`);
  if (!(await page.eval(() => !!document.querySelector('#rx-email')))) return;
  await page.type('#rx-email', 'customer@bikeshop.test');
  await page.type('#rx-password', 'password');
  await submitAndLoad(page, 'form button[type=submit]');
}

test('a field is checked live as it is left, and the error goes once it is fixed', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/about/fields`);
    await page.type('[name=price]', '-5', { clear: true });
    await page.press('Tab');
    await page.waitFor(() => document.querySelector('[name=price]').getAttribute('aria-invalid') === 'true', { message: 'live validation' });
    await page.type('[name=price]', '4.50', { clear: true });
    await page.waitFor(() => !document.querySelector('[name=price]').hasAttribute('aria-invalid'), { message: 'the error gone' });
    page.assertClean({ allow: [/422/] });
  }));

test('every kind of field comes back as it was sent', () =>
  browser.with(async (page) => {
    await logIn(page);
    await page.goto(`${app.url}/about/fields`);
    await page.waitFor(() => !!document.querySelector('trix-editor')?.editor && !!document.querySelector('[data-rx-code-editor] [contenteditable]'));
    await page.type('#rx-name', 'Round trip');
    await page.type('#rx-brand', 'Riverside');
    await page.type('#rx-description', 'Smooth *and* quick');
    await page.type('#rx-stock', '12', { clear: true });
    await page.type('#rx-weight_kg', '10.75', { clear: true });
    await page.type('#rx-price', '612.99', { clear: true });
    await page.click('label[for="rx-available"], #rx-available');
    await page.click('label[for="rx-size-3"]');
    await page.click('label[for="rx-colors-2"]');
    await page.type('#rx-tags', 'commuter');
    await page.press('Enter');
    await page.type('#rx-tags', 'gravel');
    await page.press('Enter');
    await page.click('.rx-kv [data-rx-row-add]');
    await page.waitFor(() => !!document.querySelector('input[name="specs[0][key]"]'));
    await page.type('input[name="specs[0][key]"]', 'Frame');
    await page.type('input[name="specs[0][value]"]', 'Steel');
    await page.click('trix-editor');
    await page.type('trix-editor', 'Serviced yearly');
    await page.click('[data-rx-code-editor] [contenteditable]');
    await page.type('[data-rx-code-editor] [contenteditable]', '{"wheel": "700c"}');
    await page.eval(() => {
      const set = (id, value) => {
        const el = document.getElementById(id);
        el.value = value;
        el.dispatchEvent(new Event('input', { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));
      };
      set('rx-pickup_at', '08:30');
      set('rx-launch_at', '2026-11-01T09:30');
      // The date picker's own field (its calendar is tested in ui-forms).
      set('rx-released_on', '2026-12-24');
    });
    const values = (selector) =>
      page.eval((s) => [...new FormData(document.querySelector(s))].filter(([k, v]) => k !== '_token' && typeof v === 'string'), selector);
    const sent = await values('#sample-form');
    await submitAndLoad(page, '#sample-form button[type=submit]');
    const errors = await page.eval(() => [...document.querySelectorAll('[aria-invalid="true"]')].map((el) => el.name));
    const show = await page.eval(() => location.pathname);
    assert.match(show, /^\/about\/fields\/[0-9a-f-]{36}$/, `saved, on its page (errors: ${errors})`);
    await shot(page, 'fields-sample');
    await page.goto(`${app.url}${show}/edit`);
    const back = await values('#sample-form');
    const pick = (pairs, name) => pairs.filter(([k]) => k === name).map(([, v]) => v);
    for (const name of ['name', 'brand', 'description', 'stock', 'price', 'available', 'size', 'colors', 'tags', 'specs[0][key]', 'specs[0][value]', 'settings', 'pickup_at', 'released_on']) {
      assert.deepEqual(pick(back, name).map((v) => v.replace(/:00$/, '')), pick(sent, name).map((v) => v.replace(/:00$/, '')), name);
    }
    assert.equal(Number(pick(back, 'weight_kg')[0]), 10.75);
    assert.match(pick(back, 'launch_at')[0], /^2026-11-01T09:30/);
    assert.match(pick(back, 'details')[0], /Serviced yearly/);
    page.assertClean();
  }));

test('the pages fit a phone and a desktop, light and dark', async () => {
  for (const [size, options] of [['desktop', undefined], ['phone', PHONE]]) {
    for (const scheme of ['light', 'dark']) {
      await browser.with(async (page) => {
        await logIn(page);
        await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: scheme }] });
        await page.goto(`${app.url}/about/fields`);
        assert.ok(await page.eval(() => document.documentElement.scrollWidth <= window.innerWidth), 'no sideways scrolling');
        await sleep(400);
        await shot(page, `fields-${size}-${scheme}`);
        const sample = await page.eval(() => document.querySelector('a[href^="/about/fields/"]')?.getAttribute('href'));
        if (sample) {
          await page.goto(`${app.url}${sample}`);
          assert.ok(await page.eval(() => document.documentElement.scrollWidth <= window.innerWidth), 'no sideways scrolling');
          await sleep(400);
          await shot(page, `fields-show-${size}-${scheme}`);
        }
        page.assertClean();
      }, options);
    }
  }
});
