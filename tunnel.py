import atexit
import re
import shutil as _shutil
import subprocess as _subprocess
import threading as _threading

# A szerver a gevent monkey patch után importálja: a subprocess és a threading itt már
# kooperatív (a pipe olvasása nem blokkolja a szervert).

tunnel_process = None
public_url = None     # a tunnel által adott publikus cím (amint kiderül)
_port = None


def status():
    """A tunnel állapota az admin panelnek: {'state': 'running' | 'starting' | 'stopped' | 'unavailable', 'url'}."""
    if _shutil.which('cloudflared') is None and tunnel_process is None:
        return {'state': 'unavailable', 'url': None}
    if tunnel_process is None or tunnel_process.poll() is not None:
        return {'state': 'stopped', 'url': None}
    return {'state': 'running' if public_url else 'starting', 'url': public_url}


def restart_tunnel():
    """A tunnel újraindítása a korábban használt porton (az új publikus cím más lesz)."""
    if _port is None:
        return False
    stop_tunnel()
    start_tunnel(_port)
    return tunnel_process is not None


def start_tunnel(port):
    """Cloudflare tunnel indítása háttérben."""
    global tunnel_process, public_url, _port
    _port = port
    public_url = None
    cloudflared = _shutil.which('cloudflared')
    if not cloudflared:
        print("\n  [!] cloudflared nincs telepítve - tunnel nem elérhető")
        print("      Telepítés: sudo pacman -S cloudflared\n")
        return

    print("\n  [*] Cloudflare tunnel indítása...", flush=True)
    tunnel_process = _subprocess.Popen(
        [cloudflared, 'tunnel', '--url', f'http://localhost:{port}'],
        stdout=_subprocess.PIPE,
        stderr=_subprocess.STDOUT,
        text=True,
    )

    def read_output():
        global public_url
        for line in tunnel_process.stdout:
            match = re.search(r'(https://[a-z0-9-]+\.trycloudflare\.com)', line)
            if match:
                url = match.group(1)
                public_url = url
                print(f"\n{'='*50}", flush=True)
                print(f"  PUBLIKUS URL: {url}", flush=True)
                print(f"  Oszd meg ezt a linket a barátaiddal!", flush=True)
                print(f"{'='*50}\n", flush=True)

    thread = _threading.Thread(target=read_output, daemon=True)
    thread.start()


def stop_tunnel():
    """Cloudflare tunnel leállítása."""
    global tunnel_process
    if tunnel_process:
        tunnel_process.terminate()
        try:
            tunnel_process.wait(timeout=5)
        except Exception:
            tunnel_process.kill()
        tunnel_process = None


import signal

def _signal_handler(sig, frame):
    stop_tunnel()
    raise SystemExit(0)

for _sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(_sig, _signal_handler)

atexit.register(stop_tunnel)
