// renox-blocks: geolocate. The device's position, asked for only when the
// button is pressed (never on load). A fix fills four hidden inputs
// (`name[lat]`, `name[lng]`, `name[accuracy]` in metres, `name[at]` as an
// RFC 3339 UTC time) and enables them; they stay disabled before a press and
// after an error, so a form never posts a stale or half position. A polite
// live region says what is happening: locating, done, denied, unavailable or
// timed out, and the button then reads "Try again". A required block stops
// a submit without a position and says why. Nothing is animated.

const TIMEOUT_MS = 15000;

export function setup(el, kit) {
  const button = el.querySelector("[data-rx-geolocate-button]");
  const status = el.querySelector("[data-rx-geolocate-status]");
  const lat = el.querySelector("[data-rx-geolocate-lat]");
  const lng = el.querySelector("[data-rx-geolocate-lng]");
  const accuracyField = el.querySelector("[data-rx-geolocate-accuracy-field]");
  const at = el.querySelector("[data-rx-geolocate-at]");
  if (!button || !status || !lat || !lng || !accuracyField || !at || !kit.claim(el)) return;
  const texts = kit.texts(el);
  const wanted = parseFloat(el.getAttribute("data-rx-geolocate-accuracy"));
  const required = el.hasAttribute("data-rx-geolocate-required");
  const inputs = [lat, lng, accuracyField, at];
  let busy = false;

  function say(text) {
    status.textContent = text;
  }

  function clear() {
    inputs.forEach((input) => {
      input.disabled = true;
      input.value = "";
    });
  }

  function fail(text) {
    clear();
    busy = false;
    button.disabled = false;
    button.textContent = texts.retry;
    say(text);
  }

  function found(position) {
    const coords = position.coords;
    const metres = Math.round(coords.accuracy * 10) / 10;
    lat.value = String(coords.latitude);
    lng.value = String(coords.longitude);
    accuracyField.value = String(metres);
    at.value = new Date(position.timestamp || Date.now()).toISOString();
    inputs.forEach((input) => (input.disabled = false));
    busy = false;
    button.disabled = false;
    button.textContent = texts.retry;
    if (wanted > 0 && metres > wanted) {
      say(kit.fill(texts.wanted, { accuracy: Math.round(metres), wanted: Math.round(wanted) }));
    } else {
      say(kit.fill(texts.done, { lat: lat.value, lng: lng.value, accuracy: Math.round(metres) }));
    }
    kit.fire(el, "change");
  }

  function failed(error) {
    // PERMISSION_DENIED 1, POSITION_UNAVAILABLE 2, TIMEOUT 3.
    const code = error && error.code;
    fail(code === 1 ? texts.denied : code === 3 ? texts.timeout : texts.unavailable);
  }

  // The permission is asked for here and nowhere else.
  button.addEventListener("click", () => {
    if (busy) return;
    if (!navigator.geolocation || typeof navigator.geolocation.getCurrentPosition !== "function") {
      fail(texts.unavailable);
      return;
    }
    busy = true;
    button.disabled = true;
    say(texts.locating);
    try {
      navigator.geolocation.getCurrentPosition(found, failed, {
        enableHighAccuracy: wanted > 0,
        timeout: TIMEOUT_MS,
        maximumAge: 0,
      });
    } catch (_) {
      fail(texts.unavailable);
    }
  });

  const form = el.closest("form");
  if (form) {
    form.addEventListener("submit", (event) => {
      if (required && lat.disabled) {
        event.preventDefault();
        say(texts.needed);
        button.focus();
      }
    });
    form.addEventListener("reset", () =>
      setTimeout(() => {
        clear();
        button.textContent = texts.button;
        say(texts.idle);
      }),
    );
  }
}
