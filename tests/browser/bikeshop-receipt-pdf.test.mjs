// A returned rental's receipt as a PDF (#30) in a browser, on the seeded demo
// shop: a customer reserves a bike at South, South's cashier hands it over and
// takes it back, and the receipt page then offers "Download PDF". The link is
// fetched with the cashier's session: a PDF file named after the reservation
// code, not an HTML page. While the bike is still out the PDF doesn't exist
// (404), and a customer can't fetch the staff link.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

// Demo staff log in without setting up two-factor login (#239).
const STAFF_2FA_OPTIONAL = { BIKESHOP_STAFF_2FA: 'optional' };

let browser;
let app;
/** The reservation code made by the first test, used by the next. */
let code;
/** The rental's receipt page path, found by the cashier's test. */
let receiptPath;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: STAFF_2FA_OPTIONAL });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

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

/** Fetches `path` with the page's cookies: its status, content type, file name and first bytes. */
const fetchFile = (page, path) =>
  page.eval(async (url) => {
    const res = await fetch(url, { credentials: 'same-origin' });
    const bytes = new Uint8Array(await res.arrayBuffer());
    return {
      status: res.status,
      type: res.headers.get('content-type') || '',
      disposition: res.headers.get('content-disposition') || '',
      head: String.fromCharCode(...bytes.slice(0, 5)),
      size: bytes.length,
    };
  }, path);

describe('the rental receipt as a PDF', () => {
  test('a customer reserves a bike at South', () =>
    browser.with(async (page) => {
      await login(page, 'customer@bikeshop.test');
      await page.goto(`${app.url}/rent`);
      const south = await page.eval(() => [...document.querySelectorAll('#rx-store option')].find((o) => o.textContent === 'South')?.value);
      assert.ok(south, 'South is a store to pick');
      const start = fieldTime(24 + 2);
      const end = fieldTime(24 + 5);
      await page.goto(`${app.url}/rent?store=${south}&starts_at=${start}&ends_at=${end}`);
      await page.settle();
      await page.eval(() => document.querySelector('#reserve-form').requestSubmit(document.querySelector('#reserve-form button[type=submit]')));
      await page.waitFor(() => /^\/rentals\/[0-9A-Z]{26}$/.test(location.pathname), { message: 'the reservation page' });
      code = await page.eval(() => location.pathname.split('/').pop());
      await page.settle();
      assert.ok(code, 'a reservation code');
      page.assertClean();
    }));

  test("South's cashier takes the bike back and downloads the receipt", () =>
    browser.with(async (page) => {
      assert.ok(code, 'the reservation from the first test');
      await login(page, 'cashier.south@bikeshop.test');
      await page.goto(`${app.url}/staff/rentals?q=${code}`);
      await page.click('#found-rentals a');
      await page.waitFor(() => !!document.querySelector('#pickup-form'), { message: 'the pick-up form' });
      await page.eval(() => {
        for (const box of [...document.querySelectorAll('#pickup-form input[name=checklist]')].slice(0, 5)) box.checked = true;
      });
      await page.click('#pickup-form button[type=submit]');
      await page.waitFor(() => !!document.querySelector('#return-form'), { message: 'the return form after the pick-up' });

      // Out with the customer: no receipt yet, so no PDF.
      const rentalPath = await page.eval(() => location.pathname);
      const early = await fetchFile(page, `${rentalPath}/receipt.pdf`);
      assert.equal(early.status, 404, 'a rental that is out has no receipt');

      await page.click('#return-form button[type=submit]');
      await page.waitFor(() => /\/receipt$/.test(location.pathname), { message: 'the receipt page' });
      receiptPath = await page.eval(() => location.pathname);
      await page.settle();

      // The page links to the PDF.
      const href = await page.eval(() => document.querySelector('a[href$="/receipt.pdf"]')?.getAttribute('href'));
      assert.equal(href, `${receiptPath}.pdf`, 'the receipt links to its PDF');
      assert.match(await page.text('main'), /Download PDF/);

      // And the link gives a PDF file named after the reservation code.
      const file = await fetchFile(page, href);
      assert.equal(file.status, 200);
      assert.match(file.type, /^application\/pdf/);
      assert.equal(file.head, '%PDF-', 'the body is a PDF');
      assert.ok(file.size > 1000, 'a real document');
      assert.ok(file.disposition.includes(`receipt-${code}.pdf`), file.disposition);
      // The one 404 is the early fetch above, which the test asks for.
      page.assertClean({ allow: [/404/] });
    }));

  test("a customer can't fetch the staff link", () =>
    browser.with(async (page) => {
      assert.ok(receiptPath, 'the receipt from the cashier test');
      await login(page, 'customer@bikeshop.test');
      await page.goto(`${app.url}/rentals`);
      const file = await fetchFile(page, `${receiptPath}.pdf`);
      assert.ok([403, 404].includes(file.status), `refused, got ${file.status}`);
      assert.notEqual(file.head, '%PDF-');
    }));
});
