"""Közös kiinduló adatbázis: felhasználók (u1..u6, admin), amit mindkét szerver másolatként kap."""
import os, sys
out = sys.argv[1]
if os.path.exists(out): os.remove(out)
os.environ['SCRABBLE_DB_PATH'] = out
sys.path.insert(0, os.environ['PYTHON_FINAL_DIR'])
import auth
auth.init_db() if hasattr(auth, 'init_db') else None
for i in range(1, 7):
    ok, uid = auth.create_user(f'u{i}@example.com', f'Jatekos{i}', 'secret12'); assert ok, uid
ok, uid = auth.create_user('admin@example.com', 'Admin', 'secret12'); assert ok
print('ok')
