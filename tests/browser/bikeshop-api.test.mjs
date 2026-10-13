// The bike shop's API pages (#241) in a browser, on the seeded demo shop: a
// customer makes a personal token on a phone (shown once, with the kit's copy
// button) and reads /about/api; North's manager makes a kiosk token and
// revokes it. Clean consoles, no sideways scroll (code samples scroll inside
// their frame). Screenshots go to BIKESHOP_SCREENS when it's set.

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

/** Ticks `values` of the checkbox list `name` and sends the token form. */
async function makeToken(page, name, values) {
  await page.eval((n, vs) => {
    document.querySelector('.bs-token-form input[name=name]').value = n;
    for (const v of vs) document.querySelector(`.bs-token-form input[name=abilities][value="${v}"]`).checked = true;
    document.querySelector('.bs-token-form').requestSubmit();
  }, name, values);
  await page.waitFor(() => !!document.querySelector('#fresh-token'), { message: 'the token is shown' });
}

describe('API tokens', () => {
  test('a customer makes a token on a phone and reads the API page', () =>
    browser.with(async (page) => {
      await login(page, 'customer@bikeshop.test');
      await page.goto(`${app.url}/account/api-tokens`);
      await makeToken(page, 'My phone', ['read', 'rent']);
      const token = await page.eval(() => document.querySelector('#fresh-token').value);
      assert.match(token, /^\d+\|/);
      assert.ok(await fitsWidth(page), 'the token page fits a phone');
      await shot(page, 'api-tokens-phone');
      // The token works.
      const answer = await page.eval(async (t) => (await fetch('/api/v1/me', { credentials: 'omit', headers: { Authorization: `Bearer ${t}`, Accept: 'application/json' } })).status, token);
      assert.equal(answer, 200);
      // Shown once.
      await page.goto(`${app.url}/account/api-tokens`);
      assert.equal(await page.eval(() => !!document.querySelector('#fresh-token')), false);
      await page.goto(`${app.url}/about/api`);
      assert.match(await page.text('main'), /\/api\/v1\/kiosk\/bikes/);
      assert.ok(await fitsWidth(page), 'code samples scroll inside their frame');
      await shot(page, 'api-about-phone');
      page.assertClean();
    }, PHONE));

  test("North's manager makes a kiosk token and revokes it", () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/api-tokens`);
      await makeToken(page, 'Racks by the door', ['rentals:read', 'rentals:checkout', 'rentals:return']);
      const token = await page.eval(() => document.querySelector('#fresh-token').value);
      const ok = await page.eval(async (t) => (await fetch('/api/v1/kiosk/bikes', { credentials: 'omit', headers: { Authorization: `Bearer ${t}`, Accept: 'application/json' } })).status, token);
      assert.equal(ok, 200);
      await shot(page, 'api-kiosks-desktop');
      const dialog = await page.eval(() => document.querySelector('[data-rx-open^="revoke-kiosk-"]').getAttribute('data-rx-open'));
      await page.eval((d) => document.querySelector(`[data-rx-open="${d}"]`).click(), dialog);
      await page.waitFor((d) => document.getElementById(d)?.open, { message: 'the confirmation' }, dialog);
      await page.eval((d) => document.querySelector(`#${d} button.rx-button--danger`).click(), dialog);
      // The confirmation posts a plain form and the page loads again (no
      // <main> while the next document is being parsed).
      await page.waitFor(() => /Revoked/.test(document.querySelector('main')?.textContent || ''), { message: 'revoked' });
      await page.settle();
      const gone = await page.eval(async (t) => (await fetch('/api/v1/kiosk/bikes', { credentials: 'omit', headers: { Authorization: `Bearer ${t}`, Accept: 'application/json' } })).status, token);
      assert.equal(gone, 401);
      page.assertClean({ allow: [/status of 401/] }); // the revoked token's call, on purpose
    }));
});
