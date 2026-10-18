// renox-blocks' field blocks (#54): rating, geolocate and barcode_scan, in a
// browser, under CSP=strict.
//
// On the fixture's /field page (rating, and two barcode fields, one with
// `submit`): the rating's arrow keys and its "N of MAX" names, the same form
// posted with every script blocked, and a right-to-left page; a scanner that
// types like a keyboard (fast keys ending in Enter) against slow typing; the
// camera with a stubbed BarcodeDetector; and the vendored zxing fallback, with
// window.BarcodeDetector deleted before the page loads. The camera is faked in
// the page, not with Chrome's fake device: getUserMedia answers with the stream
// of a canvas that shows an EAN-13 barcode drawn by this file, so the frame
// zxing decodes is known (the part, the <video> and the decoder are the real
// ones; only the camera is not). The zxing file must be requested only when
// the camera opens, and once.
//
// On the bike shop's /about/blocks form: the position with a granted and a
// denied permission (Browser.grantPermissions / setPermission and
// Emulation.setGeolocationOverride), a position worse than wanted, a
// stubbed timeout and an unavailable API, no request for the position before
// the button is pressed, and the whole form posted; and the files the page
// loads, with and without BarcodeDetector.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { Browser, sleep } from './lib/cdp.mjs';
import { fixture, start } from './lib/app.mjs';

const CODE = '4006381333931';
let browser;
let lab; // the fixture
let shop; // the bike shop

before(async () => {
  browser = await Browser.launch();
  lab = await fixture({ env: { CSP: 'strict' } });
  shop = await start('bikeshop', 'examples/bikeshop', { seed: true, env: { CSP: 'strict' } });
});

after(async () => {
  await browser?.send('Browser.resetPermissions').catch(() => {});
  await shop?.stop();
  await lab?.stop();
  await browser?.close();
});

/** The files of the blocks a page asked for, as names without their hash. */
const loaded = (page) =>
  page.eval(() =>
    performance
      .getEntriesByType('resource')
      .map((r) => new URL(r.name).pathname)
      .filter((p) => p.startsWith('/_renox/blocks/'))
      .map((p) => p.split('/').pop().replace(/-[0-9a-f]{16}\./, '.'))
      .sort(),
  );

/** What the form on /field posts, as `/echo` prints it: `score=4&sku=…`. */
const echoed = async (page) => {
  await page.waitFor(() => location.pathname === '/echo', { message: 'the form to be posted' });
  return page.text('body');
};

/** The fixture's /field page with its blocks set up, after `before` ran in it. */
async function field(page, before) {
  if (before) await page.send('Page.addScriptToEvaluateOnNewDocument', { source: before });
  await page.goto(`${lab.url}/field`);
  for (const selector of ['[data-rx-barcode][data-rx-blocks-ready]']) {
    await page.waitFor((s) => !!document.querySelector(s), { message: `${selector} is set up` }, selector);
  }
}

/** Counts the submits of the form and stops them from leaving the page. */
const watchSubmits = (page) =>
  page.eval(() => {
    window.__submits = 0;
    document.querySelector('#field-form').addEventListener('submit', (event) => {
      window.__submits += 1;
      event.preventDefault();
    });
  });

const submits = (page) => page.eval(() => window.__submits);

