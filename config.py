import ipaddress
import os

# --- SMTP konfiguráció ---
SMTP_HOST = os.environ.get('SMTP_HOST', 'smtp.gmail.com')
SMTP_PORT = int(os.environ.get('SMTP_PORT', 587))
SMTP_USER = os.environ.get('SMTP_USER', '')
SMTP_PASSWORD = os.environ.get('SMTP_PASSWORD', '')
SMTP_FROM = os.environ.get('SMTP_FROM', '')

# Ha bármelyik SMTP mező üres, a kód a konzolra íródik ki
SMTP_CONFIGURED = all([SMTP_USER, SMTP_PASSWORD, SMTP_FROM])

# --- Auth konfiguráció ---
DB_PATH = os.environ.get('SCRABBLE_DB_PATH', 'scrabble.db')
SESSION_MAX_AGE_DAYS = 30
VERIFICATION_CODE_EXPIRY_MINUTES = 10
VERIFICATION_MAX_ATTEMPTS = 5
EMAIL_VERIFIED_WINDOW_MINUTES = 30  # ennyi ideig regisztrálható a kóddal megerősített email

# --- Szótár-építő ---
# Ennyivel kell több „nem szó” szavazatnak lennie a „rendes szó” szavazatoknál, hogy a szót kizárja a
# játék szótára. Az alapérték 1: egyetlen elutasítás elég; több ember átnézésénél emelhető (pl. 2).
WORD_REJECT_THRESHOLD = max(1, int(os.environ.get('WORD_REJECT_THRESHOLD', 1)))

# --- Rate limiting (IP-alapú, auth endpointokra) ---
AUTH_RATE_LIMITS = {
    'request_code': (3, 300),    # 3 kérés / 5 perc
    'login': (10, 300),          # 10 kérés / 5 perc
    'register': (3, 3600),       # 3 kérés / 1 óra
    'search_users': (20, 60),    # 20 kérés / 1 perc
    'leaderboard': (30, 60),     # 30 kérés / 1 perc
    'dictionary': (60, 60),      # 60 kérés / 1 perc
    'replay': (60, 60),          # 60 kérés / 1 perc (megosztott visszajátszás)
    'analysis': (30, 60),        # 30 kérés / 1 perc (játékelemzés lekérdezése)
    'daily': (60, 60),           # 60 kérés / 1 perc (napi feladvány, ranglista)
    'practice': (120, 60),       # 120 kérés / 1 perc (kvíz, rövid szavak)
    'push': (20, 60),            # 20 kérés / 1 perc (push feliratkozás)
    'word_review': (240, 60),    # 240 kérés / 1 perc (szótár-építő: egy kérés egy döntés)
    'admin': (120, 60),          # 120 kérés / 1 perc (admin panel, minden admin végpont)
    'admin_danger': (20, 60),    # 20 kérés / 1 perc (az admin panel romboló műveletei)
}

# --- Admin panel ---
# Vesszővel elválasztott e-mail címek (kisbetűsítve hasonlítva). Üres → nincs admin, a panel ki van
# kapcsolva (minden admin útvonal 404). Az adminság szándékosan csak ezen a környezeti változón múlik:
# nem adatbázis-oszlop és nem a megjelenítési névhez kötött.
ADMIN_EMAILS = frozenset(e.strip().lower() for e in os.environ.get('ADMIN_EMAILS', '').split(',') if e.strip())
# Ennyi tétlenség után az admin panel újra jelszót kér (nem dob ki a játékból)
ADMIN_SESSION_IDLE_MINUTES = max(1, int(os.environ.get('ADMIN_SESSION_IDLE_MINUTES', 30)))
# A romboló műveletekhez szükséges friss jelszó-megerősítés („sudo mód”) érvényessége
ADMIN_SUDO_MINUTES = max(1, int(os.environ.get('ADMIN_SUDO_MINUTES', 10)))


def parse_ip_networks(raw):
    """Vesszővel elválasztott IP-címek / CIDR-ek listája → hálózatok halmaza.

    Hibás elemnél ValueError: az engedélylista elírása ne kapcsolja ki csendben a korlátozást
    (a szerver inkább el se induljon, mint hogy mindenhonnan elérhető legyen a panel).
    """
    networks = []
    for item in (raw or '').split(','):
        item = item.strip()
        if not item:
            continue
        try:
            networks.append(ipaddress.ip_network(item, strict=False))
        except ValueError:
            raise ValueError(f'ADMIN_IP_ALLOWLIST: érvénytelen IP-cím vagy hálózat: {item!r}') from None
    return frozenset(networks)


# Opcionális: az admin panel csak ezekről az IP-címekről / hálózatokról érhető el (üres = bármelyikről)
ADMIN_IP_ALLOWLIST = parse_ip_networks(os.environ.get('ADMIN_IP_ALLOWLIST', ''))
