// The bike shop's rentals (#235) in a browser, on the seeded demo shop: a
// customer finds a bike at South and reserves it on a phone (390 px); South's
// cashier finds the reservation at the counter, hands the bike over and takes
// it back damaged (desktop); the fleet board as a grid on a desktop and as
// cards on a phone. Every page with a clean console and no sideways scroll.
// Screenshots go to BIKESHOP_SCREENS when it's set.

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
/** The reservation code made by the first test, used by the counter's. */
let code;

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

/** `YYYY-MM-DDTHH:00` in the browser's clock, `hours` from now (the app runs in UTC). */
const fieldTime = (hours) => {
  const d = new Date(Date.now() + hours * 3600_000);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}T${pad(d.getUTCHours())}:00`;
};

describe('renting a bike', () => {
  test('a customer finds a bike at South and reserves it (phone)', () =>
    browser.with(async (page) => {
      await login(page, 'customer@bikeshop.test');
      // Tomorrow, 10:00–13:00 (UTC), at South, through the htmx search.
      await page.goto(`${app.url}/rent`);
      const south = await page.eval(() => [...document.querySelectorAll('#rx-store option')].find((o) => o.textContent === 'South')?.value);
      assert.ok(south, 'South is a store to pick');
      const start = fieldTime(24 + 2);
      const end = fieldTime(24 + 5);
      await page.goto(`${app.url}/rent?store=${south}&starts_at=${start}&ends_at=${end}`);
      await page.settle();
      assert.ok(await fitsWidth(page), 'the rent page fits a phone');
      const offers = await page.eval(() => document.querySelectorAll('#reserve-form input[name=bike]').length);
      assert.ok(offers > 0, 'bikes are free at South');
      const text = await page.text('#rent-results');
      assert.match(text, /Deposit/, 'the price and the deposit are shown before confirming');
      // The store's day as a timeline (the availability block).
      assert.ok(await page.eval(() => !!document.querySelector('#rent-day table')), 'the day timeline is drawn');
      // Changing the type re-asks with htmx: only the results are swapped.
      await page.eval(() => {
        window.__kept = true;
      });
      await page.eval(() => {
        const select = document.querySelector('#rx-size');
        const option = [...select.options].find((o) => o.value);
        if (option) {
          select.value = option.value;
          select.dispatchEvent(new Event('change', { bubbles: true }));
        }
      });
      await page.settle();
      assert.equal(await page.eval(() => window.__kept), true, 'no full page load');
      await shot(page, 'rentals-search-phone');
      // Back to every size, then reserve the first bike.
      await page.goto(`${app.url}/rent?store=${south}&starts_at=${start}&ends_at=${end}`);
      // The button is below the fold on a phone: submit it as a tap would.
      await page.eval(() => document.querySelector('#reserve-form').requestSubmit(document.querySelector('#reserve-form button[type=submit]')));
      await page.waitFor(() => /^\/rentals\/[0-9A-Z]{26}$/.test(location.pathname), { message: 'the reservation page' });
      code = await page.eval(() => location.pathname.split('/').pop());
      const shown = await page.text('main');
      assert.ok(shown.includes(code), 'the code is shown as text');
      assert.match(shown, /Pay the deposit/);
      assert.ok(await fitsWidth(page), 'the reservation fits a phone');
      await shot(page, 'rentals-reservation-phone');
      page.assertClean();
    }, PHONE));
});

describe('the counter', () => {
  test("South's cashier hands the bike over and takes it back damaged (desktop)", () =>
    browser.with(async (page) => {
      assert.ok(code, 'the reservation from the first test');
      await login(page, 'cashier.south@bikeshop.test');
      await page.goto(`${app.url}/staff/rentals?q=${code}`);
      assert.ok((await page.text('main')).includes(code), 'found by its code');
      await shot(page, 'rentals-counter');
      await page.click('#found-rentals a');
      await page.waitFor(() => !!document.querySelector('#pickup-form'), { message: 'the pick-up form' });
      // Tick two items, pay by card, hand over.
      await page.eval(() => {
        for (const box of [...document.querySelectorAll('#pickup-form input[name=checklist]')].slice(0, 5)) box.checked = true;
      });
      await page.click('#pickup-form button[type=submit]');
      await page.waitFor(() => !!document.querySelector('#return-form'), { message: 'the return form after the pick-up' });
      await shot(page, 'rentals-desk-out');
      // Damaged: the damage fields appear (show_when), then take it back.
      await page.eval(() => document.querySelector('#return-form input[type=checkbox][name=damaged]').click());
      await page.waitFor(() => !document.querySelector('#return-form [data-rx-show-when]').hidden, { message: 'the damage fields show' });
      await page.type('#rx-damage_fee', '150');
      await page.type('#rx-damage_note', 'Scratched frame');
      await page.click('#return-form button[type=submit]');
      await page.waitFor(() => /\/receipt$/.test(location.pathname), { message: 'the receipt' });
      const receipt = await page.text('main');
      assert.match(receipt, /\$150\.00/, 'the damage fee on the receipt');
      assert.match(receipt, /Between the stores/);
      assert.ok(await fitsWidth(page));
      await shot(page, 'rentals-receipt');
      page.assertClean();
    }));
});

describe('the fleet board', () => {
  for (const [size, options] of [
    ['desktop', undefined],
    ['phone', PHONE],
  ]) {
    test(`a grid of the store's bikes (${size})`, () =>
      browser.with(async (page) => {
        await login(page, 'manager.north@bikeshop.test');
        await page.goto(`${app.url}/staff/fleet`);
        await page.waitFor(() => document.querySelectorAll('.rx-grid tbody tr, .rx-grid [role=row]').length > 1, { message: 'the grid has rows' });
        const text = await page.text('main');
        for (const part of ['Fleet board', 'Ours, here', 'Placed here by others', 'Ours, elsewhere']) {
          assert.ok(text.includes(part), `the board shows "${part}"`);
        }
        assert.ok(await fitsWidth(page), 'no sideways scroll');
        await shot(page, `rentals-fleet-${size}`);
        // A bike's page: who may do what, and its history.
        await page.goto(`${app.url}/staff/fleet?view=mine_here`);
        const href = await page.eval(() => document.querySelector('a[href^="/staff/fleet/"]')?.getAttribute('href'));
        assert.ok(href, 'a bike links to its page');
        await page.goto(`${app.url}${href}`);
        assert.match(await page.text('main'), /Who may do what/);
        assert.ok(await fitsWidth(page));
        await shot(page, `rentals-bike-${size}`);
        page.assertClean();
      }, options));
  }
});
