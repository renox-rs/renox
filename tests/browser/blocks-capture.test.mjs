// renox-blocks' signature pad and camera capture (#53) in a browser, on the
// bike shop's /about/blocks form, under CSP=strict: a signature drawn with the
// mouse (the hidden file input holds signature.png, Undo and Clear take it
// back, the form posts it), the typed name reached and completed with the
// keyboard alone, the last way used winning; the camera with Chrome's fake
// device (the shutter adds a downsized photo and a thumbnail, Remove deletes
// it, the shutter stops at the maximum, the photos are posted); the camera
// denied or missing, where a visible file input takes over and what it picks is
// downsized; right-to-left, prefers-reduced-motion, and the parts that load.
// The camera is faked with flags (`Browser.launch({ args })`); the denied and
// the missing camera are a script run before the page's own, which makes
// getUserMedia reject with NotAllowedError, or removes navigator.mediaDevices.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser, sleep } from './lib/cdp.mjs';
import { start } from './lib/app.mjs';

const PAGE = '/about/blocks';
const SIGN = '[data-rx-signature][data-rx-blocks-ready]';
const CAMERA = '[data-rx-camera][data-rx-blocks-ready]';
const CANVAS = '[data-rx-signature-canvas]';
let browser;
let app;

before(async () => {
  browser = await Browser.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] });
  app = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { CSP: 'strict' } });
});

after(async () => {
  await app?.stop();
  await browser?.close();
});

const DENIED = `navigator.mediaDevices.getUserMedia = () => Promise.reject(new DOMException('denied', 'NotAllowedError'));`;
const MISSING = `Object.defineProperty(navigator, 'mediaDevices', { value: undefined, configurable: true });`;

/** The page with the pad and the camera set up, after `before` ran in it. */
async function open(page, before) {
  if (before) await page.send('Page.addScriptToEvaluateOnNewDocument', { source: before });
  await page.goto(`${app.url}${PAGE}#sign`);
  await page.waitFor((s) => !!document.querySelector(s), { message: 'the pad is set up' }, SIGN);
  await page.waitFor((s) => !!document.querySelector(s), { message: 'the camera is set up' }, CAMERA);
}

/** The file on a file input, as the form would post it, or null. */
const posted = (page, selector) =>
  page.eval((s) => {
    const input = document.querySelector(s);
    const file = input.files[0];
    return { count: input.files.length, name: file?.name ?? null, type: file?.type ?? null, size: file?.size ?? 0 };
  }, selector);

/** A real mouse stroke across the pad (fractions of its size). */
async function stroke(page, points) {
  await page.point(CANVAS);
  const box = await page.eval((s) => {
    const r = document.querySelector(s).getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height };
  }, CANVAS);
  const at = ([fx, fy]) => ({ x: box.x + box.w * fx, y: box.y + box.h * fy });
  const mouse = (type, point, held) =>
    page.send('Input.dispatchMouseEvent', {
      type,
      ...at(point),
      button: held || type === 'mouseReleased' ? 'left' : 'none',
      buttons: held ? 1 : 0,
      clickCount: type === 'mouseMoved' ? 0 : 1,
    });
  await mouse('mouseMoved', points[0], false);
  await mouse('mousePressed', points[0], true);
  for (const point of points.slice(1)) await mouse('mouseMoved', point, true);
  await mouse('mouseReleased', points[points.length - 1], false);
}

const WAVE = [[0.1, 0.5], [0.25, 0.2], [0.4, 0.8], [0.6, 0.2], [0.8, 0.7], [0.9, 0.4]];
const LINE = [[0.1, 0.9], [0.5, 0.85], [0.9, 0.9]];

/** Submits the demo form and waits for the toast that says what arrived. */
async function send(page, pattern) {
  await page.click('#sign-form button[type=submit]');
  await page.waitFor((p) => new RegExp(p).test(document.body?.textContent), { message: `a toast like ${pattern}` }, pattern);
  return page.text('body');
}

/** A JPEG file in the page: `width` by `height` pixels, set as the files of `selector`. */
const choose = (page, selector, width, height) =>
  page.eval(
    async (s, w, h) => {
      const canvas = document.createElement('canvas');
      canvas.width = w;
      canvas.height = h;
      const context = canvas.getContext('2d');
      context.fillStyle = '#c33';
      context.fillRect(0, 0, w, h);
      const blob = await new Promise((done) => canvas.toBlob(done, 'image/jpeg', 0.9));
      const transfer = new DataTransfer();
      transfer.items.add(new File([blob], 'big.jpg', { type: 'image/jpeg' }));
      const input = document.querySelector(s);
      input.files = transfer.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
    },
    selector,
    width,
    height,
  );

