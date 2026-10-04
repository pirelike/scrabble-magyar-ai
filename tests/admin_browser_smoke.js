// Az admin felület böngészős füstpróbája (a `test_admin_browser.py` futtatja): minden menüpont betöltődik, nincs JS hiba,
// a legfontosabb műveletek (párbeszéd, sudo, kereső) és az élő Socket.IO események működnek.
// Környezet: ADMIN_SMOKE_BASE, PLAYWRIGHT_MODULE, PLAYWRIGHT_CHROMIUM.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE);
const BASE = process.env.ADMIN_SMOKE_BASE;
const ROUTES = [
  'overview', 'users', 'users/{user}', 'users/{user}?tab=actions', 'users/{user}?tab=games', 'users/{user}?tab=stats',
  'users/{user}?tab=security', 'users/{user}?tab=notes', 'rooms', 'rooms/{room}', 'games', 'games/{game}', 'games/{game}?tab=moves',
  'games/{game}?tab=replay', 'games/{game}?tab=actions', 'async', 'ratings', 'ratings?tab=suspicious', 'ratings?tab=recompute',
  'dictionary', 'dictionary?tab=word&w=alma', 'dictionary?tab=rejected', 'dictionary?tab=custom', 'dictionary?tab=reviewers',
  'dictionary?tab=caches', 'daily', 'daily?tab=archive', 'daily?tab=preview', 'moderation', 'moderation?tab=chat',
  'moderation?tab=chat&source=log', 'moderation?tab=words', 'moderation?tab=names', 'comm', 'comm?tab=maintenance',
  'comm?tab=push', 'comm?tab=email', 'stats', 'stats?metric=active', 'stats?metric=retention', 'stats?metric=games',
  'stats?metric=bots', 'stats?metric=words', 'stats?metric=challenges', 'stats?metric=practice', 'stats?metric=review',
  'stats?metric=heatmap', 'security', 'security?tab=limits', 'security?tab=ipbans', 'security?tab=codes',
  'security?tab=sessions', 'system', 'settings', 'audit',
];

