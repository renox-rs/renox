(function () {
  "use strict";
  if (window.RenoxOffline) return;

  // Offline forms (#52). Loaded only by pages that have an `<rx-form offline="name">`.
  //
  // The form carries this contract (written by `<rx-form>`, read here):
  //   data-rx-offline="name"          the outbox name; its presence marks an offline form
  //   data-rx-offline-csrf="/url"     GET returns { "token": "..." }, fetched before each replay
  //   data-rx-offline-waiting         texts rendered by the server (`ui.offline.*`); ":count" and
  //   data-rx-offline-attention       a "one|many" split are filled in here
  //   data-rx-offline-saved, -sent, -signin, -sending, -failed
  //   data-rx-offline-review, -resend, -discard, -problem   the "needs attention" list
  // The form's own action and method are the replay target. Each queued post sends
  // Idempotency-Key, X-Client-Id and X-Recorded-At (see `Offline` in the Rust docs).
  //
  // What is stored, in IndexedDB "renox-offline" on this device and NOT encrypted: per post the
  // action, method, key, client id, recorded time and the fields (files as Blobs). Password
  // fields and the CSRF token are never stored; a fresh token is fetched before each replay.
  // The store is cleared on logout (a form whose action ends in /logout, or a [data-rx-logout]
  // link or form, on any page that loads this script).

  var DB_NAME = "renox-offline";
  var ITEMS = "items";
  var META = "meta";
  var MAX_WAIT = 300000;

  var dbp = null;
  var busy = false;
  var signIn = false;
  var note = "";
  var wait = 15000;
  var timer = null;
  var editing = new WeakMap();

  function forms() {
    return Array.prototype.slice.call(document.querySelectorAll("form[data-rx-offline]"));
  }

  // ---------- Texts ----------

  function text(key, n) {
    var el = forms()[0];
    var raw = (el && el.getAttribute("data-rx-offline-" + key)) || "";
    var parts = raw.split("|");
    var one = parts.length > 1 && n === 1;
    return (one ? parts[0] : parts[parts.length - 1]).replace(":count", String(n == null ? "" : n));
  }

  // ---------- IndexedDB ----------

  function open() {
    dbp = dbp || new Promise(function (resolve, reject) {
      var req = indexedDB.open(DB_NAME, 1);
      req.onupgradeneeded = function () {
        req.result.createObjectStore(ITEMS, { keyPath: "id", autoIncrement: true });
        req.result.createObjectStore(META);
      };
      req.onsuccess = function () { resolve(req.result); };
      req.onerror = function () { dbp = null; reject(req.error); };
    });
    return dbp;
  }

  function tx(stores, mode, work) {
    return open().then(function (db) {
      return new Promise(function (resolve, reject) {
        var t = db.transaction(stores, mode);
        var req = work(t);
        t.oncomplete = function () { resolve(req && req.result); };
        t.onerror = t.onabort = function () { reject(t.error); };
      });
    });
  }

  function listItems() {
    return tx(ITEMS, "readonly", function (t) { return t.objectStore(ITEMS).getAll(); }).then(function (all) {
      return (all || []).sort(function (a, b) { return a.recordedMs - b.recordedMs || a.id - b.id; });
    });
  }

  function putItem(item) {
    return tx(ITEMS, "readwrite", function (t) { return t.objectStore(ITEMS).put(item); }).then(function (id) {
      item.id = id;
      return item;
    });
  }

  function dropItem(id) {
    return tx(ITEMS, "readwrite", function (t) { return t.objectStore(ITEMS).delete(id); });
  }

  function clearAll() {
    return tx([ITEMS, META], "readwrite", function (t) {
      t.objectStore(META).clear();
      return t.objectStore(ITEMS).clear();
    });
  }

  function uuid() {
    if (window.crypto && crypto.randomUUID) return crypto.randomUUID();
    var b = crypto.getRandomValues(new Uint8Array(16));
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    var h = Array.prototype.map.call(b, function (x) { return (x + 256).toString(16).slice(1); }).join("");
    return h.slice(0, 8) + "-" + h.slice(8, 12) + "-" + h.slice(12, 16) + "-" + h.slice(16, 20) + "-" + h.slice(20);
  }

  // The device's id, made once and kept next to the queue.
  function clientId() {
    return tx(META, "readonly", function (t) { return t.objectStore(META).get("client"); }).then(function (id) {
      if (id) return id;
      id = uuid();
      return tx(META, "readwrite", function (t) { return t.objectStore(META).put(id, "client"); }).then(function () { return id; });
    });
  }

  // ---------- Capturing a post ----------

  function capture(form, submitter) {
    var secret = {};
    Array.prototype.forEach.call(form.querySelectorAll("input[type=password]"), function (i) { secret[i.name] = true; });
    var fields = [];
    var data = submitter ? new FormData(form, submitter) : new FormData(form);
    data.forEach(function (value, name) {
      if (name === "_token" || secret[name]) return;
      if (typeof value !== "string" && !value.name && !value.size) return;
      fields.push([name, value]);
    });
    return fields;
  }

  // What a body is, to see whether an edit changed it.
  function signature(fields) {
    return JSON.stringify(fields.map(function (f) {
      var v = f[1];
      return [f[0], typeof v === "string" ? v : [v.name, v.size, v.type, v.lastModified]];
    }));
  }

  function summary(fields) {
    for (var i = 0; i < fields.length; i++) {
      var v = fields[i][1];
      if (typeof v === "string" && v.trim()) return v.trim().slice(0, 60);
    }
    return "";
  }

  function build(form, fields, previous) {
    return clientId().then(function (client) {
      var now = new Date();
      var same = previous && signature(previous.fields) === signature(fields);
      return {
        id: previous ? previous.id : undefined,
        outbox: form.getAttribute("data-rx-offline"),
        action: form.action,
        method: (form.getAttribute("method") || "post").toUpperCase(),
        key: same ? previous.key : uuid(),
        clientId: previous ? previous.clientId : client,
        recorded: previous ? previous.recorded : now.toISOString(),
        recordedMs: previous ? previous.recordedMs : now.getTime(),
        fields: fields,
        summary: summary(fields),
        status: "pending"
      };
    });
  }

  // ---------- Sending ----------

  function send(item, token) {
    var body = new FormData();
    item.fields.forEach(function (f) {
      if (typeof f[1] === "string") body.append(f[0], f[1]);
      else body.append(f[0], f[1], f[1].name);
    });
    if (token) body.set("_token", token);
    var headers = {
      "Accept": "application/json",
      "Idempotency-Key": item.key,
      "X-Client-Id": item.clientId,
      "X-Recorded-At": item.recorded
    };
    if (token) headers["X-CSRF-Token"] = token;
    return fetch(item.action, { method: item.method, body: body, headers: headers, credentials: "same-origin" });
  }

  function csrfToken() {
    var el = forms()[0];
    var url = (el && el.getAttribute("data-rx-offline-csrf")) || "/_renox/csrf";
    return fetch(url, { credentials: "same-origin", cache: "no-store", headers: { "Accept": "application/json" } }).then(function (res) {
      if (res.status === 401 || res.status === 403 || res.redirected) return null;
      return res.ok ? res.json().then(function (d) { return d && d.token || null; }, function () { return null; }) : undefined;
    });
  }

  function signedOut(res) {
    return res.status === 401 || (res.redirected && /\/login\b/.test(new URL(res.url).pathname));
  }

  function readErrors(res) {
    return res.text().then(function (raw) {
      try {
        var data = JSON.parse(raw);
        return { message: data.message || "", errors: data.errors || {} };
      } catch (e) {
        return { message: "", errors: {} };
      }
    }, function () { return { message: "", errors: {} }; });
  }

  // ---------- Replay ----------

  // One replay pass, strictly in recorded order. Resolves when it is done or stopped.
  function replay() {
    if (busy || !navigator.onLine) return Promise.resolve();
    busy = true;
    note = "";
    var sent = 0;
    var stopped = false;
    render();
    return listItems().then(function (items) {
      var queue = items.filter(function (i) { return i.status === "pending"; });
      if (!queue.length) return null;
      return csrfToken().then(function (token) {
        if (token === null) { signIn = true; stopped = true; return; }
        if (token === undefined) { stopped = true; return; }
        signIn = false;
        return queue.reduce(function (chain, item) {
          return chain.then(function () {
            if (stopped) return;
            return step(item, token).then(function (outcome) {
              if (outcome === "sent") sent++;
              if (outcome === "stop" || outcome === "signin") stopped = true;
              if (outcome === "signin") signIn = true;
            });
          });
        }, Promise.resolve());
      });
    }).catch(function () { stopped = true; }).then(function () {
      busy = false;
      if (sent) { note = text("sent"); wait = 15000; }
      render();
      if (stopped && !signIn) schedule();
    });
  }

  // "sent" (removed), "kept" (needs attention), "stop" (try later) or "signin".
  function step(item, token) {
    return send(item, token).then(function (res) {
      if (signedOut(res)) return "signin";
      if (res.ok || (res.status >= 300 && res.status < 400)) return dropItem(item.id).then(function () { return "sent"; });
      if (res.status >= 500 || res.status === 429 || res.status === 408 || res.status === 419) return "stop";
      return readErrors(res).then(function (e) {
        item.status = "attention";
        item.code = res.status;
        item.message = e.message;
        item.errors = e.errors;
        return putItem(item).then(function () { return "kept"; });
      });
    }, function () { return "stop"; });
  }

  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(replay, wait);
    wait = Math.min(wait * 2, MAX_WAIT);
  }

  // ---------- Submitting a form ----------

  function showErrors(form, errors) {
    Array.prototype.forEach.call(form.querySelectorAll("[data-error-for]"), function (s) { s.textContent = ""; });
    Array.prototype.forEach.call(form.querySelectorAll("[aria-invalid]"), function (i) { i.removeAttribute("aria-invalid"); });
    var first = null;
    Object.keys(errors || {}).forEach(function (name) {
      var slot = form.querySelector('[data-error-for="' + CSS.escape(name) + '"]');
      var input = form.querySelector('[name="' + CSS.escape(name) + '"]');
      var msg = [].concat(errors[name])[0] || "";
      if (slot) slot.textContent = msg;
      if (input) { input.setAttribute("aria-invalid", "true"); first = first || input; }
    });
    if (first) first.focus();
  }

  function submit(form, submitter) {
    var previous = editing.get(form) || null;
    editing.delete(form);
    form.removeAttribute("data-rx-offline-editing");
    note = "";
    return build(form, capture(form, submitter), previous).then(function (item) {
      if (!navigator.onLine) return queue(form, item);
      return send(item, formToken(form)).then(function (res) {
        if (signedOut(res)) return queue(form, item);
        if (res.status >= 500 && res.status !== 501) return queue(form, item);
        if (res.ok || (res.status >= 300 && res.status < 400)) return done(form, item, res);
        return readErrors(res).then(function (e) {
          showErrors(form, e.errors);
          if (previous) { item.status = "attention"; item.code = res.status; item.message = e.message; item.errors = e.errors; return putItem(item).then(render); }
        });
      }, function () { return queue(form, item); });
    }).catch(function () {
      note = text("failed");
      render();
    });
  }

  function formToken(form) {
    var input = form.querySelector('input[name="_token"]');
    return input ? input.value : "";
  }

  function queue(form, item) {
    return putItem(item).then(function () {
      note = text("saved");
      form.reset();
      showErrors(form, {});
      render();
      schedule();
    });
  }

  function done(form, item, res) {
    var finish = item.id == null ? Promise.resolve() : dropItem(item.id);
    return finish.then(function () {
      if (res.redirected) { location.assign(res.url); return; }
      var type = res.headers.get("Content-Type") || "";
      if (type.indexOf("text/html") === 0) {
        return res.text().then(function (html) { document.open(); document.write(html); document.close(); });
      }
      form.reset();
      note = text("sent");
      render();
    });
  }

  document.addEventListener("submit", function (event) {
    var form = event.target;
    if (!form || !form.matches) return;
    if (isLogout(form)) return logout(event, form);
    if (!form.matches("form[data-rx-offline]") || !window.indexedDB || event.defaultPrevented) return;
    event.preventDefault();
    submit(form, event.submitter);
  });

  // ---------- Logout ----------

  function isLogout(el) {
    if (el.hasAttribute("data-rx-logout")) return true;
    var target = el.tagName === "FORM" ? el.getAttribute("action") : el.getAttribute("href");
    return !!target && /\/logout\/?(\?.*)?$/.test(target.split("#")[0]);
  }

  // Clears the device's queue, then lets the logout go on.
  function logout(event, el) {
    if (!window.indexedDB) return;
    event.preventDefault();
    var go = function () {
      if (el.tagName === "FORM") HTMLFormElement.prototype.submit.call(el);
      else location.assign(el.href);
    };
    clearAll().then(go, go);
  }

  document.addEventListener("click", function (event) {
    var link = event.target.closest && event.target.closest("a[href]");
    if (link && isLogout(link)) logout(event, link);
  });

  // ---------- The panel: pending badge and "needs attention" list ----------

  function panelFor(name) {
    var existing = document.querySelector('[data-rx-offline-panel="' + CSS.escape(name) + '"]');
    if (existing) return existing;
    var form = forms().filter(function (f) { return f.getAttribute("data-rx-offline") === name; })[0];
    if (!form) return null;
    var panel = document.createElement("div");
    panel.className = "rx-offline";
    panel.setAttribute("data-rx-offline-panel", name);
    var status = document.createElement("p");
    status.className = "rx-offline__status";
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    var list = document.createElement("ul");
    list.className = "rx-offline__list";
    panel.appendChild(status);
    panel.appendChild(list);
    form.parentNode.insertBefore(panel, form.nextSibling);
    return panel;
  }

  function button(label, act, id) {
    var b = document.createElement("button");
    b.type = "button";
    b.className = "rx-button rx-button--secondary rx-button--small";
    b.textContent = label;
    b.setAttribute("data-rx-offline-act", act);
    b.setAttribute("data-id", String(id));
    return b;
  }

  function render() {
    return listItems().then(function (items) {
      var names = {};
      forms().forEach(function (f) { names[f.getAttribute("data-rx-offline")] = true; });
      Object.keys(names).forEach(function (name) {
        var panel = panelFor(name);
        if (!panel) return;
        var mine = items.filter(function (i) { return i.outbox === name; });
        var waiting = mine.filter(function (i) { return i.status === "pending"; });
        var attention = mine.filter(function (i) { return i.status === "attention"; });
        var parts = [];
        if (waiting.length) parts.push(busy ? text("sending") : text("waiting", waiting.length));
        if (attention.length) parts.push(text("attention", attention.length));
        if (signIn && waiting.length) parts.push(text("signin"));
        if (note) parts.push(note);
        var status = panel.querySelector(".rx-offline__status");
        var line = parts.join(" ");
        if (status.textContent !== line) status.textContent = line;
        var list = panel.querySelector(".rx-offline__list");
        list.textContent = "";
        attention.forEach(function (item) {
          var li = document.createElement("li");
          li.className = "rx-offline__item";
          var label = document.createElement("span");
          label.className = "rx-offline__summary";
          label.textContent = (item.summary ? item.summary + " · " : "") + new Date(item.recorded).toLocaleString();
          li.appendChild(label);
          var messages = [item.message].concat(Object.keys(item.errors || {}).map(function (k) { return [].concat(item.errors[k])[0]; }));
          messages = messages.filter(Boolean);
          if (!messages.length) messages = [text("problem")];
          messages.forEach(function (m) {
            var p = document.createElement("span");
            p.className = "rx-offline__problem";
            p.textContent = m;
            li.appendChild(p);
          });
          var actions = document.createElement("span");
          actions.className = "rx-offline__actions";
          actions.appendChild(button(text("review"), "review", item.id));
          actions.appendChild(button(text("resend"), "resend", item.id));
          actions.appendChild(button(text("discard"), "discard", item.id));
          li.appendChild(actions);
          list.appendChild(li);
        });
        list.hidden = !attention.length;
      });
    }, function () {});
  }

  // Puts an item back in its form, with the server's errors, to edit and send again.
  function review(item) {
    var form = forms().filter(function (f) { return f.getAttribute("data-rx-offline") === item.outbox; })[0];
    if (!form) return;
    form.reset();
    var by = {};
    item.fields.forEach(function (f) { (by[f[0]] = by[f[0]] || []).push(f[1]); });
    Object.keys(by).forEach(function (name) {
      var values = by[name];
      Array.prototype.forEach.call(form.querySelectorAll('[name="' + CSS.escape(name) + '"]'), function (el) {
        if (el.type === "file") {
          try {
            var dt = new DataTransfer();
            values.forEach(function (v) { if (typeof v !== "string") dt.items.add(v); });
            el.files = dt.files;
          } catch (e) { /* the browser cannot fill a file input: the user picks the file again */ }
        } else if (el.type === "checkbox" || el.type === "radio") {
          el.checked = values.indexOf(el.value) >= 0;
        } else if (el.tagName === "SELECT" && el.multiple) {
          Array.prototype.forEach.call(el.options, function (o) { o.selected = values.indexOf(o.value) >= 0; });
        } else if (typeof values[0] === "string") {
          el.value = values[0];
        }
      });
    });
    showErrors(form, item.errors);
    editing.set(form, item);
    form.setAttribute("data-rx-offline-editing", String(item.id));
    form.scrollIntoView({ block: "center", behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  }

  document.addEventListener("click", function (event) {
    var b = event.target.closest && event.target.closest("[data-rx-offline-act]");
    if (!b) return;
    var id = Number(b.getAttribute("data-id"));
    var act = b.getAttribute("data-rx-offline-act");
    listItems().then(function (items) {
      var item = items.filter(function (i) { return i.id === id; })[0];
      if (!item) return render();
      if (act === "review") return review(item);
      if (act === "discard") return dropItem(id).then(render);
      item.status = "pending";
      delete item.errors;
      delete item.message;
      return putItem(item).then(function () { note = ""; return replay(); });
    });
  });

  // ---------- Start ----------

  window.RenoxOffline = {
    // The forms on the page that opted in.
    forms: forms,
    // Sends the waiting posts now, in order.
    replay: replay,
    // Deletes everything the outbox keeps on this device.
    clear: function () { return clearAll().then(render); }
  };

  if (window.indexedDB) {
    window.addEventListener("online", function () { wait = 15000; replay(); });
    document.addEventListener("visibilitychange", function () { if (!document.hidden) replay(); });
    var start = function () { render().then(replay); };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
    else start();
  }
})();