/** The pixel size of the first file on an input. */
const sizeOf = (page, selector) =>
  page.eval(async (s) => {
    const bitmap = await createImageBitmap(document.querySelector(s).files[0]);
    return { width: bitmap.width, height: bitmap.height };
  }, selector);

const shots = (page) => page.eval(() => document.querySelectorAll('[data-rx-camera-shots] li').length);

describe('the signature pad', () => {
  test('a stroke fills the file, Undo and Clear take it back, the form posts it', () =>
    browser.with(async (page) => {
      await open(page);
      assert.equal((await posted(page, '[data-rx-signature-file]')).count, 0, 'nothing drawn, nothing posted');
      assert.equal(await page.eval(() => document.querySelector('[data-rx-signature-undo]').disabled), true);
      await stroke(page, WAVE);
      await page.waitFor(() => document.querySelector('[data-rx-signature-file]').files.length === 1, { message: 'the drawing in the file' });
      const file = await posted(page, '[data-rx-signature-file]');
      assert.equal(file.name, 'signature.png');
      assert.equal(file.type, 'image/png');
      assert.ok(file.size > 100, `a real PNG (${file.size} bytes)`);
      assert.equal(await page.eval(() => document.querySelector('[data-rx-signature-undo]').disabled), false);
      // Ink was drawn on a white page: the canvas is no longer blank.
      assert.ok(await page.eval((s) => {
        const c = document.querySelector(s);
        const data = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
        for (let i = 0; i < data.length; i += 4) if (data[i] < 128) return true;
        return false;
      }, CANVAS), 'dark ink on the pad');
      // A second stroke, Undo takes back only it, a second Undo empties the file.
      await stroke(page, LINE);
      const two = await posted(page, '[data-rx-signature-file]');
      assert.notEqual(two.size, file.size, 'the file follows the drawing');
      await page.click('[data-rx-signature-undo]');
      assert.equal((await posted(page, '[data-rx-signature-file]')).size, file.size, 'one stroke left: the same picture as before');
      await page.click('[data-rx-signature-undo]');
      assert.equal((await posted(page, '[data-rx-signature-file]')).count, 0, 'no strokes, no file');
      assert.equal(await page.eval(() => document.querySelector('[data-rx-signature-clear]').disabled), true);
      // Clear.
      await stroke(page, WAVE);
      await stroke(page, LINE);
      await page.click('[data-rx-signature-clear]');
      assert.equal((await posted(page, '[data-rx-signature-file]')).count, 0, 'Clear empties the input');
      assert.match(await page.text('[data-rx-signature-status]'), /cleared/i, 'said in words');
      // Draw again and send: the server accepts the PNG.
      await stroke(page, WAVE);
      await page.waitFor(() => document.querySelector('[data-rx-signature-file]').files.length === 1);
      const text = await send(page, 'Received a drawn signature \\(image/png');
      assert.match(text, /0 photos|1 photos/);
      page.assertClean();
    }));

  test('the typed name, reached with the keyboard, is posted without a file', () =>
    browser.with(async (page) => {
      await open(page);
      // Tab from the pad goes past the disabled tools to the typed name.
      await page.focus(CANVAS);
      await page.press('Tab');
      assert.equal(await page.focused(), 'rx-signature-name');
      // The help text is linked from the pad and the group.
      const linked = await page.eval(() => {
        const canvas = document.querySelector('[data-rx-signature-canvas]');
        const help = document.getElementById(canvas.getAttribute('aria-describedby'));
        const group = document.querySelector('[data-rx-signature]');
        return {
          help: help?.textContent.trim().length > 20,
          group: group.getAttribute('aria-describedby').split(' ').includes(help.id),
          label: canvas.getAttribute('aria-label'),
        };
      });
      assert.deepEqual(linked, { help: true, group: true, label: 'Signature area' });
      await page.type('#rx-signature-name', 'Ana R.');
      assert.equal((await posted(page, '[data-rx-signature-file]')).count, 0, 'no file');
      await page.press('Enter');
      const text = await page.waitFor(() => /typed name \(Ana R\.\)/.test(document.body?.textContent) && document.body?.textContent, { message: 'the typed name arrived' });
      assert.match(text, /Received a typed name/);
      page.assertClean();
    }));

  test('the last way used wins: typing drops the drawing, drawing empties the name', () =>
    browser.with(async (page) => {
      await open(page);
      await stroke(page, WAVE);
      await page.waitFor(() => document.querySelector('[data-rx-signature-file]').files.length === 1);
      assert.equal(await page.eval(() => document.querySelector('#rx-signature-name').required), false, 'a drawing satisfies required');
      await page.type('#rx-signature-name', 'B');
      assert.equal((await posted(page, '[data-rx-signature-file]')).count, 0, 'typing drops the drawing');
      assert.equal(await page.eval(() => document.querySelector('[data-rx-signature-undo]').disabled), true);
      assert.equal(await page.eval(() => document.querySelector('#rx-signature-name').required), true, 'the name is asked for again');
      await stroke(page, LINE);
      await page.waitFor(() => document.querySelector('[data-rx-signature-file]').files.length === 1);
      assert.equal(await page.eval(() => document.querySelector('#rx-signature-name').value), '', 'drawing empties the name');
      page.assertClean();
    }));
});

