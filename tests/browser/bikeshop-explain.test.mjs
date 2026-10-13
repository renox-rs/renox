// "About this page" beside every page of the bike shop: on a wide screen
// (1440 px) the panel is docked to the right of the page, scrolls on its
// own, folds to a rail and stays folded after a reload (a cookie the server
// reads), shows the Renox features as chips that open their "why" and the
// code in the kit's tabs (keyboard too); on a phone (390 px) the "About this
// page" button opens the same content, code included, in the kit's sheet,
// with nothing wider than the screen. Light and dark, under CSP=strict.
// Screenshots of the home page, a product page, the staff dashboard, the
// rental counter and a report go to BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';
import { logInWithFetch } from './lib/bikeshop.mjs';

const WIDE = { width: 1440, height: 900 };
const PHONE = { width: 390, height: 844 };

let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', {
    seed: true,
    env: { CSP: 'strict', BIKESHOP_STAFF_2FA: 'optional' },
  });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `explain-${name}.png`));
}

/** No sideways scrolling: nothing is wider than the viewport. */
const fitsWidth = (page) => page.eval(() => document.documentElement.scrollWidth <= window.innerWidth);

/** The docked panel's box, and the page's main column's. */
const boxes = (page) =>
  page.eval(() => {
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, width: r.width, height: r.height, top: r.top };
    };
    return { dock: box(document.querySelector('#explain-dock')), main: box(document.querySelector('#main')), width: document.documentElement.clientWidth };
  });

async function dark(page, on) {
  await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: on ? 'dark' : 'light' }] });
}

/** Logs in as a seeded demo user with the login form's own fields, sent
 *  from the page with fetch (see bikeshop-mobile.test.mjs, #342). */
async function logIn(page, email) {
  const status = await logInWithFetch(page, app.url, email);
  assert.ok(!status.endsWith(' /login'), `logged in as ${email} (${status})`);
}

/** The first product page's address, from the catalogue. */
async function productUrl(page) {
  await page.goto(`${app.url}/shop`);
  return page.eval(() => document.querySelector('main a[href^="/products/"]').href);
}

