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
