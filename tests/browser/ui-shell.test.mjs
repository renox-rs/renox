// #24: the dashboard standard, `<rx-shell fill>` (fixture /shell-fill and
// /grid/shell). On a large screen the window never scrolls: the sidebar,
// the content and each pane scroll on their own, and the grid or the row of
// panes (`rx-fill`) takes the height left, so the panels reach the bottom
// and the right edge. htmx's `revealed`, anchors and htmx's history work
// with the content as the scroller. On a tablet and a phone the page
// scrolls as a document and a `rx-fill` element keeps its minimum height.
// Screenshots go to UI_SHELL_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

const DESKTOP = { width: 1280, height: 800 };
const WIDE = { width: 1920, height: 1080 };
const TABLET = { width: 820, height: 1180 };
const PHONE = { width: 390, height: 844 };

let browser;
let app;

before(async () => {
  app = await fixture({ seed: true });
  browser = await Browser.launch();
});

after(async () => {
  await browser?.close();
  await app?.stop();
});

async function shot(page, name) {
  if (process.env.UI_SHELL_SCREENS) await page.screenshot(join(process.env.UI_SHELL_SCREENS, `${name}.png`));
}

/** Turns the mouse wheel over the middle of `selector`, as a person would. */
async function wheel(page, selector, deltaY) {
  const { x, y } = await page.eval((s) => {
    const r = document.querySelector(s).getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + Math.min(r.height / 2, 200) };
  }, selector);
  await page.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY });
  await page.waitFor(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done(true)))));
}

/** How far the window scrolls, and how far each element does. */
const scrolls = (page, ...selectors) =>
  page.eval((list) => {
    const out = { window: Math.round(window.scrollY), page: document.scrollingElement.scrollHeight - innerHeight };
    for (const s of list) out[s] = Math.round(document.querySelector(s).scrollTop);
    return out;
  }, selectors);

const box = (page, selector) =>
  page.eval((s) => {
    const r = document.querySelector(s).getBoundingClientRect();
    return { top: r.top, bottom: r.bottom, left: r.left, right: r.right, height: r.height, width: r.width };
  }, selector);

