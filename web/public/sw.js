// Zero-dependency Service Worker for Isle Settlers offline single-player & DIY Workshop.
const CACHE_NAME = 'isle-settlers-v1';
const SHELL_FILES = ['./', './index.html', './isle-icon.svg', './manifest.webmanifest'];

function getScopePath() {
  return new URL(self.registration.scope).pathname;
}

function isCacheableResponse(response) {
  return Boolean(response && response.status === 200 && response.type === 'basic');
}

function hasDebugParam(rawUrl) {
  if (!rawUrl) return false;
  try {
    const url = new URL(rawUrl, self.location.origin);
    return url.searchParams.has('debug') || url.search.includes('debug');
  } catch {
    return false;
  }
}

function isApiPath(pathname, scopePath) {
  if (pathname === '/api' || pathname.startsWith('/api/')) return true;
  if (scopePath !== '/' && (pathname === `${scopePath}api` || pathname.startsWith(`${scopePath}api/`))) {
    return true;
  }
  return false;
}

function isImmutableAssetPath(pathname, scopePath) {
  if (pathname.startsWith('/assets/')) return true;
  return scopePath !== '/' && pathname.startsWith(`${scopePath}assets/`);
}

function isDevModulePath(pathname) {
  return (
    pathname.startsWith('/@') ||
    pathname.startsWith('/src/') ||
    pathname.startsWith('/node_modules/')
  );
}

function extractHtmlAssetUrls(html, baseUrl) {
  const urls = new Set();
  const attrPattern = /(?:src|href)=["']([^"']+)["']/g;
  let match;
  while ((match = attrPattern.exec(html)) !== null) {
    try {
      const candidate = new URL(match[1], baseUrl);
      if (
        candidate.origin === self.location.origin &&
        isImmutableAssetPath(candidate.pathname, getScopePath()) &&
        !hasDebugParam(candidate.href)
      ) {
        urls.add(candidate.href);
      }
    } catch {
      // Ignore malformed attribute URLs.
    }
  }
  return [...urls];
}

self.addEventListener('install', (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      const scope = self.registration.scope;
      let discoveredAssets = [];

      await Promise.allSettled(
        SHELL_FILES.map(async (relPath) => {
          const targetUrl = new URL(relPath, scope).href;
          const response = await fetch(targetUrl, { cache: 'no-cache' });
          if (!isCacheableResponse(response)) return;
          if (relPath === './index.html') {
            const html = await response.clone().text();
            discoveredAssets = extractHtmlAssetUrls(html, targetUrl);
          }
          await cache.put(targetUrl, response);
        }),
      );

      if (discoveredAssets.length > 0) {
        await Promise.allSettled(
          discoveredAssets.map(async (assetUrl) => {
            if (await cache.match(assetUrl)) return;
            const response = await fetch(assetUrl);
            if (isCacheableResponse(response)) {
              await cache.put(assetUrl, response);
            }
          }),
        );
      }

      await self.skipWaiting();
    })(),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const keys = await caches.keys();
      await Promise.all(
        keys
          .filter((key) => key.startsWith('isle-settlers-') && key !== CACHE_NAME)
          .map((key) => caches.delete(key)),
      );
      await self.clients.claim();
    })(),
  );
});

self.addEventListener('message', (event) => {
  const data = event.data;
  if (!data || data.type !== 'WARM_ASSETS' || !Array.isArray(data.urls)) return;

  const scopePath = getScopePath();
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      await Promise.allSettled(
        data.urls.map(async (rawUrl) => {
          if (typeof rawUrl !== 'string') return;
          const url = new URL(rawUrl, self.location.origin);
          if (url.origin !== self.location.origin) return;
          if (!isImmutableAssetPath(url.pathname, scopePath)) return;
          if (hasDebugParam(url.href)) return;
          if (await cache.match(url.href)) return;
          const response = await fetch(url.href);
          if (isCacheableResponse(response)) {
            await cache.put(url.href, response);
          }
        }),
      );
    })(),
  );
});

async function cacheFirst(request) {
  const cache = await caches.open(CACHE_NAME);
  const cached = await cache.match(request);
  if (cached) return cached;

  const response = await fetch(request);
  if (isCacheableResponse(response)) {
    await cache.put(request, response.clone());
  }
  return response;
}

async function networkFirst(request, isNavigation) {
  const cache = await caches.open(CACHE_NAME);
  const scope = self.registration.scope;
  const indexUrl = new URL('./index.html', scope).href;
  const scopeUrl = new URL('./', scope).href;

  try {
    const response = await fetch(request);
    if (isCacheableResponse(response)) {
      await cache.put(request, response.clone());
      if (isNavigation) {
        await cache.put(indexUrl, response.clone());
      }
      return response;
    }
    if (response.status >= 500) {
      const fallback =
        (await cache.match(request)) ||
        (await cache.match(request, { ignoreSearch: true })) ||
        (isNavigation ? (await cache.match(indexUrl)) || (await cache.match(scopeUrl)) : undefined);
      if (fallback) return fallback;
    }
    return response;
  } catch (err) {
    const cached =
      (await cache.match(request)) ||
      (await cache.match(request, { ignoreSearch: true })) ||
      (isNavigation ? (await cache.match(indexUrl)) || (await cache.match(scopeUrl)) : undefined);
    if (cached) return cached;
    throw err;
  }
}

self.addEventListener('fetch', (event) => {
  const { request } = event;
  if (request.method !== 'GET') return;
  if (request.cache === 'only-if-cached' && request.mode !== 'same-origin') return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  const scopePath = getScopePath();
  if (isApiPath(url.pathname, scopePath)) return;
  if (url.pathname === '/sw.js' || url.pathname === `${scopePath}sw.js`) return;
  if (hasDebugParam(url.href) || hasDebugParam(request.referrer)) return;
  if (isDevModulePath(url.pathname)) return;

  if (isImmutableAssetPath(url.pathname, scopePath)) {
    event.respondWith(cacheFirst(request));
    return;
  }

  event.respondWith(networkFirst(request, request.mode === 'navigate'));
});
