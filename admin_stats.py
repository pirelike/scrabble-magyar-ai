"""Admin panel: statisztika és a napi feladvány kezelése.

A statisztikák időszak-választóval (7 / 30 / 90 / 365 nap) készülnek, mindegyik táblázatos alakban (`table`) is
elérhető a CSV exporthoz. A számítások az adatbázisból dolgoznak (a megmaradt játékokból és naplókból), a
szerver állapotát nem érintik.
"""
import json
from collections import Counter
from datetime import timedelta

import admin
import admin_live
import ai_player
import auth
import daily
from admin import AdminError

RANGES = (7, 30, 90, 365)
METRICS = ('registrations', 'active', 'retention', 'games', 'bots', 'words', 'challenges', 'practice', 'review',
           'heatmap')
MAX_MOVE_ROWS = 60000
TOP_WORDS = 30
TOP_MOVES = 20
RETENTION_DAYS = (1, 7, 30)
PRACTICE_KEYS = ('quiz', 'answer', 'short_words', 'rack', 'rack_word', 'word_review', 'word_review_vote')

try:
    from zoneinfo import ZoneInfo
    _TZ = ZoneInfo('Europe/Budapest')
except Exception:     # nincs időzóna-adatbázis: UTC
    _TZ = None


def _range_days(value):
    try:
        days = int(value)
    except (TypeError, ValueError):
        return 30
    if days not in RANGES:
        raise AdminError('Érvénytelen időszak.', 400, field='range')
    return days


def _table(columns, rows):
    return {'columns': columns, 'rows': rows}


def _series_table(days, **series):
    names = list(series)
    return _table(['day'] + names, [[d] + [series[n][i]['value'] for n in names] for i, d in enumerate(days)])


def stats(metric, range_value):
    """Egy statisztika: {metric, range, ... , table}. Érvénytelen metrika / időszak: AdminError."""
    if metric not in METRICS:
        raise AdminError('Ismeretlen statisztika.', 400, field='metric')
    days = _range_days(range_value)
    result = {'metric': metric, 'range': days}
    result.update(globals()[f'_stat_{metric}'](days))
    return result


# ===== Regisztráció, aktivitás, megtartás =====

def _stat_registrations(days):
    charts = admin_live.charts(days)
    with auth.transaction() as conn:
        total = conn.execute('SELECT COUNT(*) FROM users WHERE deleted_at IS NULL').fetchone()[0]
    return {'days': charts['days'], 'series': charts['registrations'], 'total_users': total,
            'table': _series_table(charts['days'], registrations=charts['registrations'])}


def _activity_count(conn, since_days):
    since = (auth.utcnow().date() - timedelta(days=since_days - 1)).isoformat()
    return conn.execute(f'SELECT COUNT(DISTINCT user_id) FROM ({admin_live.ACTIVITY_SQL}) WHERE day >= ?',
                        (since,)).fetchone()[0]


def _stat_active(days):
    charts = admin_live.charts(days)
    with auth.transaction() as conn:
        dau, wau, mau = (_activity_count(conn, n) for n in (1, 7, 30))
    return {'days': charts['days'], 'series': charts['active_users'], 'dau': dau, 'wau': wau, 'mau': mau,
            'table': _series_table(charts['days'], active_users=charts['active_users'])}


def _stat_retention(days):
    """Az X. napon visszatérők aránya: a legalább X napja regisztrált (az időszakban regisztrált) felhasználók közül
    hányan voltak aktívak pontosan a regisztráció utáni X. napon."""
    today = auth.utcnow().date()
    since = (today - timedelta(days=days)).isoformat()
    with auth.transaction() as conn:
        users = [(r['id'], r['day']) for r in conn.execute(
            'SELECT id, date(created_at) AS day FROM users WHERE date(created_at) >= ? AND deleted_at IS NULL',
            (since,))]
        active = {(r['user_id'], r['day']) for r in conn.execute(
            f'SELECT user_id, day FROM ({admin_live.ACTIVITY_SQL}) WHERE day >= ?', (since,))}
    rows = []
    for n in RETENTION_DAYS:
        eligible = [(uid, day) for uid, day in users
                    if (today - _date(day)).days >= n]
        returned = sum(1 for uid, day in eligible if (uid, (_date(day) + timedelta(days=n)).isoformat()) in active)
        rows.append({'day': n, 'eligible': len(eligible), 'returned': returned,
                     'share': round(returned / len(eligible) * 100, 1) if eligible else None})
    return {'retention': rows,
            'table': _table(['day', 'eligible', 'returned', 'share'],
                            [[r['day'], r['eligible'], r['returned'], r['share']] for r in rows])}