describe('the camera capture', () => {
  test('the fake camera: shutter, thumbnails, Remove, the maximum, and the photos posted', () =>
    browser.with(async (page) => {
      await open(page);
      await page.waitFor(
        () => {
          const live = document.querySelector('[data-rx-camera-live]');
          return !live.hidden && document.querySelector('[data-rx-camera-video]').videoWidth > 0 && !document.querySelector('[data-rx-camera-shutter]').disabled;
        },
        { message: 'the preview running' },
      );
      assert.equal(await page.eval(() => document.querySelector('[data-rx-camera-fallback]').hidden), true, 'the file input gives way to the preview');
      await page.click('[data-rx-camera-shutter]');
      await page.waitFor(() => document.querySelectorAll('[data-rx-camera-shots] li').length === 1, { message: 'a thumbnail' });
      const first = await posted(page, '[data-rx-camera-file]');
      assert.equal(first.name, 'photo-1.jpg');
      assert.equal(first.type, 'image/jpeg');
      const size = await sizeOf(page, '[data-rx-camera-file]');
      assert.ok(size.width > 0 && size.width <= 2048, `at most max-width wide (${size.width})`);
      assert.equal(await page.eval(() => document.querySelector('.rx-camera__thumb').alt), 'Photo 1');
      for (const n of [2, 3]) {
        await page.waitFor(() => !document.querySelector('[data-rx-camera-shutter]').disabled, { message: 'the shutter ready' });
        await page.click('[data-rx-camera-shutter]');
        await page.waitFor((count) => document.querySelectorAll('[data-rx-camera-shots] li').length === count, { message: `${n} thumbnails` }, n);
      }
      assert.equal((await posted(page, '[data-rx-camera-file]')).count, 3);
      assert.equal(await page.eval(() => document.querySelector('[data-rx-camera-shutter]').disabled), true, 'the shutter stops at max');
      // Remove the first one: two are left, the shutter works again.
      await page.click('[data-rx-camera-shots] li:first-child button');
      assert.equal(await shots(page), 2);
      assert.equal((await posted(page, '[data-rx-camera-file]')).count, 2);
      assert.equal(await page.eval(() => document.querySelector('[data-rx-camera-shutter]').disabled), false);
      // Send them with a typed name: the server counts the photos.
      await page.type('#rx-signature-name', 'Ana R.');
      const text = await send(page, '2 photos');
      assert.match(text, /Received a typed name \(Ana R\.\) and 2 photos/);
      page.assertClean();
    }));

  test('without a camera permission the file input takes over and downsizes what it picks', () =>
    browser.with(async (page) => {
      await open(page, DENIED);
      await page.waitFor(() => !document.querySelector('[data-rx-camera-fallback]').hidden, { message: 'the file input shown' });
      const state = await page.eval(() => {
        const input = document.querySelector('[data-rx-camera-file]');
        return {
          live: document.querySelector('[data-rx-camera-live]').hidden,
          visible: input.getClientRects().length > 0,
          accept: input.accept,
          capture: input.getAttribute('capture'),
          status: document.querySelector('[data-rx-camera-status]').textContent,
        };
      });
      assert.equal(state.live, true, 'no preview');
      assert.equal(state.visible, true);
      assert.equal(state.accept, 'image/*');
      assert.equal(state.capture, 'environment');
      assert.match(state.status, /denied/i, 'the status says why');
      // A 3000 x 1000 photo comes out 2048 wide, as photo-1.jpg, with a thumbnail.
      await choose(page, '[data-rx-camera-file]', 3000, 1000);
      await page.waitFor(() => document.querySelectorAll('[data-rx-camera-shots] li').length === 1, { message: 'the picked photo shown' });
      assert.equal((await posted(page, '[data-rx-camera-file]')).name, 'photo-1.jpg');
      assert.deepEqual(await sizeOf(page, '[data-rx-camera-file]'), { width: 2048, height: 683 });
      await page.type('#rx-signature-name', 'Ana R.');
      const text = await send(page, '1 photos');
      assert.match(text, /and 1 photos/);
      page.assertClean();
    }));

  test('with no mediaDevices at all it is the same, with its own message', () =>
    browser.with(async (page) => {
      await open(page, MISSING);
      await page.waitFor(() => !document.querySelector('[data-rx-camera-fallback]').hidden, { message: 'the file input shown' });
      assert.match(await page.text('[data-rx-camera-status]'), /No camera is available/);
      assert.equal(await page.eval(() => document.querySelector('[data-rx-camera-live]').hidden), true);
      page.assertClean();
    }));
});

