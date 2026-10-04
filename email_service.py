import smtplib
import socket
import ssl
import threading
from contextlib import contextmanager
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from email.utils import formataddr, formatdate, make_msgid

import mail_config

SEND_TIMEOUT = 15      # mp: küldés
CHECK_TIMEOUT = 10     # mp: az admin panel kapcsolat-próbája


def _ssl_context(cfg):
    context = ssl.create_default_context()
    if not cfg.get('verify_tls', True):      # saját / önaláírt tanúsítványú levelezőhöz (admin döntés)
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
    return context


@contextmanager
def _session(cfg, timeout=SEND_TIMEOUT):
    """Bejelentkezett SMTP kapcsolat a megadott beállítással (titkosítás: starttls | ssl | none)."""
    host, port = cfg['host'], cfg['port']
    if cfg['security'] == 'ssl':
        connection = smtplib.SMTP_SSL(host, port, timeout=timeout, context=_ssl_context(cfg))
    else:
        connection = smtplib.SMTP(host, port, timeout=timeout)
    with connection as server:
        if cfg['security'] == 'starttls':
            server.starttls(context=_ssl_context(cfg))
        if cfg.get('username'):
            server.login(cfg['username'], cfg['password'])
        yield server


def _sender(cfg):
    """A `From` fejléc értéke: „Név <cím>”, név nélkül a cím."""
    name = cfg.get('from_name') or ''
    return formataddr((name, cfg['from_address'])) if name else cfg['from_address']


def _finish(msg, cfg, to_email):
    msg['From'] = _sender(cfg)
    msg['To'] = to_email
    msg['Date'] = formatdate(localtime=True)
    msg['Message-ID'] = make_msgid(domain=cfg['from_address'].rpartition('@')[2] or None)
    return msg


def send_verification_email(to_email, code):
    """Verifikációs kód küldése emailben. Ha SMTP nincs konfigurálva, konzolra ír."""
    if not mail_config.is_configured():
        print(f'\n  [VERIFIKÁCIÓ] Email: {to_email} | Kód: {code}\n')
        return

    # Háttérszálon küldés, hogy ne blokkolja a requestet
    thread = threading.Thread(target=_send_email, args=(to_email, code), daemon=True)
    thread.start()


def _send_email(to_email, code):
    """SMTP email küldés."""
    try:
        cfg = mail_config.current()
        msg = MIMEMultipart('alternative')
        msg['Subject'] = 'Magyar Scrabble - Verifikációs kód'

        text = f'A verifikációs kódod: {code}\n\nEz a kód 10 percig érvényes.\nHa nem te kérted, hagyd figyelmen kívül ezt az emailt.'

        html = f'''
        <div style="font-family: Arial, sans-serif; max-width: 400px; margin: 0 auto; padding: 20px; background: #1a1a2e; color: #eee; border-radius: 12px;">
            <h2 style="color: #e8b930; text-align: center;">Magyar Scrabble</h2>
            <p style="text-align: center;">A verifikációs kódod:</p>
            <div style="text-align: center; font-size: 32px; font-weight: bold; letter-spacing: 8px; color: #e8b930; padding: 20px; background: #16213e; border-radius: 8px; margin: 16px 0;">
                {code}
            </div>
            <p style="text-align: center; color: #aaa; font-size: 14px;">Ez a kód 10 percig érvényes.</p>
        </div>
        '''

        msg.attach(MIMEText(text, 'plain'))
        msg.attach(MIMEText(html, 'html'))
        _finish(msg, cfg, to_email)

        with _session(cfg) as server:
            server.sendmail(cfg['from_address'], to_email, msg.as_string())

        print(f'  [EMAIL] Verifikációs kód elküldve: {to_email}')
    except Exception as e:
        print(f'  [EMAIL HIBA] {e}')
        print(f'  [VERIFIKÁCIÓ] Email: {to_email} | Kód: {code}')


def send_plain_email(to_email, subject, body, wait=False):
    """Egyszerű szöveges e-mail (értesítések, admin üzenetek). Visszatér: (elküldve-e, hibakód: `classify_error`).

    SMTP nélkül a konzolra ír és (False, 'smtp_not_configured')-et ad. `wait`: a küldés megvárása (a hibát
    visszaadja); különben háttérszálon megy, és a visszatérés csak azt jelzi, hogy elindult."""
    if not mail_config.is_configured():
        print(f'\n  [EMAIL] Címzett: {to_email} | Tárgy: {subject}\n  {body}\n')
        return False, 'smtp_not_configured'

    def deliver():
        cfg = mail_config.current()
        msg = MIMEText(body, 'plain', 'utf-8')
        msg['Subject'] = subject
        _finish(msg, cfg, to_email)
        with _session(cfg) as server:
            server.sendmail(cfg['from_address'], to_email, msg.as_string())

    if wait:
        try:
            deliver()
            return True, None
        except Exception as exc:
            print(f'  [EMAIL HIBA] {exc}')
            return False, classify_error(exc)

    def background():
        try:
            deliver()
        except Exception as exc:
            print(f'  [EMAIL HIBA] {exc}')

    threading.Thread(target=background, daemon=True).start()
    return True, None


# ===== Kapcsolat-próba (admin panel) =====

def classify_error(exc):
    """Egy SMTP / hálózati hiba rövid, biztonságos kódja. A kiszolgáló szövege szándékosan nem kerül a felületre
    (egy tetszőleges címre mutató próbával különben belső szolgáltatások üdvözlőszövege szivároghatna ki)."""
    if isinstance(exc, ssl.SSLCertVerificationError):
        return 'certificate'
    if isinstance(exc, ssl.SSLError):
        return 'tls'
    if isinstance(exc, socket.gaierror):
        return 'dns'
    if isinstance(exc, (socket.timeout, TimeoutError)):
        return 'timeout'
    if isinstance(exc, ConnectionRefusedError):
        return 'refused'
    if isinstance(exc, smtplib.SMTPAuthenticationError):
        return 'auth'
    if isinstance(exc, smtplib.SMTPNotSupportedError):
        return 'unsupported'
    if isinstance(exc, smtplib.SMTPSenderRefused):
        return 'sender'
    if isinstance(exc, smtplib.SMTPRecipientsRefused):
        return 'recipient'
    if isinstance(exc, smtplib.SMTPServerDisconnected):
        return 'disconnected'
    if isinstance(exc, smtplib.SMTPException):
        return 'protocol'
    if isinstance(exc, OSError):
        return 'connect'
    return 'error'


def check_connection(cfg, recipient=None):
    """Kapcsolat-próba a megadott beállítással: csatlakozás, titkosítás, bejelentkezés; `recipient` megadásakor teszt
    levél is megy. Visszatér: (sikerült-e, kód) — a kód `ok` vagy a `classify_error` értéke."""
    try:
        with _session(cfg, CHECK_TIMEOUT) as server:
            if recipient:
                msg = MIMEText('Ez egy teszt e-mail az admin panelről. A levelező szerver beállítása működik.',
                               'plain', 'utf-8')
                msg['Subject'] = 'Magyar Scrabble — teszt e-mail'
                _finish(msg, cfg, recipient)
                server.sendmail(cfg['from_address'], recipient, msg.as_string())
            else:
                server.noop()
    except Exception as exc:
        print(f'  [EMAIL PRÓBA] {type(exc).__name__}: {str(exc)[:200]}')
        return False, classify_error(exc)
    return True, 'ok'
