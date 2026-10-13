// The catalogue's behaviour (#233), on top of htmx and the kit. Everything
// here is an improvement: without it the filters are a form with an "Apply"
// button, the search box a plain search form, and the cart's count a link.
// Motion (motion.dev, loaded by the layout) moves things with `transform`
// only; nothing moves under prefers-reduced-motion. No inline handlers, so
// CSP=strict works.
(function () {
  "use strict";
  if (window.BikeshopCatalog) return;
  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
  var narrow = window.matchMedia("(max-width: 52rem)");

  // ---------- filters: folded on phones, sent on change ----------
  function setUpFilters(root) {
    (root || document).querySelectorAll("[data-bs-filters]").forEach(function (box) {
      if (box.dataset.bsReady) return;
      box.dataset.bsReady = "1";
      if (window.htmx) box.dataset.bsLive = "";
      var fit = function () {
        // Open beside the results on a wide screen; folded under "Filters" on a phone.
        if (narrow.matches) box.removeAttribute("open");
        else box.setAttribute("open", "");
      };
      fit();
      narrow.addEventListener("change", fit);
    });
  }

  // ---------- the navbar's search suggestions: arrows and Escape ----------
  function items(form) {
    return Array.prototype.slice.call(form.querySelectorAll("[data-bs-suggestion]"));
  }
  function close(form) {
    var panel = form.querySelector(".bs-suggest");
    if (panel) panel.innerHTML = "";
  }
  document.addEventListener("keydown", function (event) {
    var form = event.target.closest && event.target.closest("[data-bs-search]");
    if (!form) return;
    var input = form.querySelector("input[name=q]");
    var list = items(form);
    var at = list.indexOf(document.activeElement);
    if (event.key === "ArrowDown" && list.length) {
      event.preventDefault();
      list[Math.min(at + 1, list.length - 1)].focus();
    } else if (event.key === "ArrowUp" && list.length) {
      event.preventDefault();
      if (at <= 0) input.focus();
      else list[at - 1].focus();
    } else if (event.key === "Escape") {
      if (list.length) {
        event.preventDefault();
        close(form);
        input.focus();
      }
    }
  });
  document.addEventListener("click", function (event) {
    document.querySelectorAll("[data-bs-search]").forEach(function (form) {
      if (!form.contains(event.target)) close(form);
    });
  });
  // The panel slides in a little.
  document.addEventListener("htmx:afterSwap", function (event) {
    var panel = event.detail && event.detail.target;
    if (!panel || !panel.classList || !panel.classList.contains("bs-suggest")) return;
    if (!window.Motion || reduce.matches || !panel.firstElementChild) return;
    Motion.animate(panel, { opacity: [0, 1], transform: ["translateY(-4px)", "translateY(0)"] }, { duration: 0.18 });
  });

  // ---------- the cart's count gives a little bounce when it changes ----------
  document.addEventListener("htmx:oobAfterSwap", function (event) {
    var link = event.detail && event.detail.target;
    if (!link || link.id !== "nav-cart" || !window.Motion || reduce.matches) return;
    Motion.animate(link, { transform: ["scale(1)", "scale(1.12)", "scale(1)"] }, { duration: 0.35, ease: [0.22, 1, 0.36, 1] });
  });

  window.BikeshopCatalog = { setUpFilters: setUpFilters };
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { setUpFilters(document); });
  } else {
    setUpFilters(document);
  }
  document.addEventListener("htmx:afterSettle", function (event) { setUpFilters(event.target); });
})();
