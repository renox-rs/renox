// The storefront's look (#328, "warm editorial"): every photo on the public
// pages loads, the hero and its photo are there, the brand tokens apply, no
// page is wider than a phone, and the sign-in pages show their photo beside
// the form only on wide screens. Light and dark, desktop and phone.
// Screenshots go to BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PHONE = { width: 390, height: 844, deviceScaleFactor: 1, mobile: true };
const DESKTOP = { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false };

let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { BIKESHOP_STAFF_2FA: 'optional' } });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

/** Loads every lazy image (scrolls to the end), then lists the ones that failed. */
async function brokenImages(page) {
  await page.eval(() => {
    for (const img of document.images) img.loading = 'eager';
    window.scrollTo(0, document.documentElement.scrollHeight);
  });
  await page.waitFor(() => [...document.images].every((img) => img.complete), { message: 'the images loaded' });
  return page.eval(() => [...document.images].filter((img) => !img.naturalWidth).map((img) => img.getAttribute('src')));
}

const PAGES = [
  ['home', '/', true],
  ['shop', '/shop', false],
  ['rent', '/rent', true],
  ['plans', '/plans', true],
];

describe('bikeshop design', () => {
  for (const [size, metrics] of [['desktop', DESKTOP], ['phone', PHONE]]) {
    for (const scheme of ['light', 'dark']) {
      test(`the public pages, their photos and the brand (${size}, ${scheme})`, () =>
        browser.with(async (page) => {
          await page.send('Network.clearBrowserCookies');
          await page.send('Emulation.setDeviceMetricsOverride', metrics);
          await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: scheme }] });
          for (const [name, path, hero] of PAGES) {
            await page.goto(`${app.url}${path}`);
            assert.deepEqual(await brokenImages(page), [], `${path}: every photo loads`);
            if (hero) {
              assert.ok(await page.eval(() => !!document.querySelector('.bs-hero .bs-hero__title')), `${path}: a hero`);
              assert.ok(await page.eval(() => document.querySelector('.bs-hero__photo')?.naturalWidth > 0), `${path}: its photo`);
            }
            const [scroll, width] = await page.eval(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
            assert.ok(scroll <= width + 1, `${path} fits (${scroll} > ${width})`);
            await page.eval(() => window.scrollTo(0, 0));
            await shot(page, `design-${name}-${size}-${scheme}`);
          }
          // The brand: a cream page in light mode, pill buttons on public pages.
          const [bg, radius] = await page.eval(() => [
            getComputedStyle(document.body).backgroundColor,
            getComputedStyle(document.querySelector('.rx-button')).borderTopLeftRadius,
          ]);
          if (scheme === 'light') assert.equal(bg, 'rgb(246, 241, 232)', 'the cream page');
          assert.ok(parseFloat(radius) >= 20, `pill buttons (${radius})`);
          // The home page's products show photos, not drawings.
          await page.goto(`${app.url}/`);
          const photos = await page.eval(() => [...document.querySelectorAll('.rx-media-card__image')].map((img) => img.getAttribute('src')));
          assert.ok(photos.length >= 4 && photos.every((src) => src.endsWith('.webp')), `product photos: ${photos.join(', ')}`);
          page.assertClean();
        }));
    }
  }

  test('the sign-in page shows its photo beside the form on wide screens, as a band above it on phones', () =>
    browser.with(async (page) => {
      await page.send('Network.clearBrowserCookies');
      await page.send('Emulation.setDeviceMetricsOverride', DESKTOP);
      await page.goto(`${app.url}/login`);
      assert.deepEqual(await brokenImages(page), []);
      assert.ok(await page.eval(() => getComputedStyle(document.querySelector('.bs-auth__art')).display !== 'none'), 'the photo on a wide screen');
      await shot(page, 'design-login-desktop');
      await page.send('Emulation.setDeviceMetricsOverride', PHONE);
      await page.goto(`${app.url}/login`);
      // A phone: a short band above the form, without the quote.
      const [artBottom, formTop, quote] = await page.eval(() => [
        document.querySelector('.bs-auth__art').getBoundingClientRect().bottom,
        document.querySelector('.rx-auth').getBoundingClientRect().top,
        getComputedStyle(document.querySelector('.bs-auth__quote')).display,
      ]);
      assert.ok(artBottom <= formTop + 1 && artBottom < 200, `a band above the form (${artBottom}, ${formTop})`);
      assert.equal(quote, 'none', 'no quote on a phone');
      page.assertClean();
    }));
});
