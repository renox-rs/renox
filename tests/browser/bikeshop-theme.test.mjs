// Each store's brand theme (#36) in a browser, on the seeded demo shop: the
// store hosts (`north.localhost`, `south.localhost`, `west.localhost`) link
// their own `/_renox/theme/{hash}.css` after the kit, and the accent tokens
// the page resolves (buttons, links, the focus ring, the soft tint and the
// first chart colour the grid and charts draw with) are that store's brand in
// the light and the dark colour scheme, while the shop's own host keeps the
// kit's teal. Screenshots go to BIKESHOP_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const STORES = ['north', 'south', 'west'];
const TOKENS = ['--rx-accent', '--rx-accent-hover', '--rx-accent-text', '--rx-focus', '--rx-accent-soft', '--rx-chart-1'];
const KIT_ACCENT = 'rgb(11, 110, 102)'; // #0b6e66, the kit's default

let browser;
let app;
let port;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true });
  port = new URL(app.url).port;
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BIKESHOP_SCREENS) await page.screenshot(join(process.env.BIKESHOP_SCREENS, `${name}.png`));
}

const scheme = (page, value) =>
  page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value }] });

/** The token values of one block of the theme stylesheet (`open` is its selector). */
function block(css, open) {
  const start = css.indexOf(open);
  assert.ok(start >= 0, `${open} in the theme stylesheet`);
  const body = css.slice(start + open.length, css.indexOf('}', start));
  return Object.fromEntries([...body.matchAll(/(--[\w-]+):([^;]+)/g)].map(([, name, value]) => [name, value.trim()]));
}

/**
 * What the page resolves each token to, next to what the theme stylesheet says
 * it should be: probes painted with `var(--token)` and with the literal value
 * resolve to the same computed colour.
 */
function resolved(tokens) {
  const probe = (css) => {
    const el = document.createElement('i');
    el.style.cssText = css;
    document.body.append(el);
    const out = getComputedStyle(el).backgroundColor;
    el.remove();
    return out;
  };
  return Object.fromEntries(
    Object.entries(tokens).map(([name, literal]) => [name, [probe(`background:var(${name})`), probe(`background:${literal}`)]]),
  );
}

async function themeOf(page) {
  const href = await page.eval(() => [...document.querySelectorAll('link[rel=stylesheet]')].map((l) => l.getAttribute('href')).find((h) => h.startsWith('/_renox/theme/')));
  assert.ok(href, 'a theme link on the page');
  const css = await page.eval(async (h) => (await fetch(h)).text(), href);
  return { href, css };
}

describe('store brands', () => {
  for (const mode of ['light', 'dark']) {
    test(`each store page wears its own brand (${mode})`, async () => {
      const accents = new Set();
      for (const store of STORES) {
        await browser.with(async (page) => {
          await scheme(page, mode);
          await page.goto(`http://${store}.localhost:${port}/`);
          const { href, css } = await themeOf(page);
          const order = await page.eval((h) => {
            const links = [...document.querySelectorAll('link[rel=stylesheet]')].map((l) => l.getAttribute('href'));
            return links.indexOf(h) > links.findIndex((l) => l.startsWith("/_renox/ui-"));
          }, href);
          assert.ok(order, 'the theme link comes after the kit stylesheet');

          const light = block(css, ':root{');
          const dark = block(css, ':root[data-theme="dark"]{');
          const media = block(css, '@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){');
          assert.deepEqual(media, dark, 'the media block and the attribute block agree');
          const expected = Object.fromEntries(TOKENS.map((t) => [t, (mode === 'dark' ? dark : light)[t]]));
          assert.ok(Object.values(expected).every(Boolean), `every token in the ${mode} block`);

          const pairs = await page.eval(resolved, expected);
          for (const [name, [seen, wanted]] of Object.entries(pairs)) {
            assert.equal(seen, wanted, `${store} ${name} (${mode})`);
          }
          accents.add(pairs['--rx-accent'][0]);
          assert.notEqual(pairs['--rx-accent'][0], KIT_ACCENT, `${store} is not the kit's teal`);
          await shot(page, `theme-${store}-${mode}`);
          page.assertClean();
        });
      }
      assert.equal(accents.size, STORES.length, 'three stores, three accents');
    });
  }

  test('an explicit data-theme beats the colour scheme and keeps the brand', () =>
    browser.with(async (page) => {
      await scheme(page, 'light');
      await page.goto(`http://south.localhost:${port}/`);
      const { css } = await themeOf(page);
      const dark = block(css, ':root[data-theme="dark"]{');
      await page.eval(() => document.documentElement.setAttribute('data-theme', 'dark'));
      const [seen, wanted] = (await page.eval(resolved, { '--rx-accent': dark['--rx-accent'] }))['--rx-accent'];
      assert.equal(seen, wanted);
      page.assertClean();
    }));

  test('the kit look keys (classic, warm) do not undo the brand', () =>
    browser.with(async (page) => {
      await page.goto(`http://north.localhost:${port}/`);
      const { css } = await themeOf(page);
      const light = block(css, ':root{');
      for (const look of ['classic', 'warm']) {
        await page.eval((l) => document.documentElement.setAttribute('data-rx-theme', l), look);
        const [seen, wanted] = (await page.eval(resolved, { '--rx-accent': light['--rx-accent'] }))['--rx-accent'];
        assert.equal(seen, wanted, `brand accent under data-rx-theme=${look}`);
      }
      page.assertClean();
    }));

  test("the shop's own host has no theme link and keeps the kit accent", () =>
    browser.with(async (page) => {
      await scheme(page, 'light');
      await page.goto(`${app.url}/`);
      const links = await page.eval(() => [...document.querySelectorAll('link[rel=stylesheet]')].map((l) => l.getAttribute('href')));
      assert.ok(!links.some((h) => h.startsWith('/_renox/theme/')), `no theme link in ${links}`);
      const [seen] = (await page.eval(resolved, { '--rx-accent': '#0b6e66' }))['--rx-accent'];
      assert.equal(seen, KIT_ACCENT);
      page.assertClean();
    }));
});
