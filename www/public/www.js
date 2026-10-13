// renox.rs's motion. Everything it moves is already in the HTML (search
// engines and readers without JavaScript get the same text); this only
// animates it: the terminal replays, the crates gather into one, a packet
// runs the request pipeline, the Laravel ↔ Renox pairs become tabs, the
// figures count up, the bars grow, the binary ships. Nothing moves for
// people who ask for less motion (prefers-reduced-motion).
(function () {
  "use strict";
  var root = document.documentElement;
  root.classList.remove("no-js");
  root.classList.add("js");
  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var sleep = function (ms) { return new Promise(function (r) { setTimeout(r, ms); }); };
  var once = function (el, fn) {
    if (!el) return;
    if (!("IntersectionObserver" in window)) return fn(el);
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) { io.disconnect(); fn(el); } });
    }, { threshold: 0.25 });
    io.observe(el);
  };
  var ready = function (fn) { document.readyState === "loading" ? document.addEventListener("DOMContentLoaded", fn) : fn(); };

  ready(function () {
    // ---- copy the install command ----
    document.querySelectorAll("[data-copy]").forEach(function (b) {
      b.addEventListener("click", function () {
        if (navigator.clipboard) navigator.clipboard.writeText(b.getAttribute("data-copy"));
        b.textContent = "copied";
        setTimeout(function () { b.textContent = "copy"; }, 1400);
      });
    });

    // ---- the hero's field: dots that ripple and glow near the pointer ----
    var canvas = document.getElementById("field");
    if (canvas && !reduce) {
      var x = canvas.getContext("2d"), dpr = window.devicePixelRatio || 1, w, h, dots = [], mx = -9999, my = -9999, t = 0, visible = true;
      var size = function () {
        var r = canvas.getBoundingClientRect(); w = canvas.width = r.width * dpr; h = canvas.height = r.height * dpr; dots = [];
        var g = 26 * dpr; for (var y = g / 2; y < h; y += g) for (var xx = g / 2; xx < w; xx += g) dots.push([xx, y]);
      };
      size(); window.addEventListener("resize", size);
      canvas.parentElement.addEventListener("pointermove", function (e) { var r = canvas.getBoundingClientRect(); mx = (e.clientX - r.left) * dpr; my = (e.clientY - r.top) * dpr; });
      if ("IntersectionObserver" in window) new IntersectionObserver(function (es) { visible = es[0].isIntersecting; }).observe(canvas);
      var draw = function () {
        if (visible) {
          t += 0.012; x.clearRect(0, 0, w, h);
          for (var i = 0; i < dots.length; i++) {
            var px = dots[i][0], py = dots[i][1];
            var near = Math.max(0, 1 - Math.hypot(px - mx, py - my) / (220 * dpr));
            var a = 0.05 + (Math.sin(px * 0.004 + py * 0.003 - t * 2) * 0.5 + 0.5) * 0.09 + near * 0.55;
            x.fillStyle = near > 0.05 ? "rgba(255," + (120 + near * 60) + "," + (60 + near * 40) + "," + a + ")" : "rgba(255,255,255," + a + ")";
            var s = (1 + near * 1.6) * dpr; x.fillRect(px - s / 2, py - s / 2, s, s);
          }
        }
        requestAnimationFrame(draw);
      };
      requestAnimationFrame(draw);
    }

    // ---- the terminal replays its own transcript ----
    var tty = document.getElementById("tty"), browser = document.getElementById("browser");
    if (tty && !reduce) {
      var lines = tty.innerHTML.split("\n");
      (async function play() {
        for (;;) {
          tty.innerHTML = "";
          if (browser) browser.animate([{ opacity: 1 }, { opacity: 0, transform: "translateY(24px) scale(.96)" }], { duration: 300, fill: "forwards" });
          for (var i = 0; i < lines.length; i++) {
            var line = lines[i];
            var m = line.match(/^(.*<span class="c" data-cmd>)(.*?)(<\/span>.*)$/);
            if (m) {
              tty.insertAdjacentHTML("beforeend", m[1] + "</span>" + '<span class="caret"></span>');
              var spans = tty.querySelectorAll("[data-cmd]"), el = spans[spans.length - 1], caret = tty.querySelector(".caret");
              var text = new DOMParser().parseFromString(m[2], "text/html").documentElement.textContent;
              for (var k = 0; k < text.length; k++) { el.textContent += text[k]; await sleep(38 + Math.random() * 40); }
              caret.remove(); tty.insertAdjacentHTML("beforeend", "\n"); await sleep(320);
            } else {
              tty.insertAdjacentHTML("beforeend", line + "\n");
              await sleep(/Compiling/.test(line) ? 900 : 110);
            }
          }
          if (browser) browser.animate([{ opacity: 0, transform: "translateY(24px) scale(.96)" }, { opacity: 1, transform: "none" }], { duration: 650, easing: "cubic-bezier(.2,.8,.2,1)", fill: "forwards" });
          await sleep(6500);
        }
      })();
    }

    // ---- figures count up ----
    document.querySelectorAll("[data-count]").forEach(function (el) {
      if (reduce) return;
      var to = +el.getAttribute("data-count"), suffix = el.getAttribute("data-suffix") || "";
      el.textContent = "0" + suffix;
      once(el, function () {
        var t0 = performance.now();
        var step = function (now) { var p = Math.min(1, (now - t0) / 1400), e = 1 - Math.pow(1 - p, 3); el.textContent = Math.round(to * e).toLocaleString("en-US") + suffix; if (p < 1) requestAnimationFrame(step); };
        requestAnimationFrame(step);
      });
    });

    // ---- feature cards rise in ----
    if (!reduce) document.querySelectorAll(".reveal").forEach(function (el, i) {
      el.style.opacity = 0;
      once(el, function () { el.animate([{ opacity: 0, transform: "translateY(18px)" }, { opacity: 1, transform: "none" }], { duration: 600, delay: (i % 4) * 90, easing: "cubic-bezier(.2,.8,.2,1)", fill: "forwards" }); });
    });

    // ---- twenty crates gather into one ----
    var box = document.getElementById("crates");
    if (box) {
      var one = box.querySelector(".one"), crates = Array.prototype.slice.call(box.querySelectorAll(".crate"));
      var scatter = function () {
        crates.forEach(function (d) {
          d._t = "translate(calc(-50% + " + (Math.random() - 0.5) * (box.clientWidth - 130) + "px), calc(-50% + " + (Math.random() - 0.5) * (box.clientHeight - 60) + "px)) rotate(" + (Math.random() - 0.5) * 24 + "deg)";
          d.style.transform = d._t;
        });
      };
      scatter();
      if (reduce) { crates.forEach(function (d) { d.style.opacity = 0.25; }); one.style.opacity = 1; }
      else once(box, async function () {
        for (;;) {
          await sleep(1600);
          crates.forEach(function (d, i) { d.animate([{ transform: d._t, opacity: 1 }, { transform: "translate(-50%,-50%) scale(.6)", opacity: 0 }], { duration: 900, delay: i * 25, easing: "cubic-bezier(.6,0,.2,1)", fill: "forwards" }); });
          one.animate([{ opacity: 0, scale: 0.9 }, { opacity: 1, scale: 1 }], { duration: 600, delay: 1000, easing: "cubic-bezier(.2,.8,.2,1)", fill: "forwards" });
          await sleep(4200);
          one.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 400, fill: "forwards" });
          scatter();
          crates.forEach(function (d, i) { d.animate([{ opacity: 0, transform: "translate(-50%,-50%) scale(.6)" }, { opacity: 1, transform: d._t }], { duration: 800, delay: i * 20, easing: "cubic-bezier(.2,.8,.2,1)", fill: "forwards" }); });
        }
      });
    }

    // ---- the request pipeline: a packet runs through the layers ----
    var pipe = document.getElementById("pipe");
    if (pipe) {
      var nodes = Array.prototype.slice.call(pipe.querySelectorAll(".node")), layers = pipe.querySelectorAll(".layer"), packet = pipe.querySelector(".packet"), track = pipe.querySelector(".pipe__track");
      var show = function (i) { nodes.forEach(function (n, j) { n.classList.toggle("on", j === i); }); layers.forEach(function (l, j) { l.classList.toggle("on", j === i); }); };
      var paused = false;
      nodes.forEach(function (n, i) { n.addEventListener("pointerenter", function () { paused = true; show(i); }); n.addEventListener("pointerleave", function () { paused = false; }); });
      show(0);
      if (!reduce) once(track, async function () {
        for (var i = 0; ; i = (i + 1) % nodes.length) {
          if (paused) { await sleep(300); continue; }
          var r = track.getBoundingClientRect(), n = nodes[i].getBoundingClientRect(), to = n.left - r.left + n.width / 2 - 7 + "px";
          packet.animate([{ left: packet.style.left || "4%" }, { left: to }], { duration: 650, easing: "cubic-bezier(.6,0,.2,1)", fill: "forwards" });
          packet.style.left = to;
          await sleep(650); show(i); await sleep(i === nodes.length - 1 ? 2200 : 1300);
          if (i === nodes.length - 1) { packet.style.left = "4%"; }
        }
      });
    }

    // ---- Laravel ↔ Renox: tabs, one pair at a time ----
    var morph = document.getElementById("morph");
    if (morph) {
      var tabs = Array.prototype.slice.call(morph.querySelectorAll("[role=tab]")), pairs = morph.querySelectorAll(".pair"), cur = 0, timer, manual = false;
      var go = function (i, byUser) {
        cur = i; manual = manual || !!byUser;
        tabs.forEach(function (b, j) { b.setAttribute("aria-selected", j === i ? "true" : "false"); b.tabIndex = j === i ? 0 : -1; });
        pairs.forEach(function (p, j) { p.classList.toggle("on", j === i); });
        if (!reduce) {
          pairs[i].querySelectorAll("pre").forEach(function (pre, side) {
            pre.animate([{ opacity: 0, filter: "blur(6px)", transform: "translateX(" + (side ? 8 : -8) + "px)" }, { opacity: 1, filter: "blur(0)", transform: "none" }], { duration: 480, delay: side * 220, fill: "backwards" });
          });
          tabs.forEach(function (b) { b.querySelector(".prog").getAnimations().forEach(function (a) { a.cancel(); }); });
          if (!manual) tabs[i].querySelector(".prog").animate([{ width: "0%" }, { width: "100%" }], { duration: 7000, easing: "linear" });
        }
        clearTimeout(timer);
        if (!manual && !reduce) timer = setTimeout(function () { go((cur + 1) % tabs.length); }, 7000);
      };
      tabs.forEach(function (b, i) {
        b.addEventListener("click", function () { go(i, true); });
        b.addEventListener("keydown", function (e) {
          var k = e.key === "ArrowDown" || e.key === "ArrowRight" ? 1 : e.key === "ArrowUp" || e.key === "ArrowLeft" ? -1 : 0;
          if (k) { e.preventDefault(); var n = (i + k + tabs.length) % tabs.length; go(n, true); tabs[n].focus(); }
        });
      });
      go(0);
    }

    // ---- benchmark bars grow ----
    if (!reduce) document.querySelectorAll(".chart").forEach(function (c) {
      var fills = c.querySelectorAll(".fill");
      fills.forEach(function (f) { f.style.width = "0"; });
      once(c, function () { fills.forEach(function (f, i) { f.animate([{ width: "0%" }, { width: getComputedStyle(f).getPropertyValue("--w") }], { duration: 1200, delay: i * 120, easing: "cubic-bezier(.2,.8,.2,1)", fill: "forwards" }); }); });
    });

    // ---- one binary ships ----
    var st = document.getElementById("ship");
    if (st && !reduce) {
      var parts = Array.prototype.slice.call(st.querySelectorAll(".part")), bin = st.querySelector(".binary"), srv = st.querySelector(".server"), ok = srv.querySelector("b");
      once(st, async function () {
        for (;;) {
          var b = bin.getBoundingClientRect();
          parts.forEach(function (p, i) { var r = p.getBoundingClientRect(); p.animate([{ transform: "none", opacity: 1 }, { transform: "translate(" + (b.left + b.width / 2 - r.left - r.width / 2) + "px, " + (b.top + b.height / 2 - r.top - r.height / 2) + "px) scale(.3)", opacity: 0 }], { duration: 800, delay: i * 140, easing: "cubic-bezier(.6,0,.3,1)", fill: "forwards" }); });
          await sleep(1500);
          bin.animate([{ boxShadow: "0 0 0 rgba(255,106,43,0)" }, { boxShadow: "0 0 40px rgba(255,106,43,.6)" }, { boxShadow: "0 0 0 rgba(255,106,43,0)" }], { duration: 700 });
          await sleep(500);
          var s = srv.getBoundingClientRect(), bb = bin.getBoundingClientRect();
          bin.animate([{ transform: "none" }, { transform: "translate(" + (s.left + s.width / 2 - bb.left - bb.width / 2) + "px, " + (s.top + s.height / 2 - bb.top - bb.height / 2) + "px) scale(.7)" }], { duration: 1100, easing: "cubic-bezier(.6,0,.3,1)", fill: "forwards" });
          await sleep(1000);
          srv.animate([{ opacity: 0.35 }, { opacity: 1 }], { duration: 300, fill: "forwards" }); ok.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 300, fill: "forwards" });
          await sleep(2600);
          srv.animate([{ opacity: 1 }, { opacity: 0.35 }], { duration: 300, fill: "forwards" }); ok.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 300, fill: "forwards" });
          bin.animate([{ opacity: 0, transform: "none" }, { opacity: 1, transform: "none" }], { duration: 400, fill: "forwards" });
          parts.forEach(function (p) { p.animate([{ opacity: 0 }, { opacity: 1, transform: "none" }], { duration: 500, fill: "forwards" }); });
          await sleep(900);
        }
      });
    }
  });
})();
