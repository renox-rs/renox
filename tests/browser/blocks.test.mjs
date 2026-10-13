// renox-blocks (crates/renox-blocks) in a browser, on the bike shop's
// /about/blocks gallery, which shows every block: each one at 1280 and 390
// px wide, light and dark, with WCAG AA contrast and no sideways scrolling;
// then each block with the keyboard (and the pointer where it has one):
// quantity, range_slider, keypad, swatches, datetime_range, gallery,
// history, compare_plans, month_calendar, availability and kanban; and
// prefers-reduced-motion. The loader imports only the code a page needs.
// Screenshots go to BLOCKS_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser, sleep } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PHONE = { width: 390, height: 844 };
const PAGE = '/about/blocks';
let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { CSP: 'strict' } });
});

after(async () => {
  app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BLOCKS_SCREENS) await page.screenshot(join(process.env.BLOCKS_SCREENS, `${name}.png`));
}

const fitsWidth = (page) => page.eval(() => document.documentElement.scrollWidth <= window.innerWidth);

const scheme = (page, value) =>
  page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value }] });

/** Every block, set up by its code when it has some. */
const BLOCKS = {
  quantity: '[data-rx-quantity][data-rx-blocks-ready]',
  range_slider: '[data-rx-range][data-rx-blocks-ready]',
  keypad: '[data-rx-keypad][data-rx-blocks-ready]',
  swatches: '.rx-swatches--size',
  datetime_range: '[data-rx-datetime-range][data-rx-blocks-ready]',
  gallery: '[data-rx-gallery][data-rx-blocks-ready]',
  history: '[data-rx-history]',
  compare_plans: '.rx-plans',
  month_calendar: '.rx-month',
  availability: '.rx-availability',
  kanban: '[data-rx-kanban][data-rx-blocks-ready]',
};

/** Text that must read at WCAG AA (4.5:1) on what is behind it. */
const TEXTS = [
  '.rx-quantity__input',
  '.rx-range__output',
  '.rx-range__limits span',
  '.rx-keypad__key',
  '.rx-keypad__key--tool',
  '.rx-keypad__key--enter',
  '.rx-swatches__label',
  '.rx-swatches__note',
  '.rx-gallery__caption',
  '.rx-history__meta',
  '.rx-history__title',
  '.rx-plan__interval',
  '.rx-plan__description',
  '.rx-plans__compare th[scope=row]',
  '.rx-month__weekdays span',
  '.rx-month__event',
  '.rx-month__today',
  '.rx-availability__book',
  '.rx-availability__slot--booked .rx-availability__label',
  '.rx-availability__slot--closed .rx-availability__label',
  '.rx-availability__note',
  '.rx-kanban__subtitle',
  '.rx-kanban__title',
];

/** The contrast of each selector's first element: its colour against the
 *  backgrounds behind it, composited, every colour read through a canvas. */
const contrasts = (page, selectors) =>
  page.eval((list) => {
    const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
    const rgba = (css) => {
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = '#000';
      ctx.fillStyle = css;
      ctx.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
      return [r, g, b, a / 255];
    };
    const over = (top, bottom) => top.slice(0, 3).map((c, i) => c * top[3] + bottom[i] * (1 - top[3])).concat(1);
    const backdrop = (el) => {
      const layers = [];
      for (let node = el; node; node = node.parentElement) {
        const style = getComputedStyle(node);
        const bg = rgba(style.backgroundColor);
        if (style.backgroundImage !== 'none' && node !== document.body) {
          // A striped slot: take its first stripe's colour.
          const first = style.backgroundImage.match(/(rgb|color|oklch|oklab)\([^)]*\)/);
          if (first) layers.push(rgba(first[0]));
        }
        if (bg[3] > 0) layers.push(bg);
        if (bg[3] === 1) break;
      }
      return layers.reverse().reduce((acc, layer) => over(layer, acc), [255, 255, 255, 1]);
    };
    const lum = ([r, g, b]) =>
      [r, g, b]
        .map((v) => v / 255)
        .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
        .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
    return list.map((selector) => {
      const el = document.querySelector(selector);
      if (!el) return [selector, null];
      const bg = backdrop(el);
      const fg = over(rgba(getComputedStyle(el).color), bg);
      const [a, b] = [lum(fg), lum(bg)].sort((x, y) => y - x);
      return [selector, Math.round(((a + 0.05) / (b + 0.05)) * 100) / 100];
    });
  }, selectors);

