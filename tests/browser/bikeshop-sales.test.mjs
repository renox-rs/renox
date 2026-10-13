// The bike shop's buying flow (#234) in a browser, on the seeded shop:
// add to cart, the cart's stepper (the lines and the navbar's count change
// without a reload), the checkout wizard (steps, live validation, the
// summary's delivery fee), the demo gateway, and the payment page that waits
// for the signed webhook (sent by the app's own queue) and then shows the
// order paid. Then a cashier at the counter: a line rung up, the keypad and
// the change, the receipt. At 1280 and 390 px, light and dark, with a clean
// console. Screenshots go to BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser, sleep } from './lib/cdp.mjs';
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
const scheme = (page, value) =>
  page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value }] });

/** Clicks `selector` and waits for the next page to load. */
async function clickAndLoad(page, selector) {
  const loaded = page.once('Page.loadEventFired');
  await page.click(selector);
  await loaded;
  await page.settle();
}

/** Puts one helmet (gear, in stock somewhere) in the cart from its product page. */
async function addHelmet(page) {
  await page.goto(`${app.url}/shop/helmets`);
  const href = await page.eval(() => document.querySelector('#results .rx-media-card__link').getAttribute('href'));
  await page.goto(`${app.url}${href}`);
  await page.waitFor(() => !!document.querySelector('[data-bs-add-to-cart]'));
  await page.click('[data-bs-add-to-cart] button[type=submit]');
  await page.waitFor(() => !!document.querySelector('#nav-cart .rx-button__badge'));
}

/**
 * Picks a store with two or more of the cart's line, so its stepper can go up.
 * The seeded stock depends on the time of day, and the tests before this one
 * buy helmets too: in CI the first store once had a single one left.
 */
async function storeWithTwo(page) {
  const plus = () => !document.querySelector('.bs-cart__qty [data-rx-quantity-step="1"]').disabled;
  const stores = await page.eval(() =>
    [...document.querySelectorAll('#cart-store option')].map((o) => o.value).filter(Boolean),
  );
  for (const store of stores) {
    if (await page.eval(plus)) return;
    await page.eval((value) => {
      document.querySelector('#cart').dataset.old = '';
      const select = document.querySelector('#cart-store');
      select.value = value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    }, store);
    await page.waitFor(() => !('old' in document.querySelector('#cart').dataset));
    await page.settle();
  }
  assert.ok(await page.eval(plus), 'no store has two of the helmet');
}

