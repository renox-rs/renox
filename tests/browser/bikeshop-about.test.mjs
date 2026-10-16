// The bike shop's skeleton (#232, part 1) in a browser: the public and staff
// layouts at desktop and phone width, the "About this page" panel (docked
// beside the page at desktop width, a sheet on a phone), the index
// of every page, Motion (vendored) under the default CSP and CSP=strict, and
// prefers-reduced-motion, and the shop in Spanish (the language menu, the
// panel, the staff side, money) at desktop and phone width. Staff log in with
// a password only (BIKESHOP_STAFF_2FA=optional). Screenshots go to
// BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';
import { logInWithForm } from './lib/bikeshop.mjs';

const PHONE = { width: 390, height: 844 };
const ENGLISH = ['Renox features used, and why', 'Under the hood', 'In the guide', 'Source files'];
const SPANISH = ['Funciones de Renox que usa, y por qué', 'Por dentro', 'En la guía', 'Archivos fuente'];
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

/** No sideways scrolling: nothing is wider than the viewport. */
const fitsWidth = (page) => page.eval(() => document.documentElement.scrollWidth <= window.innerWidth);

/** Waits until the content Motion slides in is in place. */
const revealed = (page) =>
  page.waitFor(() =>
    [...document.querySelectorAll('[data-bs-reveal] > *')].every(
      (el) =>
        getComputedStyle(el).opacity === '1' &&
        ['none', 'matrix(1, 0, 0, 1, 0, 0)'].includes(getComputedStyle(el).transform),
    ),
  );

/** Checks what "About this page" shows: docked beside the page on a wide
 *  screen (75rem and up), else in the sheet its button opens. */
async function openPanel(page, title, parts = ENGLISH) {
  const docked = await page.eval(() => (document.querySelector('#explain-dock')?.getBoundingClientRect().width ?? 0) > 0);
  if (!docked) {
    await page.click('[data-rx-open="about-page"]');
    await page.waitFor(() => document.querySelector('#about-page')?.open, { message: 'the panel open' });
  }
  const where = docked ? '#explain-dock' : '#about-page';
  const text = await page.text(where);
  assert.ok(text.includes(title), `the panel is about "${title}"`);
  for (const part of parts) {
    assert.ok(text.includes(part), `the panel shows "${part}"`);
  }
  // Its sections slide in with Motion (in the sheet) and end fully shown.
  await page.waitFor(
    (w) => [...document.querySelectorAll(`${w} .bs-explain > *`)].every((el) => getComputedStyle(el).opacity === '1'),
    {},
    where,
  );
  return docked;
}

/** Closes the sheet with Esc (a docked panel stays). */
async function closePanel(page, docked) {
  if (docked) return;
  // Esc goes to the sheet; if a tab that just navigated swallowed it (#327's
  // family), say what the page looked like and press it again.
  for (let attempt = 1; ; attempt++) {
    await page.press('Escape');
    try {
      await page.waitFor(() => !document.querySelector('#about-page').open, { timeout: 2000, message: 'Esc closes it' });
      return;
    } catch (error) {
      const state = await page.eval(() => ({
        focused: document.hasFocus(),
        active: document.activeElement?.outerHTML.slice(0, 100),
        visibility: document.visibilityState,
      }));
      process.stderr.write(`Esc did not close the sheet (try ${attempt}): ${JSON.stringify(state)}\n`);
      if (attempt >= 3) throw error;
      await page.send('Page.bringToFront');
    }
  }
}