describe('both blocks in the page', () => {
  test('right-to-left: the tools swap sides and nothing scrolls sideways', () =>
    browser.with(async (page) => {
      await open(page);
      await page.eval(() => (document.documentElement.dir = 'rtl'));
      const sides = await page.eval(() => {
        const undo = document.querySelector('[data-rx-signature-undo]').getBoundingClientRect();
        const clear = document.querySelector('[data-rx-signature-clear]').getBoundingClientRect();
        return { undo: undo.left, clear: clear.left, fits: document.documentElement.scrollWidth <= window.innerWidth };
      });
      assert.ok(sides.undo > sides.clear, `Undo comes first, so it is on the right: ${JSON.stringify(sides)}`);
      assert.ok(sides.fits);
      // The pad still draws.
      await stroke(page, WAVE);
      await page.waitFor(() => document.querySelector('[data-rx-signature-file]').files.length === 1);
      page.assertClean();
    }));

  test('prefers-reduced-motion: no animation while drawing or taking a photo', () =>
    browser.with(async (page) => {
      await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
      await open(page);
      await page.waitFor(() => document.querySelector('[data-rx-camera-video]').videoWidth > 0 && !document.querySelector('[data-rx-camera-shutter]').disabled);
      await stroke(page, WAVE);
      await page.click('[data-rx-camera-shutter]');
      await page.waitFor(() => document.querySelectorAll('[data-rx-camera-shots] li').length === 1);
      await sleep(150);
      const running = await page.eval(() => [...document.querySelectorAll('[data-rx-signature], [data-rx-camera]')].flatMap((el) => el.getAnimations({ subtree: true })).filter((a) => a.playState === 'running').length);
      assert.equal(running, 0);
      page.assertClean();
    }));

  test('a page downloads only the parts of the blocks it has', () =>
    browser.with(async (page) => {
      const loaded = () =>
        page.eval(() =>
          performance
            .getEntriesByType('resource')
            .map((r) => new URL(r.name).pathname)
            .filter((p) => p.startsWith('/_renox/blocks/'))
            .map((p) => p.split('/').pop().replace(/-[0-9a-f]{16}\./, '.')),
        );
      // The shop's catalogue has neither block: neither part is fetched.
      await page.goto(`${app.url}/shop/bikes`);
      await page.waitFor(() => !!document.querySelector('[data-rx-range][data-rx-blocks-ready]'));
      await sleep(200);
      const without = await loaded();
      assert.ok(!without.includes('signature.js') && !without.includes('camera.js'), `loaded: ${without}`);
      // The demo form has both.
      await open(page);
      await sleep(200);
      const both = await loaded();
      assert.ok(both.includes('signature.js') && both.includes('camera.js'), `loaded: ${both}`);
      page.assertClean();
    }));
});