describe('renox-blocks', () => {
  for (const [size, options] of [
    ['desktop', undefined],
    ['phone', PHONE],
  ]) {
    for (const mode of ['light', 'dark']) {
      test(`every block, ${size}, ${mode}: set up, readable, no sideways scrolling`, () =>
        browser.with(async (page) => {
          await scheme(page, mode);
          await page.goto(`${app.url}${PAGE}`);
          for (const [name, selector] of Object.entries(BLOCKS)) {
            await page.waitFor((s) => !!document.querySelector(s), { message: `${name} is set up` }, selector);
          }
          assert.ok(await fitsWidth(page), 'no sideways scrolling');
          // Each block fits its card (wide ones scroll inside their own frame).
          const overflowing = await page.eval(() =>
            ['.rx-quantity-field', '.rx-range', '.rx-keypad', '.rx-swatches', '.rx-datetime-range', '.rx-gallery', '.rx-history', '.rx-plans', '.rx-month', '.rx-availability', '.rx-kanban']
              .filter((s) => {
                const el = document.querySelector(s);
                const card = el.closest('.rx-card') || document.body;
                return el.getBoundingClientRect().right > card.getBoundingClientRect().right + 1;
              }),
          );
          assert.deepEqual(overflowing, [], 'blocks inside their cards');
          const low = (await contrasts(page, TEXTS)).filter(([, ratio]) => ratio !== null && ratio < 4.5);
          assert.deepEqual(low, [], 'text at WCAG AA');
          // The blocks follow the scheme: a plan card's surface is dark in dark mode.
          const surface = await page.eval(() => {
            const [r, g, b] = getComputedStyle(document.querySelector('.rx-plan')).backgroundColor.match(/\d+/g).map(Number);
            return (r + g + b) / 3;
          });
          assert.ok(mode === 'dark' ? surface < 80 : surface > 200, `the surface follows the scheme (${surface})`);
          if (size === 'phone') {
            // The month becomes a list of the days with something on.
            assert.equal(await page.eval(() => getComputedStyle(document.querySelector('.rx-month__weekdays')).display), 'none');
            const listed = await page.eval(() => [...document.querySelectorAll('.rx-month__day')].filter((d) => getComputedStyle(d).display !== 'none').length);
            assert.ok(listed >= 5 && listed <= 7, `${listed} days listed`);
            // Every key, button and slot is a comfortable target.
            const small = await page.eval(() =>
              [...document.querySelectorAll('.rx-keypad__key, .rx-quantity__button, .rx-swatches__chip, .rx-availability__book, .rx-gallery__arrow')]
                .filter((el) => el.getClientRects().length && el.getBoundingClientRect().height < 40)
                .map((el) => el.className),
            );
            assert.deepEqual(small, [], 'touch targets');
          } else {
            const columns = await page.eval(() => getComputedStyle(document.querySelector('.rx-month__days')).gridTemplateColumns.split(' ').length);
            assert.equal(columns, 7, 'seven columns on a desktop');
          }
          await shot(page, `blocks-${size}-${mode}`);
          page.assertClean();
        }, options));
    }
  }

  test('the loader imports only the code a page needs', () =>
    browser.with(async (page) => {
      // The catalogue has a range slider and no other block with code.
      await page.goto(`${app.url}/shop/bikes`);
      await page.waitFor(() => !!document.querySelector('[data-rx-range][data-rx-blocks-ready]'));
      await sleep(200);
      const loaded = await page.eval(() =>
        performance
          .getEntriesByType('resource')
          .map((r) => new URL(r.name).pathname)
          .filter((p) => p.startsWith('/_renox/blocks/'))
          .map((p) => p.split('/').pop().replace(/-[0-9a-f]{16}\./, '.'))
          .sort(),
      );
      assert.deepEqual(loaded, ['blocks.css', 'blocks.js', 'range.js']);
      page.assertClean();
    }));

  test('quantity: the field\'s arrow keys, the buttons, the limits', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('[data-rx-quantity][data-rx-blocks-ready]'));
      const qty = () => page.eval(() => document.querySelector('#rx-quantity').value);
      const plus = '[data-rx-quantity] [data-rx-quantity-step="1"]';
      const minus = '[data-rx-quantity] [data-rx-quantity-step="-1"]';
      assert.equal(await page.eval((s) => document.querySelector(s).disabled, minus), true, 'at the minimum');
      assert.equal(await page.eval((s) => document.querySelector(s).tabIndex, plus), -1, 'the buttons are skipped by Tab');
      await page.focus('#rx-quantity');
      await page.press('ArrowUp');
      await page.press('ArrowUp');
      assert.equal(await qty(), '3');
      assert.equal(await page.eval((s) => document.querySelector(s).disabled, minus), false);
      for (let i = 0; i < 3; i++) await page.click(plus);
      assert.equal(await qty(), '5');
      assert.equal(await page.eval((s) => document.querySelector(s).disabled, plus), true, 'at the maximum');
      await page.focus('#rx-quantity');
      await page.press('ArrowDown');
      assert.equal(await qty(), '4');
      page.assertClean();
    }));

  test('range_slider: the keys move each handle; they never cross', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('[data-rx-range][data-rx-blocks-ready]'));
      const low = '#rx-price_min-range-min';
      const high = '#rx-price_min-range-max';
      const value = (s) => page.eval((sel) => document.querySelector(sel).value, s);
      await page.focus(low);
      await page.press('ArrowRight');
      assert.equal(await value(low), '125000');
      assert.match(await page.eval((s) => document.querySelector(s).getAttribute('aria-valuetext'), low), /\$1[.,]250[.,]00\b/);
      await page.press('End');
      assert.equal(await value(low), '500000', 'stops at the other handle');
      await page.press('Home');
      assert.equal(await value(low), '0');
      await page.press('Tab');
      assert.equal(await page.eval(() => document.activeElement.id), 'rx-price_min-range-max', 'Tab goes to the other handle');
      await page.press('PageDown');
      assert.ok(Number(await value(high)) < 500000);
      assert.match(await page.text('[data-rx-range-shown="max"]'), /\$/);
      // The chosen part is painted between the handles.
      const from = await page.eval(() => getComputedStyle(document.querySelector('[data-rx-range-track]')).getPropertyValue('--rx-range-from').trim());
      assert.equal(from, '0');
      page.assertClean();
    }));

  test('keypad: one Tab stop, arrows between keys, digits typed on it', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('[data-rx-keypad][data-rx-blocks-ready]'));
      const paid = () => page.eval(() => document.querySelector('#rx-paid').value);
      const key = () => page.eval(() => document.activeElement.dataset.rxKeypadKey);
      await page.focus('#rx-paid');
      await page.press('Tab');
      assert.equal(await key(), '7', 'Tab lands on the first key');
      await page.press('3');
      await page.press('5');
      assert.equal(await paid(), '35');
      await page.press('ArrowRight');
      assert.equal(await key(), '8');
      await page.press('ArrowDown');
      assert.equal(await key(), '5');
      await page.press('Enter');
      assert.equal(await paid(), '355');
      await page.press('End');
      assert.equal(await key(), 'enter');
      await page.press('Home');
      await page.press('Backspace');
      assert.equal(await paid(), '35');
      await page.click('[data-rx-keypad] [data-rx-keypad-key="00"]');
      assert.equal(await paid(), '3500');
      assert.equal(
        await page.eval(() => [...document.querySelectorAll('[data-rx-keypad-key]')].filter((k) => k.tabIndex === 0).length),
        1,
        'one Tab stop',
      );
      for (const type of ['rawKeyDown', 'keyUp']) {
        await page.send('Input.dispatchKeyEvent', { type, key: 'Delete', code: 'Delete', windowsVirtualKeyCode: 46 });
      }
      assert.equal(await paid(), '', 'Delete clears');
      page.assertClean();
    }));

  test('swatches: the arrow keys choose, a sold-out chip is skipped', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.focus('.rx-swatches--size [name=size]:checked');
      await page.press('ArrowRight');
      assert.equal(await page.eval(() => document.querySelector('[name=size]:checked').value), 'L');
      await page.press('ArrowRight');
      assert.equal(await page.eval(() => document.querySelector('[name=size]:checked').value), 'S', 'XL is sold out');
      // The focus ring shows on the chip.
      const ring = await page.eval(() => getComputedStyle(document.querySelector('[name=size]:checked + .rx-swatches__chip')).outlineStyle);
      assert.equal(ring, 'solid');
      await page.focus('.rx-swatches--colour [name=colour]:checked');
      await page.press('ArrowLeft');
      assert.equal(await page.eval(() => document.querySelector('[name=colour]:checked').value), 'graphite');
      page.assertClean();
    }));

  test('datetime_range: typed days and chosen times fill the two fields', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('[data-rx-datetime-range][data-rx-blocks-ready]'));
      const iso = (n) =>
        page.eval((days) => {
          const d = new Date();
          d.setDate(d.getDate() + days);
          return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
        }, n);
      const day = await iso(7);
      const next = await iso(8);
      await page.type('#rx-starts_at-range-start-date', day);
      // A closed select answers the arrow keys: the first time, then the next.
      await page.focus('#rx-starts_at-range-start-time');
      await page.press('ArrowDown');
      await page.press('ArrowDown');
      assert.equal(await page.eval(() => document.querySelector('#rx-starts_at-range-start-time').value), '09:30');
      await page.type('#rx-starts_at-range-end-date', next);
      await page.eval(() => {
        const s = document.querySelector('#rx-starts_at-range-end-time');
        s.value = '12:00';
        s.dispatchEvent(new Event('change', { bubbles: true }));
      });
      assert.equal(await page.eval(() => document.querySelector('[name=starts_at]').value), `${day}T09:30`);
      assert.equal(await page.eval(() => document.querySelector('[name=ends_at]').value), `${next}T12:00`);
      assert.equal(await page.text('[data-rx-datetime-summary]'), '1 day 2 hours 30 min');
      assert.equal(
        await page.eval(() => document.querySelector('#rx-starts_at-range-end-date-calendar calendar-date').getAttribute('min')),
        day,
        'the end\'s calendar starts at the start\'s day',
      );
      // An end before the start is pointed out at once.
      await page.type('#rx-starts_at-range-end-date', day, { clear: true });
      await page.eval(() => {
        const s = document.querySelector('#rx-starts_at-range-end-time');
        s.value = '09:00';
        s.dispatchEvent(new Event('change', { bubbles: true }));
      });
      assert.equal(await page.eval(() => document.querySelector('[data-rx-datetime-summary]').hasAttribute('data-rx-datetime-invalid')), true);
      assert.equal(await page.eval(() => document.querySelector('#rx-starts_at-range-end-time').getAttribute('aria-invalid')), 'true');
      assert.match(await page.text('[data-rx-datetime-summary]'), /end must come after the start/);
      page.assertClean();
    }));

  test('gallery: arrows, thumbnails, keys, a drag and the enlarged photo', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('#demo-gallery[data-rx-blocks-ready]'));
      const index = () => page.eval(() => document.querySelector('#demo-gallery').dataset.rxGalleryIndex);
      assert.equal(await index(), '0');
      assert.equal(await page.eval(() => document.querySelector('[data-rx-gallery-prev]').disabled), true);
      await page.click('#demo-gallery [data-rx-gallery-next]');
      assert.equal(await index(), '1');
      assert.equal(await page.eval(() => document.querySelector('[data-rx-gallery-thumb="1"]').getAttribute('aria-current')), 'true');
      // Only the current photo is reachable by screen readers.
      assert.equal(await page.eval(() => document.querySelectorAll('#demo-gallery .rx-gallery__slide[inert]').length), 3);
      await page.click('[data-rx-gallery-thumb="3"]');
      assert.equal(await index(), '3');
      assert.equal(await page.eval(() => location.hash), '', 'a thumbnail does not jump the page');
      await page.focus('#demo-gallery [data-rx-gallery-viewport]');
      await page.press('Home');
      assert.equal(await index(), '0');
      await page.press('ArrowRight');
      await page.press('ArrowRight');
      assert.equal(await index(), '2');
      await page.press('End');
      assert.equal(await index(), '3');
      await page.press('ArrowLeft');
      assert.equal(await index(), '2');
      assert.match(await page.text('[data-rx-gallery-status]'), /Photo 3 of 4/);
      // A thumbnail with the focus follows the photo.
      await page.focus('[data-rx-gallery-thumb="2"]');
      await page.press('ArrowRight');
      assert.equal(await page.eval(() => document.activeElement.dataset.rxGalleryThumb), '3');
      // A drag to the right shows the photo before, and it ends in place.
      const box = await page.point('#demo-gallery [data-rx-gallery-viewport]');
      await drag(page, '#demo-gallery [data-rx-gallery-viewport]', { x: box.x + box.w / 2, y: box.y });
      await page.waitFor(() => document.querySelector('#demo-gallery').dataset.rxGalleryIndex === '2');
      await page.waitFor(() => {
        const width = document.querySelector('[data-rx-gallery-viewport]').clientWidth;
        return document.querySelector('[data-rx-gallery-track]').style.transform === `translateX(${-2 * width}px)`;
      });
      // Enlarge: the kit's sheet with the current photo; Escape closes it.
      await page.click('#demo-gallery [data-rx-gallery-enlarge]');
      await page.waitFor(() => document.querySelector('#demo-gallery-zoom').open);
      assert.match(await page.eval(() => document.querySelector('[data-rx-gallery-zoom]').src), /city-detail\.svg$/);
      await page.press('Escape');
      await page.waitFor(() => !document.querySelector('#demo-gallery-zoom').open);
      assert.equal(await page.eval(() => document.activeElement.hasAttribute('data-rx-gallery-enlarge')), true, 'focus back on the button');
      page.assertClean();
    }));

  test('history and compare_plans: lists screen readers read in order', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      const items = await page.eval(() => [...document.querySelectorAll('.rx-history > li')].map((li) => li.querySelector('time').getAttribute('datetime')));
      assert.ok(items.length >= 3);
      // Each kind says itself in words, not only a colour.
      assert.ok(await page.eval(() => [...document.querySelectorAll('.rx-history .rx-visually-hidden')].some((s) => /done/.test(s.textContent))));
      // The plans' table is a scrolling region the keyboard reaches.
      assert.equal(await page.eval(() => document.querySelector('.rx-plans__table').tabIndex), 0);
      assert.equal(await page.eval(() => document.querySelectorAll('.rx-plan--highlight').length), 1);
      assert.ok(await page.eval(() => [...document.querySelectorAll('.rx-plans__compare .rx-visually-hidden')].some((s) => s.textContent === 'Included')));
      page.assertClean();
    }));

  test('compare_plans and availability scroll inside their frames on a phone, by keyboard', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      for (const frame of ['.rx-plans__table', '.rx-availability']) {
        await page.eval((s) => document.querySelector(s).scrollIntoView({ block: 'center' }), frame);
        await page.focus(frame);
        assert.ok(await page.eval((s) => document.querySelector(s).scrollWidth > document.querySelector(s).clientWidth, frame), `${frame} is wider`);
        for (let i = 0; i < 5; i++) await page.press('ArrowRight');
        await sleep(300);
        assert.ok(await page.eval((s) => document.querySelector(s).scrollLeft > 0, frame), `${frame} scrolled`);
      }
      // The first column stays put while the rest scrolls.
      assert.equal(await page.eval(() => getComputedStyle(document.querySelector('.rx-availability__resource')).position), 'sticky');
      assert.ok(await fitsWidth(page));
      page.assertClean();
    }, PHONE));

  test('month_calendar and availability: links the keyboard reaches', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      const title = await page.text('.rx-month__title');
      await page.focus('[data-rx-month-next]');
      await page.press('Enter');
      await page.waitFor((t) => document.querySelector('.rx-month__title')?.textContent.trim() !== t, {}, title);
      await page.settle();
      assert.match(await page.eval(() => location.search), /month=\d{4}-\d{2}/);
      // Another month links back to today's.
      assert.ok(await page.eval(() => [...document.querySelectorAll('.rx-month__nav a')].some((a) => a.textContent.trim() === 'Today')));
      // Each weekday is said in full to screen readers.
      assert.match(await page.text('.rx-month__day .rx-month__weekday'), /day$/);
      // A free slot says what it books, and Enter books it.
      const label = await page.eval(() => document.querySelector('.rx-availability__book').getAttribute('aria-label'));
      assert.match(label, /^Book .+ at \d\d:00$/);
      await page.focus('.rx-availability__book');
      await page.press('Enter');
      await page.waitFor(() => location.search.includes('slot='));
      page.assertClean();
    }));

  test('kanban: keyboard moves, a drag, a refused move put back', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('[data-rx-kanban][data-rx-blocks-ready] [data-rx-kanban-card][tabindex="0"]'));
      await watchMoves(page);
      const live = () => page.text('[data-rx-kanban-live]');
      await page.focus('[data-rx-kanban-card="101"]');
      await page.press(' ');
      assert.match(await live(), /Picked up Tune-up, in Waiting, position 1 of 3/);
      assert.equal(await page.eval(() => document.querySelector('[data-rx-kanban-card="101"]').getAttribute('aria-grabbed')), 'true');
      await page.press('ArrowRight');
      assert.equal(await columnOf(page, 101), 'working');
      assert.match(await live(), /Tune-up: In the stand, position 1 of 3/);
      await page.press('ArrowDown');
      assert.match(await live(), /position 2 of 3/);
      assert.equal(await page.eval(() => document.activeElement.dataset.rxKanbanCard), '101', 'the card keeps the focus');
      await page.press(' ');
      assert.match(await live(), /Tune-up dropped in In the stand, position 2 of 3/);
      await page.waitFor(() => window.__moves.length === 1);
      assert.deepEqual(await page.eval(() => window.__moves[0]), { card: '101', column: 'working', position: 1, ok: true });
      assert.equal(await page.eval(() => document.querySelector('[data-rx-kanban-column="working"] [data-rx-kanban-count]').textContent), '3');

      // Escape puts a picked-up card back.
      await page.focus('[data-rx-kanban-card="103"]');
      await page.press('Enter');
      await page.press('ArrowRight');
      await page.press('ArrowRight');
      assert.equal(await columnOf(page, 103), 'ready');
      await page.press('Escape');
      assert.equal(await columnOf(page, 103), 'waiting');
      assert.match(await live(), /Move cancelled/);

      // Without holding a card, the arrows move the focus.
      await page.focus('[data-rx-kanban-card="102"]');
      await page.press('ArrowDown');
      assert.equal(await page.eval(() => document.activeElement.dataset.rxKanbanCard), '103');

      // A drag with the mouse into "Ready for pickup".
      await page.eval(() => document.querySelector('#kanban').scrollIntoView({ block: 'center' }));
      await page.settle();
      const target = await page.point('[data-rx-kanban-column="ready"] [data-rx-kanban-list]');
      await drag(page, '[data-rx-kanban-card="102"] .rx-kanban__body', { x: target.x, y: target.y + target.h / 2 - 4 });
      await page.waitFor(() => window.__moves.length === 2);
      assert.equal(await columnOf(page, 102), 'ready');
      assert.equal(await page.eval(() => window.__moves[1].ok), true);
      assert.equal(await page.eval(() => document.querySelector('[data-rx-kanban-card="102"]').getAttribute('style') || ''), '');

      // The server refuses a move (a column it doesn't know): the card goes back.
      await page.eval(() => document.querySelector('[data-rx-kanban-column="ready"]').setAttribute('data-rx-kanban-column', 'nowhere'));
      await page.focus('[data-rx-kanban-card="104"]');
      await page.press(' ');
      await page.press('ArrowRight');
      await page.press(' ');
      await page.waitFor(() => window.__moves.length === 3);
      assert.equal(await page.eval(() => window.__moves[2].ok), false);
      assert.equal(await columnOf(page, 104), 'working');
      assert.match(await live(), /couldn't be saved/);
      page.assertClean({ allow: [/422/] });
    }));

  test('kanban on a phone: the columns scroll, a card moves by keyboard', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('[data-rx-kanban][data-rx-blocks-ready]'));
      await watchMoves(page);
      assert.ok(await page.eval(() => {
        const board = document.querySelector('[data-rx-kanban]');
        return board.scrollWidth > board.clientWidth;
      }), 'the board scrolls sideways');
      await page.focus('[data-rx-kanban-card="105"]');
      await page.press(' ');
      await page.press('ArrowRight');
      await page.press(' ');
      await page.waitFor(() => window.__moves.length === 1);
      assert.equal(await columnOf(page, 105), 'ready');
      assert.ok(await fitsWidth(page));
      page.assertClean();
    }, PHONE));
});

