// renox-blocks: map_picker and map. Leaflet 1.9.4 (vendored, BSD-2) draws the
// map; the drawing is this file's own. Leaflet is fetched only when a block
// with tiles is on the page: its <script> and <link> are added from the
// paths the loader's tag carries (data-leaflet-js, data-leaflet-css), both
// served from 'self', so CSP=strict needs nothing. Leaflet sets its styles
// through the CSSOM and the CSP allows that.
//
// The picker's way without a pointer is the typed coordinates: the number
// fields of a point, or a `lat, lng` per line (polygon) / `lat, lng, radius`
// (circle) text area. They stay in sync with the shape on the map and with
// the hidden input that posts it, and work with no tiles at all (then
// Leaflet is never loaded). Polygons and circles post GeoJSON in [lng, lat]
// order (a circle as a Point with `radius` in metres); a point posts the
// number fields `name[lat]` and `name[lng]`.
//
// Pointer: a click places the point; a circle takes a click for the centre
// and a click for the radius; a polygon takes a click per vertex and closes
// with a double click. Keyboard on the map: Enter closes a polygon,
// Backspace removes the last vertex (or the circle, or the point), Escape
// discards the drawing; arrows and +/- are Leaflet's.

const MAX_ZOOM = 19;

let loading = null;

/** Leaflet, added to the page once. */
function leaflet() {
  if (window.L && window.L.map) return Promise.resolve(window.L);
  if (!loading) {
    const tag = document.querySelector("script[data-renox-blocks]");
    const js = tag && tag.getAttribute("data-leaflet-js");
    const css = tag && tag.getAttribute("data-leaflet-css");
    loading = new Promise((resolve, reject) => {
      if (!js) return reject(new Error("renox-blocks: no file for Leaflet"));
      if (css && !document.querySelector(`link[href="${css}"]`)) {
        const link = document.createElement("link");
        link.rel = "stylesheet";
        link.href = css;
        document.head.append(link);
      }
      const script = document.createElement("script");
      script.src = js;
      script.onload = () => resolve(window.L);
      script.onerror = () => {
        loading = null;
        reject(new Error("renox-blocks: Leaflet did not load"));
      };
      document.head.append(script);
    });
  }
  return loading;
}

const round = (v) => Math.round(v * 1e6) / 1e6;
const lat = (v) => Number.isFinite(v) && v >= -90 && v <= 90;
const lng = (v) => Number.isFinite(v) && v >= -180 && v <= 180;
const esc = (s) =>
  String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const hook = (el, name) => el.querySelector(`[data-rx-map-${name}]`);

function json(el, name) {
  try {
    const value = JSON.parse(el.getAttribute(`data-rx-map-${name}`) || "[]");
    return Array.isArray(value) ? value : [];
  } catch (_) {
    return [];
  }
}

/** The numbers in `text`, split on commas and spaces. */
function numbers(text) {
  const parts = String(text).split(/[,\s]+/).filter(Boolean);
  return parts.map(Number);
}

/** `[[lat, lng], …]` from one `lat, lng` per line; null when a line is wrong. */
function parseVertices(text) {
  const pts = [];
  for (const line of String(text).split(/\n|;/)) {
    if (!line.trim()) continue;
    const n = numbers(line);
    if (n.length !== 2 || !lat(n[0]) || !lng(n[1])) return null;
    pts.push([n[0], n[1]]);
  }
  return pts;
}

/** `{lat, lng, r}` from `lat, lng, radius`; null when wrong, undefined when empty. */
function parseCircle(text) {
  if (!String(text).trim()) return undefined;
  const n = numbers(text);
  if (n.length !== 3 || !lat(n[0]) || !lng(n[1]) || !(n[2] > 0) || !Number.isFinite(n[2])) return null;
  return { lat: n[0], lng: n[1], r: n[2] };
}

/** A Leaflet layer for a Circle / Polygon value or its GeoJSON; null when it is neither. */
function shapeLayer(L, shape) {
  const style = { className: "rx-map__shape" };
  try {
    if (!shape || typeof shape !== "object") return null;
    if (shape.type === "Feature") return shapeLayer(L, shape.geometry);
    if (shape.type === "Polygon" && Array.isArray(shape.coordinates)) {
      return L.polygon(
        shape.coordinates.map((ring) => ring.map((p) => [p[1], p[0]])),
        style,
      );
    }
    const c = shape.type === "Point" ? { lat: shape.coordinates[1], lng: shape.coordinates[0] } : shape.center;
    if (c && lat(Number(c.lat)) && lng(Number(c.lng)) && Number(shape.radius) > 0) {
      return L.circle([Number(c.lat), Number(c.lng)], Object.assign({ radius: Number(shape.radius) }, style));
    }
  } catch (_) {
    // A malformed shape is skipped; the page still works.
  }
  return null;
}

