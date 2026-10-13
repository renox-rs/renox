(function () {
  "use strict";
  if (window.RenoxLive) return;

  // "add(5)" -> { action: "add", args: [5] }; "save" -> { action: "save", args: [] }.
  function parseCall(text) {
    var m = /^\s*([A-Za-z][A-Za-z0-9_]*)\s*(?:\((.*)\))?\s*$/.exec(text || "");
    if (!m) return null;
    var args = [];
    if (m[2] !== undefined && m[2].trim() !== "") {
      try {
        args = JSON.parse("[" + m[2] + "]");
      } catch (e) {
        console.error("rx: bad arguments in " + JSON.stringify(text), e);
        return null;
      }
    }
    return { action: m[1], args: args };
  }

  var MODEL_SELECTOR = "[rx-model], [rx-model\\.live], [rx-model\\.blur]";

  // { name, mode } from rx-model, rx-model.live or rx-model.blur; null if none.
  function modelOf(el) {
    var modes = ["", "live", "blur"];
    for (var i = 0; i < modes.length; i++) {
      var attr = modes[i] ? "rx-model." + modes[i] : "rx-model";
      if (el.hasAttribute(attr)) return { name: el.getAttribute(attr), mode: modes[i] || "plain" };
    }
    return null;
  }

  // The component's model fields as { name: value }.
  function models(wrapper) {
    var out = {};
    wrapper.querySelectorAll(MODEL_SELECTOR).forEach(function (el) {
      var m = modelOf(el);
      if (!m || !m.name) return;
      out[m.name] = el.type === "checkbox" ? (el.checked ? "true" : "false") : el.value;
    });
    return out;
  }

  // A model input without a name gets its model name (a morph removes it).
  function nameModels(root) {
    root.querySelectorAll(MODEL_SELECTOR).forEach(function (el) {
      var m = modelOf(el);
      if (m && m.name && !el.getAttribute("name")) el.setAttribute("name", m.name);
    });
  }

  // Sends an action to the component's route; the answer is morphed in
  // by the htmx:afterRequest listener below.
  function send(wrapper, action, args, extra) {
    var values = {
      _snapshot: wrapper.getAttribute("data-rx-snapshot"),
      _args: JSON.stringify(args)
    };
    var fields = models(wrapper);
    Object.keys(fields).forEach(function (k) { values[k] = fields[k]; });
    if (extra) Object.keys(extra).forEach(function (k) { values[k] = extra[k]; });
    wrapper.setAttribute("aria-busy", "true");
    htmx.ajax(
      "POST",
      "/_renox/live/" + wrapper.getAttribute("data-rx-live") + "/" + action,
      { source: wrapper, values: values, swap: "none" }
    );
  }

  document.addEventListener("click", function (event) {
    var el = event.target.closest && event.target.closest("[rx-click]");
    if (!el) return;
    var wrapper = el.closest("[data-rx-live]");
    if (!wrapper) return;
    var call = parseCall(el.getAttribute("rx-click"));
    if (!call) return;
    event.preventDefault();
    send(wrapper, call.action, call.args);
  });

  document.addEventListener("input", function (event) {
    var el = event.target;
    if (!el.matches || !el.hasAttribute("rx-model.live")) return;
    var wrapper = el.closest("[data-rx-live]");
    if (!wrapper) return;
    clearTimeout(wrapper._rxTimer);
    wrapper._rxTimer = setTimeout(function () {
      send(wrapper, "_refresh", [], {});
    }, 300);
  });

  document.addEventListener("change", function (event) {
    var el = event.target;
    if (!el.matches || !el.hasAttribute("rx-model.blur")) return;
    var wrapper = el.closest("[data-rx-live]");
    if (wrapper) send(wrapper, "_refresh", [], {});
  });

  // In the capture phase, and prevented, so the kit's busy-button submit
  // handler (renox-ui.js, which checks defaultPrevented) leaves it alone.
  document.addEventListener("submit", function (event) {
    var form = event.target;
    if (!form.matches || !form.matches("form[rx-submit]")) return;
    var wrapper = form.closest("[data-rx-live]");
    if (!wrapper) return;
    var call = parseCall(form.getAttribute("rx-submit"));
    if (!call) return;
    event.preventDefault();
    send(wrapper, call.action, call.args, Object.fromEntries(new FormData(form)));
  }, true);

  document.addEventListener("htmx:afterRequest", function (event) {
    var w = event.detail.elt;
    if (!w || !w.matches || !w.matches("[data-rx-live]")) return;
    w.removeAttribute("aria-busy");
    if (event.detail.xhr.status === 200) {
      Idiomorph.morph(w, event.detail.xhr.responseText, {
        morphStyle: "outerHTML",
        // Without it Idiomorph resets the focused field to the new HTML's value.
        ignoreActiveValue: true,
        // The answer has no `live_attrs`: keep the attributes the page's tag put on the wrapper.
        callbacks: {
          beforeAttributeUpdated: function (name, node, type) {
            if (node === w && type === "remove") return false;
          }
        }
      });
      htmx.process(w);
      nameModels(w);
      // The kit's setup and its parts loader listen to htmx:load.
      w.dispatchEvent(new CustomEvent("htmx:load", { bubbles: true }));
    }
  });

  nameModels(document);
  document.addEventListener("DOMContentLoaded", function () { nameModels(document); });

  window.RenoxLive = { parseCall: parseCall, send: send, modelOf: modelOf, models: models };
})();