describe('rating', () => {
  test('the arrow keys change the choice, and each star is named "N of MAX"', () =>
    browser.with(async (page) => {
      await field(page);
      const names = await page.eval(() => [...document.querySelectorAll('#rate label')].map((l) => l.textContent.trim()));
      assert.deepEqual(names, ['1 of 5', '2 of 5', '3 of 5', '4 of 5', '5 of 5']);
      assert.equal(await page.eval(() => document.querySelectorAll('#rate input:checked').length), 0, 'nothing chosen yet');
      await page.click('#rate label[for=rate-2]');
      assert.equal(await page.eval(() => document.querySelector('#rate input:checked').value), '2');
      await page.press('ArrowRight');
      assert.equal(await page.eval(() => document.querySelector('#rate input:checked').value), '3');
      await page.press('ArrowRight');
      await page.press('ArrowRight');
      assert.equal(await page.eval(() => document.querySelector('#rate input:checked').value), '5');
      await page.press('ArrowLeft');
      assert.equal(await page.eval(() => document.querySelector('#rate input:checked').value), '4');
      // The stars up to the chosen one are filled, the others are outlines.
      const fills = await page.eval(() =>
        [...document.querySelectorAll('#rate .rx-rating__star')].map((s) => getComputedStyle(s).fill !== 'none'),
      );
      assert.deepEqual(fills, [true, true, true, true, false]);
      // The focus ring shows on the star.
      const ring = await page.eval(() => getComputedStyle(document.querySelector('#rate input:checked + label')).outlineStyle);
      assert.equal(ring, 'solid');
      page.assertClean();
    }));

  test('the chosen number is posted', () =>
    browser.with(async (page) => {
      await field(page);
      await page.click('#rate label[for=rate-4]');
      await page.click('#send');
      assert.match(await echoed(page), /(^|&)score=4(&|$)/);
      page.assertClean();
    }));

  test('with every script blocked the radios still select and submit', () =>
    browser.with(async (page) => {
      await page.send('Network.setBlockedURLs', { urls: ['*.js'] });
      await page.goto(`${lab.url}/field`);
      assert.equal(await page.eval(() => document.querySelectorAll('[data-rx-blocks-ready]').length), 0, 'no block was set up');
      assert.equal(await page.eval(() => document.querySelectorAll('#rate input[type=radio]').length), 5);
      await page.click('#rate label[for=rate-3]');
      assert.equal(await page.eval(() => document.querySelector('#rate input:checked').value), '3');
      // The field next to it is a plain text field.
      await page.type('#scan-sku-input', CODE);
      await page.click('#send');
      const posted = await echoed(page);
      assert.match(posted, /(^|&)score=3(&|$)/);
      assert.ok(posted.includes(`sku=${CODE}`), posted);
    }));

  test('in a right-to-left page the arrows swap direction and the stars keep their order', () =>
    browser.with(async (page) => {
      await field(page);
      await page.eval(() => (document.documentElement.dir = 'rtl'));
      await page.click('#rate label[for=rate-3]');
      await page.press('ArrowLeft');
      assert.equal(await page.eval(() => document.querySelector('#rate input:checked').value), '4', 'left is forwards');
      await page.press('ArrowRight');
      await page.press('ArrowRight');
      assert.equal(await page.eval(() => document.querySelector('#rate input:checked').value), '2');
      // The first star is on the right, and the group fits the page.
      const sides = await page.eval(() => {
        const left = (n) => document.querySelector(`#rate label[for=rate-${n}]`).getBoundingClientRect().left;
        return [left(1) > left(5), document.documentElement.scrollWidth <= window.innerWidth];
      });
      assert.deepEqual(sides, [true, true]);
      page.assertClean();
    }));
});

/** Types `text` and then `last` the way a scanner does: no pause between the keys. */
async function scan(page, text, last = 'Enter') {
  const key = (type, ch) => ({
    type,
    key: ch,
    text: type === 'keyDown' ? ch : undefined,
    code: /\d/.test(ch) ? `Digit${ch}` : `Key${ch.toUpperCase()}`,
    windowsVirtualKeyCode: ch.toUpperCase().charCodeAt(0),
  });
  for (const ch of text) {
    await page.send('Input.dispatchKeyEvent', key('keyDown', ch));
    await page.send('Input.dispatchKeyEvent', key('keyUp', ch));
  }
  await page.send('Input.dispatchKeyEvent', { type: 'keyDown', key: last, code: last, windowsVirtualKeyCode: 13, text: '\r' });
  await page.send('Input.dispatchKeyEvent', { type: 'keyUp', key: last, code: last, windowsVirtualKeyCode: 13 });
}

