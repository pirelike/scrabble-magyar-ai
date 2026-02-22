# Magyar Scrabble Klón

## Áttekintés
Webes magyar Scrabble játék online multiplayer támogatással. Flask + Socket.IO backend, vanilla JS frontend.

## Futtatás
```bash
cd ~/Documents/Scripts/scrabble
.venv/bin/python3 server.py           # Cloudflare tunnel-lel (publikus URL)
.venv/bin/python3 server.py --no-tunnel  # Csak helyi hálózat
```
Böngészőben: http://localhost:5000

## Fájlstruktúra
- `server.py` — Flask + SocketIO szerver, lobby/szoba kezelés, Cloudflare tunnel integráció
- `game.py` — Játéklogika (Game, Player osztályok), körök, pontozás, játék vége
- `board.py` — 15×15 tábla, premium mezők, szó elhelyezés validáció és pontozás, szótár-ellenőrzés
- `dictionary.py` — Magyar szótár-ellenőrzés (pyenchant / hunspell CLI fallback)
- `tiles.py` — Magyar betűkészlet (100 zseton), TileBag osztály
- `dict/` — Beágyazott hu_HU hunspell szótár fájlok (hu_HU.dic, hu_HU.aff)
- `templates/index.html` — Egyoldalas UI: név, lobby, várakozó szoba, játék
- `static/app.js` — Kliens logika, drag & drop, Socket.IO kommunikáció
- `static/style.css` — Stílusok
- `requirements.txt` — Python függőségek (flask, flask-socketio, pyenchant)
- `.venv/` — Virtual environment

## Funkciók
- 1-4 játékos (egyedül is játszható)
- Online multiplayer: lobby, szobák, Cloudflare tunnel automatikus publikus URL
- Teljes magyar betűkészlet (SZ, CS, GY, LY, NY, ZS, TY többkarakteres betűk)
- Standard Scrabble pontozás: DL, TL, DW, TW premium mezők
- 50 pont bónusz mind a 7 zseton kirakásakor
- Drag & drop és kattintásos betű elhelyezés
- Joker (üres zseton) bármely betűként használható
- Betűcsere és passz
- Szótár-ellenőrzés: pyenchant + beágyazott hu_HU szótár (cross-platform, Windows-kompatibilis)

## Következő lépés
- **Biztonság növelése**: A `server.py`-ban a `SECRET_KEY` hardkódolt, input validáció hiányos, nincs rate limiting, a Socket.IO CORS minden origint engedélyez (`cors_allowed_origins="*"`). Ezeket kell átnézni és megerősíteni, különösen mert a Cloudflare tunnel publikusan elérhetővé teszi a szervert.

## Ismert problémák / TODO
- Nincs challenge rendszer (szó megkérdőjelezése más játékos által)
- Nincs chat
- Nincs spectator mód
- Nincs adatbázis / felhasználói fiókok
