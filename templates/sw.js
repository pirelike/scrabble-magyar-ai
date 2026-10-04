/* Magyar Scrabble — service worker.
 *
 * Feladata: az alkalmazás "váza" (HTML, CSS, JS, ikonok) elérhető marad kapcsolat nélkül is, így
 * a telepített alkalmazás azonnal elindul, és kapcsolat híján érthető üzenetet mutat. A játék
 * maga (Socket.IO, API) mindig a hálózatot használja, azt nem gyorsítótárazzuk.
 *
 * A VERSION a kliens fájlok módosítási idejéből jön: új kiadáskor új gyorsítótár készül, a régi törlődik.
 */
const VERSION = {{ version|tojson }};
const SHELL_CACHE = 'scrabble-shell-' + VERSION;
const RUNTIME_CACHE = 'scrabble-runtime-v1';

const SHELL_URLS = [
    '/',
    '/static/style.css?v=' + VERSION,
    '/static/i18n-data.js?v=' + VERSION,
    '/static/i18n.js?v=' + VERSION,
    '/static/app.js?v=' + VERSION,
    '/static/offline.html',
    '/manifest.webmanifest',
    '/static/icons/icon-192.png',
    '/static/icons/icon-512.png',
    '/static/icons/apple-touch-icon.png',
];

self.addEventListener('install', (event) => {
    event.waitUntil(
        caches.open(SHELL_CACHE)
            .then((cache) => cache.addAll(SHELL_URLS))
            .then(() => self.skipWaiting())
    );
});

self.addEventListener('activate', (event) => {
    event.waitUntil(
        caches.keys()
            .then((keys) => Promise.all(
                keys
                    .filter((key) => key.startsWith('scrabble-shell-') && key !== SHELL_CACHE)
                    .map((key) => caches.delete(key))
            ))
            .then(() => self.clients.claim())
    );
});

self.addEventListener('message', (event) => {
    if (event.data === 'SKIP_WAITING') self.skipWaiting();
});

// Web Push: "Te jössz!" értesítés (a szerver a felhasználó nyelvén küldi a szöveget)
self.addEventListener('push', (event) => {
    let data = {};
    try {
        data = event.data ? event.data.json() : {};
    } catch (err) {
        data = { body: event.data ? event.data.text() : '' };
    }
    event.waitUntil(self.registration.showNotification(data.title || 'Magyar Scrabble', {
        body: data.body || '',
        tag: data.tag || 'scrabble',
        renotify: true,
        icon: '/static/icons/icon-192.png',
        badge: '/static/icons/icon-192.png',
        data: { url: data.url || '/' },
    }));
});

self.addEventListener('notificationclick', (event) => {
    event.notification.close();
    const url = (event.notification.data && event.notification.data.url) || '/';
    event.waitUntil(
        self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((windows) => {
            for (const client of windows) {
                if ('focus' in client) return client.focus();
            }
            return self.clients.openWindow(url);
        })
    );
});

function isApiRequest(url) {
    return url.pathname.startsWith('/socket.io/') || url.pathname.startsWith('/api/');
}

// Az admin felület (oldal és assetek) soha nem kerül gyorsítótárba, és kapcsolat nélkül sem helyettesítjük
function isAdminRequest(url) {
    return url.pathname === '/admin' || url.pathname.startsWith('/admin/');
}

// Hálózat először (friss HTML), kapcsolat nélkül a gyorsítótárazott váz, végső esetben az offline oldal
async function handleNavigation(request) {
    try {
        const response = await fetch(request);
        if (response && response.ok && new URL(request.url).pathname === '/') {
            const cache = await caches.open(SHELL_CACHE);
            cache.put('/', response.clone());
        }
        return response;
    } catch (err) {
        const cached = await caches.match('/');
        return cached || caches.match('/static/offline.html');
    }
}

// Gyorsítótár először, a háttérben frissítve (statikus fájlok, CDN könyvtárak)
async function staleWhileRevalidate(request, cacheName) {
    const cache = await caches.open(cacheName);
    const cached = await cache.match(request);
    const network = fetch(request)
        .then((response) => {
            if (response && (response.ok || response.type === 'opaque')) {
                cache.put(request, response.clone());
            }
            return response;
        })
        .catch(() => cached);
    return cached || network;
}

self.addEventListener('fetch', (event) => {
    const request = event.request;
    if (request.method !== 'GET') return;

    const url = new URL(request.url);
    if (url.origin === self.location.origin && (isApiRequest(url) || isAdminRequest(url))) return;  // mindig hálózat

    if (request.mode === 'navigate') {
        event.respondWith(handleNavigation(request));
        return;
    }

    if (url.origin === self.location.origin) {
        event.respondWith(
            caches.match(request).then((cached) => cached || staleWhileRevalidate(request, SHELL_CACHE))
        );
        return;
    }

    // Külső erőforrások (Socket.IO kliens a CDN-ről, betűtípus): futás közbeni gyorsítótár
    if (url.hostname === 'cdnjs.cloudflare.com' || url.hostname.endsWith('googleapis.com')
            || url.hostname.endsWith('gstatic.com')) {
        event.respondWith(staleWhileRevalidate(request, RUNTIME_CACHE));
    }
});
