"""Közös pytest fixture-ök az összes teszthez."""
import pytest


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
                 '_online_users', '_sid_to_user_id', '_pending_invites']:
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