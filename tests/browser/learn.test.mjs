// #523: learn.renox.rs's skeleton. The home page lists every stage, loads
// with no console errors or CSP violations (also under CSP=strict), and
// has no horizontal scroll on a phone.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

let browser;

before(async () => {
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
});

for (const csp of ['relaxed', 'strict']) {
  describe(`learn.renox.rs (CSP=${csp})`, () => {
    let app;

    before(async () => {
      app = await start('renox-learn', 'learn', { env: { CSP: csp, QUEUE_WORKERS: '0', SCHEDULER: 'false' } });
    });

    after(async () => {
      await app?.stop();
    });

    test('the home page lists the stages and loads cleanly', () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}/`);
        const stages = await page.eval(`document.querySelectorAll('.stage').length`);
        assert.equal(stages, 9);
        const h1 = await page.eval(`document.querySelector('h1').textContent`);
        assert.match(h1, /one stage at a time/);
        // The stylesheet applied: the numbers are drawn as circles.
        const radius = await page.eval(`getComputedStyle(document.querySelector('.stage__number')).borderRadius`);
        assert.equal(radius, '50%');
        page.assertClean();
      }));

    test('a phone gets no horizontal scroll', () =>
      browser.with(
        async (page) => {
          await page.goto(`${app.url}/`);
          const overflow = await page.eval(`document.documentElement.scrollWidth - document.documentElement.clientWidth`);
          assert.equal(overflow, 0);
          page.assertClean();
        },
        { width: 375, height: 800 },
      ));
  });
}
