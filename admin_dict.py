"""Admin panel: a szótár — szó-vizsgáló, kizárt szavak, felülbírálatok, szótár-építő felügyelet, gyorsítótárak.

A tartós lista (`dict/hu_rejected.txt`) git-ben követett fájl: a panel módosításai a szerveren élnek (a fájl
atomikusan íródik, a memóriában azonnal érvényesülnek), a repóba való átvezetéshez diff tölthető le.
"""
import difflib
import os
import random
import re
import tempfile
import time
from datetime import timedelta

import admin
import ai_player
import auth
import dictionary
import practice
import settings
import word_review
from admin import AdminError
from tiles import TILE_VALUES, tokenize_word

MAX_BULK_WORDS = 5000
MAX_WORD_LEN = 15
_WORD_SPLIT_RE = re.compile(r'[\s,;]+')
SECOND_OPINION_DEFAULT = 20
SUSPICIOUS_MIN_DECISIONS = 50
SUSPICIOUS_INVALID_SHARE = 80.0
LIST_PAGE_DEFAULT = 100
CACHE_NAMES = ('valid', 'short_words', 'vocabulary', 'analysis')
# Az eredeti (a repóban lévő) lista tartalma: a „Változások letöltése” diff ehhez viszonyít. A szerver
# indulásakor olvasódik be, így a diff azt mutatja, mi változott azóta a panelről.
_baseline = {'path': None, 'text': None}


# ===== Szó normalizálása =====

def normalize(raw):
    """Szó → nagybetűs, ellenőrzött alak; AdminError, ha nem lehet magyar szó a táblán."""
    word = raw.strip().upper() if isinstance(raw, str) else ''
    if not word or len(word) > MAX_WORD_LEN:
        raise AdminError('Érvénytelen szó.', 400, field='word')
    if tokenize_word(word) is None:
        raise AdminError('A szó nem rakható ki a táblán.', 400, field='word')
    return word


def parse_words(text):
    """Beillesztett lista (szóköz / vessző / sortörés) → egyedi, kisbetűs szavak sorrendben; (szavak, hibás)."""
    if not isinstance(text, str):
        raise AdminError('Érvénytelen kérés.', 400, field='words')
    seen, words, bad = set(), [], []
    for line in text.splitlines():
        for part in _WORD_SPLIT_RE.split(line.split('#', 1)[0]):     # a `#` után megjegyzés áll (mint a fájlban)
            part = part.strip().lower()
            if not part:
                continue
            if len(part) > MAX_WORD_LEN or tokenize_word(part.upper()) is None:
                bad.append(part[:30])
                continue
            if part not in seen:
                seen.add(part)
                words.append(part)
    if len(words) + len(bad) > MAX_BULK_WORDS:
        raise AdminError('Túl sok szó egyszerre.', 400, field='words')
    return words, bad


# ===== Szó-vizsgáló =====

def _votes(conn, word):
    rows = conn.execute(
        'SELECT r.user_id, u.display_name, u.review_blocked, r.verdict, r.created_at FROM word_reviews r '
        'LEFT JOIN users u ON u.id = r.user_id WHERE r.word = ? ORDER BY r.created_at', (word,)).fetchall()
    good = sum(1 for r in rows if r['verdict'] and not r['review_blocked'])
    bad = sum(1 for r in rows if not r['verdict'] and not r['review_blocked'])
    return [{'user_id': r['user_id'], 'display_name': r['display_name'], 'valid': bool(r['verdict']),
             'blocked': bool(r['review_blocked']), 'at': r['created_at']} for r in rows], good, bad