def _date(text):
    from datetime import date
    return date.fromisoformat(text)


# ===== Játékok =====

def _stat_games(days):
    charts = admin_live.charts(days)
    since = charts['days'][0]
    with auth.transaction() as conn:
        types = conn.execute(
            "SELECT SUM(CASE WHEN is_async = 1 THEN 1 ELSE 0 END) AS async_games, "
            "SUM(CASE WHEN is_async = 0 AND has_bots = 1 THEN 1 ELSE 0 END) AS bot_games, "
            "SUM(CASE WHEN is_async = 0 AND has_bots = 0 THEN 1 ELSE 0 END) AS human_games "
            "FROM saved_games WHERE status = 'finished' AND date(updated_at) >= ?", (since,)).fetchone()
        daily_attempts = conn.execute('SELECT COALESCE(SUM(attempts), 0) FROM daily_scores WHERE puzzle_date >= ?',
                                      (since,)).fetchone()[0]
        status = {r['status']: r['n'] for r in conn.execute(
            'SELECT status, COUNT(*) AS n FROM saved_games WHERE date(created_at) >= ? GROUP BY status', (since,))}
        length = conn.execute(
            "SELECT AVG(m.n) AS avg_moves FROM (SELECT COUNT(*) AS n FROM game_moves gm JOIN saved_games sg "
            "ON sg.id = gm.game_id WHERE sg.status = 'finished' AND date(sg.updated_at) >= ? GROUP BY gm.game_id) m",
            (since,)).fetchone()
        duration = conn.execute(
            "SELECT AVG((julianday(updated_at) - julianday(created_at)) * 1440.0) AS minutes FROM saved_games "
            "WHERE status = 'finished' AND is_async = 0 AND date(updated_at) >= ?", (since,)).fetchone()
    finished = status.get('finished', 0)
    abandoned = status.get('abandoned', 0)
    result = {
        'days': charts['days'], 'human': charts['games_human'], 'bots': charts['games_bots'],
        'types': {'human': types['human_games'] or 0, 'bots': types['bot_games'] or 0,
                  'async': types['async_games'] or 0, 'daily': daily_attempts},
        'avg_moves': round(length['avg_moves'], 1) if length['avg_moves'] else None,
        'avg_minutes': round(duration['minutes'], 1) if duration['minutes'] else None,
        'finished': finished, 'abandoned': abandoned,
        'completion_rate': round(finished / (finished + abandoned) * 100, 1) if finished + abandoned else None,
    }
    result['table'] = _table(['day', 'human', 'bots'],
                             [[d, charts['games_human'][i]['value'], charts['games_bots'][i]['value']]
                              for i, d in enumerate(charts['days'])])
    return result