/** Leaflet's map in `canvas`, with the block's tiles and attribution. */
function basemap(L, el, canvas) {
  const height = parseFloat(el.getAttribute("data-rx-map-height")) || 320;
  canvas.style.setProperty("block-size", `${height}px`);
  canvas.style.setProperty("min-block-size", `${Math.min(height, 320)}px`);
  const map = L.map(canvas, { worldCopyJump: true, maxZoom: MAX_ZOOM }).setView([20, 0], 2);
  L.tileLayer(el.getAttribute("data-rx-map-tiles"), {
    attribution: esc(el.getAttribute("data-rx-map-attribution") || ""),
    maxZoom: MAX_ZOOM,
  }).addTo(map);
  if (typeof ResizeObserver === "function") new ResizeObserver(() => map.invalidateSize()).observe(canvas);
  return map;
}

/** A marker drawn with CSS (the theme's accent), so no image file is needed. */
function pin(L, at, options) {
  const icon = L.divIcon({ className: "rx-map__pin", iconSize: [22, 22], iconAnchor: [11, 22] });
  return L.marker(at, Object.assign({ icon, keyboard: true }, options || {}));
}

/** <rx-map>: markers and shapes, read only. */
function view(el) {
  const canvas = hook(el, "canvas");
  if (!canvas) return;
  leaflet().then((L) => {
    const map = basemap(L, el, canvas);
    const group = L.featureGroup().addTo(map);
    for (const m of json(el, "markers")) {
      if (!m || !lat(Number(m.lat)) || !lng(Number(m.lng))) continue;
      const label = m.label ? String(m.label) : "";
      const marker = pin(L, [Number(m.lat), Number(m.lng)], { title: label, alt: label }).addTo(group);
      if (label) {
        const text = document.createElement("span");
        text.textContent = label;
        marker.bindPopup(text);
      }
    }
    for (const s of json(el, "shapes")) {
      const layer = shapeLayer(L, s);
      if (layer) layer.addTo(group);
    }
    const bounds = group.getBounds();
    if (bounds.isValid()) map.fitBounds(bounds, { padding: [24, 24], maxZoom: 15 });
  }, (error) => console.error(error));
}