def inspect_word(raw):
    """A teljes döntési lánc egy szóra: érvényes-e, zsetonok és pont, a szótár levezetései (szótő, előtag, toldalékok,
    kockázat), a használati lista, az elutasítások (lista, szavazatok, felülbírálat), a robot szókincse,
    javaslatok."""
    word = normalize(raw)
    lower = word.lower()
    tiles = tokenize_word(word)
    checker = dictionary.get_checker()
    result = {
        'word': word, 'tiles': tiles, 'score': sum(TILE_VALUES[t] for t in tiles),
        'valid': dictionary.is_word_valid(word), 'dictionary_available': dictionary.is_available(),
    }
    vowels = bool(re.search('[aáeéiíoóöőuúüű]', lower))
    result['has_vowel'] = vowels or lower in dictionary._VOWELLESS_INTERJECTIONS
    result['in_dictionary'] = bool(checker and checker.check(lower)) if checker else None
    if checker:
        result['derivations'] = checker.explain(lower)
        result['needs_attestation'] = bool(checker.needs_attestation(lower))
        attested = getattr(checker, '_attested', None)
        result['attested'] = None if attested is None else lower in attested
    else:
        result['derivations'], result['needs_attestation'], result['attested'] = [], None, None
    allow, reject, additions = dictionary.admin_lists()
    result['rejected_listed'] = lower in dictionary.listed_rejected()
    result['rejected_voted'] = lower in dictionary.voted_rejected()
    result['override'] = None
    result['addition'] = lower in additions
    with auth.transaction() as conn:
        row = conn.execute('SELECT o.verdict, o.reason, o.created_at, u.display_name AS admin_name '
                           'FROM word_overrides o LEFT JOIN users u ON u.id = o.admin_user_id WHERE o.word = ?',
                           (lower,)).fetchone()
        if row:
            result['override'] = {'verdict': row['verdict'], 'reason': row['reason'], 'at': row['created_at'],
                                  'admin': row['admin_name']}
        votes, good, bad = _votes(conn, lower)
    result['votes'] = votes
    result['balance'] = {'good': good, 'bad': bad, 'threshold': word_review.threshold(), 'diff': bad - good}
    vocabulary = ai_player.get_vocabulary()
    result['in_vocabulary'] = word in vocabulary
    result['suggestions'] = [] if result['valid'] else dictionary.suggest_words(word, limit=8)
    return result


# ===== A tartós lista (dict/hu_rejected.txt) =====

def _read_list_text():
    try:
        with open(dictionary.rejected_path(), encoding='utf-8') as fh:
            return fh.read()
    except OSError:
        return ''


def remember_baseline():
    """A lista jelenlegi tartalmának megjegyzése a diffhez (szerverindításkor; már rögzítettet nem ír felül)."""
    path = dictionary.rejected_path()
    if _baseline['path'] != path:
        _baseline.update(path=path, text=_read_list_text())


def _write_list_text(text):
    """Atomikus írás: ideiglenes fájl ugyanabban a mappában, majd átnevezés."""
    path = dictionary.rejected_path()
    folder = os.path.dirname(path) or '.'
    fd, tmp = tempfile.mkstemp(prefix='.hu_rejected-', suffix='.tmp', dir=folder)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as fh:
            fh.write(text)
        os.replace(tmp, path)
    except Exception:
        if os.path.exists(tmp):
            os.remove(tmp)
        raise


def list_diff():
    """A tartós lista változásai a szerver indulása óta (unified diff) — a repóba való átvezetéshez."""
    remember_baseline()
    old = (_baseline['text'] or '').splitlines(keepends=True)
    new = _read_list_text().splitlines(keepends=True)
    diff = ''.join(difflib.unified_diff(old, new, 'a/dict/hu_rejected.txt', 'b/dict/hu_rejected.txt'))
    return diff


