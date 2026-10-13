// The stock and multi-store pages' small behaviour (#240, #245). No inline handlers, so
// CSP=strict works; motion through Motion (motion.dev, vendored), `transform` only, and
// nothing moves under prefers-reduced-motion.
(function () {
  "use strict";
  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)");

  // The supplier's printable order: its "Print" button.
  document.addEventListener("click", function (event) {
    var button = event.target.closest && event.target.closest("[data-bs-print]");
    if (button) window.print();
  });

  // A status stepper's done steps slide in from the left, one after the other, up to the
  // current step: where the shipment or order stands, at a glance.
  function grow(root) {
    if (!window.Motion || reduce.matches) return;
    (root || document).querySelectorAll(".bs-flow:not([data-bs-grown])").forEach(function (flow) {
      flow.setAttribute("data-bs-grown", "");
      var done = flow.querySelectorAll("li[data-done]");
      if (!done.length) return;
      Motion.animate(
        done,
        { transform: ["translateX(-8px)", "translateX(0)"], opacity: [0.4, 1] },
        { duration: 0.3, delay: Motion.stagger(0.06), ease: [0.22, 1, 0.36, 1] }
      );
    });
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { grow(document); });
  } else {
    grow(document);
  }
  document.addEventListener("htmx:afterSettle", function (event) { grow(event.target); });
})();
