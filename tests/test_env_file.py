"""A helyi `.env` fájl: a gépre jellemző beállítás (pl. PORT), amelyet a GitHubról frissítés nem írhat felül."""
import subprocess

import config

ROOT = config.os.path.dirname(config.ENV_FILE)


def write_env(tmp_path, text):
    path = tmp_path / '.env'
    path.write_text(text, encoding='utf-8')
    return str(path)


def test_values_are_loaded(tmp_path, monkeypatch):
    monkeypatch.delenv('PORT', raising=False)
    monkeypatch.delenv('SCRABBLE_X', raising=False)
    path = write_env(tmp_path, '# megjegyzés\n\nPORT=8123\nexport SCRABBLE_X = "érték"\nrossz sor\n1BAD=3\n')
    assert sorted(config.load_env_file(path)) == ['PORT', 'SCRABBLE_X']
    assert config.os.environ['PORT'] == '8123' and config.os.environ['SCRABBLE_X'] == 'érték'


def test_a_real_environment_variable_wins(tmp_path, monkeypatch):
    monkeypatch.setenv('PORT', '9000')
    assert config.load_env_file(write_env(tmp_path, 'PORT=8123\n')) == []
    assert config.os.environ['PORT'] == '9000'


def test_missing_file_is_fine(tmp_path):
    assert config.load_env_file(str(tmp_path / 'nincs.env')) == []


def test_the_env_file_is_not_tracked_by_git():
    ignored = subprocess.run(['git', 'check-ignore', '-q', '.env'], cwd=ROOT).returncode
    assert ignored == 0
