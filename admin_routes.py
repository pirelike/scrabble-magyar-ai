"""Admin panel HTTP réteg: őr, munkamenet (újraigazolás, sudo), napló és a panel kiszolgálása.

Az admin felület létezése másnak nem derülhet ki: nem admin kérésre MINDEN admin útvonal (oldal, asset, API,
rossz metódus, nem létező alútvonal) ugyanazt a szokásos 404-et adja, mint egy nem létező cím. Az őr ezért
az egész alkalmazás elé kerül (`before_app_request`), és az útvonal-illesztés előtt dönt. A logika az
`admin.py`-ban van; részletes leírás: `docs/ADMIN_PANEL.md`.
"""
import os
import re
from functools import wraps
from urllib.parse import urlsplit

from flask import (
    Blueprint, abort, g, jsonify, make_response, render_template, request, send_from_directory,
)

import admin
import auth
import config
import routes as main_routes
from auth import is_admin_user, validate_session, verify_password

admin_bp = Blueprint('admin_api', __name__, url_prefix='/api/admin')
admin_pages_bp = Blueprint('admin_pages', __name__)

_BASE_DIR = os.path.dirname(os.path.abspath(__file__))
# Az admin JS / CSS nem a nyilvános `static/` mappában van: csak az őrzött útvonalon keresztül érhető el
ADMIN_ASSETS_DIR = os.path.join(_BASE_DIR, 'admin_assets')

_API_PREFIX = '/api/admin'
_PAGE_PREFIX = '/admin'
_UNSAFE_METHODS = frozenset({'POST', 'PUT', 'PATCH', 'DELETE'})
# Ezek a végpontok tétlenség után is működnek (a panel ezekkel kéri a jelszót), és nem frissítik az időzítőt
_IDLE_EXEMPT = frozenset({'admin_api.session_info', 'admin_api.reauth',
                          'admin_pages.page', 'admin_pages.asset'})

# A Socket.IO kliens ugyanonnan töltődik, mint a nyilvános oldalon (cdnjs); a WebSocket kapcsolat a saját hosttal
_CSP = ("default-src 'none'; script-src 'self' https://cdnjs.cloudflare.com; style-src 'self'; img-src 'self' data:; "
        "font-src 'self'; connect-src 'self' ws: wss:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")


# ===== Őr =====

def _admin_path_kind(path):
    """'api' / 'page' az admin útvonalaknál, különben None. A dupla perjel (//admin) összevonva."""
    path = re.sub(r'/{2,}', '/', path)
    for prefix, kind in ((_API_PREFIX, 'api'), (_PAGE_PREFIX, 'page')):
        if path == prefix or path.startswith(prefix + '/'):
            return kind
    return None


def _json_error(status, message, **extra):
    return jsonify({'success': False, 'message': message, **extra}), status


def _same_origin():
    """A kérés Origin (vagy Referer) fejléce a saját hostra mutat-e (a SameSite=Lax mellé)."""
    source = request.headers.get('Origin') or request.headers.get('Referer')
    if not source:
        return False
    try:
        netloc = urlsplit(source).netloc
    except ValueError:
        return False
    return bool(netloc) and netloc.lower() == request.host.lower()


@admin_bp.before_app_request
def _guard():
    kind = _admin_path_kind(request.path)
    if kind is None:
        return None

    # 1. Azonosítás: nem admin → ugyanaz a 404, mint egy nem létező útvonalra
    if not config.ADMIN_EMAILS:
        abort(404)
    token = request.cookies.get('session_token')
    user = validate_session(token)
    if not is_admin_user(user):
        abort(404)

    # 2. IP-engedélylista (ha be van állítva) — kívülről szintén 404
    ip = main_routes._get_client_ip()
    if not admin.ip_allowed(ip):
        abort(404)

    g.admin_user = user
    g.admin_token = token
    g.admin_ctx = admin.AdminContext(user['id'], ip, request.headers.get('User-Agent', ''))

    # 3. Forgalomkorlát
    if not main_routes._rate_limiter.check_ip(ip, 'admin'):
        return _json_error(429, 'Túl sok kérés. Várj egy kicsit.')

    # 4. Más oldalról indított kérések ellen: egyedi fejléc + azonos eredetű Origin / Referer
    if request.method in _UNSAFE_METHODS:
        if request.headers.get('X-Admin-Request') != '1' or not _same_origin():
            return _json_error(403, 'Érvénytelen kérés.')

    # 5. Tétlenségi időkorlát: lejárt munkamenetnél jelszó kell (az oldal és az újraigazolás kivétel)
    if request.endpoint in _IDLE_EXEMPT:
        return None
    status = admin.session_status(token)
    if status is None or status.idle_expired:
        return _json_error(401, 'A munkamenet lejárt, add meg újra a jelszavad.', reauth=True)
    admin.touch(token, status)
    return None