/** <rx-map-picker>. */
function picker(el, kit) {
  const mode = el.getAttribute("data-rx-map-mode") || "point";
  const canvas = hook(el, "canvas");
  const hidden = hook(el, "value");
  const typed = hook(el, "typed");
  const latField = hook(el, "lat");
  const lngField = hook(el, "lng");
  const state = { point: null, circle: null, center: null, pts: [], closed: false };
  let syncing = false;
  let draw = () => {};

  /** Reads the typed text into the state; false when it does not parse. */
  function readTyped() {
    if (mode === "circle") {
      const c = parseCircle(typed.value);
      if (c === null) return false;
      state.circle = c || null;
      state.center = null;
    } else {
      const pts = parseVertices(typed.value);
      if (!pts) return false;
      state.pts = pts;
      state.closed = pts.length >= 3;
    }
    return true;
  }

  function readPoint() {
    const a = latField.value.trim() === "" ? NaN : Number(latField.value);
    const b = lngField.value.trim() === "" ? NaN : Number(lngField.value);
    state.point = lat(a) && lng(b) ? { lat: a, lng: b } : null;
  }

  /** The shape as the text of the typed field. */
  function text() {
    if (mode === "circle") {
      const c = state.circle;
      return c ? `${c.lat}, ${c.lng}, ${c.r}` : "";
    }
    return state.pts.map((p) => `${p[0]}, ${p[1]}`).join("\n");
  }

  /** The shape as the GeoJSON that is posted; "" until it is complete. */
  function geojson() {
    if (mode === "circle" && state.circle) {
      const c = state.circle;
      return JSON.stringify({ type: "Point", coordinates: [c.lng, c.lat], radius: c.r });
    }
    if (mode === "polygon" && state.closed && state.pts.length >= 3) {
      const ring = state.pts.map((p) => [p[1], p[0]]);
      ring.push(ring[0]);
      return JSON.stringify({ type: "Polygon", coordinates: [ring] });
    }
    return "";
  }

  /** Puts the state in the fields; `fromTyped` leaves the typed text as it is. */
  function output(fromTyped) {
    syncing = true;
    if (mode === "point") {
      latField.value = state.point ? String(state.point.lat) : "";
      lngField.value = state.point ? String(state.point.lng) : "";
    } else {
      if (!fromTyped) typed.value = text();
      hidden.value = geojson();
    }
    syncing = false;
  }

  /** The map changed the shape: fields, layers, and a change event for the form. */
  function changed() {
    output(false);
    draw();
    syncing = true;
    kit.fire(mode === "point" ? latField : typed, "input");
    syncing = false;
  }

  function fromField() {
    if (syncing) return;
    if (mode === "point") readPoint();
    else if (readTyped()) hidden.value = geojson();
    else hidden.value = "";
    draw(true);
  }

  if (mode === "point") {
    readPoint();
    [latField, lngField].forEach((f) => f && f.addEventListener("input", fromField));
  } else if (typed && hidden) {
    readTyped();
    hidden.value = hidden.value || geojson();
    typed.addEventListener("input", fromField);
  } else {
    return;
  }
  if (!canvas) return;

  leaflet().then((L) => {
    const map = basemap(L, el, canvas);
    const layers = L.layerGroup().addTo(map);
    const shape = { className: "rx-map__shape" };
    let preview = null;
    if (mode === "polygon") map.doubleClickZoom.disable();

    /** The place of a pointer event, rounded and inside the usual range. */
    const where = (e) => {
      const at = e.latlng.wrap();
      return { lat: round(at.lat), lng: round(at.lng) };
    };

    /** Redraws the layers; `focus` also moves the view to the shape. */
    draw = (focus) => {
      layers.clearLayers();
      preview = null;
      let bounds = null;
      if (mode === "point" && state.point) {
        const p = state.point;
        pin(L, [p.lat, p.lng], { draggable: true, title: p.lat + ", " + p.lng })
          .on("dragend", (e) => {
            state.point = where({ latlng: e.target.getLatLng() });
            changed();
          })
          .addTo(layers);
        if (focus) map.setView([p.lat, p.lng], Math.max(map.getZoom(), 12));
      } else if (mode === "circle") {
        const c = state.circle || state.center;
        if (c) {
          pin(L, [c.lat, c.lng], { draggable: true, title: c.lat + ", " + c.lng })
            .on("dragend", (e) => {
              const at = where({ latlng: e.target.getLatLng() });
              if (state.circle) Object.assign(state.circle, at);
              else state.center = at;
              changed();
            })
            .addTo(layers);
        }
        if (state.circle) {
          const circle = L.circle([state.circle.lat, state.circle.lng], Object.assign({ radius: state.circle.r }, shape)).addTo(layers);
          bounds = circle.getBounds();
        } else if (state.center) {
          preview = L.circle([state.center.lat, state.center.lng], Object.assign({ radius: 1 }, shape)).addTo(layers);
        }
      } else if (mode === "polygon" && state.pts.length) {
        const latlngs = state.pts.map((p) => [p[0], p[1]]);
        const line = state.closed ? L.polygon(latlngs, shape) : L.polyline(latlngs, shape);
        line.addTo(layers);
        latlngs.forEach((p) => L.circleMarker(p, { radius: 5, className: "rx-map__vertex" }).addTo(layers));
        bounds = line.getBounds();
      }
      if (focus && bounds && bounds.isValid()) map.fitBounds(bounds, { padding: [24, 24] });
    };

    map.on("click", (e) => {
      const at = where(e);
      if (mode === "point") {
        state.point = at;
      } else if (mode === "circle") {
        if (state.center) {
          const r = Math.round(map.distance([state.center.lat, state.center.lng], [at.lat, at.lng]));
          if (!(r > 0)) return;
          state.circle = Object.assign({ r }, state.center);
          state.center = null;
        } else {
          state.center = at;
          state.circle = null;
        }
      } else {
        if (state.closed) {
          state.pts = [];
          state.closed = false;
        }
        const last = state.pts[state.pts.length - 1];
        // A double click is two clicks in one place: one vertex.
        if (last && map.latLngToContainerPoint(last).distanceTo(e.containerPoint) < 4) return;
        state.pts.push([at.lat, at.lng]);
      }
      changed();
    });

    map.on("mousemove", (e) => {
      if (preview && state.center) preview.setRadius(map.distance([state.center.lat, state.center.lng], e.latlng));
    });

    map.on("dblclick", () => {
      if (mode === "polygon" && !state.closed && state.pts.length >= 3) {
        state.closed = true;
        changed();
      }
    });

    canvas.addEventListener("keydown", (e) => {
      let handled = true;
      if (e.key === "Escape") {
        Object.assign(state, { point: null, circle: null, center: null, pts: [], closed: false });
      } else if (e.key === "Enter" && mode === "polygon" && !state.closed && state.pts.length >= 3) {
        state.closed = true;
      } else if (e.key === "Backspace" || e.key === "Delete") {
        if (mode === "polygon" && state.pts.length) {
          state.pts.pop();
          state.closed = false;
        } else if (mode === "circle" && (state.circle || state.center)) {
          state.circle = null;
          state.center = null;
        } else if (mode === "point" && state.point) {
          state.point = null;
        } else {
          handled = false;
        }
      } else {
        handled = false;
      }
      if (handled) {
        e.preventDefault();
        changed();
      }
    });

    draw(true);
  }, (error) => console.error(error));
}

export function setup(el, kit) {
  if (!kit.claim(el)) return;
  if (el.hasAttribute("data-rx-map-picker")) picker(el, kit);
  else view(el);
}
