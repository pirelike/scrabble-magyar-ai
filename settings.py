"""Futásidejű beállítások (funkciókapcsolók), újraindítás nélkül módosíthatók az admin panelről.

A beállítások az `app_settings` táblában élnek (`cfg.<kulcs>` → JSON érték, `cfgmeta.<kulcs>` → ki és mikor
módosította), a szerver pedig gyorsítótárazott `get(kulcs, alapérték)` hívással olvassa őket. Nincs
felülbírálat → az alapérték számít (a környezeti változó vagy a kódban lévő konstans). A módosítás
(`store`) a hívó tranzakciójában íródik, a gyorsítótárat utána kell érvényteleníteni (`invalidate`).
"""
import json

import auth
import config

_PREFIX = 'cfg.'
_META_PREFIX = 'cfgmeta.'
_MISSING = object()

# Egy felülbírálat típusai: bool, int, float, choice (választható értékek listája), json (ellenőrzött szerkezet)
GROUPS = ('access', 'rooms', 'bots', 'features', 'dictionary', 'timing', 'chat', 'limits', 'logging')

# kulcs → leírás. `default`: hívható is lehet (a konfigurációból számolt alapérték)
DEFINITIONS = {
    'registration_open': {'group': 'access', 'type': 'bool', 'default': True},
    'guest_allowed': {'group': 'access', 'type': 'bool', 'default': True},
    'room_creation': {'group': 'rooms', 'type': 'choice', 'default': 'everyone',
                      'choices': ('everyone', 'registered', 'none')},
    'max_spectators': {'group': 'rooms', 'type': 'int', 'default': 30, 'min': 1, 'max': 200},
    'default_hint_limit': {'group': 'rooms', 'type': 'choice', 'default': 3, 'choices': (0, 1, 3, 5, 10)},
    'bots_enabled': {'group': 'bots', 'type': 'bool', 'default': True},
    'max_bots': {'group': 'bots', 'type': 'int', 'default': 3, 'min': 0, 'max': 3},
    'default_bot_level': {'group': 'bots', 'type': 'int', 'default': 6, 'min': 1, 'max': 10},
    'bot_think_multiplier': {'group': 'bots', 'type': 'float', 'default': 1.0, 'min': 0.0, 'max': 5.0},
    'feature_daily': {'group': 'features', 'type': 'bool', 'default': True},
    'feature_async': {'group': 'features', 'type': 'bool', 'default': True},
    'feature_practice': {'group': 'features', 'type': 'bool', 'default': True},
    'feature_word_review': {'group': 'features', 'type': 'bool', 'default': True},
    'word_reject_threshold': {'group': 'dictionary', 'type': 'int', 'default': lambda: config.WORD_REJECT_THRESHOLD,
                              'min': 1, 'max': 50},
    'grace_disconnect': {'group': 'timing', 'type': 'int', 'default': 120, 'min': 15, 'max': 1800},
    'grace_waiting_owner': {'group': 'timing', 'type': 'int', 'default': 600, 'min': 60, 'max': 7200},
    'chat_max_length': {'group': 'chat', 'type': 'int', 'default': 200, 'min': 20, 'max': 500},
    'chat_rate_count': {'group': 'chat', 'type': 'int', 'default': 10, 'min': 1, 'max': 60},
    'chat_rate_window': {'group': 'chat', 'type': 'int', 'default': 10, 'min': 1, 'max': 120},
    'banned_word_action': {'group': 'chat', 'type': 'choice', 'default': 'mask', 'choices': ('mask', 'drop')},
    'chat_log_enabled': {'group': 'logging', 'type': 'bool', 'default': False},
    'chat_log_days': {'group': 'logging', 'type': 'int', 'default': 14, 'min': 1, 'max': 365},
    'backup_daily': {'group': 'logging', 'type': 'bool', 'default': False},
    'backup_keep': {'group': 'logging', 'type': 'int', 'default': 7, 'min': 1, 'max': 60},
    'rate_limits_http': {'group': 'limits', 'type': 'limits', 'default': dict},
    'rate_limits_socket': {'group': 'limits', 'type': 'limits', 'default': dict},
}

# A karbantartási mód külön tárolódik (nem a beállítások oldalán szerkeszthető, hanem a Kommunikáció alatt)
MAINTENANCE_KEY = 'maintenance'
DEFINITIONS[MAINTENANCE_KEY] = {'group': 'access', 'type': 'json', 'default': dict, 'hidden': True}

_cache = {'path': None, 'values': {}, 'meta': {}}


class SettingError(ValueError):
    """Érvénytelen beállítás (a HTTP réteg 400-zal válaszol)."""


def invalidate():
    """A gyorsítótár eldobása: a következő olvasás újratölti az adatbázisból."""
    _cache['path'] = None


def _load():
    if _cache['path'] == auth.DB_PATH:
        return
    values, meta = {}, {}
    try:
        with auth.transaction() as conn:
            rows = conn.execute("SELECT key, value FROM app_settings WHERE key LIKE 'cfg%'").fetchall()
    except Exception:    # nincs még adatbázis / séma: minden alapértelmezett
        rows = []
    for row in rows:
        key, raw = row['key'], row['value']
        try:
            parsed = json.loads(raw)
        except ValueError:
            continue
        if key.startswith(_PREFIX):
            values[key[len(_PREFIX):]] = parsed
        elif key.startswith(_META_PREFIX):
            meta[key[len(_META_PREFIX):]] = parsed
    _cache.update(path=auth.DB_PATH, values=values, meta=meta)


