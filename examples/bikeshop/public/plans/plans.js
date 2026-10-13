// The service plans' motion (#237), with Motion (motion.dev, loaded by the layout
// as the global `Motion`). Everything here is an improvement: without it the
// forms post as they are. Only `transform` and `opacity` move (the compositor's
// work, 60 fps); nothing moves under prefers-reduced-motion. No inline handlers,
// so CSP=strict works.
(function () {
  "use strict";
  if (window.BikeshopPlans) return;
  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
  var moving = function () { return window.Motion && !reduce.matches; };

  // Choosing a plan, a store or a weekday: htmx swaps the summary in; its price
  // gives a small nod so the eye finds what changed.
  document.addEventListener("htmx:afterSettle", function (event) {
    var live = event.target && event.target.id === "subscribe-live" ? event.target : null;
    if (!live || !moving()) return;
    var price = live.querySelector(".bs-subscribe__price");
    if (price) {
      Motion.animate(price, { transform: ["scale(1)", "scale(1.08)", "scale(1)"] }, { duration: 0.35, ease: "easeOut" });
    }
  });

  // Skipping a visit: its row slides away before the page reloads without it.
  document.addEventListener("submit", function (event) {
    var form = event.target;
    var match = /\/plans\/visits\/(\d+)\/skip$/.exec(form.getAttribute("action") || "");
    if (!match || !moving()) return;
    var row = document.querySelector('[data-bs-visit="' + match[1] + '"]');
    if (!row || row.dataset.bsLeaving) return;
    event.preventDefault();
    row.dataset.bsLeaving = "1";
    var dialog = form.closest("dialog");
    if (dialog && dialog.open) dialog.close();
    Motion.animate(row, { opacity: [1, 0], transform: ["translateX(0)", "translateX(-24px)"] }, { duration: 0.22, ease: "easeIn" });
    setTimeout(function () { form.submit(); }, 230);
  });

  window.BikeshopPlans = true;
})();