(async () => {
  const browser = await chromium.launch({ executablePath: process.env.PLAYWRIGHT_CHROMIUM, args: ['--no-sandbox'] });
  const result = { problems: [], checks: {} };
  const problems = result.problems;
  try {
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: 'hu-HU' });
    const page = await context.newPage();
    page.on('pageerror', (e) => problems.push('pageerror: ' + e.message));
    page.on('console', (m) => {
      // a CDN-es Socket.IO kliens nem érhető el mindenhol; a sudo-t kérő 401 várt
      if (m.type() === 'error' && !/ERR_TUNNEL|ERR_NAME|ERR_INTERNET|ERR_CONNECTION|cdnjs|status of 401/.test(m.text())) {
        problems.push('console: ' + m.text());
      }
    });
    // Socket.IO-hamisítvány: a kliens élő eseménykezelése így CDN nélkül is kipróbálható
    await page.addInitScript(() => {
      window.__emitted = [];
      window.io = function () {
        const handlers = {};
        const sock = { on(e, f) { (handlers[e] = handlers[e] || []).push(f); }, emit(e, p) { window.__emitted.push([e, p]); },
          fire(e, p) { (handlers[e] || []).forEach((f) => f(p)); } };
        window.__sock = sock;
        setTimeout(() => sock.fire('connect'), 30);
        return sock;
      };
    });
    const login = await page.request.post(BASE + '/api/auth/login', { data: { email: 'boss@example.com', password: 'secret12' } });
    result.checks.login = login.status();
    const rooms = await (await page.request.get(BASE + '/api/admin/rooms')).json();
    const users = await (await page.request.get(BASE + '/api/admin/users?q=anna')).json();
    const games = await (await page.request.get(BASE + '/api/admin/games')).json();
    const ids = { room: rooms.items[0].id, user: users.items[0].id, game: games.items[0].id };
    await page.goto(BASE + '/admin#overview', { waitUntil: 'domcontentloaded' });
    await page.waitForSelector('#admin-nav .admin-nav-item');
    result.checks.nav_items = await page.$$eval('#admin-nav .admin-nav-item', (n) => n.length);

    for (const raw of ROUTES) {
      const route = raw.replace('{room}', ids.room).replace('{user}', ids.user).replace('{game}', ids.game);
      const before = problems.length;
      await page.evaluate((r) => { location.hash = r; }, route);
      await page.waitForTimeout(500);
      await page.waitForFunction(() => !document.querySelector('#admin-main .admin-skeleton'), null, { timeout: 20000 })
        .catch(() => problems.push('betöltés nem fejeződött be: ' + route));
      const failure = await page.evaluate(() => {
        const box = document.querySelector('#admin-main .admin-error-box');
        return box ? box.textContent : null;
      });
      if (failure) problems.push('hibadoboz (' + route + '): ' + failure);
      const length = await page.evaluate(() => document.getElementById('admin-main').innerText.length);
      if (length < 20) problems.push('üres nézet: ' + route);
      if (problems.length > before) problems.push('  ↑ ' + route);
    }

    // művelet-párbeszéd: némítás indoklással
    await page.evaluate((r) => { location.hash = r; }, '#users/' + ids.user + '?tab=actions');
    await page.waitForSelector('.admin-action-row button');
    await page.click('text=Chat némítás');
    await page.waitForSelector('#admin-form-dialog:not(.hidden)');
    await page.fill('#admin-field-reason', 'böngészős próba');
    await page.click('#admin-form-submit');
    await page.waitForSelector('.toast');
    await page.waitForTimeout(700);
    result.checks.muted_badge = (await page.textContent('.admin-badge-row')).includes('némított');

    // sudo-s művelet: a jelszókérés után lefut
    await page.click('text=Kitiltás 🔒');
    await page.waitForSelector('#admin-form-dialog:not(.hidden)');
    await page.fill('#admin-field-confirm_name', 'Anna');
    await page.fill('#admin-field-reason', 'böngészős próba');
    await page.click('#admin-form-submit');
    await page.waitForSelector('#admin-sudo-dialog:not(.hidden)');
    await page.fill('#admin-sudo-password', 'secret12');
    await page.click('#admin-sudo-submit');
    await page.waitForFunction(() => document.querySelector('#admin-form-dialog.hidden'), null, { timeout: 8000 });
    await page.waitForTimeout(900);
    result.checks.banned_badge = (await page.textContent('.admin-badge-row')).includes('kitiltott');

    // globális kereső
    await page.keyboard.press('Control+k');
    await page.fill('#admin-search-input', 'béla');
    await page.waitForSelector('.admin-search-row');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(500);
    result.checks.search_hash = await page.evaluate(() => location.hash);

    // élő események a hamis Socket.IO-n át
    await page.evaluate((r) => { location.hash = '#overview'; }, null);
    await page.waitForSelector('.admin-stat-grid');
    await page.waitForTimeout(300);
    result.checks.subscribed = await page.evaluate(() => window.__emitted.some((e) => e[0] === 'admin_subscribe' && !!e[1].auth_token));
    await page.evaluate(() => window.__sock.fire('admin_subscribed'));
    await page.evaluate(() => window.__sock.fire('admin_overview', { live: { online_users: 77, online_guests: 0, sockets: 80,
      rooms_total: 1, rooms_waiting: 0, rooms_active: 1, rooms_async: 0, rooms_puzzle: 0, rooms_public: 1, rooms_private: 0,
      games_running: 1, games_with_bots: 0, spectators: 0, pending_votes: 0, turn_timers: 0, grace_players: 0 },
      server: { uptime: 5, rss: 1e6, cpu_percent: 1, greenlets: 3, threads: 1, python: '3', db_size: 1 } }));
    await page.waitForTimeout(200);
    result.checks.live_overview = (await page.textContent('.admin-stat-grid')).includes('77');
    await page.evaluate((r) => { location.hash = r; }, '#rooms/' + ids.room);
    await page.waitForSelector('.admin-board');
    await page.waitForTimeout(300);
    result.checks.watch_room = await page.evaluate((id) => window.__emitted.some((e) => e[0] === 'admin_watch_room' && e[1].room_id === id), ids.room);
    const detail = await (await page.request.get(BASE + '/api/admin/rooms/' + ids.room)).json();
    detail.room.name = 'Élő átnevezés';
    await page.evaluate((state) => window.__sock.fire('admin_room_state', state), detail.room);
    await page.waitForTimeout(200);
    result.checks.live_room = (await page.textContent('#admin-main')).includes('Élő átnevezés');
    await page.evaluate(() => window.__sock.fire('admin_report', { id: 99, room: 'X', reported: 'Y' }));
    await page.waitForTimeout(200);
    result.checks.report_toast = (await page.$$eval('.toast', (t) => t.map((x) => x.textContent).join('|'))).includes('Új bejelentés');

    // keskeny képernyő: a menü lap
    await page.setViewportSize({ width: 390, height: 800 });
    await page.evaluate(() => { location.hash = '#users'; });
    await page.waitForTimeout(600);
    await page.click('#admin-menu-btn');
    result.checks.mobile_nav = await page.evaluate(() => document.body.classList.contains('admin-nav-open'));
    // nyelvváltás
    await page.click('.btn-lang-toggle');
    await page.waitForTimeout(300);
    result.checks.english_nav = await page.textContent('#admin-nav .admin-nav-item');
  } catch (error) {
    problems.push('kivétel: ' + error.message);
  } finally {
    await browser.close();
  }
  process.stdout.write('RESULT ' + JSON.stringify(result) + '\n');
})();
