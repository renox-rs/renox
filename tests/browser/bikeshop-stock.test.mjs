// The bike shop's stock, consignment and purchasing (#240) in a browser, on the
// seeded demo shop: North's manager reads the stock grid by owner and location,
// groups it by category and opens a ledger; South's manager counts a shelf on a
// phone; North sends goods to another store on consignment and that store's
// manager receives them in part; a purchase order starts from what the store
// needs and is sent; a supplier's price list import sheet. Every page with a
// clean console and no sideways scroll. Screenshots go to BIKESHOP_SCREENS when
// it's set.

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

async function submit(page, selector, path) {
  await page.eval((s) => document.querySelector(s).requestSubmit(), selector);
  await page.waitFor((p) => new RegExp(p).test(location.pathname + location.search) && document.readyState === 'complete', { message: `went to ${path}` }, path);
  await page.settle();
}

describe('the stock grid and the ledger', () => {
  test("North's manager reads stock by owner and location (desktop)", () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/stock`);
      let text = await page.text('main');
      for (const part of ['Our goods here', 'Held for other stores', 'Ours at other stores', 'Under reorder level', 'Value at cost']) {
        assert.ok(text.includes(part), `the grid shows "${part}"`);
      }
      assert.ok(await page.eval(() => document.querySelectorAll('#grid-stock tbody tr').length > 0), 'rows');
      // Goods held for other stores.
      await page.goto(`${app.url}/staff/stock?view=held`);
      assert.ok(await page.eval(() => document.querySelectorAll('#grid-stock tbody tr').length > 0), 'consigned goods at North');
      await shot(page, 'stock-grid-desktop');
      // One level's ledger, from the grid's link.
      const href = await page.eval(() => document.querySelector('#grid-stock a[href^="/staff/stock/"]')?.getAttribute('href'));
      assert.ok(href, 'a row links to its ledger');
      await page.goto(`${app.url}${href}`);
      text = await page.text('main');
      for (const part of ['Movements', 'Who may do what', 'Consigned']) {
        assert.ok(text.includes(part), `the ledger shows "${part}"`);
      }
      assert.ok(await fitsWidth(page));
      await shot(page, 'stock-ledger-desktop');
      page.assertClean();
    }));

  test('the grid on a phone shows cards', () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/stock`);
      assert.ok(await fitsWidth(page), 'no sideways scroll');
      await shot(page, 'stock-grid-phone');
      page.assertClean();
    }, PHONE));
});

describe('a stock take', () => {
  test("South's manager counts a shelf on a phone", () =>
    browser.with(async (page) => {
      await login(page, 'manager.south@bikeshop.test');
      await page.goto(`${app.url}/staff/stock/take`);
      const text = await page.text('main');
      assert.ok(text.includes('Our goods'), 'own goods listed');
      assert.ok(await fitsWidth(page), 'the count sheet fits a phone');
      await shot(page, 'stock-take-phone');
      // Count the first line one short.
      await page.eval(() => {
        const input = document.querySelector('#take-form input[name$="[counted]"]');
        input.value = String(Math.max(0, Number(input.placeholder) - 1));
      });
      await submit(page, '#take-form', '/staff/stock/take');
      await page.waitFor(() => /Count saved/.test(document.body?.textContent || ''), { message: 'the toast' });
      page.assertClean();
    }, PHONE));
});

describe('consignment between stores', () => {
  test('North sends goods; the other store receives them in part', () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/consignments/new?direction=send`);
      const other = await page.eval(() => {
        const select = document.querySelector('#consignment-pick select[name=store]');
        return select.options[select.selectedIndex].textContent.trim();
      });
      await page.eval(() => {
        const input = document.querySelector('#consignment-form input[name$="[quantity]"]');
        input.value = '2';
      });
      assert.ok(await fitsWidth(page));
      await submit(page, '#consignment-form', '^/staff/consignments/\\d+$');
      let text = await page.text('main');
      assert.ok(text.includes('Approved'), 'our own goods start approved');
      await shot(page, 'consignment-approved-desktop');
      await submit(page, '#ship-form', '^/staff/consignments/\\d+$');
      text = await page.text('main');
      assert.ok(text.includes('Goods are on their way'), 'in transit');
      const shipment = await page.eval(() => location.pathname);

      // The other store's manager receives one of the two.
      await login(page, `manager.${other.toLowerCase()}@bikeshop.test`);
      await page.goto(`${app.url}${shipment}`);
      await page.eval(() => {
        document.querySelector('#receive-form input[name$="[received]"]').value = '1';
      });
      await submit(page, '#receive-form', '^/staff/consignments/\\d+$');
      text = await page.text('main');
      assert.ok(text.includes('Partly received'), 'a partial receipt');
      assert.ok(await page.eval(() => !!document.querySelector('.bs-flow li[aria-current="step"]')), 'the stepper marks the step');
      await shot(page, 'consignment-partly-desktop');
      page.assertClean();
    }));
});

describe('purchasing', () => {
  test('a purchase order starts from what the store needs, and is sent', () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/purchase-orders/new`);
      const supplier = await page.eval(() => document.querySelector('#po-supplier select[name=supplier] option[value]:not([value=""])')?.value);
      assert.ok(supplier, 'a supplier to pick');
      await page.goto(`${app.url}/staff/purchase-orders/new?supplier=${supplier}`);
      // Order one of the first line if nothing is prefilled.
      await page.eval(() => {
        const inputs = [...document.querySelectorAll('#po-form input[name$="[quantity]"]')];
        if (!inputs.some((i) => Number(i.value) > 0)) inputs[0].value = '1';
      });
      assert.ok(await fitsWidth(page));
      await shot(page, 'purchase-order-new-desktop');
      await submit(page, '#po-form', '^/staff/purchase-orders/\\d+$');
      assert.ok((await page.text('main')).includes('Draft'));
      await submit(page, '#send-po-form', '^/staff/purchase-orders/\\d+$');
      const text = await page.text('main');
      assert.ok(text.includes('Arrived'), 'the receiving form is there once ordered');
      await shot(page, 'purchase-order-ordered-desktop');
      page.assertClean();
    }));

  test("a supplier's page opens the import sheet", () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/suppliers`);
      const href = await page.eval(() => document.querySelector('main table a[href^="/staff/suppliers/"]')?.getAttribute('href'));
      assert.ok(href);
      await page.goto(`${app.url}${href}`);
      await page.click('[data-rx-open="price-list"]');
      await page.waitFor(() => document.getElementById('price-list')?.open, { message: 'the sheet opened' });
      assert.ok((await page.text('#price-list')).includes('sku'), 'the columns are listed');
      await shot(page, 'supplier-import-desktop');
      page.assertClean();
    }));
});