describe('barcode_scan: keys', () => {
  test('a scanner (fast keys ending in Enter) fills the field and does not submit', () =>
    browser.with(async (page) => {
      await field(page);
      await watchSubmits(page);
      await page.eval(() => {
        window.__events = [];
        const input = document.querySelector('#scan-shelf-input');
        for (const type of ['input', 'change']) input.addEventListener(type, () => window.__events.push(type));
      });
      await page.focus('#scan-shelf-input');
      await scan(page, CODE);
      assert.equal(await page.eval(() => document.querySelector('#scan-shelf-input').value), CODE);
      assert.equal(await submits(page), 0, 'a scan does not post a half-filled form');
      const events = await page.eval(() => window.__events);
      assert.ok(events.includes('input') && events.includes('change'), `events: ${events}`);
      assert.match(await page.text('#scan-shelf [data-rx-barcode-status]'), new RegExp(`Code read: ${CODE}`));
      page.assertClean();
    }));

  test('with `submit` a scan posts the form', () =>
    browser.with(async (page) => {
      await field(page);
      await page.focus('#scan-sku-input');
      await scan(page, CODE);
      assert.ok((await echoed(page)).includes(`sku=${CODE}`));
    }));

  test('slow typing then Enter keeps what was typed, and does not submit', () =>
    browser.with(async (page) => {
      await field(page);
      await watchSubmits(page);
      await page.focus('#scan-shelf-input');
      for (const ch of 'A12') {
        await page.press(ch);
        await sleep(90);
      }
      await page.press('Enter');
      assert.equal(await page.eval(() => document.querySelector('#scan-shelf-input').value), 'A12');
      assert.equal(await submits(page), 0);
      // The status did not claim a scan.
      assert.doesNotMatch(await page.text('#scan-shelf [data-rx-barcode-status]'), /Code read/);
      // With `submit`, Enter after typing posts like any form.
      await page.focus('#scan-sku-input');
      for (const ch of 'B7') {
        await page.press(ch);
        await sleep(90);
      }
      await page.press('Enter');
      assert.ok((await echoed(page)).includes('sku=B7'));
    }));
});

/** A BarcodeDetector that finds `CODE` in the first frame it is asked about. */
const DETECTOR = `
  window.BarcodeDetector = class {
    static async getSupportedFormats() { return ['ean_13', 'qr_code', 'code_128']; }
    constructor(options) { window.__detectorFormats = options && options.formats; }
    async detect() { return [{ rawValue: '${CODE}' }]; }
  };
  window.__tracks = 0;
  const mediaDevices = navigator.mediaDevices;
  if (mediaDevices) {
    mediaDevices.getUserMedia = async () => {
      window.__tracks = 0;
      const canvas = document.createElement('canvas');
      canvas.width = 160;
      canvas.height = 120;
      const stream = canvas.captureStream(10);
      const draw = () => { const c = canvas.getContext('2d'); c.fillStyle = '#fff'; c.fillRect(0, 0, 160, 120); };
      draw();
      setInterval(draw, 100);
      stream.getTracks().forEach((t) => { window.__tracks += 1; const stop = t.stop.bind(t); t.stop = () => { stop(); window.__tracks -= 1; }; });
      return stream;
    };
  }
`;

