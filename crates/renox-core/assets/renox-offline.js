(function () {
  "use strict";
  if (window.RenoxOffline) return;

  // Offline forms (#52). Loaded only by pages that have an `<rx-form offline="name">`.
  //
  // The form carries this contract (written by `<rx-form>`, read here):
  //   data-rx-offline="name"       the outbox name; its presence marks an offline form
  //   data-rx-offline-csrf="/url"  GET returns { "token": "..." }, fetched before each replay
  // The form's own action and method are the replay target. Each queued post sends
  // Idempotency-Key, X-Client-Id and X-Recorded-At (see `Offline` in the Rust docs).
  //
  // The outbox itself (IndexedDB queue, pending badge, ordered replay) arrives in a later part.

  window.RenoxOffline = {
    // The forms on the page that opted in.
    forms: function () {
      return Array.prototype.slice.call(document.querySelectorAll("form[data-rx-offline]"));
    },
  };
})();
