// The bike shop's data model and access foundations (#232) in a browser:
// /about/data at desktop and phone width with its "About this page" panel,
// and the staff shell's store switcher (a person with roles in two stores
// switches; someone with one store sees a label). Screenshots go to
// BIKESHOP_SCREENS when it's set.

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
  // A fresh session: the tabs share the browser's cookies.
  await page.send('Network.clearBrowserCookies', {});
  await page.goto(`${app.url}/login`);
  await page.type('#rx-email', email);
  await page.type('#rx-password', 'password');
  await page.click('form button[type=submit]');
  await page.waitFor(() => location.pathname !== '/login', { message: 'logged in' });
}

describe('/about/data', () => {
  for (const [size, options] of [
    ['desktop', undefined],
    ['phone', PHONE],
  ]) {
    test(`explains the data model (${size})`, () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}/about/data`);
        const text = await page.text('main');
        for (const part of ['The data model', 'Tables, by area', 'Relations', 'From Pagila', 'Owner, location and operating store', 'Money as integers', 'RBAC + ABAC']) {
          assert.ok(text.includes(part), `the page shows "${part}"`);
        }
        // Live counts: the seeded rentals are there.
        const rentals = await page.eval(() =>
          [...document.querySelectorAll('tr')].find((tr) => tr.querySelector('code')?.textContent === 'rentals')?.lastElementChild.textContent.trim(),
        );
        assert.ok(rentals && rentals !== '0', `rentals counted: ${rentals}`);
        // Wide tables scroll inside their frame, never the page.
        assert.ok(await fitsWidth(page), 'no sideways scrolling');
        await shot(page, `about-data-${size}`);
        // "About this page": docked beside the page on a wide screen, a sheet on a phone.
        if (size === 'phone') {
          await page.click('[data-rx-open="about-page"]');
          await page.waitFor(() => document.querySelector('#about-page')?.open, { message: 'the panel open' });
          assert.ok((await page.text('#about-page')).includes('renox::db::relations'));
          await page.press('Escape');
        } else {
          assert.ok((await page.text('#explain-dock')).includes('renox::db::relations'));
        }
        page.assertClean();
      }, options));
  }
});

describe('the store switcher', () => {
  test('roles in two stores: switch from the staff shell', () =>
    browser.with(async (page) => {
      await login(page, 'floater@bikeshop.test');
      await page.goto(`${app.url}/staff`);
      assert.ok((await page.text('.bs-store-switcher')).includes('Working in North'));
      await shot(page, 'switcher-closed');
      // The kit's menu: open it, pick South.
      await page.click('.bs-store-switcher [aria-haspopup="menu"]');
      await page.waitFor(() => !document.querySelector('#store-menu').hidden, { message: 'the menu open' });
      const items = await page.eval(() => [...document.querySelectorAll('#store-menu [role=menuitem]')].map((b) => b.textContent.trim()));
      assert.deepEqual(items, ['North', 'South'], 'only the stores with a role');
      await shot(page, 'switcher-open');
      await page.eval(() =>
        [...document.querySelectorAll('#store-menu [role=menuitem]')].find((b) => b.textContent.trim() === 'South').click(),
      );
      await page.waitFor(() => document.querySelector('.bs-store-switcher')?.textContent.includes('Working in South'), {
        message: 'working in South',
      });
      page.assertClean();
    }));

  test('one store: a label, no menu (phone)', () =>
    browser.with(async (page) => {
      await login(page, 'cashier.west@bikeshop.test');
      await page.goto(`${app.url}/staff`);
      const text = await page.text('.bs-store-switcher');
      assert.ok(text.includes('Working in West'), text);
      assert.equal(await page.eval(() => document.querySelector('#store-menu')), null);
      assert.ok(await fitsWidth(page));
      await shot(page, 'switcher-phone');
      page.assertClean();
    }, PHONE));
});
