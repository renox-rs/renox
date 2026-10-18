// #28, right-to-left: the kit, the grid, the blocks and the admin in a browser.
// The fixture runs with APP_LOCALE=ar, so its pages come out with
// `<html lang="ar" dir="rtl">` from `app.dir` itself: the sidebar on the
// right, a slide-over sheet from the left, mirrored chevrons, the grid's
// frozen columns swapped, no sideways scrolling, and the Arabic font fetched
// only by a page with Arabic text. The bike shop stays Spanish, so its blocks
// and admin pages are flipped by setting `dir` on the page (every part reads
// the direction when it is used, never once at load): the gallery's and the
// range slider's keys follow what is drawn.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser, sleep } from './lib/cdp.mjs';
import { fixture, start } from './lib/app.mjs';
import { logInWithFetch } from './lib/bikeshop.mjs';

const DESKTOP = { width: 1280, height: 800 };
const PHONE = { width: 390, height: 844 };

let browser;
let app;
let shop;

before(async () => {
  app = await fixture({ seed: true, env: { APP_LOCALE: 'ar' } });
  shop = await start('bikeshop', 'examples/bikeshop', { seed: true });
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await shop?.stop();
  await app?.stop();
});

const fitsWidth = (page) => page.eval(() => document.documentElement.scrollWidth <= window.innerWidth);

/** Flips a page of the (Spanish) bike shop to right-to-left, as `app.dir` would. */
async function flip(page) {
  await page.eval(() => {
    document.documentElement.dir = 'rtl';
  });
  await sleep(150);
}

describe('the kit on the fixture, locale ar', () => {
  test('the page says rtl and mirrors chevrons, not other icons', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/rtl`);
      const html = await page.eval(() => [document.documentElement.lang, document.documentElement.dir]);
      assert.deepEqual(html, ['ar', 'rtl']);
      const transform = (selector) =>
        page.eval((s) => getComputedStyle(document.querySelector(`${s} svg`)).transform, selector);
      assert.equal(await transform('#next-icon'), 'matrix(-1, 0, 0, 1, 0, 0)', 'the chevron is mirrored');
      assert.equal(await transform('#check-icon'), 'none', 'a check mark is not');
      page.assertClean();
    }, DESKTOP));

  test('the sidebar is on the right of the content', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/shell`);
      const middle = await page.eval(() => {
        const mid = (el) => {
          const r = el.getBoundingClientRect();
          return r.left + r.width / 2;
        };
        return { sidebar: mid(document.querySelector('.rx-sidebar')), main: mid(document.querySelector('.rx-shell__main')) };
      });
      assert.ok(middle.sidebar > middle.main, `the sidebar is on the right: ${JSON.stringify(middle)}`);
      page.assertClean();
    }, DESKTOP));

  test('a slide-over sheet comes from the left edge', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/overlays`);
      await page.click('[data-rx-open="side-sheet"]');
      await page.waitFor(() => document.querySelector('#side-sheet').open, { message: 'the sheet opens' });
      await page.waitFor(
        () => {
          const r = document.querySelector('#side-sheet').getBoundingClientRect();
          const centre = r.left + r.width / 2;
          return centre > 0 && centre < window.innerWidth / 2;
        },
        { message: 'the sheet in the left half' },
      );
      await page.press('Escape');
      page.assertClean();
    }, DESKTOP));

  test('the grid freezes its first column on the right', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/grid?state=1`);
      const sides = await page.eval(() => {
        const mid = (el) => {
          const r = el.getBoundingClientRect();
          return r.left + r.width / 2;
        };
        return {
          dir: document.documentElement.dir,
          number: mid(document.querySelector('tbody tr[data-id] td[data-col="number"]')),
          actions: mid(document.querySelector('tbody tr[data-id] td[data-col="actions"]')),
          centre: mid(document.querySelector('.rx-grid__scroll')),
        };
      });
      assert.equal(sides.dir, 'rtl');
      assert.ok(sides.number > sides.centre, `the first frozen column is on the right: ${JSON.stringify(sides)}`);
      assert.ok(sides.actions < sides.centre, `the last frozen column is on the left: ${JSON.stringify(sides)}`);
      page.assertClean({ allow: [/422/] });
    }, DESKTOP));

  test('pages do not scroll sideways', () =>
    browser.with(async (page) => {
      for (const path of ['/', '/shell', '/widgets', '/overlays', '/charts', '/form', '/grid?state=1', '/rtl']) {
        await page.goto(`${app.url}${path}`);
        assert.equal(await page.eval(() => document.documentElement.dir), 'rtl', `${path} is right-to-left`);
        assert.ok(await fitsWidth(page), `${path} fits the window`);
      }
    }, DESKTOP));
});

