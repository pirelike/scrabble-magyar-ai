"""Közös pytest fixture-ök az összes teszthez."""
import pytest

# A szerver modul az elején gevent monkey patch-et végez: minden másnál (ssl, requests, pywebpush...)
# előbb kell betölteni, különben a patch "későn" érkezik.
import server  # noqa: F401


@pytest.fixture(autouse=True)
def temp_db(monkeypatch, tmp_path):
    """Ideiglenes adatbázis minden teszthez.

    Beállítja a config.DB_PATH és auth.DB_PATH értékét tmp_path-re,
    majd inicializálja az adatbázis sémát.
    """
    db_path = str(tmp_path / 'test.db')
    monkeypatch.setattr('config.DB_PATH', db_path)
    import auth
    monkeypatch.setattr(auth, 'DB_PATH', db_path)
    auth.init_db()
    yield db_path

@pytest.fixture(autouse=True)
def clean_state():
    """ServerState tisztítása minden teszthez."""
    from server import state as st
    for attr in ['rooms', 'player_names', 'player_rooms', 'player_auth',
                 '_reconnect_tokens', '_sid_to_token', '_disconnected_players',
                 '_online_users', '_sid_to_user_id', '_pending_invites', 'admin_sids']:
        d = getattr(st, attr, None)
        if isinstance(d, dict):
            d.clear()
        elif isinstance(d, set):
            d.clear()
    if hasattr(st, '_invite_counter'):
        st._invite_counter = 0
    
    # Clean up server structures just to be absolutely sure
    import server
    if hasattr(server, 'join_codes'):
        server.join_codes.clear()
        
    yield


@pytest.fixture(autouse=True)
def clean_rejected_words():
    """A szótár-építő szavazatai (a memóriában tartott elutasított szavak) ne szivárogjanak át a tesztek között."""
    yield
    import dictionary
    if dictionary._rejected_voted:
        dictionary.set_voted_rejected([])


@pytest.fixture
def admin_env(monkeypatch):
    """Az admin panel bekapcsolva: az `ADMIN_EMAILS` a teszt admin címét tartalmazza, nincs IP-lista."""
    import config
    import server
    from helpers import ADMIN_EMAIL
    monkeypatch.setattr(config, 'ADMIN_EMAILS', frozenset({ADMIN_EMAIL}))
    monkeypatch.setattr(config, 'ADMIN_IP_ALLOWLIST', frozenset())
    server._ip_rate_limits.clear()
    yield
    server._ip_rate_limits.clear()


@pytest.fixture
def api(admin_env):
    """Bejelentkezett admin API-kliens (a munkamenet frissen használt, a sudo nincs bekapcsolva)."""
    import auth
    from helpers import ADMIN_EMAIL, ADMIN_PASSWORD, AdminApi, create_user_with_session
    user_id, token = create_user_with_session(ADMIN_EMAIL, 'Főnök', ADMIN_PASSWORD)
    auth.touch_admin_session(token)
    return AdminApi(token, user_id)


@pytest.fixture
def isolated_dictionary(monkeypatch, tmp_path):
    """A szótár tartós listája (hu_rejected.txt) egy ideiglenes másolat: a teszt nem írja a repó fájlját.
    A teszt végén a szótár állapota visszaáll."""
    import shutil
    import dictionary
    path = tmp_path / 'hu_rejected.txt'
    shutil.copy(dictionary._REJECTED_PATH, path)
    monkeypatch.setattr(dictionary, '_REJECTED_PATH', str(path))
    import admin_dict
    monkeypatch.setattr(admin_dict, '_baseline', {'path': None, 'text': None})
    dictionary.reload_rejected()
    yield path
    monkeypatch.undo()
    dictionary.set_admin_lists([], [], [])
    dictionary.reload_rejected()
