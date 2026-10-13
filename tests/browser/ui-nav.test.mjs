// #346: the kit's navbar with `tabs` and `nav_search` (fixture /tabs). On a
// 390 px phone: one row on top, the search behind a button, a tab bar at the
// bottom (the current tab by route, a sheet behind the last one, the page's
// end above it, nothing wider than the screen); on a desktop the bar as
// before and no tab bar. Screenshots go to UI_NAV_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

const PHONE = { width: 390, height: 844 };
const DESKTOP = { width: 1280, height: 800 };

let browser;
let app;

before(async () => {
  app = await fixture();
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await app?.stop();
});

const visible = (page, selector) =>
  page.eval((s) => {
    const el = document.querySelector(s);
    if (!el) return false;
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0 && getComputedStyle(el).visibility !== 'hidden';
  }, selector);

/** Screenshots in light and dark, when UI_NAV_SCREENS names a folder. */
async function shots(page, name) {
  const dir = process.env.UI_NAV_SCREENS;
  if (!dir) return;
  for (const scheme of ['light', 'dark']) {
    await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: scheme }] });
    await page.screenshot(join(dir, `${name}-${scheme}.png`));
  }
  await page.send('Emulation.setEmulatedMedia', { features: [] });
}

describe('the navbar on a phone', () => {
  test('one row on top, the tabs at the bottom, the current one by route', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/tabs/orders`);
      const bar = await page.eval(() => document.querySelector('.rx-navbar').getBoundingClientRect().height);
      assert.ok(bar < 70, `the bar is one row (${bar}px)`);
      assert.equal(await visible(page, '.rx-navbar__links'), false, 'the links leave the bar');
      assert.equal(await visible(page, '#nav-account'), false, 'rx-hide-narrow leaves the bar');
      assert.equal(await visible(page, '#rx-q'), false, 'the search is behind its button');
      assert.ok(await visible(page, '.rx-tabbar'), 'the tab bar shows');
      // Within the screen, at the bottom, in its own landmark.
      const box = await page.eval(() => {
        const r = document.querySelector('.rx-tabbar').getBoundingClientRect();
        return { left: r.left, right: r.right, bottom: r.bottom, label: document.querySelector('.rx-tabbar').getAttribute('aria-label') };
      });
      assert.ok(box.left >= 0 && box.right <= 390 && box.bottom <= 844 && box.bottom > 700, JSON.stringify(box));
      assert.equal(box.label, 'Sections');
      // The current tab: aria-current, by the route, and only that one.
      const current = await page.eval(() => [...document.querySelectorAll('.rx-tabbar [aria-current]')].map((a) => a.textContent.trim()));
      assert.deepEqual(current, ['Orders3']);
      // Icons are decorative; the label names the tab.
      assert.equal(await page.eval(() => document.querySelector('.rx-tabbar__icon svg').getAttribute('aria-hidden')), 'true');
      // A long label is cut short instead of widening the bar.
      const [scroll, width] = await page.eval(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
      assert.ok(scroll <= width, `no sideways scrolling (${scroll} > ${width})`);
      // Scrolled to the end, the last line sits above the tab bar.
      const [lastBottom, barTop] = await page.eval(() => {
        window.scrollTo(0, document.documentElement.scrollHeight);
        return [document.querySelector('#last').getBoundingClientRect().bottom, document.querySelector('.rx-tabbar').getBoundingClientRect().top];
      });
      assert.ok(lastBottom <= barTop, `the page's end clears the tab bar (${lastBottom} > ${barTop})`);
      await page.eval(() => window.scrollTo(0, 0));
      await shots(page, 'nav-phone');
      page.assertClean();
    }, PHONE));

  test('the search opens under the bar, focused, and Escape closes it', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/tabs`);
      await page.click('[data-rx-search-toggle]');
      await page.waitFor(() => document.activeElement?.id === 'rx-q', { message: 'the search field focused' });
      assert.equal(await page.eval(() => document.querySelector('[data-rx-search-toggle]').getAttribute('aria-expanded')), 'true');
      const [barRows, fieldWidth] = await page.eval(() => [
        document.querySelector('.rx-navbar').getBoundingClientRect().height,
        document.querySelector('#rx-q').getBoundingClientRect().width,
      ]);
      assert.ok(barRows > 70, 'the search takes a row under the bar');
      assert.ok(fieldWidth > 300, `the field spans the row (${fieldWidth}px)`);
      await shots(page, 'nav-phone-search');
      await page.press('Escape');
      await page.waitFor(() => !document.querySelector('.rx-navbar').classList.contains('rx-navbar--searching'), { message: 'closed' });
      assert.equal(await page.eval(() => document.activeElement?.hasAttribute('data-rx-search-toggle')), true, 'the focus is back on the button');
      assert.equal(await page.eval(() => document.querySelector('[data-rx-search-toggle]').getAttribute('aria-expanded')), 'false');
      // The button closes it too.
      await page.click('[data-rx-search-toggle]');
      await page.waitFor(() => document.querySelector('.rx-navbar').classList.contains('rx-navbar--searching'));
      await page.eval(() => document.querySelector('[data-rx-search-toggle]').click());
      assert.equal(await visible(page, '#rx-q'), false);
      page.assertClean();
    }, PHONE));

  test('the keyboard reaches every tab, and the last one opens its sheet', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/tabs`);
      // Tab from the search button: the next stops are the tabs, in order.
      await page.focus('[data-rx-search-toggle]');
      const stops = [];
      for (let i = 0; i < 5; i++) {
        await page.press('Tab');
        stops.push(await page.eval(() => document.activeElement.closest('.rx-tabbar') ? document.activeElement.querySelector('.rx-tabbar__label').textContent : document.activeElement.tagName));
      }
      assert.deepEqual(stops, ['Home', 'Orders', 'Widgets', 'Overlays with a long name', 'More']);
      // The focused tab shows its ring.
      assert.notEqual(await page.eval(() => getComputedStyle(document.activeElement).outlineStyle), 'none');
      await page.press('Enter');
      await page.waitFor(() => document.querySelector('#nav-more').open, { message: 'the sheet open' });
      await shots(page, 'nav-phone-sheet');
      await page.press('Escape');
      await page.waitFor(() => !document.querySelector('#nav-more').open, { message: 'the sheet closed' });
      page.assertClean();
    }, PHONE));
});

describe('the navbar on a desktop', () => {
  test('the bar as before, the search in it, no tab bar', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/tabs`);
      assert.ok(await visible(page, '.rx-navbar__links'), 'the links in the bar');
      assert.ok(await visible(page, '#rx-q'), 'the search field in the bar');
      assert.ok(await visible(page, '#nav-account'), 'the account link in the bar');
      assert.equal(await visible(page, '[data-rx-search-toggle]'), false, 'no search button');
      assert.equal(await visible(page, '.rx-tabbar'), false, 'no tab bar');
      assert.equal(await page.eval(() => document.querySelector('.rx-navbar__links [aria-current]').textContent.trim()), 'Home');
      const bar = await page.eval(() => document.querySelector('.rx-navbar').getBoundingClientRect().height);
      assert.ok(bar < 70, `one row (${bar}px)`);
      const room = await page.eval(() => parseFloat(getComputedStyle(document.body).paddingBottom));
      assert.ok(room < 40, `no room kept for a tab bar (${room}px)`);
      await shots(page, 'nav-desktop');
      page.assertClean();
    }, DESKTOP));
});
