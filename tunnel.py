import atexit
import re
import shutil
import subprocess
import threading

tunnel_process = None


def start_tunnel(port):
    """Cloudflare tunnel indítása háttérben."""
    global tunnel_process
    cloudflared = shutil.which('cloudflared')
    if not cloudflared:
        print("\n  [!] cloudflared nincs telepítve - tunnel nem elérhető")
        print("      Telepítés: sudo pacman -S cloudflared\n")
        return

    print("\n  [*] Cloudflare tunnel indítása...")
    tunnel_process = subprocess.Popen(
        [cloudflared, 'tunnel', '--url', f'http://localhost:{port}'],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    def read_output():
        for line in tunnel_process.stdout:
            match = re.search(r'(https://[a-z0-9-]+\.trycloudflare\.com)', line)
            if match:
                url = match.group(1)
                print(f"\n{'='*50}")
                print(f"  PUBLIKUS URL: {url}")
                print(f"  Oszd meg ezt a linket a barátaiddal!")
                print(f"{'='*50}\n")

    thread = threading.Thread(target=read_output, daemon=True)
    thread.start()


def stop_tunnel():
    """Cloudflare tunnel leállítása."""
    global tunnel_process
    if tunnel_process:
        tunnel_process.terminate()
        tunnel_process = None


atexit.register(stop_tunnel)
