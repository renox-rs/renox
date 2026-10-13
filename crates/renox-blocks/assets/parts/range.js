// renox-blocks: range_slider. Two native range inputs on one track: this
// keeps them from crossing, paints the chosen part between them, and says
// each value in words (aria-valuetext) as the server would show it.

/** How a range shows its values: money (from the server's "0" in that
 *  currency, e.g. "$0.00": the values are in its smallest unit, so they are
 *  divided by 10^decimals), a number, or as they are. */
function formatter(field) {
  const kind = field.getAttribute("data-rx-range-format");
  const locale = field.getAttribute("data-rx-range-locale") || undefined;
  const prefix = field.getAttribute("data-rx-range-prefix") || "";
  const suffix = field.getAttribute("data-rx-range-suffix") || "";
  function numbers(decimals) {
    const options = { minimumFractionDigits: decimals, maximumFractionDigits: decimals };
    try {
      return new Intl.NumberFormat(locale, Object.assign({ useGrouping: "always" }, options));
    } catch (_) {
      return new Intl.NumberFormat(locale, options);
    }
  }
  if (kind === "money") {
    const unit = field.getAttribute("data-rx-range-unit") || "0";
    const zero = (unit.match(/0[.,]?0*/) || ["0"])[0];
    const decimals = zero.length > 1 ? zero.length - 2 : 0;
    const money = numbers(decimals);
    const scale = Math.pow(10, decimals);
    return (value) => unit.replace(zero, money.format(value / scale));
  }
  if (kind === "number") {
    const plain = numbers(0);
    return (value) => plain.format(value);
  }
  return (value) => prefix + value + suffix;
}

export function setup(field, kit) {
  const low = field.querySelector('[data-rx-range-input="min"]');
  const high = field.querySelector('[data-rx-range-input="max"]');
  const track = field.querySelector("[data-rx-range-track]");
  if (!low || !high || !track || !kit.claim(field)) return;
  const format = formatter(field);
  const min = parseFloat(low.min);
  const max = parseFloat(low.max);
  const span = max - min || 1;

  function update(moved) {
    let a = parseFloat(low.value);
    let b = parseFloat(high.value);
    // The handles never cross: the one moving stops at the other.
    if (a > b) {
      if (moved === high) high.value = b = a;
      else low.value = a = b;
    }
    track.style.setProperty("--rx-range-from", String((a - min) / span));
    track.style.setProperty("--rx-range-to", String((b - min) / span));
    // When both sit at the top end, the lower handle must be the one on top
    // (else it couldn't be dragged down again).
    low.style.zIndex = a === b && b === max ? "3" : "";
    for (const [input, value, which] of [
      [low, a, "min"],
      [high, b, "max"],
    ]) {
      const text = format(value);
      input.setAttribute("aria-valuetext", text);
      const shown = field.querySelector(`[data-rx-range-shown="${which}"]`);
      if (shown) shown.textContent = text;
    }
  }
  low.addEventListener("input", () => update(low));
  high.addEventListener("input", () => update(high));
  update(null);
}
