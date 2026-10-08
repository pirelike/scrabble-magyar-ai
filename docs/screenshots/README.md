# Képernyőképek

A dokumentációban (README, INSTALL, USER_GUIDE) használt képek. **Nem valódi felhasználók adatai**: egy ideiglenes demó adatbázisból készültek,
kitalált játékosokkal (`anna@example.com`…) és a robot saját motorjával lejátszott játékokkal.

| Kép | Mit mutat |
|---|---|
| `login.png`, `lobby.png`, `new-room.png`, `waiting-room.png` | belépés, lobby, új szoba, várakozó szoba |
| `game.png`, `game-dark.png`, `hint.png`, `mobile.png` | játék képernyő (világos / sötét), tipp, telefonos nézet |
| `daily-puzzle.png`, `practice.png`, `quiz.png`, `hunt.png` | napi feladvány és gyakorló módok |
| `leaderboard.png`, `profile.png`, `replay-analysis.png` | ranglista, profil, visszajátszás elemzéssel |
| `admin-overview.png`, `admin-users.png`, `admin-rooms.png`, `admin-stats.png` | az admin panel nézetei |

**Újrakészítés** (ha a felület megváltozik): indíts egy szervert friss adatbázissal (`SCRABBLE_DB_PATH=/tmp/demo.db PORT=5077 ADMIN_EMAILS=admin@example.com target/release/scrabble --no-tunnel`),
töltsd fel néhány fiókkal és befejezett játékkal, majd egy böngészőautomatizálóval (Playwright + Chromium, 1280–1440 px széles ablak, telefonnál 390×844, 2× pixelsűrűség) lépj végig a nézeteken.
A kimeneti PNG-ket érdemes 256 színűre csökkenteni (a felület képein alig látszik, a fájlméret töredékére esik). A CDN-ről töltött Socket.IO kliens (`socket.io.min.js`) hiányában a játék nem kapcsolódik,
ezért korlátozott hálózatú környezetben azt helyi másolatból kell kiszolgálni.
