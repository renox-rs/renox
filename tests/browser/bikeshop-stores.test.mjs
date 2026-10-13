// Each store's own page on its own host (#351, `Routes::domain`): the home
// page's store tiles link to `{store}.localhost`, which Chrome sends to this
// machine; the page fits a phone and a desktop, light and dark, and its
// links lead back to the shop's host. Screenshots go to BIKESHOP_SCREENS.

import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser, sleep } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PHONE = { width: 390, height: 844 };
let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

test('a store tile on the home page opens the store on its own host', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/`);
    const port = new URL(app.url).port;
    const href = await page.eval(() => document.querySelector('.bs-store__link')?.getAttribute('href'));
    assert.equal(href, `http://north.localhost:${port}/`);
    await page.goto(href);
    assert.equal(await page.eval(() => location.host), `north.localhost:${port}`);
    assert.match(await page.text('h1'), /North/);
    // Any other path on the store's host comes back to its page.
    await page.goto(`http://north.localhost:${port}/shop`);
    assert.equal(await page.eval(() => location.pathname), '/');
    // The links lead to the shop's own host.
    const shop = await page.eval(() => document.querySelector('.rx-navbar__brand')?.getAttribute('href'));
    assert.equal(shop, `${app.url}/`);
    page.assertClean();
  }));

test('the store page fits a phone and a desktop, light and dark', async () => {
  const port = new URL(app.url).port;
  for (const [size, options] of [['desktop', undefined], ['phone', PHONE]]) {
    for (const scheme of ['light', 'dark']) {
      await browser.with(async (page) => {
        await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: scheme }] });
        await page.goto(`http://south.localhost:${port}/`);
        assert.ok(await page.eval(() => document.documentElement.scrollWidth <= window.innerWidth), 'no sideways scrolling');
        await sleep(600);
        await shot(page, `store-${size}-${scheme}`);
        page.assertClean();
      }, options);
    }
  }
});