/** getUserMedia answers with a canvas showing the EAN-13 barcode of `CODE`; no BarcodeDetector. */
const EAN = `
  delete window.BarcodeDetector;
  const L = ['0001101', '0011001', '0010011', '0111101', '0100011', '0110001', '0101111', '0111011', '0110111', '0001011'];
  const G = ['0100111', '0110011', '0011011', '0100001', '0011101', '0111001', '0000101', '0010001', '0001001', '0010111'];
  const R = ['1110010', '1100110', '1101100', '1000010', '1011100', '1001110', '1010000', '1000100', '1001000', '1110100'];
  const PARITY = ['LLLLLL', 'LLGLGG', 'LLGGLG', 'LLGGGL', 'LGLLGG', 'LGGLLG', 'LGGGLL', 'LGLGLG', 'LGLGGL', 'LGGLGL'];
  function ean13(digits) {
    const parity = PARITY[+digits[0]];
    let bits = '101';
    for (let i = 1; i <= 6; i++) bits += (parity[i - 1] === 'L' ? L : G)[+digits[i]];
    bits += '01010';
    for (let i = 7; i <= 12; i++) bits += R[+digits[i]];
    return bits + '101';
  }
  const bits = ean13('${CODE}');
  window.__tracks = 0;
  navigator.mediaDevices.getUserMedia = async () => {
    const canvas = document.createElement('canvas');
    canvas.width = 640;
    canvas.height = 480;
    const draw = () => {
      const c = canvas.getContext('2d');
      c.fillStyle = '#fff';
      c.fillRect(0, 0, 640, 480);
      c.fillStyle = '#000';
      const module = 5;
      const left = Math.round((640 - bits.length * module) / 2);
      for (let i = 0; i < bits.length; i++) if (bits[i] === '1') c.fillRect(left + i * module, 140, module, 200);
    };
    draw();
    setInterval(draw, 100);
    const stream = canvas.captureStream(10);
    stream.getTracks().forEach((t) => {
      window.__tracks += 1;
      const stop = t.stop.bind(t);
      t.stop = () => { stop(); window.__tracks -= 1; };
    });
    return stream;
  };
`;

/** Opens the camera of a barcode block and waits for the field to be filled. */
async function scanned(page, id) {
  await page.click(`#${id} [data-rx-barcode-button]`);
  await page.waitFor((s) => document.querySelector(s).value !== '', { message: 'the camera to fill the field', timeout: 20000 }, `#${id} [data-rx-barcode-input]`);
}

describe('barcode_scan: the camera', () => {
  test('a BarcodeDetector result fills the field, closes the camera and never loads zxing', () =>
    browser.with(async (page) => {
      await field(page, DETECTOR);
      assert.equal(await page.eval(() => document.querySelector('#scan-shelf [data-rx-barcode-button]').hidden), false, 'the camera button is shown');
      assert.equal(await page.eval(() => document.querySelector('#scan-shelf [data-rx-barcode-live]').hidden), true);
      await watchSubmits(page);
      await page.click('#scan-shelf [data-rx-barcode-button]');
      await page.waitFor(() => document.querySelector('#scan-shelf [data-rx-barcode-input]').value !== '', { message: 'the camera to fill the field' });
      assert.equal(await page.eval(() => document.querySelector('#scan-shelf-input').value), CODE);
      const closed = await page.eval(() => ({
        live: document.querySelector('#scan-shelf [data-rx-barcode-live]').hidden,
        stream: document.querySelector('#scan-shelf [data-rx-barcode-video]').srcObject,
        tracks: window.__tracks,
        expanded: document.querySelector('#scan-shelf [data-rx-barcode-button]').getAttribute('aria-expanded'),
        focused: document.activeElement.id,
        formats: window.__detectorFormats,
      }));
      assert.deepEqual(closed, { live: true, stream: null, tracks: 0, expanded: 'false', focused: 'scan-shelf-input', formats: ['ean_13', 'qr_code'] });
      assert.equal(await submits(page), 0, 'the camera never submits a form without `submit`');
      assert.ok(!(await loaded(page)).includes('zxing.js'), 'zxing is not requested');
      page.assertClean();
    }));

  test('Escape closes the camera, and the field stays empty', () =>
    browser.with(async (page) => {
      // A detector that never finds anything.
      await field(page, DETECTOR.replace("return [{ rawValue: '" + CODE + "' }]", 'return []'));
      await page.click('#scan-shelf [data-rx-barcode-button]');
      await page.waitFor(() => !document.querySelector('#scan-shelf [data-rx-barcode-live]').hidden, { message: 'the camera panel' });
      await page.press('Escape');
      const state = await page.eval(() => ({
        live: document.querySelector('#scan-shelf [data-rx-barcode-live]').hidden,
        tracks: window.__tracks,
        value: document.querySelector('#scan-shelf-input').value,
      }));
      assert.deepEqual(state, { live: true, tracks: 0, value: '' });
      page.assertClean();
    }));

  test('a denied camera is said, and the field still works', () =>
    browser.with(async (page) => {
      await field(page, `navigator.mediaDevices.getUserMedia = () => Promise.reject(new DOMException('denied', 'NotAllowedError'));`);
      await page.click('#scan-shelf [data-rx-barcode-button]');
      await page.waitFor(() => /denied|not allowed|permission/i.test(document.querySelector('#scan-shelf [data-rx-barcode-status]').textContent), { message: 'the denied message' });
      await page.type('#scan-shelf-input', 'Z9');
      assert.equal(await page.eval(() => document.querySelector('#scan-shelf-input').value), 'Z9');
      page.assertClean();
    }));
});