def _stat_bots(days):
    """A robot fokozatok népszerűsége és az emberek nyerési aránya fokozatonként (a kalibráláshoz: összevethető a
    `LEVEL_STRENGTH`-szel)."""
    since = (auth.utcnow().date() - timedelta(days=days - 1)).isoformat()
    played, human_wins = Counter(), Counter()
    with auth.transaction() as conn:
        rows = conn.execute("SELECT id, state_json FROM saved_games WHERE status = 'finished' AND has_bots = 1 "
                            'AND date(updated_at) >= ? ORDER BY id DESC LIMIT 5000', (since,)).fetchall()
        winners = {}
        for r in conn.execute('SELECT game_id, player_name FROM game_players WHERE is_winner = 1 AND game_id IN '
                              "(SELECT id FROM saved_games WHERE status = 'finished' AND has_bots = 1 "
                              'AND date(updated_at) >= ?)', (since,)):
            winners.setdefault(r['game_id'], set()).add(r['player_name'])
    for row in rows:
        try:
            players = json.loads(row['state_json']).get('players', [])
        except ValueError:
            continue
        humans = [p['name'] for p in players if not p.get('is_bot')]
        levels = {str(p.get('difficulty')) for p in players if p.get('is_bot')}
        human_won = any(name in winners.get(row['id'], ()) for name in humans)
        for level in levels:
            played[level] += 1
            if human_won:
                human_wins[level] += 1
    order = [str(n) for n in range(ai_player.MIN_LEVEL, ai_player.MAX_LEVEL + 1)] + [ai_player.ADAPTIVE]
    items = []
    for level in order:
        if played[level] or level != ai_player.ADAPTIVE:
            strength = ai_player.LEVEL_STRENGTH[int(level) - 1] if level.isdigit() else None
            items.append({'level': level, 'games': played[level], 'human_wins': human_wins[level],
                          'human_win_rate': round(human_wins[level] / played[level] * 100, 1) if played[level] else None,
                          'strength': strength})
    return {'levels': items,
            'table': _table(['level', 'games', 'human_wins', 'human_win_rate', 'strength'],
                            [[i['level'], i['games'], i['human_wins'], i['human_win_rate'], i['strength']]
                             for i in items])}


# ===== Szavak, lépések, megtámadások =====

def _moves(conn, since, kinds):
    marks = ','.join('?' * len(kinds))
    return conn.execute(
        'SELECT m.game_id, m.player_name, m.action_type, m.details_json, m.created_at FROM game_moves m '
        f"WHERE date(m.created_at) >= ? AND m.action_type IN ({marks}) ORDER BY m.id DESC LIMIT ?",
        [since, *kinds, MAX_MOVE_ROWS]).fetchall()


def _details(raw):
    try:
        return json.loads(raw or '{}')
    except ValueError:
        return {}


def _stat_words(days):
    since = (auth.utcnow().date() - timedelta(days=days - 1)).isoformat()
    words, best, bingos = Counter(), [], 0
    with auth.transaction() as conn:
        for row in _moves(conn, since, ('place', 'challenge_accept')):
            details = _details(row['details_json'])
            for word in details.get('words', []):
                words[word] += 1
            if len(details.get('tiles', [])) >= 7:
                bingos += 1
            if details.get('score'):
                best.append((details['score'], row['game_id'], row['player_name'], details.get('words', [])))
    best.sort(key=lambda b: b[0], reverse=True)
    top_words = [{'word': w, 'count': n} for w, n in words.most_common(TOP_WORDS)]
    top_moves = [{'score': s, 'game_id': g, 'player': p, 'words': w} for s, g, p, w in best[:TOP_MOVES]]
    return {'top_words': top_words, 'top_moves': top_moves, 'bingos': bingos,
            'table': _table(['word', 'count'], [[w['word'], w['count']] for w in top_words])}


def _stat_challenges(days):
    """Megtámadásos játékok: hány lerakás ment át, hány lett elutasítva."""
    since = (auth.utcnow().date() - timedelta(days=days - 1)).isoformat()
    with auth.transaction() as conn:
        counts = {r['action_type']: r['n'] for r in conn.execute(
            'SELECT m.action_type, COUNT(*) AS n FROM game_moves m JOIN saved_games sg ON sg.id = m.game_id '
            "WHERE sg.challenge_mode = 1 AND date(m.created_at) >= ? AND m.action_type IN "
            "('challenge_accept', 'challenge_reject') GROUP BY m.action_type", (since,))}
        games = conn.execute('SELECT COUNT(*) FROM saved_games WHERE challenge_mode = 1 AND date(created_at) >= ?',
                             (since,)).fetchone()[0]
    accepted, rejected = counts.get('challenge_accept', 0), counts.get('challenge_reject', 0)
    total = accepted + rejected
    return {'games': games, 'accepted': accepted, 'rejected': rejected,
            'rejection_rate': round(rejected / total * 100, 1) if total else None,
            'table': _table(['accepted', 'rejected', 'rejection_rate'],
                            [[accepted, rejected, round(rejected / total * 100, 1) if total else None]])}