def list_rejected(args):
    """A kizárt szavak: forrás (`listed` tartós lista / `voted` szavazatok / összes), keresés és lapozás."""
    source = args.get('source') or 'all'
    if source not in ('all', 'listed', 'voted'):
        raise AdminError('Érvénytelen szűrő.', 400)
    q = (args.get('q') or '').strip().lower()
    listed = dictionary.listed_rejected()
    voted = dictionary.voted_rejected()
    allow = dictionary.admin_lists()[0]
    words = set()
    if source in ('all', 'listed'):
        words |= listed
    if source in ('all', 'voted'):
        words |= voted
    if q:
        words = {w for w in words if q in w}
    ordered = sorted(words)
    limit, offset = admin.page_args(args, default=LIST_PAGE_DEFAULT, maximum=500)
    page = ordered[offset:offset + limit]
    with auth.transaction() as conn:
        counts = {}
        if page:
            marks = ','.join('?' * len(page))
            for r in conn.execute(
                    f'SELECT r.word AS word, COALESCE(SUM(1 - r.verdict), 0) AS bad, COALESCE(SUM(r.verdict), 0) AS good '
                    f'FROM word_reviews r JOIN users u ON u.id = r.user_id AND u.review_blocked = 0 '
                    f'WHERE r.word IN ({marks}) GROUP BY r.word', page):
                counts[r['word']] = (r['good'], r['bad'])
    items = [{'word': w.upper(), 'listed': w in listed, 'voted': w in voted, 'allowed': w in allow,
              'good': counts.get(w, (0, 0))[0], 'bad': counts.get(w, (0, 0))[1]} for w in page]
    return {'items': items, 'total': len(ordered), 'limit': limit, 'offset': offset,
            'counts': {'listed': len(listed), 'voted': len(voted)}}


def classify_words(words, assume_valid=False):
    """Beillesztett szavak besorolása a tartós listához: {'new', 'already', 'invalid'} (szóalakok listái).
    `new`: a szótár most elfogadja, és még nincs a listán; `invalid`: most sem érvényes (nincs mit kizárni).
    `assume_valid`: a szavak érvényességét nem vizsgáljuk (a szavazatból kizárt szó már nem érvényes)."""
    listed = dictionary.listed_rejected()
    already = [w for w in words if w in listed]
    candidates = [w for w in words if w not in listed]
    valid = (set(candidates) if assume_valid
             else {w.lower() for w in dictionary.filter_valid({w.upper() for w in candidates})})
    new = [w for w in candidates if w in valid]
    invalid = [w for w in candidates if w not in valid]
    return {'new': new, 'already': already, 'invalid': invalid}


def preview_add(text):
    words, bad = parse_words(text)
    result = classify_words(words)
    result['bad'] = bad
    return result


def add_rejected(ctx, text, reason, assume_valid=False):
    """Szavak felvétele a tartós listára (csak a most érvényes, még nem listázott szavak). Visszatér: a besorolás."""
    words, bad = parse_words(text)
    result = classify_words(words, assume_valid)
    result['bad'] = bad
    if not result['new']:
        raise AdminError('Nincs felvehető szó (nincs olyan, amely most érvényes és még nincs a listán).', 409,
                         field='words')
    remember_baseline()
    before = _read_list_text()
    stamp = auth.utcnow().strftime('%Y-%m-%d')
    addition = ('' if before.endswith('\n') or not before else '\n') + f'# --- admin panel, {stamp} ---\n' \
        + ''.join(f'{w}\n' for w in sorted(result['new']))
    try:
        with admin.action(ctx, 'dict.reject_add', 'word', None, reason=reason,
                          details={'count': len(result['new']), 'words': result['new'][:500],
                                   'skipped_invalid': len(result['invalid']), 'skipped_listed': len(result['already'])}):
            _write_list_text(before + addition)
        dictionary.reload_rejected()
    except Exception:
        _write_list_text(before)       # a napló hibája esetén a fájl is visszaáll
        dictionary.reload_rejected()
        raise
    return result