describe('barcode_scan: the zxing fallback', () => {
  test('without BarcodeDetector the camera loads zxing once and reads the barcode', () =>
    browser.with(async (page) => {
      await field(page, EAN);
      await sleep(200);
      assert.ok(!(await loaded(page)).includes('zxing.js'), 'not requested before the camera opens');
      await scanned(page, 'scan-shelf');
      assert.equal(await page.eval(() => document.querySelector('#scan-shelf-input').value), CODE);
      assert.equal(await page.eval(() => window.__tracks), 0, 'the camera stopped');
      // A second time: the file is not requested again.
      await page.eval(() => (document.querySelector('#scan-shelf-input').value = ''));
      await scanned(page, 'scan-shelf');
      const files = await loaded(page);
      assert.equal(files.filter((f) => f === 'zxing.js').length, 1, `zxing requested once: ${files}`);
      page.assertClean();
    }));

  test('the page with the blocks loads their parts, not the others', () =>
    browser.with(async (page) => {
      await field(page, EAN);
      await sleep(200);
      assert.deepEqual(await loaded(page), ['barcode.js', 'blocks.css', 'blocks.js']);
    }));
});

describe('geolocate on the bike shop', () => {
  const PAGE = `${'/about/blocks'}#field`;
  const GEO = '[data-rx-geolocate][data-rx-blocks-ready]';

  /** The page, set up, with `before` run first; counts the calls for a position. */
  async function open(page, before = '') {
    await page.send('Page.addScriptToEvaluateOnNewDocument', {
      source: `${before}
        { const real = navigator.geolocation && navigator.geolocation.getCurrentPosition.bind(navigator.geolocation);
          window.__asked = 0;
          if (real) navigator.geolocation.getCurrentPosition = function (...args) { window.__asked += 1; return real(...args); }; }`,
    });
    await page.goto(`${shop.url}${PAGE}`);
    await page.waitFor((s) => !!document.querySelector(s), { message: 'the block is set up' }, GEO);
  }

  const status = (page) => page.text('[data-rx-geolocate-status]');
  const texts = (page) => page.eval(() => JSON.parse(document.querySelector('[data-rx-geolocate]').getAttribute('data-rx-blocks-texts')));
  const fields = (page) =>
    page.eval(() =>
      [...document.querySelectorAll('[data-rx-geolocate] input[type=hidden]')].map((i) => ({ name: i.name, value: i.value, disabled: i.disabled })),
    );
  const press = async (page) => {
    await page.click('[data-rx-geolocate-button]');
  };

  test('nothing is asked for before the press, and the four inputs are disabled', () =>
    browser.with(async (page) => {
      await browser.send('Browser.resetPermissions');
      await open(page);
      await sleep(300);
      assert.equal(await page.eval(() => window.__asked), 0, 'no request for the position on load');
      assert.equal((await page.eval(async () => (await navigator.permissions.query({ name: 'geolocation' })).state)), 'prompt');
      const inputs = await fields(page);
      assert.deepEqual(inputs.map((i) => i.name), ['location[lat]', 'location[lng]', 'location[accuracy]', 'location[at]']);
      assert.ok(inputs.every((i) => i.disabled && i.value === ''), JSON.stringify(inputs));
      page.assertClean();
    }));

  test('granted: one press fills the four inputs with the position', () =>
    browser.with(async (page) => {
      await browser.send('Browser.grantPermissions', { permissions: ['geolocation'], origin: shop.url });
      await page.send('Emulation.setGeolocationOverride', { latitude: 40.4168, longitude: -3.7038, accuracy: 25 });
      await open(page);
      await press(page);
      await page.waitFor(() => !document.querySelector('[data-rx-geolocate] [name="location[lat]"]').disabled, { message: 'the fix' });
      const values = Object.fromEntries((await fields(page)).map((i) => [i.name, i]));
      assert.equal(Number(values['location[lat]'].value), 40.4168);
      assert.equal(Number(values['location[lng]'].value), -3.7038);
      assert.equal(Number(values['location[accuracy]'].value), 25);
      assert.match(values['location[at]'].value, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z$/);
      assert.ok(Object.values(values).every((i) => !i.disabled));
      assert.match(await status(page), /Position found: 40\.4168, -3\.7038 \(about 25 m\)/);
      assert.equal(await page.text('[data-rx-geolocate-button]'), (await texts(page)).retry);
      assert.equal(await page.eval(() => window.__asked), 1);
      // The form posts it: the demo says what it got.
      await page.click('#field-form button[type=submit]');
      await page.waitFor(() => /a position \(40\.4168, -3\.7038, within 25 m\)/.test(document.body.textContent), { message: 'the toast with the position' });
      page.assertClean();
    }));

  test('a position worse than wanted is kept and said', () =>
    browser.with(async (page) => {
      await browser.send('Browser.grantPermissions', { permissions: ['geolocation'], origin: shop.url });
      await page.send('Emulation.setGeolocationOverride', { latitude: 41, longitude: 2, accuracy: 120 });
      await open(page);
      await press(page);
      await page.waitFor(() => !document.querySelector('[name="location[lat]"]').disabled, { message: 'the fix' });
      assert.match(await status(page), /about 120 m, wanted 50 m/);
      assert.equal(await page.eval(() => document.querySelector('[name="location[accuracy]"]').value), '120');
      page.assertClean();
    }));

  test('denied: the message and a retry button, and nothing is posted', () =>
    browser.with(async (page) => {
      await browser.send('Browser.setPermission', { permission: { name: 'geolocation' }, setting: 'denied', origin: shop.url });
      await open(page);
      await press(page);
      await page.waitFor(() => /denied/i.test(document.querySelector('[data-rx-geolocate-status]').textContent), { message: 'the denied message' });
      const said = await texts(page);
      assert.equal(await status(page), said.denied);
      assert.equal(await page.text('[data-rx-geolocate-button]'), said.retry);
      assert.equal(await page.eval(() => document.querySelector('[data-rx-geolocate-button]').disabled), false, 'Retry can be pressed');
      assert.ok((await fields(page)).every((i) => i.disabled && i.value === ''));
      await page.click('#field-form button[type=submit]');
      await page.waitFor(() => /Nothing was filled in/.test(document.body.textContent), { message: 'a toast that nothing arrived' });
      page.assertClean();
    }));

  test('a stubbed timeout and an unavailable API are said', () =>
    browser.with(async (page) => {
      await open(page, `navigator.geolocation.getCurrentPosition = (ok, fail) => setTimeout(() => fail({ code: 3 }), 0);`);
      await press(page);
      const said = await texts(page);
      await page.waitFor((t) => document.querySelector('[data-rx-geolocate-status]').textContent === t, { message: 'the timeout message' }, said.timeout);
      assert.ok((await fields(page)).every((i) => i.disabled));
      assert.equal(await page.text('[data-rx-geolocate-button]'), said.retry);
      page.assertClean();
    }));

  test('without the geolocation API the position is unavailable', () =>
    browser.with(async (page) => {
      await open(page, `Object.defineProperty(navigator, 'geolocation', { value: undefined, configurable: true });`);
      await press(page);
      const said = await texts(page);
      await page.waitFor((t) => document.querySelector('[data-rx-geolocate-status]').textContent === t, { message: 'the unavailable message' }, said.unavailable);
      page.assertClean();
    }));

  test('a position that arrives after a failed one replaces it', () =>
    browser.with(async (page) => {
      await open(page, `{ let calls = 0; const real = navigator.geolocation.getCurrentPosition.bind(navigator.geolocation);
        navigator.geolocation.getCurrentPosition = (ok, fail, options) => (calls++ === 0 ? setTimeout(() => fail({ code: 2 }), 0) : ok({ coords: { latitude: 1.5, longitude: 2.5, accuracy: 10 }, timestamp: Date.now() })); }`);
      await press(page);
      const said = await texts(page);
      await page.waitFor((t) => document.querySelector('[data-rx-geolocate-status]').textContent === t, { message: 'unavailable' }, said.unavailable);
      await press(page);
      await page.waitFor(() => !document.querySelector('[name="location[lat]"]').disabled, { message: 'the second try' });
      assert.equal(await page.eval(() => document.querySelector('[name="location[lat]"]').value), '1.5');
    }));
});

