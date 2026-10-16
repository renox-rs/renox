// The receipt block (#48) in a browser, on the bike shop's /about/blocks page
// (an 80 mm and a 58 mm receipt) and on a counter sale's receipt page:
//
// - on screen the paper has the roll's width (80 mm = 302 px, 58 mm = 219 px
//   at 96 dpi), black on white whatever the colour scheme, with no sideways
//   scrolling on a phone;
// - the "Print" button calls window.print() (under CSP=strict: no inline
//   handler);
// - with print media emulated, everything but the receipt is dropped, the
//   paper keeps its width and the buttons are gone;
// - Chrome's own PDF of the printed page (`Page.printToPDF` with
//   `preferCSSPageSize`) has the size of the roll: 80 x 297 mm or 58 x 297 mm,
//   on one page.
//
// Screenshots go to BLOCKS_SCREENS when it's set.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Browser } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PHONE = { width: 390, height: 844 };
const PAGE = '/about/blocks';
const MM = 96 / 25.4; // CSS pixels per millimetre
const PT = 72 / 25.4; // PDF points per millimetre
let browser;
let app;

before(async () => {
  browser = await Browser.launch();
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { CSP: 'strict', BIKESHOP_STAFF_2FA: 'optional' } });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

async function shot(page, name) {
  if (process.env.BLOCKS_SCREENS) await page.screenshot(join(process.env.BLOCKS_SCREENS, `${name}.png`));
}

const media = (page, value) => page.send('Emulation.setEmulatedMedia', { media: value });
const scheme = (page, value) =>
  page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value }] });
const close = (actual, expected, slack, message) =>
  assert.ok(Math.abs(actual - expected) <= slack, `${message}: ${actual} is not ${expected} +/- ${slack}`);

/** The width in pixels of a receipt's paper. */
const paperWidth = (page, id) =>
  page.eval((s) => document.querySelector(`#${s} .rx-receipt__paper`).getBoundingClientRect().width, id);

/** Hides every receipt but `id` the way a page would (inline style), for printing one at a time. */
const only = (page, id) =>
  page.eval((keep) => {
    for (const r of document.querySelectorAll('.rx-receipt')) r.style.display = r.id === keep ? '' : 'none';
  }, id);

/** The size in points of the first page of the PDF Chrome prints, and how many pages. */
async function printed(page) {
  const { data } = await page.send('Page.printToPDF', { preferCSSPageSize: true, printBackground: true });
  const pdf = Buffer.from(data, 'base64').toString('latin1');
  const box = pdf.match(/\/MediaBox\s*\[\s*0\s+0\s+([\d.]+)\s+([\d.]+)\s*\]/);
  assert.ok(box, 'the PDF has a MediaBox');
  return { width: Number(box[1]), height: Number(box[2]), pages: (pdf.match(/\/Type\s*\/Page[^s]/g) || []).length };
}

