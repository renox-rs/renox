// Renox UI: behavior for the components of renox/ui.html. Plain DOM, no
// inline handlers (works under a strict CSP), everything keyboard-usable.
(function () {
  "use strict";

  var reduceMotion = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  function csrf() {
    var meta = document.querySelector('meta[name="csrf-token"]');
    return meta ? meta.content : "";
  }

  // A component announces what happened to it as `rx:<component>:<event>`,
  // bubbling from its root element (`data-rx-component`).
  function emit(el, name, detail) { var c = el && el.getAttribute && el.getAttribute("data-rx-component"); if (!c) return; el.dispatchEvent(new CustomEvent("rx:" + c + ":" + name, { bubbles: true, detail: detail || {} })); }

  // ---------- Toasts ----------

  var ICONS = {
    success: '<svg viewBox="0 0 20 20" width="20" height="20"><circle cx="10" cy="10" r="9" fill="currentColor"/><path d="M6 10.5l2.5 2.5L14 7.5" stroke="#fff" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    info: '<svg viewBox="0 0 20 20" width="20" height="20"><circle cx="10" cy="10" r="9" fill="currentColor"/><path d="M10 9v5M10 6h.01" stroke="#fff" stroke-width="2" stroke-linecap="round"/></svg>',
    warning: '<svg viewBox="0 0 20 20" width="20" height="20"><path d="M10 2l8.5 15h-17z" fill="currentColor" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M10 8v4M10 14.5h.01" stroke="#fff" stroke-width="2" stroke-linecap="round"/></svg>',
    error: '<svg viewBox="0 0 20 20" width="20" height="20"><circle cx="10" cy="10" r="9" fill="currentColor"/><path d="M7 7l6 6M13 7l-6 6" stroke="#fff" stroke-width="2" stroke-linecap="round"/></svg>'
  };
  var CLOSE = '<svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true"><path d="M5 5l10 10M15 5L5 15" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>';

  function region() {
    var el = document.querySelector("[data-renox-toasts]");
    if (!el) {
      el = document.createElement("div");
      el.className = "rx-toasts";
      el.setAttribute("data-renox-toasts", "");
      el.setAttribute("aria-live", "polite");
      document.body.appendChild(el);
    }
    return el;
  }

  function dismiss(toast) {
    if (!toast || toast.hasAttribute("data-leaving")) return;
    toast.setAttribute("data-leaving", "");
    setTimeout(function () { toast.remove(); }, reduceMotion ? 0 : 200);
  }

  // Escape closes a toast: the one holding the focus, else the newest. It
  // goes first to anything else Escape closes (a sheet, a menu or the
  // bell's panel, a popover, a tooltip, the period filter, a grid cell being
  // edited), so it runs early (capture) to look before they close.
  document.addEventListener("keydown", function (event) {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    var toasts = document.querySelectorAll("[data-renox-toast]:not([data-leaving])");
    if (!toasts.length) return;
    var target = event.target.closest ? event.target : null;
    var focused = target && target.closest("[data-renox-toast]");
    if (focused) { dismiss(focused); return; }
    var busy = '[aria-expanded="true"], .rx-tip, details[data-rx-period][open], dialog[open]';
    try {
      if (document.querySelector(busy + ", :popover-open")) return;
    } catch (_) {
      if (document.querySelector(busy)) return; // no :popover-open in this browser
    }
    if (target && target.closest(".rx-grid__editing")) return;
    dismiss(toasts[toasts.length - 1]);
  }, true);

  // Success and info leave after a while, longer for longer messages (or
  // after data-duration), and wait while hovered or focused; errors and
  // persistent toasts stay until dismissed.
  function arm(toast) {
    if (toast.hasAttribute("data-sticky") || toast.hasAttribute("data-armed")) return;
    toast.setAttribute("data-armed", "");
    var text = (toast.textContent || "").length;
    var delay = parseInt(toast.getAttribute("data-duration"), 10) || Math.min(10000, 4000 + text * 40);
    var timer;
    function start() { timer = setTimeout(function () { dismiss(toast); }, delay); }
    function stop() { clearTimeout(timer); }
    toast.addEventListener("mouseenter", stop);
    toast.addEventListener("mouseleave", start);
    toast.addEventListener("focusin", stop);
    toast.addEventListener("focusout", start);
    start();
  }

  // Links built from data: http(s), mailto, tel or relative only.
  function safeUrl(url) {
    var cleaned = String(url || "").replace(/[\u0000-\u0020\u007f]/g, "").toLowerCase();
    var colon = cleaned.indexOf(":");
    var path = cleaned.search(/[\/?#]/);
    if (colon < 0 || (path >= 0 && path < colon)) return true;
    return /^(https?|mailto|tel)$/.test(cleaned.slice(0, colon));
  }

  // A path on this site (not `//other.site`): where a request with the
  // CSRF token may go.
  function localUrl(url) {
    var cleaned = String(url || "").replace(/[\u0000-\u0020\u007f]/g, "");
    return cleaned.charAt(0) === "/" && cleaned.charAt(1) !== "/" && cleaned.charAt(1) !== "\\";
  }

  var METHODS = ["POST", "PUT", "PATCH", "DELETE"];

  function toastAction(action) {
    var el;
    var method = String(action.method || "").toUpperCase();
    if (action.method) {
      if (METHODS.indexOf(method) < 0 || !localUrl(action.url)) return null;
      el = document.createElement("button");
      el.type = "button";
      el.setAttribute("data-rx-request", action.url);
      el.setAttribute("data-rx-method", method);
    } else if (action.url && safeUrl(action.url)) {
      el = document.createElement("a");
      el.href = action.url;
      if (action.new_tab) { el.target = "_blank"; el.rel = "noopener"; }
    } else if (action.event) {
      el = document.createElement("button");
      el.type = "button";
      el.setAttribute("data-rx-toast-event", action.event);
    } else {
      return null;
    }
    el.className = "rx-toast__action";
    el.setAttribute("data-renox-dismiss", "");
    el.textContent = action.label;
    return el;
  }

  function failedLabel() {
    return region().getAttribute("data-failed-label") || "That didn't work. Try again.";
  }
  function hasToast(trigger) { return !!trigger && trigger.indexOf("renox:toast") >= 0; }

  // A request action that failed (sent from the toast region) and brought no
  // toast of its own: say so.
  document.addEventListener("htmx:afterRequest", function (event) {
    var elt = event.detail.elt;
    if (!elt || !elt.hasAttribute || !elt.hasAttribute("data-renox-toasts") || event.detail.successful) return;
    var xhr = event.detail.xhr;
    if (!hasToast(xhr && xhr.getResponseHeader("HX-Trigger"))) showToast({ kind: "error", message: failedLabel() });
  });

  // A request action (ToastAction::post …): sent with htmx, swapping
  // nothing, so the answer's HX-Trigger (its toasts), HX-Redirect or
  // HX-Refresh do the rest. The source is the toast region, which stays on
  // the page (the toast itself is gone by the time the answer comes).
  // A failure without a toast of its own shows an error toast.
  function sendRequest(method, url) {
    method = String(method || "POST").toUpperCase();
    if (METHODS.indexOf(method) < 0 || !localUrl(url)) return;
    var source = region();
    if (window.htmx) {
      window.htmx.ajax(method, url, { source: source, target: source, swap: "none" });
      return;
    }
    var failed = failedLabel();
    fetch(url, { method: method, credentials: "same-origin", headers: { "HX-Request": "true", "X-CSRF-Token": csrf() } }).then(function (res) {
      var redirect = res.headers.get("HX-Redirect");
      if (redirect) { window.location.href = redirect; return; }
      if (res.headers.get("HX-Refresh") === "true") { window.location.reload(); return; }
      var trigger = res.headers.get("HX-Trigger");
      if (trigger) {
        try {
          var events = JSON.parse(trigger);
          Object.keys(events).forEach(function (name) { document.dispatchEvent(new CustomEvent(name, { detail: events[name] })); });
        } catch (e) {
          trigger.split(",").forEach(function (name) { document.dispatchEvent(new CustomEvent(name.trim())); });
        }
      }
      if (!res.ok && !hasToast(trigger)) showToast({ kind: "error", message: failed });
    }, function () { showToast({ kind: "error", message: failed }); });
  }

  // {kind, message, body?, actions?: [{label, url | event, method?, new_tab?}],
  // duration? (ms, 0 = stays), id?}: the same shape as `Toast` in Rust.
  function showToast(toast) {
    var kind = ICONS[toast.kind] ? toast.kind : "info";
    if (toast.id) {
      document.querySelectorAll("[data-toast-id]").forEach(function (old) {
        if (old.getAttribute("data-toast-id") === String(toast.id)) old.remove();
      });
    }
    var el = document.createElement("div");
    el.className = "rx-toast rx-toast--" + kind;
    el.setAttribute("role", kind === "error" ? "alert" : "status");
    el.setAttribute("data-renox-toast", "");
    if (toast.id) el.setAttribute("data-toast-id", toast.id);
    if (toast.duration === 0 || (kind === "error" && toast.duration == null)) el.setAttribute("data-sticky", "");
    else if (toast.duration) el.setAttribute("data-duration", toast.duration);
    var icon = document.createElement("span");
    icon.className = "rx-toast__icon";
    icon.setAttribute("aria-hidden", "true");
    icon.innerHTML = ICONS[kind];
    var content = document.createElement("div");
    content.className = "rx-toast__content";
    var message = document.createElement("p");
    message.className = "rx-toast__message";
    message.textContent = toast.message;
    content.appendChild(message);
    if (toast.body) {
      var body = document.createElement("p");
      body.className = "rx-toast__body";
      body.textContent = toast.body;
      content.appendChild(body);
    }
    var actions = (toast.actions || []).map(toastAction).filter(Boolean);
    if (actions.length) {
      var row = document.createElement("div");
      row.className = "rx-toast__actions";
      actions.forEach(function (a) { row.appendChild(a); });
      content.appendChild(row);
    }
    var close = document.createElement("button");
    close.type = "button";
    close.className = "rx-toast__close";
    close.setAttribute("data-renox-dismiss", "");
    close.setAttribute("aria-label", region().getAttribute("data-dismiss-label") || "Dismiss");
    close.innerHTML = CLOSE;
    el.append(icon, content, close);
    region().appendChild(el);
    arm(el);
    return el;
  }

  document.addEventListener("renox:toast", function (event) {
    var detail = event.detail || {};
    (detail.toasts || []).forEach(showToast);
  });

  // The page's own script: Renox.toast({kind: "success", message: "Saved"}),
  // Renox.dismissToast("order-7").
  window.Renox = window.Renox || {};
  window.Renox.toast = showToast;
  // Renox.request("POST", "/orders/7/retry"): what a request action does.
  window.Renox.request = sendRequest;
  window.Renox.dismissToast = function (id) {
    document.querySelectorAll("[data-toast-id]").forEach(function (toast) {
      if (toast.getAttribute("data-toast-id") === String(id)) dismiss(toast);
    });
  };

  // ---------- The notifications stream ----------

  // One Server-Sent Events stream per page (`/notifications/stream`), shared
  // by the bell and `event_stream()`. The app's own events
  // (`state.broadcast(…)`) arrive as `broadcast` and are dispatched on
  // `document` under their own name, with their data as `detail`.
  // The stream closes on `pagehide` (a page kept in the back/forward cache
  // would hold its connection, and browsers allow six per host) and opens
  // again on `pageshow` with the same listeners. Listeners are added through
  // the object `openStream` returns, which keeps them for that reopening.
  var stream = null;
  var streamUrl = null;
  var streamListeners = [];
  var streamHandle = {
    addEventListener: function (name, listener) {
      streamListeners.push([name, listener]);
      if (stream) stream.addEventListener(name, listener);
    }
  };
  function connectStream() {
    stream = new EventSource(streamUrl);
    streamListeners.forEach(function (pair) { stream.addEventListener(pair[0], pair[1]); });
  }
  function openStream(url) {
    if (streamUrl) return streamHandle;
    if (!url || !window.EventSource) return null;
    streamUrl = url;
    streamHandle.addEventListener("broadcast", function (event) {
      var message;
      try { message = JSON.parse(event.data); } catch (e) { return; }
      if (!message || typeof message.event !== "string") return;
      document.dispatchEvent(new CustomEvent(message.event, { detail: message.data }));
    });
    connectStream();
    window.addEventListener("pagehide", function () {
      if (stream) stream.close();
      stream = null;
    });
    window.addEventListener("pageshow", function (event) {
      if (event.persisted && !stream) connectStream();
    });
    return streamHandle;
  }
  window.Renox.stream = function () { return stream; };

  // ---------- Notification bell ----------

  // The bell (notification_bell): the panel is the notifications page's
  // `panel` block, fetched when opened and after each action in it; a
  // Server-Sent Events stream keeps the badge current and shows new ones.
  function setupBell(bell) {
    if (bell._rxBell) return;
    bell._rxBell = true;
    var button = bell.querySelector(".rx-bell__button");
    var panel = document.getElementById(button.getAttribute("aria-controls"));
    var badge = bell.querySelector("[data-rx-bell-count]");
    if (!panel) return;

    function setCount(value) {
      var n = parseInt(value, 10) || 0;
      badge.textContent = n > 99 ? "99+" : String(n);
      badge.hidden = n === 0;
      var label = button.getAttribute("data-label");
      button.setAttribute("aria-label", n ? label + ", " + button.getAttribute("data-label-unread").split(":count").join(n) : label);
    }

    function load(url, options) {
      var init = { credentials: "same-origin", headers: { "HX-Request": "true", "X-CSRF-Token": csrf(), "Accept": "text/html" } };
      if (options) Object.keys(options).forEach(function (k) { init[k] = options[k]; });
      return fetch(url, init).then(function (res) {
        if (!res.ok) throw new Error(res.status);
        return res.text();
      }).then(function (html) {
        panel.innerHTML = html;
        var list = panel.querySelector("[data-rx-notifications]");
        if (list) setCount(list.getAttribute("data-unread"));
      });
    }

    function open() {
      panel.hidden = false;
      // The panel lists them: their toasts would only cover it.
      document.querySelectorAll('[data-toast-id^="rx-notification-"]').forEach(dismiss);
      // On phones the panel spans the screen, just under the bar.
      panel.style.top = window.matchMedia("(max-width: 36rem)").matches
        ? Math.round(button.getBoundingClientRect().bottom + 6) + "px" : "";
      button.setAttribute("aria-expanded", "true");
      load(bell.getAttribute("data-panel")).catch(function () {
        // Can't load it here: the page has the same list.
        window.location.href = button.href;
      });
    }

    function close(restore) {
      if (panel.hidden) return;
      panel.hidden = true;
      button.setAttribute("aria-expanded", "false");
      if (restore) button.focus();
    }

    button.addEventListener("click", function (event) {
      event.preventDefault();
      if (panel.hidden) open(); else close();
    });
    // Mark read or unread, delete, mark all read, clear: the panel again.
    panel.addEventListener("submit", function (event) {
      var form = event.target.closest("[data-rx-notification-form]");
      if (!form) return;
      event.preventDefault();
      load(form.action, { method: "POST", body: new URLSearchParams(new FormData(form)) }).then(function () {
        var first = panel.querySelector(".rx-notifications__header button, .rx-notification button");
        if (first && !panel.contains(document.activeElement)) first.focus();
      }, function () { form.submit(); });
    });
    document.addEventListener("click", function (event) {
      if (!bell.contains(event.target)) close();
    });
    bell.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && !panel.hidden) { event.stopPropagation(); close(true); }
    });

    var events = openStream(bell.getAttribute("data-stream"));
    if (!events) return;
    events.addEventListener("count", function (event) { setCount(event.data); });
    events.addEventListener("notification", function (event) {
      var n;
      try { n = JSON.parse(event.data); } catch (e) { return; }
      var actions = n.url ? [{ label: bell.getAttribute("data-open-label") || "Open", url: n.url }] : [];
      showToast({
        kind: n.status, message: n.title, body: n.body, id: "rx-notification-" + n.id,
        actions: actions.concat(n.actions || [])
      });
      if (!panel.hidden) load(bell.getAttribute("data-panel")).catch(function () {});
    });
  }

  // ---------- Sheets (dialogs) ----------

  function openSheet(id, opener) {
    var dialog = document.getElementById(id);
    if (!dialog || typeof dialog.showModal !== "function" || dialog.open) return;
    dialog._opener = opener;
    dialog.showModal();
    dialog._rxConfirmed = false;
    emit(dialog, "opened", { id: dialog.id });
    var first = dialog.querySelector("[autofocus]") || dialog.querySelector("[data-rx-initial-focus]") ||
      dialog.querySelector("form[data-rx-action] .rx-sheet__body :is(input:not([type=hidden]), select, textarea, button):not([disabled])");
    if (first) first.focus();
  }

  // An action sheet's form: a success closes the sheet (a 422 isn't one:
  // renox.js shows its errors in the form); closing it any way resets the
  // form and clears its errors, so the next open starts fresh.
  // An import's report goes away; parts loaded later add their own resets
  // (`Renox._kit.onClear`: a wizard starts over at its first step).
  var clearHooks = [];
  function clearActionForm(form) {
    form.reset();
    form.querySelectorAll("[data-renox-error]").forEach(function (el) { el.remove(); });
    form.querySelectorAll("[data-error-for]").forEach(function (el) { el.textContent = ""; });
    form.querySelectorAll("[aria-invalid]").forEach(function (el) { el.removeAttribute("aria-invalid"); });
    form.querySelectorAll("[data-rx-action-result]").forEach(function (el) { el.innerHTML = ""; });
    clearHooks.forEach(function (hook) { hook(form); });
  }
  document.addEventListener("htmx:afterRequest", function (event) {
    var form = event.target;
    if (!form.matches || !form.matches("form[data-rx-action]")) return;
    var dialog = form.closest("dialog");
    if (!event.detail.successful) { emit(dialog, "failed", { id: dialog && dialog.id, status: event.detail.xhr.status }); return; }
    emit(dialog, "saved", { id: dialog && dialog.id, status: event.detail.xhr.status });
    // An answer that asks to stay (an import's report of refused rows).
    if (form.querySelector("[data-rx-keep-open]")) return;
    if (dialog && dialog.open) dialog.close();
    else clearActionForm(form);
  });
  document.addEventListener("close", function (event) {
    var form = event.target.querySelector && event.target.querySelector("form[data-rx-action]");
    if (!form) return;
    // Some rows of an import went in: the page shows them after the sheet.
    var reload = form.querySelector("[data-rx-refresh-on-close]");
    clearActionForm(form);
    if (reload) location.reload();
  }, true);

  document.addEventListener("close", function (event) {
    var dialog = event.target;
    if (dialog && dialog._opener && dialog._opener.focus) dialog._opener.focus();
    if (dialog && dialog.matches && dialog.matches("dialog[data-rx-component]")) {
      if (dialog.getAttribute("data-rx-component") === "confirm" && !dialog._rxConfirmed) emit(dialog, "cancelled", { id: dialog.id });
      emit(dialog, "closed", { id: dialog.id });
    }
  }, true);

  // A confirm's form is a plain POST: this fires just before the navigation.
  document.addEventListener("submit", function (event) {
    var dialog = event.target.closest && event.target.closest('dialog[data-rx-component="confirm"]');
    if (!dialog) return;
    dialog._rxConfirmed = true;
    emit(dialog, "confirmed", { id: dialog.id });
  }, true);

  // A click on the backdrop (outside the sheet's box) closes it.
  document.addEventListener("click", function (event) {
    var dialog = event.target;
    if (!dialog || dialog.tagName !== "DIALOG" || !dialog.classList.contains("rx-sheet")) return;
    var box = dialog.getBoundingClientRect();
    var inside = event.clientX >= box.left && event.clientX <= box.right &&
      event.clientY >= box.top && event.clientY <= box.bottom;
    if (!inside) dialog.close();
  });

  // ---------- Menus ----------

  function menuItems(list) {
    return Array.prototype.slice.call(list.querySelectorAll('[role="menuitem"]'));
  }

  function openMenu(button, focusLast) {
    var list = document.getElementById(button.getAttribute("aria-controls"));
    if (!list) return;
    closeMenus(list);
    list.hidden = false;
    // Hangs from the button's right edge; on a narrow screen, never past
    // the left one.
    list.style.right = "";
    // (measured from the menu's box: the list's own grows in as it opens)
    var left = list.parentElement.getBoundingClientRect().right - list.offsetWidth;
    if (left < 8) list.style.right = (left - 8) + "px";
    button.setAttribute("aria-expanded", "true");
    var items = menuItems(list);
    var target = focusLast ? items[items.length - 1] : items[0];
    if (target) target.focus();
  }

  function closeMenus(except, restoreFocus) {
    document.querySelectorAll("[data-rx-menu] > [aria-expanded='true']").forEach(function (button) {
      var list = document.getElementById(button.getAttribute("aria-controls"));
      if (list === except) return;
      if (list) list.hidden = true;
      button.setAttribute("aria-expanded", "false");
      if (restoreFocus) button.focus();
    });
  }

  // ---------- Tabs (segmented control) ----------

  function selectTab(tab, focus) {
    var list = tab.closest('[role="tablist"]');
    if (!list) return;
    var was = tab.getAttribute("aria-selected") === "true";
    list.querySelectorAll('[role="tab"]').forEach(function (other) {
      var selected = other === tab;
      other.setAttribute("aria-selected", selected ? "true" : "false");
      other.tabIndex = selected ? 0 : -1;
      var panel = document.getElementById(other.getAttribute("aria-controls"));
      if (panel) panel.hidden = !selected;
    });
    if (!was) emit(list, "changed", { name: tab.getAttribute("data-rx-tab") });
    if (focus) tab.focus();
  }

  // The first input sent as `name`: "photos.1" (an item's error) falls back
  // to "photos". In `scope` (the summary's form) first, else on the page.
  function fieldNamed(name, scope) {
    var names = [name];
    if (name.indexOf(".") > 0) {
      var parts = name.split(".");
      names.push(parts[0] + parts.slice(1).map(function (p) { return "[" + p + "]"; }).join(""));
      names.push(parts[0]);
    }
    for (var i = 0; i < names.length; i++) {
      var selector = '[name="' + CSS.escape(names[i]) + '"]:not([type="hidden"])';
      var found = (scope && scope.querySelector(selector)) || document.querySelector(selector);
      if (found) return found;
    }
    return null;
  }

  // The first field a user fills in `root`.
  function firstField(root) {
    return root.querySelector("input:not([type=hidden]):not([disabled]), select:not([data-rx-enhanced]), textarea, [role=combobox]");
  }

  // ---------- Clicks ----------

  document.addEventListener("click", function (event) {
    var target = event.target.closest ? event.target : event.target.parentElement;
    if (!target) return;

    // The error summary's links go to the field by name, so a field with
    // its own `id` (or a radio group) is found too.
    var summaryLink = target.closest("[data-rx-error-summary] [data-rx-field]");
    if (summaryLink) {
      var field = fieldNamed(summaryLink.getAttribute("data-rx-field"), summaryLink.closest("form"));
      if (field) { event.preventDefault(); field.focus(); field.scrollIntoView({ block: "center", behavior: reduceMotion ? "auto" : "smooth" }); }
      return;
    }

    // A request action, in a toast or in the notification list.
    var requester = target.closest("[data-rx-request]");
    if (requester) {
      event.preventDefault();
      sendRequest(requester.getAttribute("data-rx-method"), requester.getAttribute("data-rx-request"));
    }

    var dismisser = target.closest("[data-renox-dismiss]");
    if (dismisser) {
      var toastEl = dismisser.closest("[data-renox-toast]");
      var eventName = dismisser.getAttribute("data-rx-toast-event");
      if (eventName) {
        document.dispatchEvent(new CustomEvent(eventName, { detail: { toast: toastEl && toastEl.getAttribute("data-toast-id") } }));
      }
      dismiss(toastEl);
      return;
    }

    var opener = target.closest("[data-rx-open]");
    if (opener) {
      event.preventDefault();
      var sheetId = opener.getAttribute("data-rx-open");
      // From a menu (an action group): the menu closes, and the sheet gives
      // the focus back to the menu's button.
      var owner = opener.closest("[data-rx-menu]");
      if (owner && opener.closest('[role="menu"]')) {
        closeMenus(null);
        opener = owner.querySelector("[aria-haspopup='menu']") || opener;
      }
      openSheet(sheetId, opener);
      return;
    }

    var closer = target.closest("[data-rx-close]");
    if (closer) { var d = closer.closest("dialog"); if (d) d.close(); return; }

    var menuButton = target.closest("[data-rx-menu] > [aria-haspopup]");
    if (menuButton) {
      if (menuButton.getAttribute("aria-expanded") === "true") closeMenus(null, true);
      else openMenu(menuButton);
      return;
    }
    if (!target.closest("[data-rx-menu]")) closeMenus(null);

    var tab = target.closest('[role="tab"]');
    if (tab) selectTab(tab, false);
  });

  // ---------- Keys ----------

  document.addEventListener("keydown", function (event) {
    var target = event.target;
    // Menus: arrows move, Esc closes and returns focus to the button.
    var list = target.closest && target.closest('[role="menu"]');
    var menuButton = target.closest && target.closest("[data-rx-menu] > [aria-haspopup]");
    if (menuButton && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault();
      openMenu(menuButton, event.key === "ArrowUp");
      return;
    }
    if (list) {
      var items = menuItems(list);
      var i = items.indexOf(target);
      if (event.key === "ArrowDown") { event.preventDefault(); items[(i + 1) % items.length].focus(); }
      else if (event.key === "ArrowUp") { event.preventDefault(); items[(i - 1 + items.length) % items.length].focus(); }
      else if (event.key === "Home") { event.preventDefault(); items[0].focus(); }
      else if (event.key === "End") { event.preventDefault(); items[items.length - 1].focus(); }
      else if (event.key === "Escape") { event.preventDefault(); closeMenus(null, true); }
      else if (event.key === "Tab") { closeMenus(null); }
      return;
    }
    if (event.key === "Escape") closeMenus(null, true);

    // Tabs: arrows, Home and End move the selection.
    if (target.getAttribute && target.getAttribute("role") === "tab") {
      var tabs = Array.prototype.slice.call(target.closest('[role="tablist"]').querySelectorAll('[role="tab"]'));
      var j = tabs.indexOf(target);
      var next = null;
      if (event.key === "ArrowRight") next = tabs[(j + 1) % tabs.length];
      else if (event.key === "ArrowLeft") next = tabs[(j - 1 + tabs.length) % tabs.length];
      else if (event.key === "Home") next = tabs[0];
      else if (event.key === "End") next = tabs[tabs.length - 1];
      if (next) { event.preventDefault(); selectTab(next, true); }
    }
  });

  // ---------- Busy buttons ----------

  // A regular form shows its submit button as busy and can't be sent twice.
  document.addEventListener("submit", function (event) {
    var form = event.target;
    if (event.defaultPrevented || form.hasAttribute("hx-post") || form.hasAttribute("hx-put") ||
        form.hasAttribute("hx-patch") || form.hasAttribute("hx-delete")) return;
    if (form.hasAttribute("data-rx-sending")) { event.preventDefault(); return; }
    form.setAttribute("data-rx-sending", "");
    var button = event.submitter || form.querySelector('button[type="submit"], button:not([type])');
    if (button && button.classList.contains("rx-button")) button.setAttribute("aria-busy", "true");
  });
  // Back/forward cache: a page shown again is ready again, with its sheets
  // and menus closed as after any other way back (closing a sheet resets its
  // action form, as Escape does).
  window.addEventListener("pageshow", function (event) {
    document.querySelectorAll("[data-rx-sending]").forEach(function (form) { form.removeAttribute("data-rx-sending"); });
    document.querySelectorAll('.rx-button[aria-busy="true"]').forEach(function (b) { b.removeAttribute("aria-busy"); });
    if (!event.persisted) return;
    document.querySelectorAll("dialog.rx-sheet[open]").forEach(function (dialog) { dialog.close(); });
    closeMenus(null);
  });
  document.addEventListener("htmx:beforeRequest", function (event) {
    var elt = event.detail && event.detail.elt;
    var button = elt && (elt.classList && elt.classList.contains("rx-button") ? elt : elt.querySelector && elt.querySelector('.rx-button[type="submit"]'));
    if (button) button.setAttribute("aria-busy", "true");
  });
  document.addEventListener("htmx:afterRequest", function (event) {
    var elt = event.detail && event.detail.elt;
    if (!elt || !elt.querySelectorAll) return;
    if (elt.removeAttribute) elt.removeAttribute("aria-busy");
    elt.querySelectorAll('.rx-button[aria-busy="true"]').forEach(function (b) { b.removeAttribute("aria-busy"); });
  });

  // ---------- Live validation ----------

  // Forms with data-live-validate check a field when it's left, then as it's
  // typed in while it has an error: quick to confirm a fix, never nagging
  // about a field that's half-typed. The handler doesn't run.
  var timers = new WeakMap();

  // `items[0][name]` as its errors are keyed: `items.0.name`.
  function errorKey(name) {
    return name.indexOf("[") < 0 ? name : name.replace(/\]/g, "").split(/[\[.]/).filter(Boolean).join(".");
  }

  function setFieldError(form, name, messages) {
    var slot = form.querySelector('[data-error-for="' + CSS.escape(errorKey(name)) + '"]');
    var inputs = form.querySelectorAll('[name="' + CSS.escape(name) + '"]');
    var message = (messages || [])[0] || "";
    if (slot) slot.textContent = message;
    inputs.forEach(function (input) {
      if (message) input.setAttribute("aria-invalid", "true");
      else input.removeAttribute("aria-invalid");
    });
  }

  // Resolves with the field's messages (none: valid), or null when it
  // can't be checked.
  function validate(form, input) {
    var name = input.name;
    if (!name || input.type === "file" || input.type === "password" && !input.value) return Promise.resolve(null);
    var body = new FormData(form);
    var method = (form.getAttribute("method") || "post").toUpperCase();
    if (method === "GET") return Promise.resolve(null);
    return fetch(form.getAttribute("action") || location.href, {
      method: "POST",
      body: body,
      credentials: "same-origin",
      headers: { "X-Renox-Validate": name, "X-CSRF-Token": csrf(), "Accept": "application/json" }
    })
      .then(function (res) { return res.ok ? res.json() : null; })
      .then(function (data) {
        if (!data || data.field !== name) return null;
        input.setAttribute("data-rx-checked", "");
        setFieldError(form, name, data.errors);
        return data.errors || [];
      })
      .catch(function () { return null; });
  }

  document.addEventListener("focusout", function (event) {
    var input = event.target;
    var form = input.form;
    if (!form || !form.hasAttribute("data-live-validate") || !input.name) return;
    if (!input.value && !input.hasAttribute("data-rx-checked")) return; // not touched yet
    validate(form, input);
  });

  document.addEventListener("input", function (event) {
    var input = event.target;
    var form = input.form;
    if (!form || !form.hasAttribute("data-live-validate") || input.getAttribute("aria-invalid") !== "true") return;
    clearTimeout(timers.get(input));
    timers.set(input, setTimeout(function () { validate(form, input); }, 400));
  });

  // ---------- Field buttons: show the password, copy ----------

  document.addEventListener("click", function (event) {
    var target = event.target.closest ? event.target : event.target.parentElement;
    if (!target) return;
    var reveal = target.closest("[data-rx-reveal]");
    if (reveal) {
      var field = document.getElementById(reveal.getAttribute("data-rx-reveal"));
      if (!field) return;
      var shown = field.type === "password";
      field.type = shown ? "text" : "password";
      reveal.setAttribute("aria-pressed", shown ? "true" : "false");
      // The label says what a press does next.
      var next = reveal.getAttribute("data-label-hide");
      reveal.setAttribute("data-label-hide", reveal.getAttribute("aria-label"));
      reveal.setAttribute("aria-label", next);
      return;
    }
    var copy = target.closest("[data-rx-copy]");
    if (copy) {
      // A field's value, or the text an infolist entry carries.
      var text = copy.getAttribute("data-rx-copy-text");
      if (text === null) {
        var source = document.getElementById(copy.getAttribute("data-rx-copy"));
        text = source ? source.value : null;
      }
      if (text === null || !navigator.clipboard) return;
      navigator.clipboard.writeText(text).then(function () {
        var said = copy.querySelector("[aria-live]");
        copy.setAttribute("data-rx-done", "");
        if (said) said.textContent = copy.getAttribute("data-label-done");
        setTimeout(function () {
          copy.removeAttribute("data-rx-done");
          if (said) said.textContent = "";
        }, 1500);
      }, function () {});
    }
  });

  // ---------- File fields ----------

  function fileSize(bytes) {
    if (bytes < 1024) return bytes + " B";
    if (bytes < 1024 * 1024) return Math.round(bytes / 1024) + " KB";
    return (bytes / 1024 / 1024).toFixed(1) + " MB";
  }

  function listFiles(input) {
    var box = input.closest("[data-rx-file]");
    var list = box && box.parentElement.querySelector("[data-rx-file-list]");
    if (!list) return;
    list.querySelectorAll("img").forEach(function (img) { URL.revokeObjectURL(img.src); });
    list.textContent = "";
    Array.prototype.forEach.call(input.files || [], function (file) {
      var item = document.createElement("li");
      item.className = "rx-file__item";
      if (box.hasAttribute("data-rx-preview") && /^image\//.test(file.type)) {
        var img = document.createElement("img");
        img.className = "rx-file__thumb";
        img.alt = "";
        img.src = URL.createObjectURL(file);
        item.appendChild(img);
      }
      var name = document.createElement("span");
      name.className = "rx-file__name";
      name.textContent = file.name;
      var size = document.createElement("span");
      size.className = "rx-file__size";
      size.textContent = fileSize(file.size);
      item.append(name, size);
      list.appendChild(item);
    });
  }

  document.addEventListener("change", function (event) {
    var input = event.target;
    if (input.classList && input.classList.contains("rx-file__input")) listFiles(input);
  });
  ["dragenter", "dragover"].forEach(function (type) {
    document.addEventListener(type, function (event) {
      var box = event.target.closest && event.target.closest("[data-rx-file]");
      if (box) box.setAttribute("data-rx-dragging", "");
    });
  });
  ["dragleave", "drop"].forEach(function (type) {
    document.addEventListener(type, function (event) {
      var box = event.target.closest && event.target.closest("[data-rx-file]");
      if (box) box.removeAttribute("data-rx-dragging");
    });
  });

  // ---------- The navbar's search on phones (nav_search) ----------

  // Its button opens the search on a row under the bar and focuses its
  // field; a second press, or Escape inside it, closes it and the focus goes
  // back to the button.
  function closeNavSearch(bar, toggle) {
    bar.classList.remove("rx-navbar--searching");
    toggle.setAttribute("aria-expanded", "false");
    toggle.focus();
  }
  document.addEventListener("click", function (event) {
    var toggle = event.target.closest && event.target.closest("[data-rx-search-toggle]");
    var bar = toggle && toggle.closest(".rx-navbar");
    if (!bar) return;
    if (bar.classList.contains("rx-navbar--searching")) { closeNavSearch(bar, toggle); return; }
    bar.classList.add("rx-navbar--searching");
    toggle.setAttribute("aria-expanded", "true");
    var region = document.getElementById(toggle.getAttribute("aria-controls"));
    var field = region && region.querySelector("input:not([type=hidden]), select, textarea");
    if (field) field.focus();
  });
  // On `window`, after the page's own handlers: a suggestion list that
  // closes on Escape (and says so with preventDefault) keeps the search open.
  window.addEventListener("keydown", function (event) {
    if (event.key !== "Escape" || event.defaultPrevented || !event.target.closest) return;
    var region = event.target.closest(".rx-navbar--searching .rx-navbar__search");
    var bar = region && region.closest(".rx-navbar");
    var toggle = bar && bar.querySelector('[data-rx-search-toggle][aria-controls="' + CSS.escape(region.id) + '"]');
    if (toggle) closeNavSearch(bar, toggle);
  });

  // ---------- Date picker ----------

  var ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;

  function today() {
    var d = new Date();
    return d.getFullYear() + "-" + String(d.getMonth() + 1).padStart(2, "0") + "-" + String(d.getDate()).padStart(2, "0");
  }

  // The header names the month shown, in the page's language.
  function calendarHeading(cal, iso) {
    var slot = cal.querySelector(".rx-calendar__heading");
    if (!slot) return;
    // A YYYY-MM-DD string, or the Date Cally's focusday event carries.
    var day = new Date();
    if (iso instanceof Date) day = iso;
    else if (typeof iso === "string" && ISO_DATE.test(iso)) day = new Date(+iso.slice(0, 4), +iso.slice(5, 7) - 1, 1);
    try {
      slot.textContent = new Intl.DateTimeFormat(cal.getAttribute("locale") || undefined, { month: "long", year: "numeric" }).format(day);
    } catch (e) {
      slot.textContent = day.getFullYear() + "-" + String(day.getMonth() + 1).padStart(2, "0");
    }
  }
  // Days that can't be chosen (`disabled_dates`, `closed_weekdays`): a test
  // per field, read once from its data-rx-* attributes. Null: every day is open.
  var closedTests = new WeakMap();
  function closedTest(input) {
    if (!input || !input.hasAttribute("data-rx-disabled-dates")) return null;
    if (closedTests.has(input)) return closedTests.get(input);
    var dates = {}, weekdays = [];
    try {
      JSON.parse(input.getAttribute("data-rx-disabled-dates") || "[]").forEach(function (d) { dates[d] = true; });
      weekdays = JSON.parse(input.getAttribute("data-rx-closed-weekdays") || "[]");
    } catch (e) { /* a broken list closes nothing */ }
    var test = function (iso) {
      return !!dates[iso] || weekdays.indexOf(new Date(iso + "T00:00:00Z").getUTCDay()) >= 0;
    };
    closedTests.set(input, test);
    return test;
  }

  function isoOf(date) { return date.toISOString().slice(0, 10); }
  function addDays(iso, n) {
    var d = new Date(iso + "T00:00:00Z");
    d.setUTCDate(d.getUTCDate() + n);
    return isoOf(d);
  }
  function inRange(cal, iso) {
    var min = cal.getAttribute("min"), max = cal.getAttribute("max");
    return (!min || iso >= min) && (!max || iso <= max);
  }
  // The first open day from `iso` going `step` days at a time (a year at
  // most), then one day at a time (a week down onto a closed weekday finds
  // the next open day), else null.
  function openDay(cal, closed, iso, step) {
    var tries = [step, step > 0 ? 1 : -1];
    for (var t = 0; t < tries.length; t++) {
      var day = iso;
      for (var i = 0; i < 400 && inRange(cal, day); i++) {
        if (!closed(day)) return day;
        day = addDays(day, tries[t]);
      }
    }
    return null;
  }
  function calendarInput(cal) {
    var pop = cal.closest && cal.closest("[data-rx-calendar-for]");
    return pop ? document.getElementById(pop.getAttribute("data-rx-calendar-for")) : null;
  }
  function focusDay(cal, iso) {
    cal.focusedDate = iso;
    calendarHeading(cal, iso);
    setTimeout(function () { if (cal.focus) cal.focus(); });
  }

  // Cally's day is a UTC date; it greys out and won't pick what this refuses.
  function setupDatePicker(input) {
    var closed = closedTest(input);
    var pop = closed && document.getElementById(input.id + "-calendar");
    var cal = pop && pop.querySelector("calendar-date");
    if (cal && window.customElements) {
      customElements.whenDefined("calendar-date").then(function () {
        cal.isDateDisallowed = function (date) { return closed(isoOf(date)); };
      });
    }
    if (closed) checkDate(input);
  }

  // A closed day typed in: the field's error says so, and the browser
  // won't send the form. Only clears the error it set itself.
  var ownDateError = new WeakSet();
  function checkDate(input) {
    var closed = closedTest(input);
    if (!closed) return;
    var value = input.value.trim();
    var slot = document.getElementById(input.id + "-error");
    var message = input.getAttribute("data-rx-unavailable") || "";
    if (ISO_DATE.test(value) && closed(value)) {
      input.setCustomValidity(message);
      input.setAttribute("aria-invalid", "true");
      if (slot) slot.textContent = message;
      ownDateError.add(input);
    } else if (ownDateError.has(input)) {
      input.setCustomValidity("");
      input.removeAttribute("aria-invalid");
      if (slot) slot.textContent = "";
      ownDateError.delete(input);
    }
  }
  ["input", "change"].forEach(function (type) {
    document.addEventListener(type, function (event) {
      var input = event.target;
      if (input.hasAttribute && input.hasAttribute("data-rx-disabled-dates")) checkDate(input);
    });
  });

  // The arrow keys step over closed days: Cally moves the focus one day (or
  // week) and this moves it on, the same way, to the next open day.
  var lastStep = new WeakMap();
  var STEPS = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: 7, ArrowUp: -7, PageDown: 1, PageUp: -1, Home: 1, End: -1 };
  document.addEventListener("keydown", function (event) {
    // The key comes from inside <calendar-month>, in its <calendar-date>.
    var cal = event.target.closest && event.target.closest("calendar-date.rx-calendar");
    if (!cal || !(event.key in STEPS)) return;
    var step = STEPS[event.key];
    if ((event.key === "ArrowRight" || event.key === "ArrowLeft") && getComputedStyle(cal).direction === "rtl") step = -step;
    lastStep.set(cal, { step: step, from: cal.focusedDate ? String(cal.focusedDate) : null });
  }, true);

  // Moving the focus (arrows, the month buttons) changes the month shown.
  document.addEventListener("focusday", function (event) {
    var cal = event.target;
    if (!cal.classList || !cal.classList.contains("rx-calendar")) return;
    var closed = closedTest(calendarInput(cal));
    var moved = lastStep.get(cal);
    lastStep.delete(cal);
    if (closed && moved && event.detail instanceof Date && closed(isoOf(event.detail))) {
      var next = openDay(cal, closed, isoOf(event.detail), moved.step);
      if (next || moved.from) { focusDay(cal, next || moved.from); return; }
    }
    calendarHeading(cal, event.detail);
  }, true);

  // The calendar opens under its field (above it when there's no room),
  // showing the field's date, and focuses that day.
  document.addEventListener("toggle", function (event) {
    var pop = event.target;
    if (!pop.hasAttribute || !pop.hasAttribute("data-rx-calendar-for") || event.newState !== "open") return;
    var input = document.getElementById(pop.getAttribute("data-rx-calendar-for"));
    var cal = pop.querySelector("calendar-date");
    if (!input || !cal) return;
    if (ISO_DATE.test(input.value)) { cal.value = input.value; cal.focusedDate = input.value; }
    var start = ISO_DATE.test(input.value) ? input.value : cal.getAttribute("min") > today() ? cal.getAttribute("min") : null;
    // Opened on a closed day (none chosen yet): the focus starts on the next open one.
    var closed = closedTest(input);
    if (closed && !ISO_DATE.test(input.value)) {
      var open = openDay(cal, closed, start || today(), 1);
      if (open) { cal.focusedDate = open; start = open; }
    }
    calendarHeading(cal, start);
    var box = (input.closest(".rx-affix") || input).getBoundingClientRect();
    var height = pop.offsetHeight, width = pop.offsetWidth;
    var top = box.bottom + 6;
    if (top + height > window.innerHeight - 8 && box.top - height - 6 > 8) top = box.top - height - 6;
    var left = Math.min(box.right - width, window.innerWidth - width - 8);
    pop.style.top = Math.max(8, top) + "px";
    pop.style.left = Math.max(8, left) + "px";
    requestAnimationFrame(function () { if (cal.focus) cal.focus(); });
  }, true);

  document.addEventListener("change", function (event) {
    var el = event.target;
    if (el.tagName === "CALENDAR-DATE" && el.closest("[data-rx-calendar-for]")) {
      var pop = el.closest("[data-rx-calendar-for]");
      var input = document.getElementById(pop.getAttribute("data-rx-calendar-for"));
      if (!input) return;
      input.value = el.value;
      if (pop.hidePopover) pop.hidePopover();
      input.focus();
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new Event("change", { bubbles: true }));
    }
  }, true); // Cally's change event doesn't bubble: catch it on the way down.

  // #424: a field says `rx:<component>:changed` with its value, after the
  // native change (this listener bubbles; the date picker's runs first, in
  // the capture phase, and re-dispatches a change on its input).
  document.addEventListener("change", function (event) {
    var el = event.target;
    if (!el || !el.matches) return;
    var value;
    if (el.matches('select[data-rx-component="select"]')) {
      value = el.multiple ? Array.from(el.selectedOptions, function (o) { return o.value; }) : el.value;
    } else if (el.matches('[data-rx-component="tags-input"]')) {
      var box = el.closest("[data-rx-tags]");
      if (!box) return;
      value = Array.from(box.querySelectorAll('.rx-tag input[type="hidden"]'), function (i) { return i.value; });
      var json = JSON.stringify(value);
      if (json === el._rxLast) return;
      el._rxLast = json;
    } else if (el.matches('[data-rx-component="date-picker"]')) {
      value = el.value;
    } else {
      return;
    }
    emit(el, "changed", { value: value });
  });

  // ---------- show_when / hide_when ----------

  // The values `name` has in `form` now: the ticked radio or checkboxes, a
  // select's choices, or a field's text.
  function valuesOf(form, name) {
    var values = [];
    (form ? form.querySelectorAll('[name="' + CSS.escape(name) + '"]') : []).forEach(function (el) {
      if (el.disabled) return;
      if (el.type === "radio" || el.type === "checkbox") { if (el.checked) values.push(el.value); }
      else if (el.tagName === "SELECT") Array.prototype.forEach.call(el.selectedOptions, function (o) { values.push(o.value); });
      else values.push(el.value);
    });
    return values;
  }

  function applyWhen(group) {
    var show = group.hasAttribute("data-rx-show-when");
    var name = group.getAttribute(show ? "data-rx-show-when" : "data-rx-hide-when");
    var wanted;
    try { wanted = JSON.parse(group.getAttribute("data-rx-values") || "[]"); } catch (e) { wanted = []; }
    var matches = valuesOf(group.form || group.closest("form"), name).some(function (v) { return wanted.indexOf(v) >= 0; });
    var visible = show ? matches : !matches;
    // Hidden fields are disabled too, so the form doesn't send them.
    group.hidden = !visible;
    group.disabled = !visible;
  }

  function applyAllWhen(root) {
    (root || document).querySelectorAll("[data-rx-show-when], [data-rx-hide-when]").forEach(applyWhen);
  }

  ["change", "input"].forEach(function (type) {
    document.addEventListener(type, function (event) {
      var el = event.target;
      if (!el.name || !el.form) return;
      el.form.querySelectorAll("[data-rx-show-when], [data-rx-hide-when]").forEach(function (group) {
        var name = group.getAttribute("data-rx-show-when") || group.getAttribute("data-rx-hide-when");
        if (name === el.name) applyWhen(group);
      });
    });
  });

  // ---------- Searchable select (combobox) ----------

  var comboCount = 0;
  var EDIT = '<svg viewBox="0 0 20 20" width="14" height="14" aria-hidden="true"><path d="M4 13.5V16h2.5l7.4-7.4-2.5-2.5L4 13.5zM15.7 6.8a.7.7 0 000-1L14.2 4.3a.7.7 0 00-1 0l-1.2 1.2 2.5 2.5 1.2-1.2z" fill="currentColor"/></svg>';

  function fill(text, value) {
    return (text || "").split(":value").join(value);
  }

  // A form-encoded request to the options URL, with the CSRF token; resolves
  // with the status and the JSON answer (or null).
  function optionsRequest(url, method, fields) {
    var body = new URLSearchParams();
    Object.keys(fields).forEach(function (k) { body.append(k, fields[k]); });
    if (method !== "POST") body.append("_method", method);
    return fetch(url, {
      method: "POST",
      body: body,
      credentials: "same-origin",
      headers: { "X-CSRF-Token": csrf(), "Accept": "application/json", "X-Requested-With": "XMLHttpRequest" }
    }).then(function (res) {
      return res.json().catch(function () { return null; }).then(function (data) { return { status: res.status, data: data }; });
    });
  }

  function firstMessage(data) {
    if (!data) return "";
    var errors = data.errors || {};
    var key = Object.keys(errors)[0];
    return (key && errors[key] && errors[key][0]) || data.message || "";
  }

  function enhanceSelect(select) {
    if (select.hasAttribute("data-rx-enhanced")) return;
    select.setAttribute("data-rx-enhanced", "");
    select.setAttribute("tabindex", "-1");
    select.setAttribute("aria-hidden", "true");
    var multiple = select.multiple;
    var url = select.getAttribute("data-rx-options-url");
    var editable = !!url && select.hasAttribute("data-rx-editable");
    var base = select.id || "rx-combobox-" + (++comboCount);
    var wrap = document.createElement("div");
    wrap.className = "rx-combobox";
    select.parentNode.insertBefore(wrap, select);
    var box = document.createElement("div");
    box.className = "rx-combobox__box" + (multiple ? " rx-tags" : "");
    var input = document.createElement("input");
    input.type = "text";
    input.id = base + "-search";
    input.className = multiple ? "rx-tags__entry" : "rx-input rx-select";
    input.autocomplete = "off";
    input.setAttribute("role", "combobox");
    input.setAttribute("aria-autocomplete", "list");
    input.setAttribute("aria-expanded", "false");
    input.setAttribute("aria-controls", base + "-listbox");
    ["aria-describedby", "aria-invalid", "aria-required"].forEach(function (a) { if (select.hasAttribute(a)) input.setAttribute(a, select.getAttribute(a)); });
    var placeholder = select.getAttribute("data-placeholder") || "";
    input.placeholder = placeholder;
    input.disabled = select.disabled;
    var chips = document.createElement("ul");
    chips.className = "rx-tags__list";
    chips.setAttribute("role", "list");
    if (multiple) box.append(chips);
    box.append(input);
    // Single + editable: a button to rename the chosen option.
    var editButton = null;
    if (editable && !multiple) {
      editButton = document.createElement("button");
      editButton.type = "button";
      editButton.className = "rx-combobox__edit";
      editButton.innerHTML = EDIT;
      editButton.addEventListener("click", function () {
        var chosen = select.options[select.selectedIndex];
        if (chosen && chosen.value !== "") startEdit(chosen);
      });
      box.append(editButton);
      box.classList.add("rx-combobox__box--editable");
    }
    var list = document.createElement("ul");
    list.className = "rx-combobox__list";
    list.id = base + "-listbox";
    list.setAttribute("role", "listbox");
    if (multiple) list.setAttribute("aria-multiselectable", "true");
    list.hidden = true;
    // What's going on (searching, editing, an error), read out politely.
    var status = document.createElement("p");
    status.className = "rx-combobox__status";
    status.id = base + "-status";
    status.setAttribute("aria-live", "polite");
    wrap.append(box, list, status, select);
    input.setAttribute("aria-describedby", ((input.getAttribute("aria-describedby") || "") + " " + status.id).trim());
    var label = select.id && document.querySelector('label[for="' + CSS.escape(select.id) + '"]');
    if (label) { label.htmlFor = input.id; list.setAttribute("aria-labelledby", label.id || (label.id = base + "-label")); }

    // Local: the select's own options (a single select's empty "None" stays
    // choosable, to clear it). Remote: what the server last answered.
    var results = null;
    var loading = false, failed = false;
    var editing = null;
    var timer = null, controller = null;

    function localItems() {
      return Array.prototype.filter.call(select.options, function (o) { return o.value !== "" || !multiple; })
        .map(function (o) { return { value: o.value, label: o.textContent }; });
    }

    function optionFor(value) {
      return Array.prototype.find.call(select.options, function (o) { return o.value === value; });
    }

    // The native option for a value, made when the server's list had it.
    function ensureOption(value, text) {
      var option = optionFor(value);
      if (!option) {
        option = new Option(text, value);
        select.appendChild(option);
      } else if (text) {
        option.textContent = text;
        option.removeAttribute("data-rx-unresolved");
      }
      return option;
    }

    function isSelected(value) {
      var option = optionFor(value);
      return !!option && option.selected && (value !== "" || !multiple);
    }

    function selectedText() {
      var chosen = select.options[select.selectedIndex];
      return chosen && chosen.value !== "" ? chosen.textContent : "";
    }

    function say(text, alert) {
      status.textContent = text || "";
      status.classList.toggle("rx-combobox__status--error", !!alert);
    }

    function renderChips() {
      if (editButton) {
        var chosen = select.options[select.selectedIndex];
        var has = !!chosen && chosen.value !== "";
        editButton.hidden = !has;
        editButton.disabled = select.disabled;
        if (has) editButton.setAttribute("aria-label", (select.getAttribute("data-edit") || "Edit") + " " + chosen.textContent);
      }
      if (!multiple) { if (!editing) input.value = selectedText(); return; }
      chips.textContent = "";
      Array.prototype.filter.call(select.options, function (o) { return o.selected && o.value !== ""; }).forEach(function (o) {
        var li = document.createElement("li");
        li.className = "rx-tag";
        var text = document.createElement("span");
        text.className = "rx-tag__text";
        text.textContent = o.textContent;
        li.append(text);
        if (editable) {
          var edit = document.createElement("button");
          edit.type = "button";
          edit.className = "rx-tag__remove";
          edit.setAttribute("aria-label", (select.getAttribute("data-edit") || "Edit") + " " + o.textContent);
          edit.innerHTML = EDIT;
          edit.disabled = select.disabled;
          edit.addEventListener("click", function () { startEdit(o); });
          li.append(edit);
        }
        var remove = document.createElement("button");
        remove.type = "button";
        remove.className = "rx-tag__remove";
        remove.setAttribute("aria-label", (select.getAttribute("data-remove") || "Remove") + " " + o.textContent);
        remove.innerHTML = CLOSE;
        remove.disabled = select.disabled;
        remove.addEventListener("click", function () { o.selected = false; changed(); input.focus(); });
        li.append(remove);
        chips.appendChild(li);
      });
    }

    function addRow(text, className) {
      var li = document.createElement("li");
      li.className = className;
      li.setAttribute("role", "presentation");
      li.textContent = text;
      list.appendChild(li);
    }

    function optionRow(item, i) {
      var li = document.createElement("li");
      li.className = "rx-combobox__option" + (item.create ? " rx-combobox__option--create" : "");
      li.id = base + "-option-" + i;
      li.setAttribute("role", "option");
      li.setAttribute("aria-selected", !item.create && isSelected(item.value) ? "true" : "false");
      li.textContent = item.create ? fill(select.getAttribute("data-add") || "Add “:value”", item.create) : item.label;
      li.addEventListener("mousedown", function (e) { e.preventDefault(); });
      li.addEventListener("click", function () { choose(item); });
      li._item = item;
      list.appendChild(li);
    }

    function render(filter) {
      var needle = (filter || "").trim().toLowerCase();
      list.textContent = "";
      var items = url ? (results || []) : localItems().filter(function (it) {
        return !needle || it.label.toLowerCase().indexOf(needle) >= 0;
      });
      if (url && loading && !results) addRow(select.getAttribute("data-searching") || "Searching…", "rx-combobox__empty");
      else if (url && failed) addRow(select.getAttribute("data-load-failed") || "Couldn't load the options.", "rx-combobox__empty");
      items.forEach(optionRow);
      // "Add “…”" when what was typed isn't an option yet.
      var typed = (filter || "").trim();
      var exact = items.some(function (it) { return it.label.trim().toLowerCase() === typed.toLowerCase(); });
      if (editable && typed && !exact && !loading) optionRow({ create: typed }, "new");
      if (!list.querySelector("[role=option]") && !(url && (loading || failed))) {
        addRow(select.getAttribute("data-empty") || "No matches", "rx-combobox__empty");
      }
      activate(list.querySelector('[aria-selected="true"]') || list.querySelector("[role=option]"));
    }

    function search(q) {
      if (controller) controller.abort();
      controller = window.AbortController ? new AbortController() : null;
      loading = true;
      failed = false;
      if (!list.hidden) render(q);
      var sep = url.indexOf("?") < 0 ? "?" : "&";
      fetch(url + sep + "q=" + encodeURIComponent(q.trim()), {
        credentials: "same-origin",
        headers: { "Accept": "application/json", "X-Requested-With": "XMLHttpRequest" },
        signal: controller ? controller.signal : undefined
      })
        .then(function (res) { if (!res.ok) throw new Error(res.status); return res.json(); })
        .then(function (data) {
          results = (Array.isArray(data) ? data : (data && data.options) || []).map(function (o) {
            return { value: String(o.value), label: String(o.label) };
          });
          loading = false;
          if (!list.hidden) render(q);
        })
        .catch(function (err) {
          if (err && err.name === "AbortError") return;
          loading = false;
          failed = true;
          if (!list.hidden) render(q);
        });
    }

    function activate(li) {
      list.querySelectorAll("[data-active]").forEach(function (el) { el.removeAttribute("data-active"); });
      if (!li) { input.removeAttribute("aria-activedescendant"); return; }
      li.setAttribute("data-active", "");
      input.setAttribute("aria-activedescendant", li.id);
      li.scrollIntoView({ block: "nearest" });
    }

    function open() {
      if (input.disabled || editing || !list.hidden) return;
      list.hidden = false;
      input.setAttribute("aria-expanded", "true");
      var q = multiple ? input.value : "";
      if (url) search(q);
      render(q);
    }

    function close() {
      list.hidden = true;
      input.setAttribute("aria-expanded", "false");
      input.removeAttribute("aria-activedescendant");
      if (!multiple && !editing) input.value = selectedText();
    }

    function changed() {
      renderChips();
      if (!list.hidden) render(multiple ? input.value : "");
      select.dispatchEvent(new Event("change", { bubbles: true }));
    }

    function pick(option) {
      if (multiple) {
        option.selected = !option.selected;
        input.value = "";
        if (url) results = null;
        changed();
        if (url && !list.hidden) search("");
      } else {
        select.value = option.value;
        changed();
        close();
      }
    }

    // A new option from what was typed: saved on the server, then chosen.
    function create(text) {
      input.setAttribute("aria-busy", "true");
      say("");
      optionsRequest(url, "POST", { label: text }).then(function (res) {
        input.removeAttribute("aria-busy");
        if (res.status >= 200 && res.status < 300 && res.data && res.data.value !== undefined) {
          var option = ensureOption(String(res.data.value), String(res.data.label));
          if (multiple) option.selected = false;
          pick(option);
          results = null;
        } else {
          say(firstMessage(res.data) || select.getAttribute("data-save-failed") || "Couldn't save it.", true);
        }
      }).catch(function () {
        input.removeAttribute("aria-busy");
        say(select.getAttribute("data-save-failed") || "Couldn't save it.", true);
      });
    }

    function choose(item) {
      if (item.create) { create(item.create); return; }
      pick(ensureOption(item.value, item.label));
    }

    // Renaming the chosen option: the box holds its name until Enter saves
    // it or Escape gives up.
    function startEdit(option) {
      close();
      editing = option;
      wrap.setAttribute("data-rx-editing", "");
      input.value = option.textContent;
      say(fill(select.getAttribute("data-editing") || "Editing “:value”: Enter saves, Esc cancels.", option.textContent));
      input.focus();
      input.select();
    }

    function stopEdit() {
      editing = null;
      wrap.removeAttribute("data-rx-editing");
      input.value = multiple ? "" : selectedText();
    }

    function saveEdit() {
      var option = editing;
      var text = input.value.trim();
      if (!text || text === option.textContent) { stopEdit(); say(""); return; }
      input.setAttribute("aria-busy", "true");
      optionsRequest(url, "PUT", { value: option.value, label: text }).then(function (res) {
        input.removeAttribute("aria-busy");
        if (res.status >= 200 && res.status < 300) {
          option.textContent = res.data && res.data.label !== undefined ? String(res.data.label) : text;
          results = null;
          stopEdit();
          say("");
          renderChips();
          select.dispatchEvent(new Event("change", { bubbles: true }));
        } else {
          say(firstMessage(res.data) || select.getAttribute("data-save-failed") || "Couldn't save it.", true);
        }
      }).catch(function () {
        input.removeAttribute("aria-busy");
        say(select.getAttribute("data-save-failed") || "Couldn't save it.", true);
      });
    }

    input.addEventListener("focus", function () { if (!editing) open(); });
    input.addEventListener("click", function () { if (!editing) open(); });
    input.addEventListener("input", function () {
      if (editing) return;
      if (list.hidden) open();
      if (url) {
        clearTimeout(timer);
        var q = input.value;
        timer = setTimeout(function () { search(q); }, 250);
        render(q);
      } else {
        render(input.value);
      }
    });
    input.addEventListener("blur", function () {
      setTimeout(function () {
        if (editing && document.activeElement !== input) { stopEdit(); say(""); }
        close();
      }, 0);
    });
    input.addEventListener("keydown", function (e) {
      if (editing) {
        if (e.key === "Enter") { e.preventDefault(); saveEdit(); }
        else if (e.key === "Escape") { e.preventDefault(); stopEdit(); say(""); }
        return;
      }
      var items = Array.prototype.slice.call(list.querySelectorAll("[role=option]"));
      var current = list.querySelector("[data-active]");
      var i = items.indexOf(current);
      if (e.key === "ArrowDown") { e.preventDefault(); if (list.hidden) open(); else activate(items[Math.min(i + 1, items.length - 1)]); }
      else if (e.key === "ArrowUp") { e.preventDefault(); activate(items[Math.max(i - 1, 0)]); }
      else if (e.key === "Home" && !list.hidden) { e.preventDefault(); activate(items[0]); }
      else if (e.key === "End" && !list.hidden) { e.preventDefault(); activate(items[items.length - 1]); }
      else if (e.key === "Enter") { if (!list.hidden && current) { e.preventDefault(); choose(current._item); } }
      else if (e.key === "Escape") { if (!list.hidden) { e.preventDefault(); close(); } }
      else if (e.key === "Backspace" && multiple && !input.value) {
        var last = Array.prototype.filter.call(select.options, function (o) { return o.selected && o.value !== ""; }).pop();
        if (last) { last.selected = false; changed(); }
      }
    });
    box.addEventListener("click", function (e) { if (e.target === box) input.focus(); });
    // The browser checks the hidden select for `required`: point at the box.
    // After the `invalid` event the browser focuses the select itself (1px,
    // see-through): send that focus on to the box too.
    select.addEventListener("invalid", function () { input.focus(); });
    select.addEventListener("focus", function () { input.focus(); });
    select.addEventListener("rx:refresh", renderChips);

    // Values sent back without a label (after a failed submit): ask for them.
    var unresolved = Array.prototype.filter.call(select.options, function (o) { return o.hasAttribute("data-rx-unresolved"); });
    if (url && unresolved.length) {
      var sep = url.indexOf("?") < 0 ? "?" : "&";
      var query = unresolved.map(function (o) { return "values=" + encodeURIComponent(o.value); }).join("&");
      fetch(url + sep + query, { credentials: "same-origin", headers: { "Accept": "application/json" } })
        .then(function (res) { return res.ok ? res.json() : []; })
        .then(function (data) {
          (Array.isArray(data) ? data : (data && data.options) || []).forEach(function (o) { ensureOption(String(o.value), String(o.label)); });
          renderChips();
        })
        .catch(function () {});
    }
    renderChips();
  }

  // ---------- Disabled with a reason, keyboard shortcuts ----------

  // `aria-disabled` keeps a button focusable (its reason shows as a
  // tooltip) but nothing it would do happens: this runs before htmx's and
  // the kit's own click handlers.
  document.addEventListener("click", function (event) {
    var target = event.target.closest ? event.target : event.target.parentElement;
    var off = target && target.closest('[aria-disabled="true"]');
    if (off && off.matches("button, a, [role='button'], [data-rx-open]")) {
      event.preventDefault();
      event.stopImmediatePropagation();
      // A tap shows why (touch screens have no hover).
      if (off.hasAttribute("data-rx-tip")) showTip(off);
    }
  }, true);

  // Tooltips: after a short hover, at once on keyboard focus; above the
  // element (below when there's no room), kept inside the window, in the
  // open sheet when the element is in one (dialogs sit on the top layer).
  var tip = null, tipFor = null, tipTimer = null;
  var canHover = window.matchMedia && window.matchMedia("(hover: hover)").matches;

  function showTip(el) {
    clearTimeout(tipTimer);
    hideTip();
    var text = el.getAttribute("data-rx-tip");
    if (!text || !el.isConnected) return;
    tip = document.createElement("div");
    tip.className = "rx-tip";
    tip.id = "rx-tip";
    tip.setAttribute("role", "tooltip");
    tip.textContent = text;
    (el.closest("dialog[open]") || document.body).appendChild(tip);
    var box = el.getBoundingClientRect();
    var width = tip.offsetWidth, height = tip.offsetHeight;
    var top = box.top - height - 6;
    if (top < 4) top = box.bottom + 6;
    var left = Math.min(Math.max(box.left + box.width / 2 - width / 2, 4), window.innerWidth - width - 4);
    tip.style.top = top + "px";
    tip.style.left = left + "px";
    tipFor = el;
    // Said by screen readers when it isn't already the element's name.
    if (text !== el.getAttribute("aria-label")) el.setAttribute("aria-describedby", "rx-tip");
  }

  function hideTip() {
    clearTimeout(tipTimer);
    if (tipFor && tipFor.getAttribute("aria-describedby") === "rx-tip") tipFor.removeAttribute("aria-describedby");
    if (tip) tip.remove();
    tip = null;
    tipFor = null;
  }

  document.addEventListener("mouseover", function (event) {
    if (!canHover) return;
    var el = event.target.closest && event.target.closest("[data-rx-tip]");
    if (el === tipFor) return;
    hideTip();
    if (el) tipTimer = setTimeout(function () { showTip(el); }, 400);
  });
  document.addEventListener("mouseout", function (event) {
    var el = event.target.closest && event.target.closest("[data-rx-tip]");
    if (el && !(event.relatedTarget && el.contains(event.relatedTarget))) { if (el === tipFor || !tip) hideTip(); }
  });
  document.addEventListener("focusin", function (event) {
    var el = event.target.closest && event.target.closest("[data-rx-tip]");
    if (el && el.matches(":focus-visible")) showTip(el);
  });
  document.addEventListener("focusout", function (event) {
    if (tipFor && tipFor.contains(event.target)) hideTip();
  });
  document.addEventListener("keydown", function (event) { if (event.key === "Escape" && tip) hideTip(); }, true);
  window.addEventListener("scroll", function () { if (tip) hideTip(); }, true);

  var isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || "");
  var KEY_NAMES = { esc: "escape", del: "delete", space: " ", plus: "+", up: "arrowup", down: "arrowdown", left: "arrowleft", right: "arrowright" };

  // "mod+shift+s" → { mod, ctrl, meta, alt, shift, key }. `mod` is ⌘ on a
  // Mac and Ctrl elsewhere.
  function parseKey(text) {
    var parts = String(text).toLowerCase().split("+");
    var combo = { ctrl: false, meta: false, alt: false, shift: false, key: "" };
    parts.forEach(function (part, i) {
      if (i === parts.length - 1) { combo.key = KEY_NAMES[part] || part; return; }
      if (part === "mod") { if (isMac) combo.meta = true; else combo.ctrl = true; }
      else if (part === "ctrl" || part === "control") combo.ctrl = true;
      else if (part === "cmd" || part === "meta") combo.meta = true;
      else if (part === "alt" || part === "option") combo.alt = true;
      else if (part === "shift") combo.shift = true;
    });
    return combo;
  }

  // For aria-keyshortcuts and the tooltip.
  function keyLabel(combo, aria) {
    var names = [];
    if (combo.ctrl) names.push(aria ? "Control" : (isMac ? "⌃" : "Ctrl"));
    if (combo.alt) names.push(aria ? "Alt" : (isMac ? "⌥" : "Alt"));
    if (combo.shift) names.push(aria ? "Shift" : (isMac ? "⇧" : "Shift"));
    if (combo.meta) names.push(aria ? "Meta" : "⌘");
    var key = combo.key === " " ? "Space" : combo.key.length === 1 ? combo.key.toUpperCase() : combo.key.charAt(0).toUpperCase() + combo.key.slice(1);
    names.push(key);
    return names.join(aria || !isMac ? "+" : "");
  }

  function setupKey(el) {
    if (el.hasAttribute("data-rx-key-ready")) return;
    el.setAttribute("data-rx-key-ready", "");
    var combo = parseKey(el.getAttribute("data-rx-key"));
    el.setAttribute("aria-keyshortcuts", keyLabel(combo, true));
    var tip = el.getAttribute("data-rx-tip");
    if (tip) el.setAttribute("data-rx-tip", tip + " (" + keyLabel(combo, false) + ")");
    else if (!el.title) el.title = keyLabel(combo, false);
  }

  function typing(el) {
    return el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  }

  function shown(el) {
    return el.getClientRects().length > 0 && !el.closest("[hidden], dialog:not([open])");
  }

  // A key fires the first visible element bound to it, in the open sheet
  // when there is one. Plain keys don't fire while typing in a field.
  document.addEventListener("keydown", function (event) {
    if (event.defaultPrevented || event.isComposing || event.repeat) return;
    var modified = event.ctrlKey || event.metaKey || event.altKey;
    if (!modified && typing(event.target)) return;
    var dialogs = document.querySelectorAll("dialog[open]");
    var scope = dialogs.length ? dialogs[dialogs.length - 1] : document;
    var key = (event.key || "").toLowerCase();
    var els = scope.querySelectorAll("[data-rx-key]");
    for (var i = 0; i < els.length; i++) {
      var el = els[i];
      var combo = parseKey(el.getAttribute("data-rx-key"));
      if (combo.key !== key || combo.ctrl !== event.ctrlKey || combo.meta !== event.metaKey ||
          combo.alt !== event.altKey || (combo.shift !== event.shiftKey && key.length > 1)) continue;
      if (combo.key.length === 1 && /[a-z0-9]/.test(combo.key) && combo.shift !== event.shiftKey) continue;
      if (!shown(el) || el.disabled || el.getAttribute("aria-disabled") === "true") continue;
      event.preventDefault();
      el.click();
      return;
    }
  });

  // ---------- Parts loaded on demand ----------

  // Charts and the period filter, the wizard, the repeater (key/value is
  // one) and tags are modules of their own, loaded the first time setup()
  // finds their markup on the page or in what htmx swaps in. The server
  // fills PARTS in with their hashed URLs. A module runs once per page,
  // however often it's asked for; it says it's ready with
  // `Renox._kit.ready(name, setup)` and is then handed every root that has
  // its markup, those found while it loaded first. Script tags added before
  // the page's load event hold that event back until they have run.
  var PARTS = {/*renox:parts*/};
  var MARKERS = {
    chart: "[data-rx-chart], details[data-rx-period]",
    wizard: "[data-rx-wizard]",
    repeater: "[data-rx-repeater]",
    tags: "[data-rx-tags]"
  };
  var nonce = document.currentScript ? document.currentScript.nonce : "";
  var parts = {};

  function part(name) {
    return parts[name] || (parts[name] = { setup: null, waiting: [], script: null });
  }

  function loadPart(name, root) {
    var p = part(name);
    if (p.setup) { p.setup(root); return; }
    p.waiting.push(root);
    if (p.script || !PARTS[name]) return;
    p.script = document.createElement("script");
    p.script.type = "module";
    p.script.src = PARTS[name];
    if (nonce) p.script.nonce = nonce;
    document.head.appendChild(p.script);
  }

  function partReady(name, setupPart) {
    var p = part(name);
    if (p.setup) return;
    p.setup = setupPart;
    p.waiting.splice(0).forEach(function (root) { setupPart(root); });
  }

  function loadParts(root, scope) {
    Object.keys(MARKERS).forEach(function (name) {
      var marker = MARKERS[name];
      if ((root.matches && root.matches(marker)) || scope.querySelector(marker)) loadPart(name, root);
    });
  }

  // `fn` for `root` and every element in it that matches `selector`.
  function each(root, selector, fn) {
    if (root.matches && root.matches(selector)) fn(root);
    (root.querySelectorAll ? root : document).querySelectorAll(selector).forEach(fn);
  }

  // What the parts use of the core. Internal: not an API for apps.
  window.Renox._kit = {
    ready: partReady,
    each: each,
    setup: function (root) { setup(root); },
    validate: validate,
    errorKey: errorKey,
    firstField: firstField,
    emit: emit,
    onClear: function (hook) { clearHooks.push(hook); },
    CLOSE: CLOSE
  };

  // Everything that sets itself up from the markup, on the page and in
  // what htmx swaps in.
  function setup(root) {
    root = root || document;
    var scope = root.querySelectorAll ? root : document;
    if (root.matches && root.matches("select[data-rx-combobox]")) enhanceSelect(root);
    scope.querySelectorAll("select[data-rx-combobox]").forEach(enhanceSelect);
    scope.querySelectorAll("[data-rx-bell]").forEach(setupBell);
    scope.querySelectorAll("[data-rx-event-stream]").forEach(function (el) { openStream(el.getAttribute("data-rx-event-stream")); });
    if (root.matches && root.matches("[data-rx-key]")) setupKey(root);
    scope.querySelectorAll("[data-rx-key]").forEach(setupKey);
    scope.querySelectorAll("[data-rx-disabled-dates]").forEach(setupDatePicker);
    applyAllWhen(scope);
    loadParts(root, scope);
  }

  // Toasts rendered with the page leave on their own too; conditional
  // groups take their state from the fields, here and in htmx swaps.
  function armAll() {
    document.querySelectorAll("[data-renox-toast]").forEach(arm);
    setup(document);
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", armAll);
  else armAll();
  document.addEventListener("htmx:load", function (event) { setup(event.target); });
})();
