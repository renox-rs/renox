// The bike shop's staff side (#239) in a browser: two-factor login required
// for staff, the role × permission matrix switched live, a store's opening
// hours in the kit's repeater, the team at phone width, the admin panel with
// its Markdown editor and "About this page", and the audit log showing what
// was changed. Screenshots go to BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PHONE = { width: 390, height: 844 };
let browser;

before(async () => {
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

const fitsWidth = (page) => page.eval(() => document.documentElement.scrollWidth <= window.innerWidth);

async function logIn(page, app, email) {
  await page.send('Network.clearBrowserCookies');
  await page.goto(`${app.url}/login`);
  await page.type('#rx-email', email);
  await page.type('#rx-password', 'password');
  await page.click('form button[type=submit]');
  await page.waitFor(() => location.pathname !== '/login', { message: 'logged in' });
}

describe('bikeshop staff: two-factor login is required', () => {
  let app;
  before(async () => {
    app = await start('bikeshop', 'examples/bikeshop', { seed: true });
  });
  after(() => app?.stop());

  test('a member of staff is sent to set it up', () =>
    browser.with(async (page) => {
      await logIn(page, app, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff`);
      await page.waitFor(() => location.pathname === '/account', { message: 'sent to the account page' });
      await page.waitFor(() => document.querySelector('.rx-toast'), { message: 'a toast says why' });
      assert.ok((await page.text('main')).includes('Two-factor authentication'));
      await shot(page, 'staff-2fa-required');
      page.assertClean();
    }));
});

describe('bikeshop staff side', () => {
  let app;
  before(async () => {
    app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { BIKESHOP_STAFF_2FA: 'optional' } });
  });
  after(() => app?.stop());

  test('the owner switches a permission in the matrix', () =>
    browser.with(async (page) => {
      await logIn(page, app, 'owner@bikeshop.test');
      await page.goto(`${app.url}/staff/roles`);
      const cell = '[data-bs-cell="cashier:orders.refund"]';
      const before = await page.eval((s) => document.querySelector(s).checked, cell);
      await page.click(`label[for="perm-cashier-orders-refund"]`);
      await page.waitFor(() => document.querySelector('.rx-toast'), { message: 'a toast' });
      await page.goto(`${app.url}/staff/roles`);
      assert.equal(await page.eval((s) => document.querySelector(s).checked, cell), !before, 'saved');
      assert.ok(await fitsWidth(page), 'the matrix scrolls inside its card');
      await shot(page, 'staff-roles');

      // The audit log shows it, with the role used.
      await page.goto(`${app.url}/staff/audit`);
      const text = await page.text('main');
      assert.ok(text.includes('role.permission_'), 'the change is in the log');
      await shot(page, 'staff-audit');
      page.assertClean();
    }));

  test('a store\'s opening hours, one repeater row per day', () =>
    browser.with(async (page) => {
      await logIn(page, app, 'owner@bikeshop.test');
      await page.goto(`${app.url}/staff/stores`);
      await page.click('#stores a[href$="/edit"]');
      await page.waitFor(() => location.pathname.endsWith('/edit'));
      const rows = await page.eval(() => document.querySelectorAll('[data-rx-rows] .bs-hours').length);
      assert.equal(rows, 7, 'a row per day');
      assert.ok(await page.eval(() => document.querySelector('[data-rx-row-add]').disabled), 'seven days: no more to add');
      await shot(page, 'staff-store-edit');
      await page.click('#store-form > .rx-row button[type=submit]');
      await page.waitFor(() => location.pathname === '/staff/stores', { message: 'saved' });
      page.assertClean();
    }));

  test('the team on a phone', () =>
    browser.with(async (page) => {
      await logIn(page, app, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/team`);
      assert.ok((await page.text('main')).includes('Team of'));
      await page.click('table a.rx-link');
      await page.waitFor(() => /\/staff\/team\/\d+$/.test(location.pathname));
      assert.ok(await page.eval(() => !!document.querySelector('#assignments')));
      await shot(page, 'staff-member');
      await page.send('Emulation.setDeviceMetricsOverride', { ...PHONE, deviceScaleFactor: 1, mobile: true });
      await page.goto(`${app.url}/staff/team`);
      assert.ok(await fitsWidth(page), 'no sideways scrolling on a phone');
      await shot(page, 'staff-team-phone');
      page.assertClean();
    }));

  test('the admin panel: products, the Markdown editor, About this page', () =>
    browser.with(async (page) => {
      await logIn(page, app, 'owner@bikeshop.test');
      await page.goto(`${app.url}/admin/products`);
      assert.ok((await page.text('main')).includes('What fits'));
      // "About this page", docked beside the panel's page on a wide screen.
      assert.ok((await page.text('#explain-dock')).includes('renox-admin'));
      await shot(page, 'admin-products');
      await page.goto(`${app.url}/admin/products/create`);
      await page.waitFor(() => document.querySelector('textarea[name="description"]'), { message: 'the editor' });
      await shot(page, 'admin-product-create');
      page.assertClean();
    }));
});
