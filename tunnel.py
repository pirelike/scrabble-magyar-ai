import atexit
import re

import eventlet.patcher

# Az eredeti (nem eventlet-patchelt) modulok kellenek,
# mert a green thread nem tud blokkoló pipe-ot olvasni.
_subprocess = eventlet.patcher.original('subprocess')
_threading = eventlet.patcher.original('threading')
_shutil = eventlet.patcher.original('shutil')

tunnel_process = None


def start_tunnel(port):
    """Cloudflare tunnel indítása háttérben."""
    global tunnel_process
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
        for line in tunnel_process.stdout:
            match = re.search(r'(https://[a-z0-9-]+\.trycloudflare\.com)', line)
            if match:
                url = match.group(1)
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