def remove_rejected(ctx, text, reason):
    """Szavak eltávolítása a tartós listáról. Visszatér: a ténylegesen eltávolított szavak."""
    words, _bad = parse_words(text)
    listed = dictionary.listed_rejected()
    targets = [w for w in words if w in listed]
    if not targets:
        raise AdminError('Egyik szó sincs a tartós listán.', 404, field='words')
    remember_baseline()
    before = _read_list_text()
    drop = set(targets)
    kept = []
    for line in before.splitlines(keepends=True):
        word = line.split('#', 1)[0].strip().lower()
        if word and word in drop:
            continue
        kept.append(line)
    try:
        with admin.action(ctx, 'dict.reject_remove', 'word', None, reason=reason,
                          details={'count': len(targets), 'words': targets[:500]}):
            _write_list_text(''.join(kept))
        dictionary.reload_rejected()
    except Exception:
        _write_list_text(before)
        dictionary.reload_rejected()
        raise
    return targets


# ===== Felülbírálatok és saját szavak =====

def refresh_lists():
    """Az admin felülbírálatok újratöltése az adatbázisból a szótárba."""
    dictionary.set_admin_lists(*auth.get_admin_word_lists())


def list_overrides():
    with auth.transaction() as conn:
        rows = conn.execute('SELECT o.word, o.verdict, o.reason, o.created_at, u.display_name AS admin_name '
                            'FROM word_overrides o LEFT JOIN users u ON u.id = o.admin_user_id '
                            'ORDER BY o.created_at DESC, o.word').fetchall()
    return [{'word': r['word'].upper(), 'verdict': r['verdict'], 'reason': r['reason'], 'at': r['created_at'],
             'admin': r['admin_name']} for r in rows]


def set_override(ctx, raw, verdict, reason):
    """Admin felülbírálat: `allow` (a szavazatok / a lista ellenére érvényes — de csak a szótárban szereplő szó) vagy
    `reject` (érvénytelen)."""
    if verdict not in ('allow', 'reject'):
        raise AdminError('Érvénytelen kérés.', 400, field='verdict')
    word = normalize(raw)
    lower = word.lower()
    checker = dictionary.get_checker()
    if verdict == 'allow' and (checker is None or not checker.check(lower)):
        raise AdminError('A szó nincs a szótárban: az engedélyezés nem teszi érvényessé (ahhoz saját szó kell).', 409,
                         field='word')
    with admin.action(ctx, 'dict.override', 'word', lower, reason=reason) as act:
        previous = act.conn.execute('SELECT verdict FROM word_overrides WHERE word = ?', (lower,)).fetchone()
        act.conn.execute(
            'INSERT INTO word_overrides (word, verdict, admin_user_id, reason) VALUES (?, ?, ?, ?) '
            "ON CONFLICT(word) DO UPDATE SET verdict = excluded.verdict, admin_user_id = excluded.admin_user_id, "
            "reason = excluded.reason, created_at = datetime('now')",
            (lower, verdict, ctx.admin_user_id, admin.normalize_reason(reason)))
        act.details['before'] = {'verdict': previous['verdict'] if previous else None}
        act.details['after'] = {'verdict': verdict}
    refresh_lists()
    return word


def remove_override(ctx, raw, reason):
    word = normalize(raw)
    lower = word.lower()
    with admin.action(ctx, 'dict.override_remove', 'word', lower, reason=reason) as act:
        row = act.conn.execute('SELECT verdict FROM word_overrides WHERE word = ?', (lower,)).fetchone()
        if row is None:
            raise AdminError('Ehhez a szóhoz nincs felülbírálat.', 404)
        act.conn.execute('DELETE FROM word_overrides WHERE word = ?', (lower,))
        act.details['before'] = {'verdict': row['verdict']}
    refresh_lists()


def list_additions():
    with auth.transaction() as conn:
        rows = conn.execute('SELECT a.word, a.reason, a.created_at, u.display_name AS admin_name '
                            'FROM word_additions a LEFT JOIN users u ON u.id = a.admin_user_id '
                            'ORDER BY a.word').fetchall()
    return [{'word': r['word'].upper(), 'reason': r['reason'], 'at': r['created_at'], 'admin': r['admin_name']}
            for r in rows]


