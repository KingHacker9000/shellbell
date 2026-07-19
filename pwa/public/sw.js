const CACHE = 'shellbell-shell-v1'
const SHELL = ['/', '/manifest.webmanifest', '/icons/icon.svg']

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()))
})

self.addEventListener('activate', (event) => {
  event.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key)))).then(() => self.clients.claim()))
})

self.addEventListener('fetch', (event) => {
  if (event.request.method !== 'GET' || new URL(event.request.url).pathname.startsWith('/api/')) return
  event.respondWith(fetch(event.request).catch(() => caches.match(event.request).then((response) => response || caches.match('/'))))
})

self.addEventListener('push', (event) => {
  let data = { title: 'Shellbell', body: 'Your terminal is ready', url: '/?view=rings' }
  try { if (event.data) data = { ...data, ...event.data.json() } } catch { /* use privacy-safe fallback */ }
  event.waitUntil(self.registration.showNotification(data.title, { body: data.body, icon: '/icons/icon.svg', badge: '/icons/icon.svg', tag: data.event_id || 'shellbell', data: { url: data.url } }))
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  const target = event.notification.data?.url || '/?view=rings'
  event.waitUntil(self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((clients) => {
    const existing = clients.find((client) => new URL(client.url).origin === self.location.origin)
    if (existing) { existing.navigate(target); return existing.focus() }
    return self.clients.openWindow(target)
  }))
})