describe('the docked panel at 1440 px', () => {
  test('sits beside the page, scrolls on its own, and the page fits', () =>
    browser.with(async (page) => {
      await page.send('Network.clearBrowserCookies');
      await page.goto(`${app.url}/`);
      const { dock, main, width } = await boxes(page);
      assert.ok(dock && dock.width > 0, 'the panel shows');
      assert.ok(main.right <= dock.left + 1, `the page (${main.right}) ends where the panel (${dock.left}) starts`);
      const share = dock.width / width;
      assert.ok(share >= 0.28 && share <= 0.42, `the panel takes ${Math.round(share * 100)}% of the width`);
      // Under the public bar, which keeps the whole width, down to the bottom.
      const bar = await page.eval(() => {
        const r = document.querySelector('.rx-navbar').getBoundingClientRect();
        return { bottom: r.bottom, width: r.width };
      });
      assert.ok(Math.abs(dock.top - bar.bottom) < 2, `the panel starts under the bar (${dock.top}, ${bar.bottom})`);
      assert.ok(Math.abs(bar.width - width) < 2, 'the bar spans the panel too');
      assert.ok(Math.abs(dock.top + dock.height - 900) < 2, 'down to the bottom');
      // Sticky: it stays put while the page scrolls, and scrolls on its own.
      await page.eval(() => window.scrollTo(0, 1200));
      assert.equal((await boxes(page)).dock.top, dock.top, 'in place after scrolling the page');
      assert.equal(await page.eval(() => getComputedStyle(document.querySelector('.bs-dock__panel')).overflowY), 'auto');
      // The navbar's button opens the sheet only where the panel isn't docked.
      assert.equal(await page.eval(() => document.querySelector('.rx-navbar [data-rx-open="about-page"]').getBoundingClientRect().width), 0);
      assert.ok(await fitsWidth(page), 'no sideways scrolling');
      const text = await page.text('#explain-dock');
      for (const part of ['About this page', 'Home', 'Renox features used, and why', 'The code', 'Under the hood', 'In the guide']) {
        assert.ok(text.includes(part), `the panel shows "${part}"`);
      }
      page.assertClean();
    }, WIDE));

  test('folds to a rail, stays folded after a reload, and opens again', () =>
    browser.with(async (page) => {
      await page.send('Network.clearBrowserCookies');
      await page.goto(`${app.url}/`);
      const open = (await boxes(page)).dock.width;
      await page.click('[data-bs-dock-toggle="rail"]');
      await page.waitFor(() => document.body.classList.contains('bs-docked--rail'));
      const rail = (await boxes(page)).dock.width;
      assert.ok(rail < 80 && rail < open / 4, `folded to a rail (${rail} px)`);
      assert.equal(await page.eval(() => document.activeElement.dataset.bsDockToggle), 'open', 'the focus moves to the button that opens it');
      // After a reload the server draws it folded: the class is in the HTML.
      await page.goto(`${app.url}/shop`);
      const html = await page.eval(async () => (await (await fetch(location.href)).text()).includes('bs-docked--rail'));
      assert.ok(html, 'the server reads the cookie');
      assert.ok((await boxes(page)).dock.width < 80, 'still folded on the next page');
      await shot(page, 'rail-1440');
      // The keyboard opens it again.
      await page.focus('[data-bs-dock-toggle="open"]');
      await page.press('Enter');
      await page.waitFor(() => !document.body.classList.contains('bs-docked--rail'));
      assert.ok((await boxes(page)).dock.width > 300, 'open again');
      assert.equal(await page.eval(() => document.activeElement.dataset.bsDockToggle), 'rail');
      await page.goto(`${app.url}/`);
      assert.ok((await boxes(page)).dock.width > 300, 'and stays open');
      page.assertClean();
    }, WIDE));

  test('the features open their why, the code tabs work with the keyboard', () =>
    browser.with(async (page) => {
      await page.send('Network.clearBrowserCookies');
      await page.goto(`${app.url}/`);
      // A chip shows its why; another one swaps it.
      await page.click('#explain-dock [data-bs-chip]');
      await page.waitFor(() => !document.querySelector('#explain-dock-why-0').hidden);
      await page.click('#explain-dock li:nth-child(2) [data-bs-chip]');
      await page.waitFor(() => !document.querySelector('#explain-dock-why-1').hidden && document.querySelector('#explain-dock-why-0').hidden);
      // Tabs: one panel shows at a time; arrows, Home and End move.
      const shown = () =>
        page.eval(() => [...document.querySelectorAll('#explain-dock [role=tabpanel]')].filter((p) => !p.hidden).map((p) => p.id));
      assert.deepEqual(await shown(), ['explain-dock-code-panel-0']);
      await page.focus('#explain-dock-code-tab-0');
      await page.press('ArrowRight');
      assert.deepEqual(await shown(), ['explain-dock-code-panel-1']);
      assert.equal(await page.focused(), 'explain-dock-code-tab-1');
      await page.press('End');
      assert.deepEqual(await shown(), ['explain-dock-code-panel-2']);
      await page.press('Home');
      assert.deepEqual(await shown(), ['explain-dock-code-panel-0']);
      // Coloured on the server, with the file on GitHub and a copy button.
      const sample = await page.eval(() => {
        const panel = document.querySelector('#explain-dock-code-panel-0');
        return {
          spans: panel.querySelectorAll('pre [class^="hl-"]').length,
          link: panel.querySelector('.bs-code__path').href,
          copy: panel.querySelector('[data-rx-copy]').getAttribute('data-rx-copy-text'),
        };
      });
      assert.ok(sample.spans > 5, 'coloured');
      assert.match(sample.link, /^https:\/\/github\.com\/renox-rs\/renox\/blob\/main\/examples\/bikeshop\/src\/app\/home\/mod\.rs#L\d+-L\d+$/);
      assert.ok(sample.copy.startsWith('fn routes(&self) -> Routes {'), 'the copy button holds the code');
      page.assertClean();
    }, WIDE));

  test('light and dark: the code keeps its contrast', () =>
    browser.with(async (page) => {
      for (const mode of [false, true]) {
        await dark(page, mode);
        await page.goto(`${app.url}/`);
        const colours = await page.eval(() => {
          const frame = getComputedStyle(document.querySelector('#explain-dock .bs-code__frame')).backgroundColor;
          const kw = getComputedStyle(document.querySelector('#explain-dock .hl-kw')).color;
          return { frame, kw };
        });
        const lum = (rgb) => {
          const [r, g, b] = rgb.match(/\d+/g).slice(0, 3).map((v) => {
            const c = v / 255;
            return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
          });
          return 0.2126 * r + 0.7152 * g + 0.0722 * b;
        };
        const [a, b] = [lum(colours.frame), lum(colours.kw)];
        const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
        assert.ok(ratio >= 4.5, `${mode ? 'dark' : 'light'}: keywords at ${ratio.toFixed(2)}:1`);
        assert.equal(lum(colours.frame) < 0.1, mode, `${mode ? 'dark' : 'light'} code frame`);
      }
      page.assertClean();
    }, WIDE));
});

describe('the sheet at 390 px', () => {
  test('"About this page" opens the same content, code included', () =>
    browser.with(async (page) => {
      await page.send('Network.clearBrowserCookies');
      await page.goto(`${app.url}/`);
      assert.equal((await boxes(page)).dock.width, 0, 'no docked panel on a phone');
      assert.ok(await fitsWidth(page), 'no sideways scrolling');
      await page.click('.rx-navbar [data-rx-open="about-page"]');
      await page.waitFor(() => document.querySelector('#about-page')?.open, { message: 'the sheet open' });
      await page.waitFor(() => document.querySelector('#about-page').getAnimations({ subtree: true }).every((a) => a.playState !== 'running'));
      const sheet = await page.eval(() => {
        const pre = document.querySelector('#about-page-code-panel-0 pre');
        const dialog = document.querySelector('#about-page');
        return { code: pre.getBoundingClientRect().height, width: dialog.getBoundingClientRect().width, scroll: dialog.scrollWidth <= dialog.clientWidth };
      });
      assert.ok(sheet.code > 40, 'the code shows');
      assert.ok(sheet.width <= 390, 'the sheet fits the screen');
      assert.ok(sheet.scroll, 'the sheet scrolls only down; the code scrolls inside its frame');
      await page.click('#about-page-code-tab-1');
      await page.waitFor(() => !document.querySelector('#about-page-code-panel-1').hidden);
      assert.ok(await fitsWidth(page));
      page.assertClean();
    }, PHONE));
});

describe('screens', () => {
  const pages = [
    ['home', null, async () => `${app.url}/`],
    ['product', null, null],
    ['staff', 'manager.north@bikeshop.test', async () => `${app.url}/staff`],
    ['rentals', 'manager.north@bikeshop.test', async () => `${app.url}/staff/rentals`],
    ['report', 'manager.north@bikeshop.test', async () => `${app.url}/staff/reports/rentals`],
  ];
  for (const [name, user, address] of pages) {
    for (const [size, viewport] of [
      ['1440', WIDE],
      ['390', PHONE],
    ]) {
      test(`${name} at ${size} px, light and dark`, () =>
        browser.with(async (page) => {
          if (user) await logIn(page, user);
          else await page.send('Network.clearBrowserCookies');
          const url = address ? await address() : await productUrl(page);
          for (const mode of [false, true]) {
            await dark(page, mode);
            await page.goto(url);
            assert.equal(await page.eval(() => location.pathname === '/login'), false, `${url} answers`);
            assert.ok(await fitsWidth(page), `${name}: no sideways scrolling`);
            const docked = (await boxes(page)).dock.width > 0;
            assert.equal(docked, viewport === WIDE, 'docked only on the wide screen');
            assert.ok((await page.text(docked ? '#explain-dock' : '#about-page')).includes('The code'), 'with its code');
            await page.settle();
            await shot(page, `${name}-${size}-${mode ? 'dark' : 'light'}`);
            if (!docked && mode) {
              await page.click('.rx-navbar [data-rx-open="about-page"]');
              await page.waitFor(() => document.querySelector('#about-page')?.open);
              await page.waitFor(() => document.querySelector('#about-page').getAnimations({ subtree: true }).every((a) => a.playState !== 'running'));
              await shot(page, `${name}-${size}-sheet`);
            }
          }
          page.assertClean();
        }, viewport));
    }
  }
});
