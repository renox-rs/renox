// #47: the map blocks (renox-blocks: map_picker, map) in a browser, on the
// fixture's /maps page, under CSP=strict, with the tiles from a local stub
// (BLOCKS_MAP_TILES points at it; the fixture allows its origin in img-src,
// as an app with an http tile host must). No real tile server is contacted.
//
// Checked: tiles and the attribution are shown; a click places a point; a
// polygon drawn by clicks and closed with Enter posts GeoJSON in [lng, lat]
// order; the typed coordinates alone (no pointer) make the same value; a
// circle from typed text; the form posts what the blocks hold; <rx-map> shows
// its markers and shapes; a page without a map block requests no Leaflet or
// map.js file; and without BLOCKS_MAP_TILES the blocks explain what is
// missing while the typed coordinates still work. No console errors or CSP
// violations in any of them.

import { after, before, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { Browser, sleep } from './lib/cdp.mjs';
import { fixture } from './lib/app.mjs';

// A 1x1 transparent PNG: every tile is this one.
const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
  'base64',
);

let browser;
let app;
let bare;
let tiles;
let tileServer;
const hits = [];

before(async () => {
  tileServer = createServer((req, res) => {
    if (/^\/\d+\/\d+\/\d+\.png$/.test(req.url)) {
      hits.push(req.url);
      res.writeHead(200, { 'content-type': 'image/png', 'content-length': PNG.length });
      res.end(PNG);
    } else {
      res.writeHead(404).end();
    }
  });
  await new Promise((resolve) => tileServer.listen(0, '127.0.0.1', resolve));
  tiles = `http://127.0.0.1:${tileServer.address().port}/{z}/{x}/{y}.png`;
  browser = await Browser.launch();
  app = await fixture({
    env: { CSP: 'strict', BLOCKS_MAP_TILES: tiles, BLOCKS_MAP_ATTRIBUTION: 'Tiles from the stub' },
  });
});

after(async () => {
  await browser?.close();
  await app?.stop();
  await bare?.stop();
  tileServer?.close();
});

/** Waits until Leaflet has built the map in `#id`. */
const ready = (page, id) =>
  page.waitFor((i) => !!document.querySelector(`#${i} .leaflet-container`), { message: `the map in #${id}` }, id);

/** The value of the field `name` in the page's form. */
const field = (page, name) =>
  page.eval((n) => new FormData(document.querySelector('#maps-form')).get(n), name);

/** Leaflet and map.js files the page has requested. */
const mapFiles = (page) =>
  page.eval(() =>
    performance
      .getEntriesByType('resource')
      .map((e) => new URL(e.name).pathname)
      .filter((p) => /\/(leaflet|map)-[0-9a-f]+\.(js|css)$/.test(p)),
  );

/** Types `lines` into a textarea, one per line, with Enter between them. */
async function typeLines(page, selector, lines) {
  await page.focus(selector);
  for (const [i, line] of lines.entries()) {
    if (i > 0) await page.press('Enter');
    await page.type(selector, line);
  }
}

