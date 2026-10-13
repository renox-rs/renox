// The bike shop's reports (#242) in a browser, on the seeded demo shop: the owner's dashboard
// at 1280 and 390 px, light and dark (figures, charts, the store comparison, no sideways
// scroll, a clean console); the period, "in whose books" and the store menu; a manager sees
// only their store; a report grid grouped and exported; the monthly report run from its page
// with the progress widget polling until the mail is sent. Screenshots go to BIKESHOP_SCREENS
// when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

// Demo staff log in without setting up two-factor login (#239 makes it
// required; tests/browser/bikeshop-staff.test.mjs checks that it is).
const STAFF_2FA_OPTIONAL = { BIKESHOP_STAFF_2FA: 'optional' };

const PHONE = { width: 390, height: 844 };
let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: STAFF_2FA_OPTIONAL });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

const fitsWidth = (page) => page.eval(() => document.documentElement.scrollWidth <= window.innerWidth);

async function login(page, email) {
  await page.send('Network.clearBrowserCookies', {});
  await page.goto(`${app.url}/login`);
  await page.type('#rx-email', email);
  await page.type('#rx-password', 'password');
  await page.click('form button[type=submit]');
  await page.waitFor(() => location.pathname !== '/login', { message: 'logged in' });
}

const scheme = (page, value) =>
  page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value }] });

describe('the dashboard', () => {
  for (const [size, viewport] of [['desktop', undefined], ['phone', PHONE]]) {
    for (const colours of ['light', 'dark']) {
      test(`the owner's dashboard (${size}, ${colours})`, () =>
        browser.with(async (page) => {
          await scheme(page, colours);
          await login(page, 'owner@bikeshop.test');
          await page.goto(`${app.url}/staff/reports?period=90d`);
          const text = await page.text('main');
          for (const part of ['Revenue', 'Fleet use', 'Revenue by stream', 'Rentals by weekday and hour', 'Between stores', 'Top products', 'Best customers', 'Store comparison']) {
            assert.ok(text.includes(part), part);
          }
          const charts = await page.eval(() => document.querySelectorAll('main svg').length);
          assert.ok(charts >= 4, `charts drawn (${charts})`);
          assert.ok(await fitsWidth(page), 'no sideways scroll');
          await shot(page, `reports-${size}-${colours}`);
          page.assertClean();
        }, viewport));
    }
  }

  test('the period, the books and one store', () =>
    browser.with(async (page) => {
      await login(page, 'owner@bikeshop.test');
      await page.goto(`${app.url}/staff/reports`);
      await page.click('.rx-period a[href*="period=12w"]');
      await page.waitFor(() => location.search.includes('period=12w'), { message: '12 weeks' });
      await page.click('.rx-link-tabs a[href*="by=books"]');
      await page.waitFor(() => location.search.includes('by=books') && location.search.includes('period=12w'), { message: 'books, still 12 weeks' });
      assert.ok((await page.text('main')).includes("whose bikes and goods") || (await page.text('main')).includes('books of the store'));
      const store = await page.eval(() => document.querySelector('#report-store a[href*="store="]')?.getAttribute('href'));
      assert.ok(store, 'a store in the menu');
      await page.goto(`${app.url}/staff/reports${store}`);
      const text = await page.text('main');
      assert.ok(/How .+ is doing/.test(text), 'the store is named');
      assert.ok(!text.includes('Store comparison'), 'one store: no comparison');
      page.assertClean();
    }));

  test("North's manager sees North only", () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff`);
      assert.ok((await page.text('main')).includes('North, last 7 days'));
      await page.goto(`${app.url}/staff/reports`);
      const text = await page.text('main');
      assert.ok(!text.includes('Store comparison'));
      assert.equal(await page.eval(() => document.querySelector('#report-store')), null, 'no store menu');
      assert.ok(await fitsWidth(page));
      page.assertClean();
    }, PHONE));
});

describe('report grids', () => {
  test('orders grouped by store, with exports', () =>
    browser.with(async (page) => {
      await login(page, 'owner@bikeshop.test');
      await page.goto(`${app.url}/staff/reports/orders?group=store`);
      assert.ok(await page.eval(() => document.querySelectorAll('#grid-report-orders tbody tr').length > 0), 'rows');
      const exports = await page.eval(() => [...document.querySelectorAll('#grid-report-orders a[href*="export="]')].map((a) => a.getAttribute('href')));
      assert.ok(exports.some((h) => h.includes('export=csv')) && exports.some((h) => h.includes('export=xlsx')), 'CSV and Excel');
      await shot(page, 'reports-orders-desktop');
      await page.goto(`${app.url}/staff/reports/customers`);
      assert.ok((await page.text('main')).includes('Lifetime value'));
      page.assertClean();
    }));

  test('the customers report fits a phone as cards', () =>
    browser.with(async (page) => {
      await login(page, 'owner@bikeshop.test');
      await page.goto(`${app.url}/staff/reports/customers`);
      assert.ok(await fitsWidth(page));
      await shot(page, 'reports-customers-phone');
      page.assertClean();
    }, PHONE));
});

describe('the monthly report', () => {
  test('a run from the page, its progress, then the workbooks', () =>
    browser.with(async (page) => {
      await login(page, 'owner@bikeshop.test');
      await page.goto(`${app.url}/staff/reports/monthly`);
      await page.click('form[action$="/staff/reports/monthly"] button[type=submit]');
      await page.waitFor(() => /is on its way/.test(document.body?.textContent || ''), { message: 'the toast' });
      // The widget polls the runs until the batch has run and mailed.
      await page.waitFor(() => /Mailed/.test(document.getElementById('monthly-runs')?.textContent || ''), { timeout: 30_000, message: 'the run finished' });
      const files = await page.eval(() => document.querySelectorAll('#monthly-runs a[href*="/staff/reports/monthly/"]').length);
      assert.equal(files, 3, 'a workbook per store');
      await shot(page, 'reports-monthly-desktop');
      page.assertClean();
    }));
});
