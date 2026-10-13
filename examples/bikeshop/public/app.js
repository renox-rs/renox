// The bike shop's motion, with Motion (motion.dev, vendored in public/vendor/motion,
// loaded before this file as the global `Motion`). Animations move things with
// `transform` and fade them with `opacity` only, which the browser does on the
// compositor (no layout work, 60 fps). People who ask their system for less motion
// (prefers-reduced-motion) get none.
(function () {
  "use strict";
  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)");

  // Slides the direct children of `root`'s [data-bs-reveal] containers in, one
  // after the other. Pages mark what should move: <div data-bs-reveal>…</div>.
  function reveal(root) {
    if (!window.Motion || reduce.matches) return;
    var groups = (root || document).querySelectorAll("[data-bs-reveal]");
    groups.forEach(function (group) {
      if (group.dataset.bsRevealed || group.closest("dialog")) return;
      group.dataset.bsRevealed = "1";
      var items = Array.prototype.slice.call(group.children);
      if (!items.length) return;
      Motion.animate(
        items,
        { opacity: [0, 1], transform: ["translateY(12px)", "translateY(0)"] },
        { duration: 0.35, delay: Motion.stagger(0.05), ease: [0.22, 1, 0.36, 1] }
      );
    });
  }

  // The kit slides the "About this page" panel in; its sections follow, one after
  // the other, as it opens.
  document.addEventListener("click", function (event) {
    var opener = event.target.closest && event.target.closest("[data-rx-open='about-page']");
    if (!opener || !window.Motion || reduce.matches) return;
    var panel = document.getElementById("about-page");
    var items = panel ? panel.querySelectorAll(".bs-explain > *") : [];
    if (items.length) {
      Motion.animate(
        items,
        { opacity: [0, 1], transform: ["translateX(16px)", "translateX(0)"] },
        { duration: 0.3, delay: Motion.stagger(0.04, { startDelay: 0.1 }), ease: [0.22, 1, 0.36, 1] }
      );
    }
  });

  // The login page's demo accounts: a tap fills the form's email and
  // password and moves to its button.
  document.addEventListener("click", function (event) {
    var account = event.target.closest && event.target.closest("[data-bs-demo-email]");
    if (!account) return;
    var form = document.querySelector("form input[name=email]");
    form = form && form.form;
    if (!form) return;
    form.elements.email.value = account.getAttribute("data-bs-demo-email");
    form.elements.password.value = account.getAttribute("data-bs-demo-password");
    var submit = form.querySelector("[type=submit]");
    if (submit) submit.focus();
  });

  window.Bikeshop = { reveal: reveal };
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { reveal(document); });
  } else {
    reveal(document);
  }
  // Content htmx swaps in moves the same way.
  document.addEventListener("htmx:afterSettle", function (event) { reveal(event.target); });
})();
