import smtplib
import threading
from email.mime.text import MIMEText
from email.mime.multipart import MIMEMultipart

from config import SMTP_HOST, SMTP_PORT, SMTP_USER, SMTP_PASSWORD, SMTP_FROM, SMTP_CONFIGURED


def send_verification_email(to_email, code):
    """Verifikációs kód küldése emailben. Ha SMTP nincs konfigurálva, konzolra ír."""
    if not SMTP_CONFIGURED:
        print(f'\n  [VERIFIKÁCIÓ] Email: {to_email} | Kód: {code}\n')
        return

    # Háttérszálon küldés, hogy ne blokkolja a requestet
    thread = threading.Thread(target=_send_email, args=(to_email, code), daemon=True)
    thread.start()


def _send_email(to_email, code):
    """SMTP email küldés."""
    try:
        msg = MIMEMultipart('alternative')
        msg['Subject'] = 'Magyar Scrabble - Verifikációs kód'
        msg['From'] = SMTP_FROM
        msg['To'] = to_email

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

        with smtplib.SMTP(SMTP_HOST, SMTP_PORT) as server:
            server.starttls()
            server.login(SMTP_USER, SMTP_PASSWORD)
            server.sendmail(SMTP_FROM, to_email, msg.as_string())

        print(f'  [EMAIL] Verifikációs kód elküldve: {to_email}')
    except Exception as e:
        print(f'  [EMAIL HIBA] {e}')
        print(f'  [VERIFIKÁCIÓ] Email: {to_email} | Kód: {code}')
