// The bike shop's main pages under CSP=strict (#243): a guest, the customer
// and the owner walk the pages of every area (and one record's page from
// each list), and every page loads without a console error, an uncaught
// exception or a CSP violation. Under the strict policy only scripts with
// the page's nonce run and Alpine evaluates no inline statements, so this
// catches a script or a handler written inline by mistake. Then a missing
// page renders in the shop's layout (tests/operations.rs checks the 500 and
// maintenance pages).

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', {
    seed: true,
    env: { CSP: 'strict', BIKESHOP_STAFF_2FA: 'optional' },
  });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

async function login(page, email) {
  await page.send('Network.clearBrowserCookies', {});
  await page.goto(`${app.url}/login`);
  await page.type('#rx-email', email);
  await page.type('#rx-password', 'password');
  await page.click('form button[type=submit]');
  await page.waitFor(() => location.pathname !== '/login', { message: 'logged in' });
}

/** Opens `path`, checks it is the page itself (no redirect, no error page). */
async function visit(page, path) {
  await page.goto(`${app.url}${path}`);
  const where = await page.eval(() => location.pathname + location.search);
  assert.equal(where, path, `${path} wasn't redirected`);
  const title = await page.eval(() => document.title);
  assert.ok(!/^\d{3} /.test(title), `${path} is not an error page (${title})`);
}

/** The first link on the page whose path matches `pattern`. */
const firstLink = (page, pattern) =>
  page.eval(
    (source) =>
      [...document.querySelectorAll('a[href]')]
        .map((a) => new URL(a.href).pathname)
        .find((p) => new RegExp(source).test(p)),
    pattern.source,
  );

const GUEST = ['/', '/shop', '/search?q=helmet', '/plans', '/rent', '/about/pages', '/about/data', '/about/blocks', '/about/api'];
const CUSTOMER = ['/rentals', '/bikes', '/plans/mine', '/notifications', '/account', '/service/book', '/cart', '/account/api-tokens'];
const OWNER = [
  '/staff',
  '/staff/rentals',
  '/staff/fleet',
  '/staff/identities',
  '/staff/workshop',
  '/staff/orders',
  '/staff/counter',
  '/staff/stock',
  '/staff/stock/fleet',
  '/staff/consignments',
  '/staff/suppliers',
  '/staff/purchase-orders',
  '/staff/help',
  '/staff/placements',
  '/staff/books',
  '/staff/books/settlements',
  '/staff/reports',
  '/staff/reports/orders',
  '/staff/stores',
  '/staff/team',
  '/staff/roles',
  '/staff/audit',
  '/staff/api-tokens',
  '/admin',
];

// From a list page, one record's page (path pattern of its links).
const DETAILS = {
  guest: [['/shop', /^\/products\/[^/]+$/]],
  customer: [
    ['/rentals', /^\/rentals\/[0-9A-Z]{26}$/],
    ['/bikes', /^\/bikes\/\d+$/],
    ['/plans/mine', /^\/plans\/mine\/\d+$/],
  ],
  owner: [
    ['/staff/fleet', /^\/staff\/fleet\/\d+$/],
    // The orders list opens on the open (paid / ready) tab, which the seed
    // doesn't promise for the owner's store; every store has completed ones.
    ['/staff/orders?status=completed', /^\/staff\/orders\/\d+$/],
    ['/staff/stock', /^\/staff\/stock\/\d+$/],
    ['/staff/purchase-orders', /^\/staff\/purchase-orders\/\d+$/],
    ['/staff/suppliers', /^\/staff\/suppliers\/\d+$/],
    ['/staff/books/settlements', /^\/staff\/books\/settlements\/\d+$/],
  ],
};

describe('the main pages under CSP=strict', () => {
  for (const [who, email, pages] of [
    ['guest', null, GUEST],
    ['customer', 'customer@bikeshop.test', CUSTOMER],
    ['owner', 'owner@bikeshop.test', OWNER],
  ]) {
    test(`as the ${who}`, () =>
      browser.with(async (page) => {
        if (email) await login(page, email);
        for (const path of pages) {
          await visit(page, path);
          page.assertClean();
        }
        for (const [list, pattern] of DETAILS[who]) {
          await visit(page, list);
          const detail = await firstLink(page, pattern);
          assert.ok(detail, `a ${pattern} link on ${list}`);
          await visit(page, detail);
          page.assertClean();
        }
        await shot(page, `walk-${who}`);
      }));
  }
});

describe('error pages', () => {
  test('a missing page in the shop layout', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/no-such-page`);
      assert.match(await page.eval(() => document.title), /^404 /);
      assert.ok(await page.eval(() => document.body.classList.contains('bs-public')), 'the public layout');
      assert.ok(await page.eval(() => !!document.querySelector('.rx-navbar a[href="/shop"]')), 'the navbar');
      await shot(page, 'walk-404');
      // The page's own 404 is the only problem.
      page.assertClean({ allow: [/status of 404/] });
    }));
});