@admin_bp.after_app_request
def _admin_headers(response):
    # Csak az admin által látott válaszokon: a nem adminnak adott 404 fejlécei nem térhetnek el a megszokottól
    if g.get('admin_user') is None:
        return response
    response.headers['Cache-Control'] = 'no-store'
    response.headers['X-Robots-Tag'] = 'noindex, nofollow'
    response.headers['X-Frame-Options'] = 'DENY'
    response.headers['X-Content-Type-Options'] = 'nosniff'
    response.headers['Referrer-Policy'] = 'no-referrer'
    response.headers['Content-Security-Policy'] = _CSP
    return response


@admin_bp.app_errorhandler(admin.AdminError)
def _admin_error(error):
    return _json_error(error.status, error.message, **error.extra)


def danger(view):
    """Romboló művelet: külön, szigorúbb forgalomkorlát (`admin_danger`)."""
    @wraps(view)
    def wrapper(*args, **kwargs):
        if not main_routes._rate_limiter.check_ip(g.admin_ctx.ip, 'admin_danger'):
            return _json_error(429, 'Túl sok kérés. Várj egy kicsit.')
        return view(*args, **kwargs)
    return wrapper


def sudo_required(view):
    """Romboló művelet, amelyhez friss jelszó-megerősítés (sudo mód) is kell."""
    @wraps(view)
    def wrapper(*args, **kwargs):
        if not admin.sudo_active(g.admin_token):
            return _json_error(401, 'Ehhez a művelethez jelszó-megerősítés (sudo) szükséges.',
                               sudo_required=True)
        return view(*args, **kwargs)
    return danger(wrapper)


# ===== A panel oldala és assetjei =====

def admin_asset_version():
    """Az admin fájlok legutóbbi módosítási ideje (gyorsítótár-törés az `?v=` paraméterrel)."""
    latest = 0
    try:
        for name in os.listdir(ADMIN_ASSETS_DIR):
            latest = max(latest, int(os.path.getmtime(os.path.join(ADMIN_ASSETS_DIR, name))))
    except OSError:
        pass
    return latest


@admin_pages_bp.route('/admin')
def page():
    return make_response(render_template('admin.html', asset_v=admin_asset_version(),
                                         public_v=main_routes.asset_version()))


@admin_pages_bp.route('/admin/assets/<path:filename>')
def asset(filename):
    return send_from_directory(ADMIN_ASSETS_DIR, filename, max_age=0)


# ===== Munkamenet =====

def _json_body():
    data = request.get_json(silent=True)
    return data if isinstance(data, dict) else {}


def _check_password(failed_action):
    """Az admin jelszavának ellenőrzése (sudo / újraigazolás). Hiba esetén a kész válasz, különben None.

    A rossz próbálkozások a bejelentkezés forgalomkorlátjába számítanak, és naplózódnak.
    """
    ctx = g.admin_ctx
    if not main_routes._rate_limiter.check_ip(ctx.ip, 'login'):
        return _json_error(429, 'Túl sok jelszópróbálkozás. Próbáld újra 5 perc múlva.')
    password = _json_body().get('password')
    if not isinstance(password, str) or not password or len(password) > 128:
        return _json_error(400, 'A jelszó megadása kötelező.')
    ok, _ = verify_password(g.admin_user['email'], password)
    if not ok:
        admin.record_failed_password(ctx, failed_action)
        return _json_error(403, 'Hibás jelszó.')
    return None


def _session_payload():
    status = admin.session_status(g.admin_token)
    return {
        'success': True,
        'admin': {'id': g.admin_user['id'], 'display_name': g.admin_user['display_name'],
                  'email': g.admin_user['email']},
        'session': status.as_dict() if status else None,
    }


@admin_bp.route('/session', methods=['GET'])
def session_info():
    """A panel állapotlekérdezése: ki az admin, lejárt-e a munkamenet, él-e a sudo (nem frissíti az időzítőt)."""
    return jsonify(_session_payload())


