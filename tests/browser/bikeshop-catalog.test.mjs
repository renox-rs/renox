// The bike shop's catalogue (#233) in a browser, on the seeded shop: the
// home page, a category with its filters sent by htmx (the address follows),
// the chips, the phone layout (filters folded), the navbar's search with its
// suggestions and the arrow keys, and the product page's variant chips and
// add to cart (the toast and the navbar's count). At 1280 and 390 px, light
// and dark, with a clean console. Screenshots go to BIKESHOP_SCREENS when
// it's set.

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
const cards = (page) => page.eval(() => [...document.querySelectorAll('#results .rx-media-card__title')].map((t) => t.textContent));
const scheme = (page, value) =>
  page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value }] });

describe('bikeshop catalogue', () => {
  for (const [size, options] of [
    ['desktop', undefined],
    ['phone', PHONE],
  ]) {
    for (const colours of ['light', 'dark']) {
      test(`home, category and product pages (${size}, ${colours})`, () =>
        browser.with(async (page) => {
          await scheme(page, colours);
          await page.goto(`${app.url}/`);
          assert.ok((await page.eval(() => document.querySelectorAll('.rx-media-card').length)) >= 4, 'featured bikes');
          assert.ok(await fitsWidth(page), 'no sideways scrolling on the home page');
          await shot(page, `catalog-home-${size}-${colours}`);

          await page.goto(`${app.url}/shop/bikes`);
          assert.equal((await cards(page)).length, 24, 'a full first page');
          assert.ok(await fitsWidth(page), 'no sideways scrolling on a category');
          const open = await page.eval(() => document.querySelector('[data-bs-filters]').open);
          assert.equal(open, size === 'desktop', 'filters beside the results, folded on a phone');
          await shot(page, `catalog-category-${size}-${colours}`);

          const href = await page.eval(() => document.querySelector('#results .rx-media-card__link').getAttribute('href'));
          await page.goto(`${app.url}${href}`);
          await page.waitFor(() => !!document.querySelector('[data-rx-gallery][data-rx-blocks-ready]'));
          assert.ok(await page.eval(() => !!document.querySelector('#buybox .bs-buybox__price')));
          assert.ok(await fitsWidth(page), 'no sideways scrolling on a product');
          await shot(page, `catalog-product-${size}-${colours}`);
          page.assertClean();
        }, options));
    }
  }

  test('filters go through htmx, keep the address and come off as chips', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/shop/bikes`);
      const before = await cards(page);
      const brand = await page.eval(() => document.querySelector('input[name=brand]').value);
      await page.click('input[name=brand]');
      await page.waitFor((b) => location.search.includes(`brand=${b}`), {}, brand);
      await page.settle();
      const after = await cards(page);
      assert.notDeepEqual(after, before, 'the results changed');
      assert.ok(await page.eval(() => !!document.querySelector('[data-bs-filters] summary')), 'the page was not reloaded');
      assert.equal(await page.eval(() => document.querySelectorAll('main .bs-chip').length), 1);
      // Sort: cheapest first.
      await page.eval(() => {
        const sort = document.querySelector('#filter-sort');
        sort.value = 'price_asc';
        sort.dispatchEvent(new Event('change', { bubbles: true }));
      });
      await page.waitFor(() => location.search.includes('sort=price_asc'));
      await page.settle();
      // The chip takes the brand off again.
      await page.click('main .bs-chip');
      await page.waitFor(() => !location.search.includes('brand='));
      await page.settle();
      assert.equal(await page.eval(() => document.querySelectorAll('main .bs-chip').length), 0);
      // Back to the filtered page with the browser's history.
      await page.eval(() => history.back());
      await page.waitFor((b) => location.search.includes(`brand=${b}`), {}, brand);
      page.assertClean();
    }));

  test('the price range slider narrows the results', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/shop/bikes`);
      await page.waitFor(() => !!document.querySelector('[data-rx-range][data-rx-blocks-ready]'));
      await page.focus('#filter-price-max');
      for (let i = 0; i < 20; i++) await page.press('PageDown');
      await page.waitFor(() => location.search.includes('price_max='));
      await page.settle();
      const max = await page.eval(() => Number(new URLSearchParams(location.search).get('price_max')));
      assert.ok(max > 0);
      page.assertClean();
    }));

  test('phone: the filters open under "Filters"', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/shop/helmets`);
      await page.click('[data-bs-filters] summary');
      assert.ok(await page.eval(() => document.querySelector('[data-bs-filters]').open));
      assert.ok(await page.eval(() => document.querySelector('#filter-sort').offsetParent !== null));
      await shot(page, 'catalog-filters-phone');
      page.assertClean();
    }, PHONE));

  test('the navbar search suggests as you type; arrows, Enter and Escape', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/`);
      await page.type('#nav-search', 'trek');
      await page.waitFor(() => document.querySelectorAll('#nav-suggestions [data-bs-suggestion]').length > 1);
      await shot(page, 'catalog-suggestions');
      await page.press('ArrowDown');
      assert.ok(await page.eval(() => document.activeElement.matches('[data-bs-suggestion]')));
      await page.press('ArrowUp');
      assert.equal(await page.eval(() => document.activeElement.id), 'nav-search');
      await page.press('ArrowDown');
      await page.press('Escape');
      assert.equal(await page.eval(() => document.querySelector('#nav-suggestions').children.length), 0);
      assert.equal(await page.eval(() => document.activeElement.id), 'nav-search');
      // Enter in the box: the results page, with the same filters as the catalogue.
      const loaded = page.once('Page.loadEventFired');
      await page.press('Enter');
      await loaded;
      await page.settle();
      assert.ok(await page.eval(() => location.pathname === '/search' && location.search.includes('q=trek')));
      assert.ok((await cards(page)).length > 0);
      page.assertClean();
    }));

  test('a product: the variant chips update the buy box; add to cart', () =>
    browser.with(async (page) => {
      // A bike with several sizes.
      await page.goto(`${app.url}/shop/road-bikes`);
      const href = await page.eval(() => document.querySelector('#results .rx-media-card__link').getAttribute('href'));
      await page.goto(`${app.url}${href}`);
      const sizes = await page.eval(() => [...document.querySelectorAll('input[name=size]')].map((i) => i.value));
      assert.ok(sizes.length > 1, 'sizes to pick from');
      const sku = await page.text('#buybox .rx-subtitle');
      await page.click(`input[name=size][value="${sizes[sizes.length - 1]}"] + .rx-swatches__chip`);
      await page.waitFor((s) => location.search.includes(`size=${encodeURIComponent(s).replace(/%20/g, '+')}`) || location.search.includes(`size=${encodeURIComponent(s)}`), {}, sizes[sizes.length - 1]);
      await page.settle();
      assert.notEqual(await page.text('#buybox .rx-subtitle'), sku, 'another SKU in the buy box');
      // Add to cart, if any store has it: a toast and the navbar's count.
      if (await page.eval(() => !!document.querySelector('[data-bs-add-to-cart]'))) {
        await page.click('[data-bs-add-to-cart] button[type=submit]');
        await page.waitFor(() => !!document.querySelector('.rx-toast--success'));
        // The navbar's count, swapped in out of band.
        await page.waitFor(() => document.querySelector('#nav-cart .rx-button__badge')?.textContent.trim() === '1');
        await shot(page, 'catalog-added');
      }
      page.assertClean();
    }));
});
