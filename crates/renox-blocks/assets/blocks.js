// renox-blocks: the loader. A JavaScript module, so it runs once per page
// however often its <script> tag arrives (htmx swaps bring it again). It
// looks for blocks on the page and in whatever htmx swaps in (htmx:load),
// and imports a block's own code only when the page has that block: a page
// with a gallery loads gallery.js and nothing else. No inline handlers, so
// CSP=strict works. Movement uses the browser's Web Animations on
// `transform` and `opacity`, and nothing moves under prefers-reduced-motion.

const script = document.querySelector("script[data-renox-blocks]");

// The blocks that have code, and how a page shows it has one.
const PARTS = [
  ["gallery", "[data-rx-gallery]"],
  ["range", "[data-rx-range]"],
  ["quantity", "[data-rx-quantity]"],
  ["keypad", "[data-rx-keypad]"],
  ["kanban", "[data-rx-kanban]"],
  ["datetime", "[data-rx-datetime-range]"],
  ["history", "[data-rx-history]"],
];

const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";

/** What every block's code gets: small helpers shared by them all. */
const kit = {
  /** Whether to skip animations: less motion asked for, or none possible. */
  still() {
    return reduce.matches || typeof Element.prototype.animate !== "function";
  },

  /** Animates `el` (keyframes as `{property: [from, to]}`) for `ms`; null when still. */
  animate(el, keyframes, ms, options) {
    if (kit.still()) return null;
    return el.animate(keyframes, Object.assign({ duration: ms, easing: EASE }, options || {}));
  },

  /** Sends a bubbling `type` event from `el`. */
  fire(el, type) {
    el.dispatchEvent(new Event(type, { bubbles: true }));
  },

  /** The texts a block carries in data-rx-blocks-texts (translated by the server). */
  texts(el) {
    try {
      return JSON.parse(el.getAttribute("data-rx-blocks-texts") || "{}");
    } catch (_) {
      return {};
    }
  },

  /** "Moved :card" with {card: "Tune-up"} → "Moved Tune-up". */
  fill(text, values) {
    return String(text || "").replace(/:(\w+)/g, (match, key) =>
      Object.prototype.hasOwnProperty.call(values, key) ? values[key] : match,
    );
  },

  /** Moves `el` from where `before` (a DOMRect) was to where it is now. */
  glide(el, before) {
    if (!before || kit.still()) return;
    const after = el.getBoundingClientRect();
    const dx = before.left - after.left;
    const dy = before.top - after.top;
    if (!dx && !dy) return;
    kit.animate(el, { transform: [`translate(${dx}px, ${dy}px)`, "translate(0px, 0px)"] }, 220);
  },

  /** Marks `el` set up; false when it already was. */
  claim(el) {
    if (el.hasAttribute("data-rx-blocks-ready")) return false;
    el.setAttribute("data-rx-blocks-ready", "");
    return true;
  },
};

const loading = {};

/** A block's code, imported once and started once. */
function load(name) {
  if (!loading[name]) {
    const url = script && script.getAttribute(`data-${name}`);
    loading[name] = url
      ? import(url).then((part) => {
          if (part.start) part.start(kit);
          return part;
        })
      : Promise.reject(new Error(`renox-blocks: no file for ${name}`));
  }
  return loading[name];
}

/** Sets up every block in `root` (and the block `root` is inside of). */
function setup(root) {
  root = root && root.querySelectorAll ? root : document;
  for (const [name, selector] of PARTS) {
    const found = Array.from(root.querySelectorAll(selector));
    if (root.matches && root.matches(selector)) found.push(root);
    const host = root.parentElement && root.parentElement.closest(selector);
    if (host) found.push(host);
    if (!found.length) continue;
    load(name).then(
      (part) => found.forEach((el) => part.setup(el, kit)),
      (error) => console.error(error),
    );
  }
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => setup(document));
} else {
  setup(document);
}
document.addEventListener("htmx:load", (event) => setup(event.target));