describe('bikeshop sales', () => {
  for (const [size, options] of [
    ['desktop', undefined],
    ['phone', PHONE],
  ]) {
    for (const colours of ['light', 'dark']) {
      test(`cart, checkout, payment (${size}, ${colours})`, () =>
        browser.with(async (page) => {
          await scheme(page, colours);
          await addHelmet(page);

          // The cart: the stepper changes the line and the navbar's count, no reload.
          await page.goto(`${app.url}/cart`);
          assert.ok(await fitsWidth(page), 'no sideways scrolling on the cart');
          await storeWithTwo(page);
          await page.eval(() => (window.__same = true));
          await page.click('.bs-cart__qty [data-rx-quantity-step="1"]');
          await page.waitFor(() => document.querySelector('#nav-cart .rx-button__badge')?.textContent.trim() === '2');
          assert.equal(await page.eval(() => window.__same), true, 'the page was not reloaded');
          await shot(page, `sales-cart-${size}-${colours}`);

          // The checkout wizard: Next checks the step first (live validation).
          await clickAndLoad(page, 'a[href="/checkout"]');
          assert.ok(await fitsWidth(page), 'no sideways scrolling on the checkout');
          await page.type('#rx-name', 'Ana Ruiz', { clear: true });
          await page.type('#rx-email', 'not-an-email', { clear: true });
          await page.type('#rx-phone', '+62 812 3456 7890', { clear: true });
          await page.click('[data-rx-wizard-next]');
          await page.waitFor(() => !!document.querySelector('#rx-email[aria-invalid="true"]'));
          await page.type('#rx-email', 'ana@example.com', { clear: true });
          await page.click('[data-rx-wizard-next]');
          await page.waitFor(() => document.querySelector('[data-rx-step="delivery"]').offsetParent !== null);
          // Delivery to a city: the summary asks for the fee.
          await page.click('input[name=fulfilment][value=delivery] + *');
          await page.eval(() => {
            const city = document.querySelector('select[name=city_id]');
            city.value = city.options[city.options.length - 1].value;
            city.dispatchEvent(new Event('change', { bubbles: true }));
          });
          await page.waitFor(() => !/Free/.test(document.querySelector('#summary').textContent));
          await shot(page, `sales-checkout-delivery-${size}-${colours}`);
          // Back to a pickup, and on.
          await page.click('input[name=fulfilment][value=pickup] + *');
          await page.waitFor(() => /Free|Gratis/.test(document.querySelector('#summary').textContent));
          await page.click('[data-rx-wizard-next]');
          await page.waitFor(() => document.querySelector('[data-rx-step="review"]').offsetParent !== null);
          await shot(page, `sales-checkout-review-${size}-${colours}`);
          await clickAndLoad(page, '[data-rx-wizard-submit]');

          // The demo gateway's page, then the payment page waiting for the webhook.
          assert.ok(await page.eval(() => location.pathname.startsWith('/pay/demo/')), 'on the gateway');
          assert.ok(await fitsWidth(page));
          await shot(page, `sales-gateway-${size}-${colours}`);
          await clickAndLoad(page, '.bs-demo-gateway__form button[type=submit]');
          assert.ok(await page.eval(() => /^\/pay\/\d+$/.test(location.pathname)));
          await page.waitFor(() => !!document.querySelector('.bs-pay__icon--paid'), { timeout: 20_000, message: 'the webhook to arrive' });
          await shot(page, `sales-paid-${size}-${colours}`);
          await clickAndLoad(page, '.bs-pay a[href^="/orders/"]');
          assert.ok(await page.eval(() => !!document.querySelector('.rx-history')));
          assert.ok(await fitsWidth(page), 'no sideways scrolling on the order');
          await shot(page, `sales-order-${size}-${colours}`);
          page.assertClean();
        }, options));
    }
  }

  test('the counter: a line rung up, the keypad, the change, the receipt', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/login`);
      await page.type('#rx-email', 'cashier.south@bikeshop.test');
      await page.type('#rx-password', 'password');
      await clickAndLoad(page, 'form button[type=submit]');
      await page.goto(`${app.url}/staff/counter`);
      // The product search asks the server as the cashier types.
      const option = await page.eval(async () => {
        const res = await fetch('/staff/counter/variants?q=helmet', { headers: { Accept: 'application/json' } });
        return (await res.json())[0];
      });
      assert.ok(option?.value, 'the search found a helmet');
      await page.eval((o) => {
        const select = document.querySelector('select[name=variant_id]');
        select.add(new Option(o.label, o.value, true, true));
      }, option);
      // Sent with requestSubmit: a click on the button in CI sometimes never
      // sent the form (#342, #327), and htmx hears the submit event the same.
      await page.eval(() => document.querySelector('.bs-counter__find button[type=submit]').form.requestSubmit());
      await page.waitFor(() => document.querySelectorAll('.bs-cart__line--counter').length === 1);
      await page.settle();
      // The keypad types the amount received; the change is worked out.
      await page.waitFor(() => !!document.querySelector('[data-rx-keypad][data-rx-blocks-ready]'));
      for (const key of ['9', '9', '9', '9', '9', '9', '9']) await page.click(`[data-rx-keypad-key="${key}"]`);
      await page.waitFor(() => /Change/.test(document.querySelector('#rx-tendered-hint')?.textContent || ''));
      await shot(page, 'sales-counter');
      // P takes the payment: the receipt.
      await page.eval(() => document.activeElement?.blur());
      const loaded = page.once('Page.loadEventFired');
      await page.press('p');
      await loaded;
      await page.settle();
      const where = await page.eval(() => location.pathname + ' ' + document.title + ' ' + (document.querySelector('.rx-error, .rx-toast')?.textContent || ''));
      assert.ok(/\/orders\/\d+\/invoice/.test(where), where);
      assert.ok(await page.eval(() => /Change to give/.test(document.body.textContent)));
      await shot(page, 'sales-receipt');
      await sleep(50);
      page.assertClean();
    }));
});