def add_addition(ctx, raw, reason):
    """Saját szó: a szótárban nem szereplő, de a játékban elfogadott szó (szavanként indoklással). A robot
    szókincsébe nem kerül automatikusan."""
    word = normalize(raw)
    lower = word.lower()
    if not re.search('[aáeéiíoóöőuúüű]', lower) or len(tokenize_word(word)) < 2:
        raise AdminError('A saját szó legalább két zsetonos és magánhangzós kell legyen.', 400, field='word')
    with admin.action(ctx, 'dict.addition_add', 'word', lower, reason=reason) as act:
        if act.conn.execute('SELECT 1 FROM word_additions WHERE word = ?', (lower,)).fetchone():
            raise AdminError('Ez a szó már szerepel a saját szavak között.', 409, field='word')
        act.conn.execute('INSERT INTO word_additions (word, admin_user_id, reason) VALUES (?, ?, ?)',
                         (lower, ctx.admin_user_id, admin.normalize_reason(reason)))
    refresh_lists()
    return word


def remove_addition(ctx, raw, reason):
    word = normalize(raw)
    lower = word.lower()
    with admin.action(ctx, 'dict.addition_remove', 'word', lower, reason=reason) as act:
        if not act.conn.execute('DELETE FROM word_additions WHERE word = ?', (lower,)).rowcount:
            raise AdminError('A szó nem szerepel a saját szavak között.', 404)
    refresh_lists()


# ===== Szavazatok =====

def delete_votes(ctx, raw, reason):
    """Egy szó összes szavazatának törlése (a kizárás újraszámolódik). Visszatér: a törölt szavazatok száma."""
    word = normalize(raw)
    lower = word.lower()
    with admin.action(ctx, 'dict.votes_delete', 'word', lower, reason=reason) as act:
        votes = act.conn.execute('SELECT user_id, verdict FROM word_reviews WHERE word = ?', (lower,)).fetchall()
        if not votes:
            raise AdminError('Ehhez a szóhoz nincs szavazat.', 404)
        act.conn.execute('DELETE FROM word_reviews WHERE word = ?', (lower,))
        act.details['count'] = len(votes)
        act.details['votes'] = [{'user_id': v['user_id'], 'valid': bool(v['verdict'])} for v in votes][:200]
    word_review.refresh()
    return len(votes)


def export_votes(ctx, reason):
    """A szavazatokból kizárt szavak átvezetése a tartós listába (a szavazatok megmaradnak). Visszatér: a besorolás."""
    allow = dictionary.admin_lists()[0]
    words = sorted(w for w in dictionary.voted_rejected() if w not in allow)
    if not words:
        raise AdminError('Nincs szavazatból kizárt szó.', 409)
    return add_rejected(ctx, '\n'.join(words), reason, assume_valid=True)


def second_opinion(count=SECOND_OPINION_DEFAULT, rng=None):
    """Véletlen minta a tartós listáról újraértékelésre: a szó, zsetonjai, és hogy a szótár nélküle elfogadná-e."""
    rng = rng or random
    listed = sorted(dictionary.listed_rejected())
    count = admin.int_field(count, 'n', 1, 100)
    sample = rng.sample(listed, min(count, len(listed)))
    checker = dictionary.get_checker()
    return [{'word': w.upper(), 'tiles': tokenize_word(w.upper()),
             'dictionary_accepts': bool(checker.check(w)) if checker else None} for w in sample]


# ===== Szótár-építő felügyelet =====

