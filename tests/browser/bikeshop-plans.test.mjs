// The bike shop's service plans (#237) in a browser, on the seeded demo shop
// without payment keys (the demo gateway): the plans compared on a phone, then a
// customer subscribes their second bike at 390 px, pays on the demo gateway's
// page, waits for its webhook (a queue worker sends it to the app itself), and
// skips the first visit from the plan's page. Every page with a clean console
// and no sideways scroll. Screenshots go to BIKESHOP_SCREENS when it's set.

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

describe('service plans', () => {
  test('the plans compared on a phone', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/plans`);
      const cards = await page.eval(() => document.querySelectorAll('.rx-plan').length);
      assert.ok(cards >= 3, `the plans' cards (${cards})`);
      const text = await page.text('main');
      assert.match(text, /Monthly tune-up/);
      assert.match(text, /off spare parts/);
      assert.ok(await fitsWidth(page), 'the comparison scrolls inside its frame');
      await shot(page, 'plans-index-phone');
      page.assertClean();
    }, PHONE));

  test('a customer subscribes a bike, pays on the demo page and skips a visit (390 px)', () =>
    browser.with(async (page) => {
      await login(page, 'customer@bikeshop.test');
      await page.goto(`${app.url}/plans`);
      // "Choose this plan" on the highlighted card.
      const choose = await page.eval(() => document.querySelector('.rx-plan--highlight .rx-plan__cta').getAttribute('href'));
      assert.match(choose, /\/plans\/subscribe\?plan=/);
      await page.goto(`${app.url}${choose}`);
      await page.waitFor(() => location.pathname === '/plans/subscribe', { message: 'the subscribe form' });
      // The demo customer's first bike has a plan; the other is offered.
      const bikes = await page.eval(() => [...document.querySelectorAll('#rx-bike option')].map((o) => o.value).filter(Boolean));
      assert.equal(bikes.length, 1, 'one bike without a plan');
      // Another weekday: htmx swaps the summary (Motion nods at the price).
      const weekday = await page.eval(() => {
        const buttons = [...document.querySelectorAll('input[name=weekday]')];
        const pick = buttons.find((b) => !b.checked);
        pick.checked = true;
        pick.dispatchEvent(new Event('change', { bubbles: true }));
        return pick.value;
      });
      await page.waitFor((w) => document.querySelector(`input[name=weekday][value="${w}"]`)?.checked && /\d/.test(document.querySelector('.bs-subscribe__price')?.textContent || ''), { message: 'the summary follows' }, weekday);
      assert.ok(await page.eval(() => document.querySelector('input[name=pay_with][value=card]')?.checked), 'the demo card is chosen');
      assert.ok(await fitsWidth(page), 'the form fits a phone');
      await shot(page, 'plans-subscribe-phone');
      await page.eval(() => document.querySelector('#subscribe-form').requestSubmit());
      // The demo gateway's page.
      await page.waitFor(() => location.pathname.startsWith('/plans/demo-pay/'), { message: 'the demo payment page' });
      await page.waitFor(() => !!document.querySelector('main .bs-demo-billing'), { message: 'the demo page loaded' });
      assert.match(await page.text('main'), /No real money/);
      assert.ok(await fitsWidth(page));
      await shot(page, 'plans-demo-pay-phone');
      await page.eval(() => document.querySelector('.bs-demo-billing form').requestSubmit());
      await page.waitFor(() => location.pathname === '/plans/mine', { message: 'back to my plans' });
      // The webhook arrives through the queue: reload until the plan runs.
      let running = 0;
      for (let i = 0; i < 40 && running < 2; i++) {
        await page.goto(`${app.url}/plans/mine`);
        running = await page.eval(() => [...document.querySelectorAll('.bs-my-plan .rx-badge')].filter((b) => /Running/.test(b.textContent)).length);
        if (running < 2) await new Promise((r) => setTimeout(r, 250));
      }
      assert.equal(running, 2, 'both bikes have a running plan');
      assert.ok(await fitsWidth(page));
      await shot(page, 'plans-mine-phone');
      // The new plan's page: its first visit is booked; skip it.
      const href = await page.eval(() => {
        const cards = [...document.querySelectorAll('.bs-my-plan')];
        const fresh = cards.sort((a, b) => Number(b.dataset.bsPlan) - Number(a.dataset.bsPlan))[0];
        return fresh.querySelector('a.rx-link').getAttribute('href');
      });
      await page.goto(`${app.url}${href}`);
      await page.waitFor(() => !!document.querySelector('#upcoming-visits [data-bs-visit]'), { message: 'a visit is booked' });
      assert.ok(await fitsWidth(page), 'the calendar becomes a list on a phone');
      await shot(page, 'plans-show-phone');
      const visit = await page.eval(() => document.querySelector('#upcoming-visits [data-bs-visit]').dataset.bsVisit);
      // Wait for the rows' slide-in (Motion) to finish before clicking.
      await page.waitFor(() => document.getAnimations().every((a) => a.playState !== 'running'), { message: 'the motion ends' });
      await page.settle();
      // The phone's navbar is sticky and tall: bring the row to the middle first.
      // (Instant scrolling, so the click lands where the button is.)
      await page.eval((v) => {
        document.documentElement.style.scrollBehavior = 'auto';
        document.querySelector(`[data-bs-visit="${v}"]`).scrollIntoView({ block: 'center' });
      }, visit);
      await page.settle();
      // Clicked from the page's own script: after the reloads above, Chrome
      // stopped delivering synthetic mouse input to this tab (no event reached
      // the document), while the kit's handler runs the same either way.
      await page.eval((v) => document.querySelector(`[data-rx-open="skip-${v}"]`).click(), visit);
      await page.waitFor((v) => document.getElementById(`skip-${v}`)?.open, { message: 'the confirmation' }, visit);
      // The sheet slides up; click once it stands still.
      await page.waitFor(() => document.getAnimations().every((a) => a.playState !== 'running'), { message: 'the sheet is open' });
      await page.eval((v) => document.querySelector(`#skip-${v} button.rx-button--danger`).click(), visit);
      // The confirmation posts a plain form and the page loads again: while
      // the next document is being parsed it has no <body> yet.
      await page.waitFor((v) => !document.querySelector(`#upcoming-visits [data-bs-visit="${v}"]`) && /Skipped/.test(document.body?.textContent || ''), { message: 'the visit is skipped' }, visit);
      await page.settle();
      page.assertClean();
    }, PHONE));
});