describe('with tiles', () => {
  test('the tiles load from the stub and the attribution is shown', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/maps`);
      await ready(page, 'pick-point');
      await page.waitFor(
        () => [...document.querySelectorAll('#pick-point img.leaflet-tile')].some((i) => i.complete && i.naturalWidth > 0),
        { message: 'a tile to load' },
      );
      assert.ok(hits.length > 0, 'the stub served tiles');
      const attribution = await page.text('#pick-point .leaflet-control-attribution');
      assert.match(attribution, /Tiles from the stub/);
      page.assertClean();
    }));

  test('a click places the point, and the fields hold it', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/maps`);
      await ready(page, 'pick-point');
      const { x, y } = await page.point('#pick-point [data-rx-map-canvas]');
      await page.clickAt(x, y);
      await page.waitFor(() => !!document.querySelector('#pick-point .rx-map__pin'), { message: 'the pin' });
      const lat = Number(await field(page, 'spot[lat]'));
      const lng = Number(await field(page, 'spot[lng]'));
      // The view starts on (20, 0) and the click is in its middle.
      assert.ok(Math.abs(lat - 20) < 2, `lat ${lat}`);
      assert.ok(Math.abs(lng) < 2, `lng ${lng}`);
      // Backspace on the map takes it away again.
      await page.focus('#pick-point [data-rx-map-canvas]');
      await page.press('Backspace');
      await page.waitFor(() => !document.querySelector('#pick-point .rx-map__pin'), { message: 'the pin to go' });
      assert.equal(await field(page, 'spot[lat]'), '');
      page.assertClean();
    }));

  test('a polygon drawn by clicks and closed with Enter posts GeoJSON as [lng, lat]', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/maps`);
      await ready(page, 'pick-polygon');
      const { x, y } = await page.point('#pick-polygon [data-rx-map-canvas]');
      // North-west, north-east, south: the first vertex is west and north of the middle.
      for (const [dx, dy] of [[-80, -60], [80, -60], [0, 60]]) await page.clickAt(x + dx, y + dy);
      assert.equal(await field(page, 'zone'), '', 'not posted until it is closed');
      await page.focus('#pick-polygon [data-rx-map-canvas]');
      await page.press('Enter');
      await page.waitFor(() => !!document.querySelector('#maps-form [name=zone]').value, { message: 'the closed polygon' });
      const geo = JSON.parse(await field(page, 'zone'));
      assert.equal(geo.type, 'Polygon');
      const ring = geo.coordinates[0];
      assert.equal(ring.length, 4, 'three vertices and the closing one');
      assert.deepEqual(ring[0], ring[3]);
      // [lng, lat]: west of the middle (about -28) and north of it (about 39).
      assert.ok(ring[0][0] < 0 && ring[0][0] > -45, `lng first: ${ring[0]}`);
      assert.ok(ring[0][1] > 20 && ring[0][1] < 60, `then lat: ${ring[0]}`);
      assert.ok(ring[1][0] > ring[0][0], 'the second vertex is further east');
      // The typed alternative shows the same vertices as `lat, lng` lines.
      const typed = await page.eval(() => document.querySelector('#pick-polygon textarea').value);
      assert.equal(typed.split('\n').length, 3);
      const [lat0, lng0] = typed.split('\n')[0].split(',').map(Number);
      assert.deepEqual([lng0, lat0], ring[0]);
      page.assertClean();
    }));

  test('the typed coordinates alone make the same value as the drawing', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/maps`);
      await ready(page, 'pick-polygon');
      await ready(page, 'pick-typed');
      const { x, y } = await page.point('#pick-polygon [data-rx-map-canvas]');
      for (const [dx, dy] of [[-80, -60], [80, -60], [0, 60]]) await page.clickAt(x + dx, y + dy);
      await page.focus('#pick-polygon [data-rx-map-canvas]');
      await page.press('Enter');
      await page.waitFor(() => !!document.querySelector('#maps-form [name=zone]').value, { message: 'the drawn polygon' });
      const drawn = await field(page, 'zone');
      const lines = (await page.eval(() => document.querySelector('#pick-polygon textarea').value)).split('\n');

      // No pointer on the other picker: only the keyboard.
      await typeLines(page, '#pick-typed textarea', lines);
      await page.waitFor(() => !!document.querySelector('#maps-form [name=typed]').value, { message: 'the typed polygon' });
      assert.equal(await field(page, 'typed'), drawn);

      // A line that is not a coordinate pair posts nothing.
      await page.type('#pick-typed textarea', ' x');
      assert.equal(await field(page, 'typed'), '');
      page.assertClean();
    }));

  test('a point can be typed, and a circle too', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/maps`);
      await ready(page, 'pick-point');
      await ready(page, 'pick-circle');
      await page.type('#pick-point input[name="spot[lat]"]', '25.2048', { clear: true });
      await page.type('#pick-point input[name="spot[lng]"]', '55.2708', { clear: true });
      await page.waitFor(() => !!document.querySelector('#pick-point .rx-map__pin'), { message: 'the pin for typed coordinates' });
      assert.equal(await field(page, 'spot[lat]'), '25.2048');
      assert.equal(await field(page, 'spot[lng]'), '55.2708');

      await page.type('#pick-circle textarea', '40.4, -3.7, 500', { clear: true });
      await page.waitFor(() => !!document.querySelector('#maps-form [name=area]').value, { message: 'the circle' });
      assert.deepEqual(JSON.parse(await field(page, 'area')), {
        type: 'Point',
        coordinates: [-3.7, 40.4],
        radius: 500,
      });
      page.assertClean();
    }));

  test('the form posts what the blocks hold', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/maps`);
      await ready(page, 'pick-point');
      await ready(page, 'pick-circle');
      await page.type('#pick-point input[name="spot[lat]"]', '25.2048', { clear: true });
      await page.type('#pick-point input[name="spot[lng]"]', '55.2708', { clear: true });
      await page.type('#pick-circle textarea', '40.4, -3.7, 500', { clear: true });
      await page.waitFor(() => !!document.querySelector('#maps-form [name=area]').value, { message: 'the circle' });
      await page.click('#send');
      await page.waitFor(() => location.pathname === '/echo', { message: 'the echo page' });
      const body = await page.eval(() => document.body.innerText);
      assert.match(body, /spot\[lat\]=25\.2048/);
      assert.match(body, /spot\[lng\]=55\.2708/);
      assert.match(body, /area=\{"type":"Point","coordinates":\[-3\.7,40\.4\],"radius":500\}/);
      page.assertClean();
    }));

  test('<rx-map> shows its markers and shapes', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/maps`);
      await ready(page, 'places');
      await page.waitFor(() => document.querySelectorAll('#places .rx-map__pin').length === 2, { message: 'two markers' });
      await page.waitFor(() => document.querySelectorAll('#places .rx-map__shape').length === 2, { message: 'a polygon and a circle' });
      const labels = await page.eval(() => [...document.querySelectorAll('#places .rx-map__pin')].map((m) => m.getAttribute('title')));
      assert.deepEqual(labels.sort(), ['Depot', 'Workshop']);
      // The markers are also a list for screen readers.
      const list = await page.eval(() => document.querySelector('#places .rx-map__list').textContent);
      assert.match(list, /Workshop \(40\.4168, -3\.7038\)/);
      assert.match(await page.text('#places .leaflet-control-attribution'), /Tiles from the stub/);
      page.assertClean();
    }));

  test('a page without a map block requests no Leaflet or map.js file', () =>
    browser.with(async (page) => {
      await page.goto(`${app.url}/maps/plain`);
      await page.waitFor(() => !!document.querySelector('[data-rx-quantity][data-rx-blocks-ready]'), {
        message: 'the quantity block to be set up',
      });
      const before = hits.length;
      assert.deepEqual(await mapFiles(page), []);
      assert.equal(await page.eval(() => typeof window.L), 'undefined');
      assert.equal(hits.length, before, 'no tile was asked for');
      page.assertClean();

      // The same loader on the map page does fetch them.
      await page.goto(`${app.url}/maps`);
      await ready(page, 'pick-point');
      const files = await mapFiles(page);
      assert.ok(files.some((f) => /\/leaflet-.*\.js$/.test(f)), files.join());
      assert.ok(files.some((f) => /\/leaflet-.*\.css$/.test(f)), files.join());
      assert.ok(files.some((f) => /\/map-.*\.js$/.test(f)), files.join());
    }));
});

describe('without tiles', () => {
  test('the blocks explain what is missing, and the typed coordinates still work', () =>
    browser.with(async (page) => {
      bare = await fixture({ env: { CSP: 'strict' } });
      await page.goto(`${bare.url}/maps`);
      await page.waitFor(() => !!document.querySelector('#pick-typed[data-rx-blocks-ready]'), {
        message: 'the picker to be set up',
      });
      assert.match(await page.text('#pick-point .rx-map__empty'), /no map tiles are configured\. Set BLOCKS_MAP_TILES/);
      assert.match(await page.text('#places .rx-map__empty'), /BLOCKS_MAP_TILES/);
      assert.equal(await page.eval(() => document.querySelectorAll('.leaflet-container').length), 0);
      assert.deepEqual((await mapFiles(page)).filter((f) => /leaflet/.test(f)), [], 'Leaflet is never fetched');

      await typeLines(page, '#pick-typed textarea', ['40.4, -3.7', '40.4, -3.6', '40.5, -3.6']);
      await page.waitFor(() => !!document.querySelector('#maps-form [name=typed]').value, { message: 'the typed polygon' });
      const geo = JSON.parse(await field(page, 'typed'));
      assert.deepEqual(geo.coordinates[0][0], [-3.7, 40.4]);
      assert.deepEqual(geo.coordinates[0][2], [-3.6, 40.5]);
      await sleep(50);
      page.assertClean();
    }));
});