def _stat_practice(days):
    """A gyakorló módok használata: a szerveroldali API hívások száma naponta (a számlálók az első használattól élnek)."""
    span = admin_live._day_range(days)
    marks = ','.join('?' * len(PRACTICE_KEYS))
    with auth.transaction() as conn:
        rows = conn.execute(f'SELECT day, key, count FROM usage_counters WHERE day >= ? AND key IN ({marks})',
                            [span[0], *PRACTICE_KEYS]).fetchall()
    data = {key: {d: 0 for d in span} for key in PRACTICE_KEYS}
    for r in rows:
        data[r['key']][r['day']] = r['count']
    series = {key: [{'day': d, 'value': data[key][d]} for d in span] for key in PRACTICE_KEYS}
    totals = {key: sum(data[key].values()) for key in PRACTICE_KEYS}
    return {'days': span, 'series': series, 'totals': totals,
            'table': _table(['day'] + list(PRACTICE_KEYS), [[d] + [data[k][d] for k in PRACTICE_KEYS] for d in span])}


def _stat_review(days):
    span = admin_live._day_range(days)
    with auth.transaction() as conn:
        rows = conn.execute('SELECT date(created_at) AS day, COUNT(*) AS decisions, COALESCE(SUM(1 - verdict), 0) '
                            'AS rejections FROM word_reviews WHERE date(created_at) >= ? GROUP BY day',
                            (span[0],)).fetchall()
    by_day = {r['day']: r for r in rows}
    decisions = [{'day': d, 'value': by_day[d]['decisions'] if d in by_day else 0} for d in span]
    rejections = [{'day': d, 'value': by_day[d]['rejections'] if d in by_day else 0} for d in span]
    return {'days': span, 'decisions': decisions, 'rejections': rejections,
            'table': _series_table(span, decisions=decisions, rejections=rejections)}


def _stat_heatmap(days):
    """Napszakos / heti aktivitás: a lépések ideje hét napja × óra szerint (magyar idő)."""
    since = (auth.utcnow().date() - timedelta(days=days - 1)).isoformat()
    grid = [[0] * 24 for _ in range(7)]
    with auth.transaction() as conn:
        stamps = [r['created_at'] for r in conn.execute(
            'SELECT created_at FROM game_moves WHERE date(created_at) >= ? ORDER BY id DESC LIMIT ?',
            (since, MAX_MOVE_ROWS))]
        stamps += [r['created_at'] for r in conn.execute(
            'SELECT created_at FROM login_events WHERE success = 1 AND date(created_at) >= ? LIMIT ?',
            (since, MAX_MOVE_ROWS))]
    from datetime import timezone
    for stamp in stamps:
        moment = auth.parse_ts(stamp)
        if moment is None:
            continue
        moment = moment.replace(tzinfo=timezone.utc)
        if _TZ is not None:
            moment = moment.astimezone(_TZ)
        grid[moment.weekday()][moment.hour] += 1
    return {'grid': grid, 'max': max((max(row) for row in grid), default=0), 'timezone': 'Europe/Budapest' if _TZ else 'UTC',
            'table': _table(['weekday'] + [str(h) for h in range(24)], [[d] + grid[d] for d in range(7)])}


# ===== Napi feladvány =====

SCORE_BUCKETS = (('perfect', 1.0), ('high', 0.9), ('good', 0.75), ('half', 0.5), ('low', 0.0))
ATTEMPT_BUCKETS = ('1', '2', '3', '4-5', '6+')


def _attempt_bucket(attempts):
    if attempts <= 3:
        return str(max(1, attempts))
    return '4-5' if attempts <= 5 else '6+'


