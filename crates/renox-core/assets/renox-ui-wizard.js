// Renox UI: the multi-step form (`wizard`). renox-ui.js loads this module
// when a page, or what htmx swaps in, has one; it runs once per page and
// sets up each root it's handed.

var kit = window.Renox._kit;
var validate = kit.validate;
var emit = kit.emit;
var firstField = kit.firstField;

// ---------- Wizard ----------

function wizardParts(wizard) {
  return {
    panels: Array.prototype.filter.call(wizard.querySelectorAll("[data-rx-step]"), function (p) { return p.closest("[data-rx-wizard]") === wizard; }),
    tabs: Array.prototype.filter.call(wizard.querySelectorAll("[data-rx-step-tab]"), function (t) { return t.closest("[data-rx-wizard]") === wizard; })
  };
}

function showStep(wizard, index, focus) {
  var prev = wizard.getAttribute("data-rx-step-index");
  var parts = wizardParts(wizard);
  var last = parts.panels.length - 1;
  parts.panels.forEach(function (panel, i) {
    panel.hidden = i !== index;
    if (!panel.hasAttribute("aria-label") && parts.tabs[i]) panel.setAttribute("aria-label", parts.tabs[i].textContent.trim());
    panel.setAttribute("tabindex", "-1");
  });
  parts.tabs.forEach(function (tab, i) {
    if (i === index) tab.setAttribute("aria-current", "step"); else tab.removeAttribute("aria-current");
    if (i < index) tab.setAttribute("data-done", ""); else tab.removeAttribute("data-done");
  });
  wizard.setAttribute("data-rx-step-index", String(index));
  if (prev !== null && prev !== String(index)) emit(wizard, "changed", { name: parts.tabs[index] && parts.tabs[index].getAttribute("data-rx-step-tab"), index: index });
  var back = wizard.querySelector("[data-rx-wizard-back]");
  var next = wizard.querySelector("[data-rx-wizard-next]");
  var submit = wizard.querySelector("[data-rx-wizard-submit]");
  if (back) back.hidden = index === 0;
  if (next) next.hidden = index >= last;
  if (submit) submit.hidden = index < last;
  if (focus && parts.panels[index]) {
    var first = firstField(parts.panels[index]);
    (first || parts.panels[index]).focus();
  }
}

// The step's fields pass the browser's rules, then (data-live-validate)
// the server's.
function checkStep(wizard, panel) {
  var form = wizard.closest("form");
  var fields = Array.prototype.filter.call(panel.querySelectorAll("input, select, textarea"), function (el) {
    return el.name && !el.disabled && el.type !== "hidden" && el.type !== "button" && el.type !== "submit";
  });
  for (var i = 0; i < fields.length; i++) {
    if (!fields[i].checkValidity()) {
      if (fields[i].hasAttribute("data-rx-enhanced")) fields[i].dispatchEvent(new Event("invalid"));
      fields[i].reportValidity();
      return Promise.resolve(false);
    }
  }
  if (!form || !form.hasAttribute("data-live-validate")) return Promise.resolve(true);
  var seen = {};
  var unique = fields.filter(function (el) {
    if (seen[el.name] || el.type === "file") return false;
    seen[el.name] = true;
    return true;
  });
  return Promise.all(unique.map(function (el) { return validate(form, el); })).then(function (results) {
    var bad = results.some(function (messages) { return messages && messages.length; });
    if (bad) {
      var first = panel.querySelector('[aria-invalid="true"]');
      if (first) first.focus();
    }
    return !bad;
  });
}

function stepIndex(wizard) {
  return parseInt(wizard.getAttribute("data-rx-step-index") || "0", 10);
}

function nextStep(wizard) {
  var parts = wizardParts(wizard);
  var index = stepIndex(wizard);
  var button = wizard.querySelector("[data-rx-wizard-next]");
  if (button) button.setAttribute("aria-busy", "true");
  checkStep(wizard, parts.panels[index]).then(function (ok) {
    if (button) button.removeAttribute("aria-busy");
    if (ok) showStep(wizard, Math.min(index + 1, parts.panels.length - 1), true);
  });
}

function setupWizard(wizard) {
  if (wizard.hasAttribute("data-rx-ready")) return;
  wizard.setAttribute("data-rx-ready", "");
  var parts = wizardParts(wizard);
  // After a failed submit, open the first step with an error.
  var start = parts.panels.findIndex(function (p) { return p.querySelector('[aria-invalid="true"]'); });
  showStep(wizard, start < 0 ? 0 : start, start >= 0);
}

document.addEventListener("click", function (event) {
  var target = event.target.closest ? event.target : event.target.parentElement;
  if (!target) return;
  var next = target.closest("[data-rx-wizard-next]");
  if (next) { nextStep(next.closest("[data-rx-wizard]")); return; }
  var back = target.closest("[data-rx-wizard-back]");
  if (back) { var w = back.closest("[data-rx-wizard]"); showStep(w, Math.max(stepIndex(w) - 1, 0), true); }
});
// Enter in a field before the last step goes on, it doesn't send the form.
document.addEventListener("keydown", function (event) {
  if (event.key !== "Enter" || event.defaultPrevented || event.isComposing) return;
  var el = event.target;
  var wizard = el.closest && el.closest("[data-rx-wizard][data-rx-ready]");
  if (!wizard || el.tagName === "TEXTAREA" || el.tagName === "BUTTON") return;
  if (stepIndex(wizard) < wizardParts(wizard).panels.length - 1) { event.preventDefault(); nextStep(wizard); }
});
document.addEventListener("submit", function (event) {
  var wizard = event.target.querySelector && event.target.querySelector("[data-rx-wizard][data-rx-ready]");
  if (wizard && stepIndex(wizard) < wizardParts(wizard).panels.length - 1) { event.preventDefault(); event.stopImmediatePropagation(); nextStep(wizard); }
}, true);

// A sent form starts over at its first step (`clearActionForm` in the core).
kit.onClear(function (form) {
  form.querySelectorAll("[data-rx-wizard][data-rx-ready]").forEach(function (wizard) { showStep(wizard, 0, false); });
});
// A 422 after an action sheet's last step: show the first step with an error.
document.addEventListener("htmx:afterRequest", function (event) {
  var form = event.target;
  if (event.detail.successful || !form.matches || !form.matches("form[data-rx-action]")) return;
  var wizard = form.querySelector("[data-rx-wizard][data-rx-ready]");
  var panels = wizard ? wizardParts(wizard).panels : [];
  var bad = panels.findIndex(function (p) { return p.querySelector('[aria-invalid="true"]'); });
  if (bad >= 0) {
    showStep(wizard, bad, false);
    var invalid = panels[bad].querySelector('[aria-invalid="true"]');
    if (invalid) invalid.focus();
  }
});

kit.ready("wizard", function (root) { kit.each(root, "[data-rx-wizard]", setupWizard); });