describe('the Arabic font is fetched only by a page with Arabic text', () => {
  test('a Latin page does not ask for it', () =>
    browser.with(async (page) => {
      const urls = [];
      page.on('Network.requestWillBeSent', ({ request }) => {
        if (/noto-sans-arabic/.test(request.url)) urls.push(request.url);
      });
      await page.goto(`${app.url}/rtl`);
      await page.eval(() => document.fonts.ready.then(() => true));
      await sleep(500);
      assert.deepEqual(urls, [], 'no request for the Arabic font');
    }, DESKTOP));

  test('an Arabic page does, and loads it', () =>
    browser.with(async (page) => {
      const urls = [];
      page.on('Network.requestWillBeSent', ({ request }) => {
        if (/noto-sans-arabic/.test(request.url)) urls.push(request.url);
      });
      await page.goto(`${app.url}/arabic`);
      const start = Date.now();
      while (urls.length === 0 && Date.now() - start < 10_000) await sleep(100);
      assert.ok(urls.length > 0, 'the Arabic font is requested');
      assert.ok(
        await page.eval(() => [...document.fonts].some((f) => /Arabic/i.test(f.family) && f.status === 'loaded')),
        'and it is loaded',
      );
      page.assertClean();
    }, DESKTOP));
});

describe('the blocks, flipped', () => {
  test('the gallery keys follow the reading direction, the track moves the other way', () =>
    browser.with(async (page) => {
      await page.goto(`${shop.url}/about/blocks`);
      await page.waitFor(() => !!document.querySelector('#demo-gallery[data-rx-blocks-ready]'));
      await flip(page);
      const index = () => page.eval(() => document.querySelector('#demo-gallery').dataset.rxGalleryIndex);
      await page.focus('#demo-gallery [data-rx-gallery-viewport]');
      await page.press('Home');
      assert.equal(await index(), '0');
      // The next photo is toward the left now.
      await page.press('ArrowLeft');
      await page.press('ArrowLeft');
      assert.equal(await index(), '2');
      await page.press('ArrowRight');
      assert.equal(await index(), '1');
      await page.press('ArrowRight');
      await page.press('ArrowRight');
      assert.equal(await index(), '0', 'it stops at the first photo');
      await page.press('End');
      await page.waitFor(() => {
        const width = document.querySelector('[data-rx-gallery-viewport]').clientWidth;
        return document.querySelector('[data-rx-gallery-track]').style.transform === `translateX(${3 * width}px)`;
      }, { message: 'the track moved to the right, so the last photo shows' });
      page.assertClean();
    }, DESKTOP));

  test('the range slider: the arrow toward the end raises the low handle', () =>
    browser.with(async (page) => {
      await page.goto(`${shop.url}/about/blocks`);
      await page.waitFor(() => !!document.querySelector('[data-rx-range][data-rx-blocks-ready]'));
      await flip(page);
      const low = '#rx-price_min-range-min';
      const value = () => page.eval((s) => Number(document.querySelector(s).value), low);
      await page.focus(low);
      await page.press('Home');
      assert.equal(await value(), 0);
      // A native range input is mirrored in right-to-left: its minimum is at
      // the right edge, so the left arrow goes up and the right arrow down.
      await page.press('ArrowLeft');
      const up = await value();
      assert.ok(up > 0, `the left arrow raises the value (${up})`);
      await page.press('ArrowRight');
      assert.equal(await value(), 0, 'the right arrow lowers it again');
      page.assertClean();
    }, DESKTOP));

  for (const [name, size] of [['desktop', DESKTOP], ['phone', PHONE]]) {
    test(`the blocks page does not scroll sideways (${name})`, () =>
      browser.with(async (page) => {
        await page.goto(`${shop.url}/about/blocks`);
        await page.waitFor(() => !!document.querySelector('#demo-gallery[data-rx-blocks-ready]'));
        await flip(page);
        assert.ok(await fitsWidth(page), 'it fits the window');
        page.assertClean();
      }, size));
  }
});

describe('the admin, flipped', () => {
  test('its pages do not scroll sideways', () =>
    browser.with(async (page) => {
      assert.match(await logInWithFetch(page, shop.url, 'owner@bikeshop.test'), /^200 /);
      for (const path of ['/admin', '/admin/products']) {
        await page.goto(`${shop.url}${path}`);
        await flip(page);
        assert.equal(await page.eval(() => getComputedStyle(document.documentElement).direction), 'rtl');
        assert.ok(await fitsWidth(page), `${path} fits the window`);
      }
      page.assertClean();
    }, DESKTOP));
});