describe('renox-blocks with prefers-reduced-motion', () => {
  for (const [size, options] of [
    ['desktop', undefined],
    ['phone', PHONE],
  ]) {
    test(`nothing moves, everything works (${size})`, () =>
      browser.with(async (page) => {
        await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
        await page.goto(`${app.url}${PAGE}`);
        await page.waitFor(() => !!document.querySelector('#demo-gallery[data-rx-blocks-ready]'));
        await page.waitFor(() => !!document.querySelector('[data-rx-kanban][data-rx-blocks-ready]'));
        await watchMoves(page);
        // The gallery jumps: no animation runs, the track is already in place.
        await page.eval(() => document.querySelector('#demo-gallery [data-rx-gallery-next]').click());
        assert.equal(await moving(page, '[data-rx-gallery-track]'), 0);
        const width = await page.eval(() => document.querySelector('[data-rx-gallery-viewport]').clientWidth);
        assert.equal(await page.eval(() => document.querySelector('[data-rx-gallery-track]').style.transform), `translateX(-${width}px)`);
        // The stepper doesn't pulse.
        await page.eval(() => document.querySelector('[data-rx-quantity] [data-rx-quantity-step="1"]').click());
        assert.equal(await moving(page, '#rx-quantity'), 0);
        // A kanban move with no glide.
        await page.focus('[data-rx-kanban-card="105"]');
        await page.press(' ');
        await page.press('ArrowRight');
        assert.equal(await moving(page, '[data-rx-kanban-card="105"]'), 0);
        await page.press(' ');
        await page.waitFor(() => window.__moves.length === 1);
        // The history never fades in, even scrolled to.
        await page.eval(() => document.querySelector('.rx-history').scrollIntoView());
        await sleep(200);
        assert.equal(await page.eval(() => [...document.querySelectorAll('.rx-history > li')].filter((li) => li.getAnimations().length).length), 0);
        assert.ok(await fitsWidth(page));
        page.assertClean();
      }, options));
  }

  test('with motion, the history fades in when scrolled to', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('[data-rx-history][data-rx-blocks-ready]'));
      await page.eval(() => document.querySelector('.rx-history').scrollIntoView());
      await page.waitFor(() => [...document.querySelectorAll('.rx-history > li')].some((li) => li.getAnimations().length > 0), { message: 'the events fade in' });
      page.assertClean();
    }));
});