describe('the receipt block', () => {
  test('on screen: the roll\'s width, white paper in both schemes, no sideways scrolling', async () => {
    for (const [size, options] of [['desktop', undefined], ['phone', PHONE]]) {
      for (const colours of ['light', 'dark']) {
        await browser.with(async (page) => {
          await scheme(page, colours);
          await page.goto(`${app.url}${PAGE}`);
          await page.waitFor(() => !!document.querySelector('[data-rx-receipt][data-rx-blocks-ready]'));
          close(await paperWidth(page, 'demo-receipt-80'), 80 * MM, 1.5, `80 mm paper (${size})`);
          close(await paperWidth(page, 'demo-receipt-58'), 58 * MM, 1.5, `58 mm paper (${size})`);
          const look = await page.eval(() => {
            const s = getComputedStyle(document.querySelector('.rx-receipt__paper'));
            return { background: s.backgroundColor, color: s.color, family: s.fontFamily };
          });
          assert.equal(look.background, 'rgb(255, 255, 255)', `white paper (${colours})`);
          assert.equal(look.color, 'rgb(0, 0, 0)', `black text (${colours})`);
          assert.match(look.family, /mono|Menlo|Consolas/i, 'monospace');
          assert.ok(await page.eval(() => document.documentElement.scrollWidth <= window.innerWidth), 'no sideways scrolling');
          // The lines, the strong total and the code slot are all there.
          assert.ok(await page.eval(() => document.querySelectorAll('#demo-receipt-80 .rx-receipt__row').length === 3));
          assert.match(await page.eval(() => document.querySelector('#demo-receipt-80 .rx-receipt__sum--strong').textContent), /\$120\.00/);
          assert.match(await page.eval(() => document.querySelector('#demo-receipt-80 .rx-receipt__code').textContent), /R-1042/);
          await shot(page, `receipt-${size}-${colours}`);
          page.assertClean();
        }, options);
      }
    }
  });

  test('the Print button opens the print dialog', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}${PAGE}`);
      await page.waitFor(() => !!document.querySelector('[data-rx-receipt][data-rx-blocks-ready]'));
      await page.eval(() => {
        window.__printed = 0;
        window.print = () => { window.__printed += 1; };
      });
      await page.click('#demo-receipt-80 [data-rx-receipt-print]');
      assert.equal(await page.eval(() => window.__printed), 1);
      page.assertClean();
    }));

  for (const [id, mm] of [['demo-receipt-80', 80], ['demo-receipt-58', 58]]) {
    test(`printed: only the receipt, ${mm} mm wide, on a ${mm} mm page`, () =>
      browser.with(async (page) => {
        await page.goto(`${app.url}${PAGE}`);
        await page.waitFor(() => !!document.querySelector('[data-rx-receipt][data-rx-blocks-ready]'));
        await only(page, id);
        await media(page, 'print');
        // Everything else is out of the printout: the page chrome, the other
        // cards, the button; the receipt keeps the roll's width, from the left edge.
        const state = await page.eval((keep) => {
          const shown = (el) => !!el && getComputedStyle(el).display !== 'none';
          const paper = document.querySelector(`#${keep} .rx-receipt__paper`);
          const box = paper.getBoundingClientRect();
          return {
            navbar: shown(document.querySelector('.rx-navbar')),
            footer: shown(document.querySelector('footer.bs-footer')),
            toc: shown(document.querySelector('.bs-showcase__toc')),
            heading: shown(document.querySelector('.bs-showcase h1, .bs-showcase .rx-title')),
            gallery: shown(document.querySelector('.rx-gallery')),
            button: shown(document.querySelector(`#${keep} .rx-receipt__actions`)),
            paper: shown(paper),
            left: box.left,
            top: box.top,
            width: box.width,
          };
        }, id);
        for (const part of ['navbar', 'footer', 'toc', 'heading', 'gallery', 'button']) {
          assert.equal(state[part], false, `${part} is left out of the printout`);
        }
        assert.equal(state.paper, true);
        close(state.width, mm * MM, 1.5, 'the paper keeps the roll\'s width');
        close(state.left, 0, 1, 'the receipt starts at the left edge');
        close(state.top, 0, 1, 'the receipt starts at the top');
        const pdf = await printed(page);
        close(pdf.width, mm * PT, 1.5, `the page is ${mm} mm wide`);
        close(pdf.height, 297 * PT, 1.5, 'the page is 297 mm long');
        assert.equal(pdf.pages, 1, 'one page');
      }));
  }

  test('a counter sale\'s receipt is an 80 mm till receipt that prints alone', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/login`);
      await page.type('#rx-email', 'cashier.south@bikeshop.test');
      await page.type('#rx-password', 'password');
      const loaded = page.once('Page.loadEventFired');
      await page.click('form button[type=submit]');
      await loaded;
      await page.settle();
      // Ring up a helmet and take cash, as a cashier does.
      await page.goto(`${app.url}/staff/counter`);
      const option = await page.eval(async () => {
        const res = await fetch('/staff/counter/variants?q=helmet', { headers: { Accept: 'application/json' } });
        return (await res.json())[0];
      });
      assert.ok(option?.value, 'the search found a helmet');
      await page.eval((o) => {
        document.querySelector('select[name=variant_id]').add(new Option(o.label, o.value, true, true));
      }, option);
      await page.eval(() => document.querySelector('.bs-counter__find button[type=submit]').form.requestSubmit());
      await page.waitFor(() => document.querySelectorAll('.bs-cart__line--counter').length === 1);
      await page.settle();
      await page.waitFor(() => !!document.querySelector('[data-rx-keypad][data-rx-blocks-ready]'));
      for (const key of ['9', '9', '9', '9', '9', '9', '9']) await page.click(`[data-rx-keypad-key="${key}"]`);
      await page.waitFor(() => /Change/.test(document.querySelector('#rx-tendered-hint')?.textContent || ''));
      await page.eval(() => document.activeElement?.blur());
      const paid = page.once('Page.loadEventFired');
      await page.press('p');
      await paid;
      await page.settle();
      await page.waitFor(() => !!document.querySelector('#counter-receipt [data-rx-receipt-print], #counter-receipt'));
      close(await paperWidth(page, 'counter-receipt'), 80 * MM, 1.5, '80 mm paper');
      assert.ok(await page.eval(() => document.querySelectorAll('#counter-receipt .rx-receipt__row').length >= 1), 'the sale\'s line');
      assert.ok(await page.eval(() => /Receipt/.test(document.querySelector('#counter-receipt').textContent)));
      await shot(page, 'receipt-counter');
      await media(page, 'print');
      assert.equal(await page.eval(() => getComputedStyle(document.querySelector('.bs-print-actions')).display), 'none');
      assert.equal(await page.eval(() => getComputedStyle(document.querySelector('.rx-alert') || document.body).display === 'none' || !document.querySelector('.rx-alert')), true);
      const pdf = await printed(page);
      close(pdf.width, 80 * PT, 1.5, 'the page is 80 mm wide');
      assert.equal(pdf.pages, 1, 'one page');
      page.assertClean();
    }));
});