def review_summary(days=14):
    """A szótár-építő összesítője: napi döntések, átnézett és kizárt szavak, aktív bírálók."""
    days = admin.int_field(days, 'days', 1, 365)
    since = auth.format_ts(auth.utcnow() - timedelta(days=days))
    with auth.transaction() as conn:
        daily = [dict(r) for r in conn.execute(
            'SELECT date(created_at) AS day, COUNT(*) AS decisions, COALESCE(SUM(1 - verdict), 0) AS rejections '
            'FROM word_reviews WHERE created_at >= ? GROUP BY day ORDER BY day', (since,))]
        totals = conn.execute(
            'SELECT COUNT(*) AS decisions, COUNT(DISTINCT word) AS words, COUNT(DISTINCT user_id) AS reviewers '
            'FROM word_reviews').fetchone()
        active = conn.execute('SELECT COUNT(DISTINCT user_id) FROM word_reviews WHERE created_at >= ?',
                              (auth.format_ts(auth.utcnow() - timedelta(days=7)),)).fetchone()[0]
    return {'daily': daily, 'decisions': totals['decisions'], 'words_reviewed': totals['words'],
            'reviewers': totals['reviewers'], 'active_reviewers': active,
            'rejected': len(dictionary.voted_rejected()), 'listed': len(dictionary.listed_rejected()),
            'threshold': word_review.threshold()}


def reviewers():
    """A bírálók: döntések száma, „nem szó” arány, egyezés a többiekkel; a gyanúsak (50 döntés felett > 80% „nem szó”)
    kiemelve."""
    with auth.transaction() as conn:
        rows = conn.execute(
            'SELECT r.user_id AS user_id, u.display_name AS display_name, u.review_blocked AS blocked, '
            'COUNT(*) AS total, COALESCE(SUM(1 - r.verdict), 0) AS bad, MAX(r.created_at) AS last_at, '
            'COALESCE(SUM(CASE WHEN t.n > 1 THEN 1 ELSE 0 END), 0) AS compared, '
            'COALESCE(SUM(CASE WHEN t.n > 1 AND ((r.verdict = 1 AND (t.good - 1) >= t.bad) '
            'OR (r.verdict = 0 AND (t.bad - 1) >= t.good)) THEN 1 ELSE 0 END), 0) AS agreed '
            'FROM word_reviews r JOIN users u ON u.id = r.user_id '
            'JOIN (SELECT word, COUNT(*) AS n, SUM(verdict) AS good, SUM(1 - verdict) AS bad '
            'FROM word_reviews GROUP BY word) t ON t.word = r.word '
            'GROUP BY r.user_id ORDER BY total DESC').fetchall()
    items = []
    for r in rows:
        share = round(r['bad'] / r['total'] * 100, 1) if r['total'] else 0
        items.append({'user_id': r['user_id'], 'display_name': r['display_name'], 'blocked': bool(r['blocked']),
                      'total': r['total'], 'invalid': r['bad'], 'invalid_share': share,
                      'agreement': round(r['agreed'] / r['compared'] * 100, 1) if r['compared'] else None,
                      'compared': r['compared'], 'last_at': r['last_at'],
                      'suspicious': r['total'] >= SUSPICIOUS_MIN_DECISIONS and share > SUSPICIOUS_INVALID_SHARE})
    return {'items': items}


# ===== Gyorsítótárak =====

def clear_cache(ctx, which, reason):
    """Egy gyorsítótár ürítése, az előállítás idejének mérésével. Visszatér: {'which', 'seconds', 'detail'}."""
    if which not in CACHE_NAMES:
        raise AdminError('Ismeretlen gyorsítótár.', 400, field='which')
    with admin.action(ctx, 'dict.cache_clear', 'cache', which, reason=reason):
        pass
    started = time.time()
    detail = None
    if which == 'valid':
        detail = dictionary.clear_valid_cache()
    elif which == 'short_words':
        practice._short_cache.clear()
        for length in practice.SHORT_LENGTHS:
            practice.short_words(length)
    elif which == 'vocabulary':
        ai_player.set_vocabulary(None)
        detail = len(ai_player.get_vocabulary())
    else:
        with auth.transaction() as conn:
            detail = conn.execute('DELETE FROM game_analysis').rowcount
    return {'which': which, 'seconds': round(time.time() - started, 2), 'detail': detail}


def threshold_info():
    return {'value': word_review.threshold(), 'default': settings.default_of('word_reject_threshold'),
            'overridden': settings.is_overridden('word_reject_threshold')}
