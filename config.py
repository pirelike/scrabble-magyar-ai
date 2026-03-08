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

# --- Rate limiting (IP-alapú, auth endpointokra) ---
AUTH_RATE_LIMITS = {
    'request_code': (3, 300),    # 3 kérés / 5 perc
    'login': (10, 300),          # 10 kérés / 5 perc
    'register': (3, 3600),       # 3 kérés / 1 óra
    'search_users': (20, 60),    # 20 kérés / 1 perc
}
