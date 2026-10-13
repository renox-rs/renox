// The bike shop's workshop (#236) in a browser, on the seeded demo shop: a
// customer books a service on a phone (390 px), the estimate following the
// form through htmx; North's manager moves a work order across the board
// with the keyboard (desktop) and the board fits a phone; the work order's
// page. Every page with a clean console and no sideways scroll.
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

describe('booking a service', () => {
  test('a customer books a tune-up on a phone', () =>
    browser.with(async (page) => {
      await login(page, 'customer@bikeshop.test');
      await page.goto(`${app.url}/service/book`);
      assert.ok(await page.eval(() => document.querySelectorAll('#rx-bike option').length > 0), 'the customer has bikes');
      // Pick the tune-up package: htmx swaps in the estimate.
      await page.eval(() => {
        const radio = document.querySelector('input[name=package][value="tune-up"]');
        radio.checked = true;
        radio.dispatchEvent(new Event('change', { bubbles: true }));
      });
      await page.waitFor(() => /min\)/.test(document.querySelector('#booking-live')?.textContent || ''), { message: 'the estimate' });
      // The first day from the day after tomorrow that is neither full nor closed.
      const day = await page.eval(() => {
        const box = document.querySelector('#rx-day');
        const blocked = JSON.parse(box.dataset.rxDisabledDates || '[]');
        const closed = JSON.parse(box.dataset.rxClosedWeekdays || '[]');
        for (let n = 2; n < 40; n++) {
          const d = new Date(Date.now() + n * 86400_000);
          const iso = d.toISOString().slice(0, 10);
          if (!blocked.includes(iso) && !closed.includes(d.getUTCDay())) return iso;
        }
        return null;
      });
      assert.ok(day, 'a day with room');
      await page.eval((d) => {
        const input = document.querySelector('#rx-day');
        input.value = d;
      }, day);
      assert.ok(await fitsWidth(page), 'the booking form fits a phone');
      await shot(page, 'workshop-book-phone');
      await page.eval(() => document.querySelector('#book-form').requestSubmit());
      await page.waitFor(() => /^\/service\/\d+$/.test(location.pathname), { message: 'the work order page' });
      const text = await page.text('main');
      assert.match(text, /Scheduled/);
      assert.ok(await fitsWidth(page));
      await shot(page, 'workshop-service-phone');
      page.assertClean();
    }, PHONE));
});

describe('the workshop board', () => {
  test("North's manager moves a work order with the keyboard (desktop)", () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/workshop`);
      await page.waitFor(() => !!document.querySelector('[data-rx-kanban][data-rx-blocks-ready]'), { message: 'the board is ready' });
      const card = await page.eval(() => document.querySelector('[data-rx-kanban-column="booked"] [data-rx-kanban-card]')?.dataset.rxKanbanCard);
      assert.ok(card, 'a scheduled work order to check in');
      await page.eval(() => {
        window.__moves = [];
        document.addEventListener('rx:kanban-moved', (e) => window.__moves.push(e.detail));
      });
      await page.focus(`[data-rx-kanban-card="${card}"]`);
      await page.press(' ');
      await page.press('ArrowRight');
      await page.press(' ');
      await page.waitFor(() => window.__moves.length === 1, { message: 'the move answered' });
      const move = await page.eval(() => window.__moves[0]);
      assert.equal(move.column, 'checked_in');
      assert.equal(move.ok, true, 'the server accepted it');
      await shot(page, 'workshop-board-desktop');
      // Its page: the bench.
      await page.goto(`${app.url}/staff/workshop/${card}`);
      const text = await page.text('main');
      for (const part of ['Checklist', 'Parts', 'Notes and photos', 'Checked in']) {
        assert.ok(text.includes(part), `the work order shows "${part}"`);
      }
      assert.ok(await fitsWidth(page));
      await shot(page, 'workshop-order-desktop');
      page.assertClean();
    }));

  test('the board on a phone', () =>
    browser.with(async (page) => {
      await login(page, 'manager.north@bikeshop.test');
      await page.goto(`${app.url}/staff/workshop`);
      await page.waitFor(() => !!document.querySelector('[data-rx-kanban][data-rx-blocks-ready]'), { message: 'the board is ready' });
      assert.ok(await fitsWidth(page), 'the columns scroll inside the board, not the page');
      await shot(page, 'workshop-board-phone');
      page.assertClean();
    }, PHONE));
});