@admin_bp.route('/reauth', methods=['POST'])
def reauth():
    """Jelszavas újraigazolás tétlenség után."""
    failure = _check_password('admin.reauth_failed')
    if failure:
        return failure
    admin.reauth(g.admin_ctx, g.admin_token)
    return jsonify(_session_payload())


@admin_bp.route('/sudo', methods=['POST'])
def sudo_start():
    """Sudo mód: a romboló műveletekhez `ADMIN_SUDO_MINUTES` percre érvényes jelszó-megerősítés."""
    failure = _check_password('admin.sudo_failed')
    if failure:
        return failure
    admin.grant_sudo(g.admin_ctx, g.admin_token)
    return jsonify(_session_payload())


@admin_bp.route('/sudo', methods=['DELETE'])
def sudo_end():
    admin.end_sudo(g.admin_ctx, g.admin_token)
    return jsonify(_session_payload())


# ===== Közös segédek az API modulokhoz =====

def json_ok(**data):
    return jsonify({'success': True, **data})


def reason_of(body):
    """Az indoklás a kérés törzséből (a hiányát / rövidségét az `admin.action` jelzi)."""
    return body.get('reason')


def body_or_empty():
    return _json_body()


def csv_response(filename, fields, rows):
    """CSV letöltés (UTF-8, a képletként értelmezhető cellák elé `'` kerül)."""
    import csv
    import io
    out = io.StringIO()
    writer = csv.writer(out, lineterminator='\r\n')
    writer.writerow(fields)
    for row in rows:
        writer.writerow([admin._csv_cell(row.get(f) if isinstance(row, dict) else row[i])
                         for i, f in enumerate(fields)])
    response = make_response(out.getvalue())
    response.headers['Content-Type'] = 'text/csv; charset=utf-8'
    response.headers['Content-Disposition'] = f'attachment; filename="{filename}"'
    return response


def wants_csv():
    return request.args.get('format') == 'csv'


def flat(value):
    """Lista / szótár cellák CSV-hez: egyszerű szöveggé alakítva."""
    if isinstance(value, (list, tuple)):
        return '; '.join(str(flat(v)) for v in value)
    if isinstance(value, dict):
        return '; '.join(f'{k}={flat(v)}' for k, v in value.items())
    return value


# ===== Admin napló =====

_AUDIT_FILTERS = ('admin', 'action', 'target_type', 'target_id', 'since', 'until', 'q')


@admin_bp.route('/audit', methods=['GET'])
def audit_list():
    """A napló szűrve és lapozva. `format=csv` vagy `download=1` az exportot adja (az export maga is naplózódik)."""
    args = request.args
    filters = {name: args.get(name) for name in _AUDIT_FILTERS}
    fmt = args.get('format', 'json')
    if fmt not in ('json', 'csv'):
        return _json_error(400, 'Ismeretlen formátum.')
    export = fmt == 'csv' or args.get('download') == '1'

    if export:
        result = admin.query_audit(**filters, limit=args.get('limit', admin.AUDIT_EXPORT_MAX), offset=0,
                                   max_limit=admin.AUDIT_EXPORT_MAX)
        with auth.transaction() as conn:
            admin.record(conn, g.admin_ctx, 'view.audit_export', 'audit', None,
                         {'format': fmt, 'filters': {k: v for k, v in filters.items() if v},
                          'rows': len(result['items'])})
        if fmt == 'csv':
            response = make_response(admin.audit_csv(result['items']))
            response.headers['Content-Type'] = 'text/csv; charset=utf-8'
            response.headers['Content-Disposition'] = 'attachment; filename="admin-audit.csv"'
            return response
        response = jsonify({'success': True, **result})
        response.headers['Content-Disposition'] = 'attachment; filename="admin-audit.json"'
        return response

    result = admin.query_audit(**filters, limit=args.get('limit'), offset=args.get('offset'))
    return jsonify({'success': True, **result})


# A további admin végpontok külön modulokban vannak (ugyanazon a blueprinten, ugyanazzal az őrrel)
import admin_api_users  # noqa: E402,F401
import admin_api_game  # noqa: E402,F401
import admin_api_dict  # noqa: E402,F401
import admin_api_comm  # noqa: E402,F401
import admin_api_system  # noqa: E402,F401
