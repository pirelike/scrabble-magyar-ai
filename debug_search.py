import sqlite3
from auth import init_db, create_user, search_users, _db

init_db()
_, u1 = create_user('alice@test.com', 'pwd', 'AliceSmith')
_, u2 = create_user('bob@test.com', 'pwd', 'BobJones')
_, u3 = create_user('charlie@test.com', 'pwd', 'CharlieAlice')

with _db() as conn:
    print([dict(r) for r in conn.execute("SELECT id, display_name, email_lower FROM users").fetchall()])

print(search_users('alice', u1))
print(search_users('Alice', u1))
