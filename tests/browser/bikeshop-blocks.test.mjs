// The blocks on the bike shop's /about/blocks page, in a browser: every block
// at 1280 and 390 px wide, light and dark, under the default CSP and
// CSP=strict, with a clean console; then what the page itself does with
// them: the range slider sent in a GET form, the date and time range and the
// shop's own date picker with closed days, the variant chips (htmx), the
// whole form sent through Valid<T>, the calendar's months (htmx) and a free
// slot. Each renox-blocks block's own keyboard, pointer, contrast and motion
// checks are tests/browser/blocks.test.mjs. Screenshots go to
// BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser, sleep } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PHONE = { width: 390, height: 844 };
const PAGE = '/about/blocks';
let browser;

before(async () => {
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

const fitsWidth = (page) => page.eval(() => document.documentElement.scrollWidth <= window.innerWidth);

/** The browser's local date, as the page's scripts see it, `days` from today. */
const isoIn = (page, days) =>
  page.eval((n) => {
    const d = new Date();
    d.setDate(d.getDate() + n);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }, days);

const BLOCKS = [
  '[data-rx-gallery][data-rx-blocks-ready]',
  '[data-rx-range][data-rx-blocks-ready]',
  '[data-rx-quantity][data-rx-blocks-ready]',
  '[data-rx-keypad][data-rx-blocks-ready]',
  '[data-rx-kanban][data-rx-blocks-ready]',
  '.rx-month',
  '.rx-availability',
  '[data-rx-datetime-range][data-rx-blocks-ready]',
  '.rx-swatches--size',
  '.rx-swatches--colour',
  '.rx-history',
  '.rx-plans',
];

/** Waits until every block's code has set it up (the loader imports it). */
const ready = async (page) => {
  for (const selector of BLOCKS) await page.waitFor((s) => !!document.querySelector(s), { message: `${selector} is set up` }, selector);
};

for (const csp of ['relaxed', 'strict']) {
  describe(`bikeshop blocks under CSP=${csp}`, () => {
    let app;
    before(async () => {
      app = await start('bikeshop', 'examples/bikeshop', { env: { CSP: csp } });
    });
    after(() => app?.stop());

    for (const [size, options] of [
      ['desktop', undefined],
      ['phone', PHONE],
    ]) {
      for (const scheme of ['light', 'dark']) {
        test(`every block is on the page (${size}, ${scheme})`, () =>
          browser.with(async (page) => {
            await page.send('Emulation.setEmulatedMedia', {
              features: [{ name: 'prefers-color-scheme', value: scheme }],
            });
            await page.goto(`${app.url}${PAGE}`);
        await ready(page);
            for (const selector of BLOCKS) {
              await page.waitFor((s) => !!document.querySelector(s), { message: `${selector} is set up` }, selector);
            }
            // The scheme reached the kit's tokens (and so the blocks).
            const surface = await page.eval(() => getComputedStyle(document.querySelector('.rx-plan')).backgroundColor);
            // The shop's surface colour (public/theme.css: #fffdf9, dark #1d1b18).
            assert.equal(surface, scheme === 'dark' ? 'rgb(29, 27, 24)' : 'rgb(255, 253, 249)', 'cards follow the scheme');
            assert.ok(await fitsWidth(page), 'no sideways scrolling');
            if (size === 'phone') {
              // The month becomes a list of the days with something on.
              const shown = await page.eval(() =>
                [...document.querySelectorAll('.rx-month__day')].filter((d) => getComputedStyle(d).display !== 'none').length,
              );
              assert.ok(shown >= 5 && shown <= 7, `${shown} days listed on a phone`);
              assert.equal(await page.eval(() => getComputedStyle(document.querySelector('.rx-month__weekdays')).display), 'none');
            } else {
              assert.equal(
                await page.eval(() => getComputedStyle(document.querySelector('.rx-month__days')).gridTemplateColumns.split(' ').length),
                7,
                'seven columns on a desktop',
              );
            }
            await shot(page, `blocks-${size}-${scheme}-${csp}`);
            page.assertClean();
          }, options));
      }
    }

    test('the range slider: keys move each handle, they never cross, a GET form sends both', () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}${PAGE}`);
        await ready(page);
        const low = '#rx-price_min-range-min';
        await page.focus(low);
        await page.press('ArrowRight');
        assert.equal(await page.eval((s) => document.querySelector(s).value, low), '125000');
        assert.match(await page.eval((s) => document.querySelector(s).getAttribute('aria-valuetext'), low), /\$1[.,]250[.,]00\b/);
        // End would pass the other handle: it stops there.
        await page.press('End');
        assert.equal(await page.eval((s) => document.querySelector(s).value, low), '500000');
        await page.press('PageDown');
        await page.press('Home');
        assert.equal(await page.eval((s) => document.querySelector(s).value, low), '0');
        await page.focus('#rx-price_min-range-max');
        await page.press('ArrowLeft');
        assert.equal(await page.eval(() => document.querySelector('#rx-price_min-range-max').value), '475000');
        assert.match(await page.text('[data-rx-range-shown="max"]'), /\$4[.,]750[.,]00\b/);
        await page.click('#filter button[type=submit]');
        await page.waitFor(() => location.search.includes('price_max='));
        const query = await page.eval(() => location.search);
        assert.match(query, /price_min=0/);
        assert.match(query, /price_max=475000/);
        assert.ok(await page.text('[data-bs-filtering]'));
        page.assertClean();
      }));

    test('the date and time range fills its two fields and refuses an end before its start', () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}${PAGE}`);
        await ready(page);
        const day = await isoIn(page, 7);
        const next = await isoIn(page, 8);
        await page.type('#rx-starts_at-range-start-date', day);
        await page.eval(() => {
          const s = document.querySelector('#rx-starts_at-range-start-time');
          s.value = '10:00';
          s.dispatchEvent(new Event('change', { bubbles: true }));
        });
        await page.type('#rx-starts_at-range-end-date', next);
        await page.focus('#rx-starts_at-range-end-time');
        // A select answers to the keyboard: pick 12:00.
        await page.eval(() => {
          const s = document.querySelector('#rx-starts_at-range-end-time');
          s.value = '12:00';
          s.dispatchEvent(new Event('change', { bubbles: true }));
        });
        assert.equal(await page.eval(() => document.querySelector('[name=starts_at]').value), `${day}T10:00`);
        assert.equal(await page.eval(() => document.querySelector('[name=ends_at]').value), `${next}T12:00`);
        assert.equal(await page.text('[data-rx-datetime-summary]'), '1 day 2 hours');
        // The end's calendar starts at the start's day.
        assert.equal(await page.eval(() => document.querySelector('#rx-starts_at-range-end-date-calendar calendar-date').getAttribute('min')), day);
        // An end before the start is pointed out at once.
        await page.type('#rx-starts_at-range-end-date', day, { clear: true });
        await page.eval(() => {
          const s = document.querySelector('#rx-starts_at-range-end-time');
          s.value = '09:00';
          s.dispatchEvent(new Event('change', { bubbles: true }));
        });
        assert.equal(await page.eval(() => document.querySelector('[data-rx-datetime-summary]').hasAttribute('data-rx-datetime-invalid')), true);
        assert.equal(await page.eval(() => document.querySelector('#rx-starts_at-range-end-time').getAttribute('aria-invalid')), 'true');

        page.assertClean();
      }));

    test('the variant chips ask the server for the price and the stock', () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}${PAGE}`);
        await ready(page);
        assert.match(await page.text('[data-bs-variant-stock]'), /6 in stock/);
        await page.click('.rx-swatches--size .rx-swatches__option:nth-child(3) .rx-swatches__chip');
        await page.waitFor(() => document.querySelector('[name=size]:checked').value === 'L');
        await page.waitFor(() => /3 in stock/.test(document.querySelector('[data-bs-variant-stock]').textContent));
        // With the keyboard: the arrows choose the next colour.
        await page.focus('.rx-swatches--colour [name=colour]:checked');
        await page.press('ArrowRight');
        await page.press('ArrowRight');
        assert.equal(await page.eval(() => document.querySelector('[name=colour]:checked').value), 'graphite');
        await page.waitFor(() => /1 in stock/.test(document.querySelector('[data-bs-variant-stock]').textContent));
        // The sold-out size can't be chosen.
        assert.equal(await page.eval(() => document.querySelector('[name=size][value=XL]').disabled), true);
        page.assertClean();
      }));

    test('the form blocks are sent and accepted by Valid<T>', () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}${PAGE}`);
        await ready(page);
        const day = await isoIn(page, 1);
        await page.click('[data-rx-quantity] [data-rx-quantity-step="1"]');
        await page.focus('[data-rx-keypad] [data-rx-keypad-key="7"]');
        await page.press('4');
        await page.press('2');
        await page.type('#rx-starts_at-range-start-date', day);
        await page.type('#rx-starts_at-range-end-date', day);
        await page.eval(() => {
          for (const [id, v] of [['start', '10:00'], ['end', '15:30']]) {
            const s = document.querySelector(`#rx-starts_at-range-${id}-time`);
            s.value = v;
            s.dispatchEvent(new Event('change', { bubbles: true }));
          }
        });
        // The keypad's Enter key sends the form.
        await page.click('[data-rx-keypad] [data-rx-keypad-key="enter"]');
        await page.waitFor(() => location.hash === '#form' && document.querySelector('.rx-alert'));
        const status = await page.text('.rx-alert');
        assert.match(status, /The server accepted: 2 × M teal, paid 42/);
        assert.match(status, new RegExp(`${day} 10:00 → ${day} 15:30`));
        page.assertClean();
      }));

    test('the calendar changes month in place; a free slot is a link', () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}${PAGE}`);
        await ready(page);
        await page.eval(() => (window.__same = true));
        const title = await page.text('.rx-month__title');
        await page.click('[data-rx-month-next]');
        await page.waitFor((t) => document.querySelector('.rx-month__title').textContent.trim() !== t, {}, title);
        assert.equal(await page.eval(() => window.__same), true, 'no page load');
        await page.settle();
        assert.match(await page.eval(() => location.search), /month=\d{4}-\d{2}/);
        await page.click('[data-rx-month-prev]');
        await page.waitFor((t) => document.querySelector('.rx-month__title').textContent.trim() === t, {}, title);
        assert.ok(await page.eval(() => document.querySelector('.rx-month__day--today')), 'today is marked');

        // htmx scrolls a boosted swap into view: wait until the page is still.
        await page.settle();
        await sleep(400);
        await page.click('.rx-availability__book');
        await page.waitFor(() => location.search.includes('slot='));
        assert.match(await page.text('#availability .rx-alert'), /Booking City 3 11:00/);
        page.assertClean();
      }));
  });
}