def definition(key):
    return DEFINITIONS.get(key)


def default_of(key):
    default = DEFINITIONS[key]['default']
    return default() if callable(default) else default


def is_overridden(key):
    _load()
    return key in _cache['values']


def get(key, default=_MISSING):
    """A beállítás értéke: a felülbírálat, ennek híján `default` (ha megadták), különben az alapérték."""
    _load()
    if key in _cache['values']:
        return _cache['values'][key]
    if default is not _MISSING:
        return default
    return default_of(key)


# ===== Ellenőrzés =====

def validate_limits(value):
    """Forgalomkorlát-felülbírálat: {név: [max_kérés, ablak_mp]} (csak ismert nevek, értelmes határok)."""
    if not isinstance(value, dict):
        raise SettingError('Érvénytelen forgalomkorlát-lista.')
    clean = {}
    for name, pair in value.items():
        if (not isinstance(name, str) or not isinstance(pair, (list, tuple)) or len(pair) != 2
                or any(isinstance(n, bool) or not isinstance(n, int) for n in pair)):
            raise SettingError('Érvénytelen forgalomkorlát-lista.')
        count, window = pair
        if not (1 <= count <= 100000 and 1 <= window <= 86400):
            raise SettingError('A forgalomkorlát értéke tartományon kívül van.')
        clean[name] = [count, window]
    return clean


def validate(key, value):
    """A megadott érték ellenőrzése és normalizálása a beállítás típusa szerint (SettingError hiba esetén)."""
    spec = DEFINITIONS.get(key)
    if spec is None:
        raise SettingError('Ismeretlen beállítás.')
    kind = spec['type']
    if kind == 'bool':
        if not isinstance(value, bool):
            raise SettingError('Érvénytelen érték.')
        return value
    if kind == 'int':
        if isinstance(value, bool) or not isinstance(value, int):
            raise SettingError('Érvénytelen érték.')
        if not spec['min'] <= value <= spec['max']:
            raise SettingError('Az érték a megengedett tartományon kívül van.')
        return value
    if kind == 'float':
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise SettingError('Érvénytelen érték.')
        value = float(value)
        if not spec['min'] <= value <= spec['max']:
            raise SettingError('Az érték a megengedett tartományon kívül van.')
        return round(value, 2)
    if kind == 'choice':
        if value not in spec['choices'] or isinstance(value, bool):
            raise SettingError('Érvénytelen érték.')
        return value
    if kind == 'limits':
        return validate_limits(value)
    if kind == 'json':
        if not isinstance(value, dict):
            raise SettingError('Érvénytelen érték.')
        return value
    raise SettingError('Érvénytelen érték.')


# ===== Írás (a hívó tranzakciójában) =====

def store(conn, key, value, admin_id=None):
    """Egy beállítás felülbírálatának beírása a megadott kapcsolaton. `value is None` → a felülbírálat törlése
    (visszaállítás az alapértékre). A hívó `invalidate()`-t hív a tranzakció után."""
    if key not in DEFINITIONS:
        raise SettingError('Ismeretlen beállítás.')
    if value is None:
        conn.execute('DELETE FROM app_settings WHERE key = ?', (_PREFIX + key,))
        conn.execute('DELETE FROM app_settings WHERE key = ?', (_META_PREFIX + key,))
        return
    clean = validate(key, value)
    stamp = auth.format_ts(auth.utcnow())
    for name, payload in ((_PREFIX + key, clean), (_META_PREFIX + key, {'by': admin_id, 'at': stamp})):
        conn.execute('INSERT INTO app_settings (key, value) VALUES (?, ?) '
                     'ON CONFLICT(key) DO UPDATE SET value = excluded.value',
                     (name, json.dumps(payload, ensure_ascii=False)))


def maintenance():
    """Az érvényben lévő karbantartási mód ({enabled, message_hu, message_en, until}), vagy None. A lejárt (`until`
    elmúlt) karbantartás magától véget ér."""
    value = get(MAINTENANCE_KEY)
    if not isinstance(value, dict) or not value.get('enabled'):
        return None
    until = auth.parse_ts(value.get('until')) if value.get('until') else None
    if until is not None and until <= auth.utcnow():
        return None
    return value


def describe(names=None):
    """Az összes (nem rejtett) beállítás: érték, alapérték, módosítva-e, ki és mikor módosította."""
    _load()
    items = []
    for key, spec in DEFINITIONS.items():
        if spec.get('hidden') or (names is not None and key not in names):
            continue
        meta = _cache['meta'].get(key) or {}
        item = {
            'key': key, 'group': spec['group'], 'type': spec['type'],
            'value': get(key), 'default': default_of(key), 'overridden': key in _cache['values'],
            'changed_by': meta.get('by'), 'changed_at': meta.get('at'),
        }
        for bound in ('min', 'max'):
            if bound in spec:
                item[bound] = spec[bound]
        if 'choices' in spec:
            item['choices'] = list(spec['choices'])
        items.append(item)
    return items
