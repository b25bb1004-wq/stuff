const CACHE_NAME = "__ATHENA_CACHE_NAME__";
const ASSETS = "__ATHENA_ASSETS__";

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then((cache) => cache.addAll(ASSETS))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key !== CACHE_NAME)
            .map((key) => caches.delete(key))
        )
      )
      .then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET") {
    return;
  }
  const url = new URL(req.url);
  if (url.origin !== self.location.origin) {
    return;
  }
  event.respondWith(
    caches.match(req).then((cached) => {
      if (cached) {
        return cached;
      }
      if (req.mode === "navigate" || url.pathname.endsWith("/")) {
        return caches.match("index.html").then((indexResp) => {
          if (indexResp) {
            return indexResp;
          }
          return fetch(req);
        });
      }
      return fetch(req);
    })
  );
});