describe('the bike shop /about/blocks field form', () => {
  test('the rating, a scanned code and the position are posted together', () =>
    browser.with(async (page) => {
      await browser.send('Browser.grantPermissions', { permissions: ['geolocation'], origin: shop.url });
      await page.send('Emulation.setGeolocationOverride', { latitude: 40.4168, longitude: -3.7038, accuracy: 30 });
      await page.goto(`${shop.url}/about/blocks`);
      await page.waitFor(() => !!document.querySelector('[data-rx-barcode][data-rx-blocks-ready]'), { message: 'the barcode field' });
      await page.click('#field-form .rx-rating label[for=rx-score-4]');
      await page.focus('#rx-code-input');
      await scan(page, CODE);
      await page.click('[data-rx-geolocate-button]');
      await page.waitFor(() => !document.querySelector('[name="location[lat]"]').disabled, { message: 'the fix' });
      await page.click('#field-form button[type=submit]');
      await page.waitFor(() => /Received a rating of 4, the code 4006381333931, a position \(40\.4168, -3\.7038, within 30 m\)/.test(document.body.textContent), {
        message: 'the toast with all three',
      });
      page.assertClean();
    }));

  test('the blocks load only their own files: barcode.js, geolocate.js and, with a BarcodeDetector, never zxing', () =>
    browser.with(async (page) => {
      await page.send('Page.addScriptToEvaluateOnNewDocument', { source: DETECTOR });
      await page.goto(`${shop.url}/about/blocks`);
      await page.waitFor(() => !!document.querySelector('[data-rx-barcode][data-rx-blocks-ready]'), { message: 'the barcode field' });
      await page.click('[data-rx-barcode-button]');
      await page.waitFor(() => document.querySelector('[data-rx-barcode-input]').value !== '', { message: 'the camera to fill the field' });
      const files = await loaded(page);
      assert.ok(files.includes('barcode.js') && files.includes('geolocate.js'), `${files}`);
      assert.ok(!files.includes('zxing.js'), `${files}`);
      assert.ok(!files.includes('rating.js'), 'the rating has no code of its own');
      page.assertClean();
    }));
});
