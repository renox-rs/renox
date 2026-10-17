// The bike shop's "pay by link" (#44) in a browser, on the seeded demo shop:
// North's manager takes a work order to "ready", sends its payment link, the
// customer opens the link (the demo gateway's page, as no Stripe keys are
// set), pays, and the work order shows paid once the gateway's webhook has
// come through the queue. Every page with a clean console and no sideways
// scroll. Screenshots go to BIKESHOP_SCREENS when it's set.

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

/**
 * Takes a work order on North's bench to "ready for pick-up" (the status
 * button on its page) and stays on the page of the first one that can be
 * paid (not a fleet repair, with something to pay). Returns its id.
 */
async function readyOrder(page) {
  await page.goto(`${app.url}/staff/workshop`);
  await page.waitFor(() => !!document.querySelector('[data-rx-kanban][data-rx-blocks-ready]'), { message: 'the board is ready' });
  const cards = await page.eval(() => [...document.querySelectorAll('[data-rx-kanban-column="in_progress"] [data-rx-kanban-card]')].map((c) => c.dataset.rxKanbanCard));
  assert.ok(cards.length > 0, 'North has work in progress');
  for (const id of cards.slice(0, 8)) {
    await page.goto(`${app.url}/staff/workshop/${id}`);
    const moved = await page.eval(() => {
      const form = [...document.querySelectorAll('#status-buttons form')].find((f) => f.querySelector('input[name=status]')?.value === 'ready');
      if (!form) return false;
      form.requestSubmit();
      return true;
    });
    if (!moved) continue;
    await page.waitFor(() => /Ready/.test(document.body.textContent) && location.pathname.startsWith('/staff/workshop/'), { message: 'the order is ready' });
    await page.goto(`${app.url}/staff/workshop/${id}`);
    if (await page.eval(() => !!document.querySelector('#pay-link-form'))) return id;
  }
  assert.fail('a work order that can be paid by link');
}

describe('paying a work order by link', () => {
  test("North's manager sends the link, the customer pays it, the order shows paid (phone)", () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      const id = await readyOrder(page);
      assert.match(await page.text('main'), /Send a payment link/);
      assert.ok(await fitsWidth(page), 'the work order fits a phone');
      await shot(page, 'workshop-paylink-order-phone');
      // Send the link: the page now waits for the customer and offers it.
      await page.eval(() => document.querySelector('#pay-link-form').requestSubmit());
      await page.waitFor(() => !!document.querySelector('#pay-link-waiting'), { message: 'the link is waiting to be paid' });
      assert.match(await page.text('#pay-link'), /Waiting for the customer to pay/);
      const href = await page.eval(() => document.querySelector('#pay-link-open')?.getAttribute('href'));
      assert.match(href, /\/plans\/demo-pay\/link\/\d+/, 'the demo gateway made the link');
      assert.ok(await fitsWidth(page));
      await shot(page, 'workshop-paylink-sent-phone');
      // The customer opens the signed link (no login needed) and pays.
      await page.send('Network.clearBrowserCookies', {});
      await page.goto(href);
      await page.waitFor(() => !!document.querySelector('#demo-link-pay'), { message: 'the demo payment page' });
      assert.match(await page.text('main'), /No real money/);
      assert.ok(await fitsWidth(page), 'the payment page fits a phone');
      await shot(page, 'workshop-paylink-demo-phone');
      await page.eval(() => document.querySelector('#demo-link-pay').requestSubmit());
      await page.waitFor(() => location.pathname === '/billing/return', { message: 'back from the gateway' });
      // The webhook arrives through the queue: reload the bench until it shows paid.
      await login(page, 'manager.north@bikeshop.test');
      let paid = false;
      for (let i = 0; i < 40 && !paid; i++) {
        await page.goto(`${app.url}/staff/workshop/${id}`);
        paid = await page.eval(() => !!document.querySelector('#pay-link-paid'));
        if (!paid) await new Promise((r) => setTimeout(r, 250));
      }
      assert.ok(paid, 'the work order shows it was paid by link');
      assert.match(await page.text('main'), /Paid by link/);
      assert.equal(await page.eval(() => !!document.querySelector('#pay-link-form, #pay-form')), false, 'nothing left to pay');
      assert.ok(await fitsWidth(page));
      await shot(page, 'workshop-paylink-paid-phone');
      page.assertClean();
    }, PHONE));
});