for (const csp of ['relaxed', 'strict']) {
  describe(`bikeshop under CSP=${csp}`, () => {
    let app;
    before(async () => {
      // Seeded: the staff side needs a role in a store (the demo users).
      // Staff log in with a password only here: two-factor login, which the
      // shop asks staff for, is tested in bikeshop-staff.test.mjs.
      app = await start('bikeshop', 'examples/bikeshop', {
        env: { CSP: csp, BIKESHOP_STAFF_2FA: 'optional' },
        seed: true,
      });
    });
    after(() => app?.stop());

    for (const [size, options] of [
      ['desktop', undefined],
      ['phone', PHONE],
    ]) {
      test(`the home page and its panel (${size})`, () =>
        browser.with(async (page) => {
          await page.goto(`${app.url}/`);
          assert.equal(await page.eval(() => typeof window.Motion?.animate), 'function', 'Motion is loaded');
          // The content slid in and ended where it belongs.
          await revealed(page);
          assert.ok(await fitsWidth(page), 'no sideways scrolling');
          await shot(page, `home-${size}-${csp}`);
          const docked = await openPanel(page, 'Home');
          assert.equal(docked, size === 'desktop', 'docked on a wide screen, a sheet on a phone');
          await shot(page, `home-panel-${size}-${csp}`);
          await closePanel(page, docked);
          page.assertClean();
        }, options));
    }

    test('the index of every page filters by feature', () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}/about/pages`);
        const all = await page.eval(() => document.querySelectorAll('#about-pages > li').length);
        assert.ok(all >= 10, `${all} pages listed`);
        await revealed(page);
        await shot(page, `about-pages-${csp}`);
        // A feature's badge is a link to the pages that use it.
        await page.click('#about-pages a[href*="feature=App%3A%3Adetect_locale"]');
        await page.waitFor(() => location.search.includes('feature='));
        const shown = await page.eval(() => document.querySelectorAll('#about-pages > li').length);
        assert.ok(shown >= 1 && shown < all, `${shown} of ${all} pages use it`);
        assert.ok(await fitsWidth(page));
        page.assertClean();
      }));

    test('the staff layout, after logging in as staff (desktop and phone)', () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}/login`);
        await page.type('#rx-email', 'staff.north@bikeshop.test');
        await page.type('#rx-password', 'password');
        await page.click('form button[type=submit]');
        await page.waitFor(() => location.pathname !== '/login');
        await page.goto(`${app.url}/staff`);
        // The dashboard standard (#24): the fill shell inside the docked body.
        assert.ok(await page.eval(() => !!document.querySelector('body.rx-page > .rx-shell.rx-shell--fill')));
        assert.ok(await fitsWidth(page));
        await revealed(page);
        await shot(page, `staff-desktop-${csp}`);
        await closePanel(page, await openPanel(page, 'Staff dashboard'));
        await page.send('Emulation.setDeviceMetricsOverride', { ...PHONE, deviceScaleFactor: 1, mobile: true });
        await page.goto(`${app.url}/staff`);
        assert.ok(await fitsWidth(page), 'no sideways scrolling on a phone');
        await revealed(page);
        await shot(page, `staff-phone-${csp}`);
        page.assertClean();
      }));
  });
}

describe('bikeshop with prefers-reduced-motion', () => {
  let app;
  before(async () => {
    app = await start('bikeshop', 'examples/bikeshop');
  });
  after(() => app?.stop());

  test('nothing moves, everything shows', () =>
    browser.with(async (page) => {
      await page.send('Emulation.setEmulatedMedia', {
        features: [{ name: 'prefers-reduced-motion', value: 'reduce' }],
      });
      await page.goto(`${app.url}/`);
      // Motion never touched the content: no inline styles left behind.
      assert.equal(
        await page.eval(() => [...document.querySelectorAll('[data-bs-reveal] > *')].filter((el) => el.style.opacity || el.style.transform).length),
        0,
      );
      page.assertClean();
    }));

  test('BIKESHOP_EXPLAIN=false hides the panel', async () => {
    const quiet = await start('bikeshop', 'examples/bikeshop', { env: { BIKESHOP_EXPLAIN: 'false' } });
    try {
      await browser.with(async (page) => {
        await page.goto(`${quiet.url}/`);
        assert.equal(await page.eval(() => document.querySelector('[data-rx-open="about-page"]')), null);
        page.assertClean();
      });
    } finally {
      await quiet.stop();
    }
  });
});