def _check_date(date_str, allow_future=False):
    if not daily.is_valid_date(date_str):
        raise AdminError('Érvénytelen dátum.', 400, field='date')
    if not allow_future and date_str > daily.today_str():
        raise AdminError('Érvénytelen dátum.', 400, field='date')
    return date_str


def daily_day(date_str=None):
    """Egy nap feladványa: tábla, kéz, legjobb lépés, résztvevők, próbálkozások és pontszámok eloszlása, ranglista."""
    date_str = _check_date(date_str or daily.today_str())
    puzzle = auth.get_daily_puzzle(date_str)
    with auth.transaction() as conn:
        rows = conn.execute(
            'SELECT ds.user_id, u.display_name, ds.best_score, ds.attempts, ds.revealed, ds.first_best_at '
            'FROM daily_scores ds LEFT JOIN users u ON u.id = ds.user_id WHERE ds.puzzle_date = ? '
            'ORDER BY ds.best_score DESC, ds.attempts ASC, ds.first_best_at ASC', (date_str,)).fetchall()
    best = puzzle['best_score'] if puzzle else None
    attempts = Counter()
    score_buckets = Counter()
    for r in rows:
        if r['attempts'] > 0:
            attempts[_attempt_bucket(r['attempts'])] += 1
            if best:
                ratio = r['best_score'] / best
                score_buckets[next(name for name, floor in SCORE_BUCKETS if ratio >= floor)] += 1
    return {
        'date': date_str, 'puzzle': puzzle, 'participants': sum(1 for r in rows if r['attempts'] > 0),
        'revealed': sum(1 for r in rows if r['revealed']),
        'attempts': {b: attempts[b] for b in ATTEMPT_BUCKETS},
        'scores': {name: score_buckets[name] for name, _floor in SCORE_BUCKETS},
        'leaderboard': [{'rank': i + 1, 'user_id': r['user_id'], 'display_name': r['display_name'],
                         'best_score': r['best_score'], 'attempts': r['attempts'], 'revealed': bool(r['revealed']),
                         'at': r['first_best_at']} for i, r in enumerate(rows[:100])],
    }


def daily_archive(days=30):
    """A korábbi napok statisztikái: résztvevők, legjobb pont, átlag, próbálkozások."""
    days = admin.int_field(days, 'days', 1, 365)
    since = (auth.utcnow().date() - timedelta(days=days)).isoformat()
    with auth.transaction() as conn:
        rows = conn.execute(
            'SELECT p.puzzle_date AS date, p.best_score AS best_score, '
            'COALESCE(SUM(CASE WHEN ds.attempts > 0 THEN 1 ELSE 0 END), 0) AS participants, '
            'COALESCE(SUM(ds.attempts), 0) AS attempts, AVG(CASE WHEN ds.attempts > 0 THEN ds.best_score END) AS avg_score '
            'FROM daily_puzzles p LEFT JOIN daily_scores ds ON ds.puzzle_date = p.puzzle_date '
            'WHERE p.puzzle_date >= ? GROUP BY p.puzzle_date ORDER BY p.puzzle_date DESC', (since,)).fetchall()
    return [{'date': r['date'], 'best_score': r['best_score'], 'participants': r['participants'],
             'attempts': r['attempts'], 'avg_score': round(r['avg_score'], 1) if r['avg_score'] is not None else None}
            for r in rows]


def daily_preview(date_str):
    """Egy (akár jövőbeli) nap feladványa mentés nélkül, a nehézség becslésével (a legjobb pontszám). Másodpercekig
    tarthat, ezért a hívó naponként egyet kér."""
    date_str = _check_date(date_str, allow_future=True)
    server = admin_live.srv()
    try:
        puzzle = daily.generate_puzzle(date_str, yield_fn=lambda: server.socketio.sleep(0))
    except RuntimeError:
        raise AdminError('A feladvány most nem állítható elő.', 503) from None
    return {'date': date_str, 'board': puzzle['board'], 'rack': puzzle['rack'], 'best_score': puzzle['best_score'],
            'best': puzzle['best']}


