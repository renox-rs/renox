// Renox service worker. Navigations always go to the network; the offline
// page is only the fallback and no page is ever stored. Only the hashed
// /_renox/* assets and the offline page are precached.
const VERSION = "__RENOX_SW_VERSION__";
const PRECACHE = __RENOX_SW_PRECACHE__;
const OFFLINE = "__RENOX_SW_OFFLINE__";
const PREFIX = "renox-pwa-";
const CACHE = PREFIX + VERSION;

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(PRECACHE))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key.startsWith(PREFIX) && key !== CACHE)
            .map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  if (request.mode === "navigate") {
    event.respondWith(
      fetch(request).catch(() => caches.match(OFFLINE, { cacheName: CACHE })),
    );
    return;
  }
  const url = new URL(request.url);
  if (url.origin === self.location.origin && PRECACHE.includes(url.pathname)) {
    event.respondWith(
      caches
        .match(request, { cacheName: CACHE })
        .then((hit) => hit || fetch(request)),
    );
  }
});