/** Collects the board's `rx:kanban-moved` events in `window.__moves`. */
const watchMoves = (page) =>
  page.eval(() => {
    window.__moves = [];
    document.addEventListener('rx:kanban-moved', (e) => window.__moves.push(e.detail));
  });

/** The column a card is in. */
const columnOf = (page, id) =>
  page.eval((card) => document.querySelector(`[data-rx-kanban-card="${card}"]`)?.closest('[data-rx-kanban-column]')?.dataset.rxKanbanColumn, id);

/** Animations on `selector` that take time (the kit's reduced-motion rule
 *  leaves 0.01 ms transitions on everything, which are instant). */
const moving = (page, selector) =>
  page.eval((s) => document.querySelector(s).getAnimations().filter((a) => a.effect.getTiming().duration > 1).length, selector);

/** Drags `from` (a selector) to the point (x, y) with the mouse, in steps. */
async function drag(page, from, to) {
  const a = await page.point(from);
  await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: a.x, y: a.y });
  await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: a.x, y: a.y, button: 'left', clickCount: 1 });
  const steps = 12;
  for (let i = 1; i <= steps; i++) {
    const x = a.x + ((to.x - a.x) * i) / steps;
    const y = a.y + ((to.y - a.y) * i) / steps;
    await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'left', buttons: 1 });
    await sleep(16);
  }
  await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: to.x, y: to.y, button: 'left', clickCount: 1 });
  await sleep(50);
}