describe('on a large screen', () => {
  test('the window never scrolls; the sidebar, the content and each pane scroll on their own', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/shell-fill`);
      await shot(page, 'panes-desktop');
      let now = await scrolls(page, '.rx-sidebar', '#list-pane', '#detail-pane');
      assert.equal(now.page, 0, 'the document is exactly as tall as the window');
      await wheel(page, '#list-pane', 600);
      now = await scrolls(page, '.rx-sidebar', '#list-pane', '#detail-pane');
      assert.ok(now['#list-pane'] > 0, 'the list pane scrolled');
      assert.equal(now['#detail-pane'], 0, 'the detail pane stayed');
      assert.equal(now.window, 0, 'the window stayed');
      await wheel(page, '#detail-pane', 400);
      now = await scrolls(page, '.rx-sidebar', '#list-pane', '#detail-pane');
      assert.ok(now['#detail-pane'] > 0, 'the detail pane scrolled on its own');
      await wheel(page, '.rx-sidebar', 600);
      now = await scrolls(page, '.rx-sidebar', '#list-pane', '#detail-pane');
      assert.ok(now['.rx-sidebar'] > 0, 'the sidebar scrolled on its own');
      assert.equal(now.window, 0, 'the window still stayed');
      // The navbar and the page header never moved.
      assert.equal((await box(page, '.rx-navbar')).top, 0);
      page.assertClean();
    }, DESKTOP));

  test('the panes cover the screen down to the bottom and the right edge', () =>
    browser.with(async (page) => {
      for (const size of [DESKTOP, WIDE]) {
        await page.send('Emulation.setDeviceMetricsOverride', { ...size, deviceScaleFactor: 1, mobile: false });
        await page.goto(`${app.url}/shell-fill`);
        const content = await box(page, '.rx-shell__content');
        const panes = await box(page, '#panes');
        const pad = await page.eval(() => parseFloat(getComputedStyle(document.querySelector('.rx-shell__content')).paddingBottom));
        assert.ok(Math.abs(content.bottom - size.height) < 1, `the content reaches the bottom at ${size.width}`);
        assert.ok(Math.abs(content.right - size.width) < 1, `the content reaches the right edge at ${size.width}`);
        assert.ok(Math.abs(panes.bottom - (size.height - pad)) < 1, `the panes end at the content's padding (${panes.bottom})`);
        assert.ok(Math.abs(panes.right - (size.width - pad)) < 2, `the panes span the width (${panes.right})`);
        // Every pane is as tall as the row, the empty one too, its message in it.
        for (const id of ['#list-pane', '#detail-pane', '#empty-pane']) {
          const pane = await box(page, id);
          assert.ok(Math.abs(pane.height - panes.height) < 1, `${id} fills the row at ${size.width}`);
        }
        assert.ok((await page.text('#empty-pane')).includes('No notes yet'));
        // Its empty state sits in its middle, not at the top of a blank box.
        const empty = await box(page, '#empty-pane .rx-empty');
        const pane = await box(page, '#empty-pane');
        const middle = (empty.top + empty.bottom) / 2 - (pane.top + pane.bottom) / 2;
        assert.ok(Math.abs(middle) < 2, `the empty state is in the middle (${middle})`);
      }
      await shot(page, 'panes-wide');
    }, DESKTOP));

  test('a short screen: the panes keep their floor and the content scrolls', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/shell-fill/tall`);
      const panes = await box(page, '#panes');
      assert.ok(panes.height >= 256 - 1, `the panes keep 16rem (${panes.height})`);
      await wheel(page, '#tall', 3000);
      const now = await scrolls(page, '.rx-shell__content');
      assert.ok(now['.rx-shell__content'] > 0, 'the content scrolled');
      assert.equal(now.window, 0, 'the window did not');
    }, DESKTOP));

  test('revealed loads when its pane scrolls it into view; an anchor scrolls its pane', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/shell-fill`);
      assert.equal(await page.eval(() => !!document.querySelector('#loaded')), false, 'not loaded while hidden');
      await page.eval(() => {
        const pane = document.querySelector('#list-pane');
        pane.scrollTop = pane.scrollHeight;
      });
      await page.waitFor(() => !!document.querySelector('#loaded'), { message: 'the revealed row to load' });
      await page.eval(() => { document.querySelector('#list-pane').scrollTop = 0; });
      await page.click('#jump');
      await page.waitFor(() => location.hash === '#row-50');
      const row = await box(page, '#row-50');
      const pane = await box(page, '#list-pane');
      assert.ok(row.top >= pane.top - 1 && row.bottom <= pane.bottom + 1, 'order 50 is in view in its pane');
      assert.equal(Math.round(await page.eval(() => window.scrollY)), 0, 'the window did not scroll');
      page.assertClean();
    }, DESKTOP));

  test('the grid takes the height left and follows the window; only its rows scroll', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/grid/shell`);
      await shot(page, 'grid-desktop');
      const pad = await page.eval(() => parseFloat(getComputedStyle(document.querySelector('.rx-shell__content')).paddingBottom));
      let grid = await box(page, '.rx-grid');
      assert.ok(Math.abs(grid.bottom - (DESKTOP.height - pad)) < 1, `the grid reaches the bottom (${grid.bottom})`);
      const bar = await box(page, '.rx-grid__bar');
      await wheel(page, '.rx-grid__scroll', 800);
      const now = await scrolls(page, '.rx-grid__scroll', '.rx-shell__content');
      assert.ok(now['.rx-grid__scroll'] > 0, 'the rows scrolled');
      assert.equal(now['.rx-shell__content'], 0, 'the content did not');
      assert.equal(now.window, 0, 'the window did not');
      assert.equal((await box(page, '.rx-grid__bar')).top, bar.top, 'the toolbar stayed');
      // Taller window, taller grid: CSS only.
      await page.send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 1000, deviceScaleFactor: 1, mobile: false });
      await page.waitFor(() => Math.abs(document.querySelector('.rx-grid').getBoundingClientRect().bottom - (1000 - parseFloat(getComputedStyle(document.querySelector('.rx-shell__content')).paddingBottom))) < 1, { message: 'the grid to follow the window' });
      // No rows: the grid still fills, with its empty state.
      await page.goto(`${app.url}/grid/shell?search=no-such-order-anywhere`);
      grid = await box(page, '.rx-grid');
      assert.ok(Math.abs(grid.bottom - (1000 - pad)) < 1, `the empty grid fills too (${grid.bottom})`);
      assert.ok(await page.eval(() => !!document.querySelector('.rx-grid__empty')), 'its empty state shows');
      page.assertClean();
    }, DESKTOP));

  test("htmx's history brings back where the content was", () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/shell-fill/tall`);
      await page.eval(() => {
        const link = document.createElement('a');
        link.id = 'go';
        link.href = '/shell-fill';
        link.setAttribute('hx-get', '/shell-fill');
        link.setAttribute('hx-target', 'body');
        link.setAttribute('hx-push-url', 'true');
        link.textContent = 'go';
        document.querySelector('#tall').prepend(link);
        htmx.process(link);
        // Scrolled after the link is in (adding it above would move the place).
        document.querySelector('.rx-shell__content').scrollTop = 300;
        link.click();
      });
      await page.waitFor(() => location.pathname === '/shell-fill' && !document.querySelector('#tall'));
      await page.eval(() => history.back());
      await page.waitFor(() => !!document.querySelector('#tall'), { message: 'the page back from the history' });
      await page.waitFor(() => document.querySelector('.rx-shell__content').scrollTop === 300, { message: 'the content back at 300' });
    }, DESKTOP));
});

describe('on tablets and phones', () => {
  for (const [name, size] of [['tablet', TABLET], ['phone', PHONE]]) {
    test(`a ${name} scrolls the page, and the panes keep their room`, () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}/shell-fill`);
        await shot(page, `panes-${name}`);
        const now = await scrolls(page, '.rx-shell__content', '#list-pane');
        assert.ok(now.page > 0, 'the document is taller than the window');
        assert.equal(now['#list-pane'], 0);
        const overflow = await page.eval(() => getComputedStyle(document.querySelector('#list-pane')).overflowY);
        assert.equal(overflow, 'visible', 'a pane grows with what it holds');
        // The empty pane keeps its room on a tablet; panes stack in one column.
        const panes = await page.eval(() => [...document.querySelectorAll('.rx-pane')].map((p) => p.getBoundingClientRect().left));
        assert.equal(new Set(panes.map(Math.round)).size, 1, 'the panes stack');
        const fill = await box(page, '#panes');
        assert.ok(fill.height >= Math.max(size.height * 0.6, 384) - 1, `the panes keep their minimum (${fill.height})`);
        assert.ok(await page.eval(() => document.documentElement.scrollWidth <= innerWidth), 'no sideways scrolling');
        await page.eval(() => window.scrollTo(0, 2000));
        assert.ok((await page.eval(() => window.scrollY)) > 0, 'the window scrolls');
        page.assertClean();
      }, size));

    test(`a ${name}: a grid with few rows keeps its minimum height`, () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}/grid/shell?search=no-such-order-anywhere`);
        const grid = await box(page, '.rx-grid');
        const min = Math.max(384, size.height * 0.6);
        assert.ok(grid.height >= min - 1, `the grid keeps ${min}px (${grid.height})`);
        page.assertClean();
      }, size));
  }
});