def daily_regenerate(ctx, date_str, reason, reset_scores=False, sudo_ok=False):
    """Egy nap feladványának újragenerálása (pl. a szótár módosítása után). Ha már voltak próbálkozások, sudo kell, és
    a ranglista torzulhat (`reset_scores`: a nap eredményei törlődnek)."""
    reset_scores = admin.bool_field(reset_scores, 'reset_scores')
    date_str = _check_date(date_str, allow_future=True)
    with auth.transaction() as conn:
        attempts = conn.execute('SELECT COUNT(*) FROM daily_scores WHERE puzzle_date = ? AND attempts > 0',
                                (date_str,)).fetchone()[0]
    if attempts and not sudo_ok:
        raise AdminError('Ehhez a művelethez jelszó-megerősítés (sudo) szükséges.', 401, sudo_required=True,
                         attempts=attempts)
    server = admin_live.srv()
    try:
        puzzle = daily.generate_puzzle(date_str, yield_fn=lambda: server.socketio.sleep(0))
    except RuntimeError:
        raise AdminError('A feladvány most nem állítható elő.', 503) from None
    with admin.action(ctx, 'daily.regenerate', 'daily', date_str, reason=reason,
                      details={'attempts': attempts, 'reset_scores': reset_scores}) as act:
        old = act.conn.execute('SELECT best_score FROM daily_puzzles WHERE puzzle_date = ?', (date_str,)).fetchone()
        act.conn.execute('DELETE FROM daily_puzzles WHERE puzzle_date = ?', (date_str,))
        act.conn.execute(
            'INSERT INTO daily_puzzles (puzzle_date, board_json, rack_json, best_score, best_json) VALUES (?, ?, ?, ?, ?)',
            (date_str, json.dumps(puzzle['board']), json.dumps(puzzle['rack']), puzzle['best_score'],
             json.dumps(puzzle['best'], ensure_ascii=False)))
        if reset_scores:
            act.details['scores_deleted'] = act.conn.execute('DELETE FROM daily_scores WHERE puzzle_date = ?',
                                                             (date_str,)).rowcount
        act.details['before'] = {'best_score': old['best_score'] if old else None}
        act.details['after'] = {'best_score': puzzle['best_score']}
    return {'date': date_str, 'best_score': puzzle['best_score'], 'attempts': attempts}


def daily_delete_score(ctx, date_str, user_id, reason):
    """A ranglista-bejegyzés törlése (csalás esetén)."""
    date_str = _check_date(date_str)
    with admin.action(ctx, 'daily.score_delete', 'daily', date_str, reason=reason) as act:
        row = act.conn.execute('SELECT best_score, attempts FROM daily_scores WHERE puzzle_date = ? AND user_id = ?',
                               (date_str, user_id)).fetchone()
        if row is None:
            raise AdminError('A bejegyzés nem található.', 404)
        act.conn.execute('DELETE FROM daily_scores WHERE puzzle_date = ? AND user_id = ?', (date_str, user_id))
        act.details['user_id'] = user_id
        act.details['before'] = {'best_score': row['best_score'], 'attempts': row['attempts']}


def daily_reset_revealed(ctx, date_str, user_id, reason):
    """A `revealed` jelző visszaállítása (a felhasználó újra rangsorolt próbálkozhat)."""
    date_str = _check_date(date_str)
    with admin.action(ctx, 'daily.revealed_reset', 'daily', date_str, reason=reason) as act:
        row = act.conn.execute('SELECT revealed FROM daily_scores WHERE puzzle_date = ? AND user_id = ?',
                               (date_str, user_id)).fetchone()
        if row is None or not row['revealed']:
            raise AdminError('A bejegyzésnél nincs megnézett megoldás.', 404)
        act.conn.execute('UPDATE daily_scores SET revealed = 0 WHERE puzzle_date = ? AND user_id = ?',
                         (date_str, user_id))
        act.details['user_id'] = user_id
