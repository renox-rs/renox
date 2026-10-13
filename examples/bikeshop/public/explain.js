// "About this page" (src/explain.rs, resources/views/about/_explain.html):
// folding the docked panel to its rail and opening it again (remembered in
// the `bikeshop_explain` cookie, which the server reads so the next page is
// drawn the same way at once), and the feature chips that show their "why".
// The code tabs and the copy buttons are the kit's own (renox-ui.js).
(function () {
  "use strict";
  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)");

  function remember(state) {
    document.cookie = "bikeshop_explain=" + state + "; path=/; max-age=31536000; samesite=lax";
  }

  // Folds the panel (`rail`) or opens it (`open`), and moves the focus to the
  // button that undoes it, so a keyboard user stays where they were.
  function dock(state) {
    var aside = document.getElementById("explain-dock");
    if (!aside) return;
    var panel = aside.querySelector(".bs-dock__panel");
    var rail = aside.querySelector(".bs-dock__rail");
    var folded = state === "rail";
    panel.hidden = folded;
    rail.hidden = !folded;
    document.body.classList.toggle("bs-docked--rail", folded);
    aside.querySelectorAll("[data-bs-dock-toggle]").forEach(function (button) {
      button.setAttribute("aria-expanded", folded ? "false" : "true");
    });
    remember(folded ? "rail" : "open");
    var next = aside.querySelector(folded ? "[data-bs-dock-toggle='open']" : "[data-bs-dock-toggle='rail']");
    if (next) next.focus();
    if (!folded && window.Motion && !reduce.matches) {
      Motion.animate(
        panel.querySelectorAll(".bs-dock__header, .bs-explain > *"),
        { opacity: [0, 1], transform: ["translateX(12px)", "translateX(0)"] },
        { duration: 0.28, delay: Motion.stagger(0.03), ease: [0.22, 1, 0.36, 1] }
      );
    }
  }

  // The public layout's bar spans the panel too, which starts under it: its
  // height, for public/explain.css, when it isn't the one-line bar's.
  var bar = document.querySelector("body.bs-public.bs-docked > .rx-navbar");
  if (bar && window.ResizeObserver) {
    new ResizeObserver(function () {
      document.body.style.setProperty("--bs-bar-h", bar.getBoundingClientRect().height + "px");
    }).observe(bar);
  }

  document.addEventListener("click", function (event) {
    if (!event.target.closest) return;
    var toggle = event.target.closest("[data-bs-dock-toggle]");
    if (toggle) {
      dock(toggle.getAttribute("data-bs-dock-toggle"));
      return;
    }
    // A feature chip: shows its "why" under the chips, one at a time.
    var chip = event.target.closest("[data-bs-chip]");
    if (!chip) return;
    var open = chip.getAttribute("aria-expanded") !== "true";
    var scope = chip.closest(".bs-explain");
    scope.querySelectorAll("[data-bs-chip]").forEach(function (other) {
      var why = document.getElementById(other.getAttribute("aria-controls"));
      var mine = other === chip && open;
      other.setAttribute("aria-expanded", mine ? "true" : "false");
      if (why) why.hidden = !mine;
    });
    var shown = open && document.getElementById(chip.getAttribute("aria-controls"));
    if (shown && window.Motion && !reduce.matches) {
      Motion.animate(shown, { opacity: [0, 1], transform: ["translateY(-4px)", "translateY(0)"] }, { duration: 0.2 });
    }
  });
})();
