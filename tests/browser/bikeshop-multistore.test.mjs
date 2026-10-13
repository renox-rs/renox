// The bike shop's multi-store operations (#245) in a browser, on the seeded demo
// shop: South's manager approves a help request and West's mechanic, helping
// South this week, has South in the store switcher; bike placements and their
// "send back" tasks on a phone; the owner reads the books between stores, a
// monthly statement and the fee rates. Every page with a clean console and no
// sideways scroll. Screenshots go to BIKESHOP_SCREENS when it's set.

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

describe('help between stores', () => {
  test("South's manager approves a request to lend someone (desktop)", () =>
    browser.with(async (page) => {
      await login(page, 'manager.south@bikeshop.test');
      await page.goto(`${app.url}/staff/help`);
      const text = await page.text('main');
      assert.ok(text.includes('Asked of us') && text.includes('Asked by us'));
      await shot(page, 'help-desktop');
      const form = await page.eval(() => document.querySelector('form[id^="approve-"]')?.id);
      assert.ok(form, 'a request waiting for South');
      await page.eval((id) => document.getElementById(id).requestSubmit(), form);
      await page.waitFor(() => /Approved: the role is given/.test(document.body?.textContent || ''), { message: 'the toast' });
      page.assertClean();
    }));

  test("West's mechanic, helping South this week, can switch to South", () =>
    browser.with(async (page) => {
      await login(page, 'mechanic.west@bikeshop.test');
      await page.goto(`${app.url}/staff`);
      const switcher = await page.text('.bs-store-switcher');
      assert.ok(/Working in/.test(switcher), 'the switcher is there');
      const stores = await page.eval(() => [...document.querySelectorAll('.bs-store-switcher form, .bs-store-switcher [action]')].map((f) => f.getAttribute('action')));
      assert.ok(stores.length >= 2, 'two stores to work in');
      page.assertClean();
    }));
});

describe('bike placements', () => {
  test('placements and send-back tasks fit a phone', () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/placements`);
      const text = await page.text('main');
      assert.ok(text.includes('Placements') && text.includes('Send back'));
      assert.ok(await fitsWidth(page), 'no sideways scroll');
      await shot(page, 'placements-phone');
      page.assertClean();
    }, PHONE));
});

describe('the books between stores', () => {
  test('the owner reads balances, a statement and the fee rates', () =>
    browser.with(async (page) => {
      await login(page, 'owner@bikeshop.test');
      await page.goto(`${app.url}/staff/books`);
      let text = await page.text('main');
      assert.ok(text.includes('Open balances') && text.includes('Entries'));
      assert.ok(await page.eval(() => document.querySelectorAll('#grid-books tbody tr').length > 0), 'entries');
      await shot(page, 'books-desktop');
      await page.goto(`${app.url}/staff/books/settlements`);
      const href = await page.eval(() => document.querySelector('main table a[href^="/staff/books/settlements/"]')?.getAttribute('href'));
      assert.ok(href, 'a monthly statement');
      await page.goto(`${app.url}${href}`);
      text = await page.text('main');
      assert.ok(/owes/.test(text) && /Net:/.test(text), 'both sides and the net');
      assert.ok(await fitsWidth(page));
      await shot(page, 'statement-desktop');
      await page.goto(`${app.url}/staff/books/fees`);
      await page.click('[data-rx-open^="fee-"]');
      await page.waitFor(() => [...document.querySelectorAll('dialog[id^="fee-"]')].some((d) => d.open), { message: 'the sheet opened' });
      await shot(page, 'fees-desktop');
      page.assertClean();
    }));
});