describe('bikeshop in Spanish', () => {
  let app;
  before(async () => {
    app = await start('bikeshop', 'examples/bikeshop', { env: { BIKESHOP_STAFF_2FA: 'optional' }, seed: true });
  });
  after(() => app?.stop());

  /** Chooses Español in the navbar's language menu, or on a phone in the
   *  menu panel the tab bar opens (#323). */
  async function spanish(page) {
    const phone = await page.eval(() => getComputedStyle(document.querySelector('.rx-tabbar')).display !== 'none');
    if (phone) {
      await page.click('.rx-tabbar [data-rx-open="site-menu"]');
      await page.waitFor(
        () => {
          const menu = document.querySelector('#site-menu');
          return menu?.open && menu.getAnimations({ subtree: true }).every((a) => a.playState !== 'running');
        },
        { message: 'the menu open' },
      );
      await page.click('#site-menu form:nth-of-type(2) button');
    } else {
      await page.click('[aria-controls="language-menu"]');
      await page.waitFor(() => !document.querySelector('#language-menu').hidden, { message: 'the menu open' });
      await page.click('#language-menu form:nth-of-type(2) button');
    }
    // The choice is a form post and the page loads again; mid-load the
    // document may have no root element yet, hence the `?.` below.
    await page.waitFor(() => document.documentElement?.lang === 'es', { message: 'the page in Spanish' });
  }

  for (const [size, options] of [
    ['desktop', undefined],
    ['phone', PHONE],
  ]) {
    test(`the language menu, the panel and the shop (${size})`, () =>
      browser.with(async (page) => {
        await page.send('Network.clearBrowserCookies');
        await page.goto(`${app.url}/`);
        await spanish(page);
        await revealed(page);
        assert.ok(await fitsWidth(page), 'no sideways scrolling');
        await shot(page, `home-es-${size}`);
        const docked = await openPanel(page, 'Inicio', SPANISH);
        await shot(page, `home-panel-es-${size}`);
        await closePanel(page, docked);
        // The catalogue: Spanish texts, money with Spanish separators.
        await page.goto(`${app.url}/shop`);
        const shop = await page.text('main');
        assert.match(shop, /\$\d{1,3}(\.\d{3})*,\d{2}\b/, '$1.249,99 in Spanish');
        assert.doesNotMatch(shop, /\$\d{1,3}(,\d{3})*\.\d{2}\b/, 'not $1,249.99');
        assert.ok(await fitsWidth(page));
        await shot(page, `shop-es-${size}`);
        // The index of every page, in Spanish.
        await page.goto(`${app.url}/about/pages`);
        assert.ok((await page.text('main')).includes('Todas las páginas y sus funciones'));
        assert.ok(await fitsWidth(page));
        page.assertClean();
      }, options));

    test(`the staff side and the admin panel (${size})`, () =>
      browser.with(async (page) => {
        // A fresh visitor (the tabs share cookies): the choice is kept in the
        // session, so Renox's login page follows it.
        await page.send('Network.clearBrowserCookies');
        await page.goto(`${app.url}/`);
        await spanish(page);
        await page.goto(`${app.url}/login`);
        assert.ok((await page.text('body')).includes('Iniciar sesión'), "Renox's login page in Spanish");
        // Logged in through the form, in this tab (the language stays), and
        // Esc closes the sheet below in the same tab (#354).
        const path = await logInWithForm(page, app.url, 'owner@bikeshop.test');
        assert.ok(path !== '/login', 'logged in as the owner');
        await page.goto(`${app.url}/staff/roles`);
        const roles = await page.text('main');
        assert.ok(roles.includes('Encargado de tienda') && roles.includes('Leer el registro de actividad.'), 'the roles in Spanish');
        assert.ok(await fitsWidth(page));
        await shot(page, `staff-roles-es-${size}`);
        await page.goto(`${app.url}/staff`);
        await revealed(page);
        await closePanel(page, await openPanel(page, 'Panel del personal', SPANISH));
        await shot(page, `staff-es-${size}`);
        page.assertClean();
      }, options));
  }
});
