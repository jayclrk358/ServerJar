#!/usr/bin/env python3
"""ServerJar panel: start/stop the Pumpkin server, watch its console, send commands, edit files.

Standard library only. Run it next to the server folder:

    python3 panel/panel.py --server ./server              # first run asks for a password
    python3 panel/panel.py --server ~/mc --bind 0.0.0.0:8080

Use --tls to serve HTTPS with a self-signed certificate (generated on first use), which makes it
safe to bind to a public address: python3 panel/panel.py --server ~/mc --bind 0.0.0.0:8080 --tls
Your browser warns once about the certificate; that is expected for self-signed certificates.
Everything the panel touches stays inside the server folder.
"""
import argparse, base64, collections, getpass, hashlib, hmac, html, json, mimetypes, os, re, secrets, shutil, signal, ssl
import subprocess, sys, threading, time, urllib.parse
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG_LINES = 2000
MAX_EDIT_BYTES = 2 * 1024 * 1024
MAX_UPLOAD_BYTES = 64 * 1024 * 1024
TEXT_EXT = {".toml", ".txt", ".json", ".yml", ".yaml", ".properties", ".log", ".md", ".sh", ".bat", ".service", ".cfg", ".conf", ".ini", ".mcmeta", ".lang", ".csv", ".env"}
HERE = os.path.dirname(os.path.abspath(__file__))


# ---------------------------------------------------------------- server process -----------------

class Server:
    """Owns the Pumpkin child process and its console."""

    def __init__(self, directory, binary):
        self.dir = directory
        self.binary = binary
        self.proc = None
        self.started_at = None
        self.lines = collections.deque(maxlen=LOG_LINES)
        self.seq = 0
        self.cond = threading.Condition()
        self.stop_requested = False
        self.restart_pending = False

    def running(self):
        return self.proc is not None and self.proc.poll() is None

    def log(self, text):
        with self.cond:
            for line in text.splitlines():
                self.seq += 1
                self.lines.append((self.seq, line))
            self.cond.notify_all()

    def start(self):
        if self.running():
            return "already running"
        path = os.path.join(self.dir, self.binary)
        if not os.path.isfile(path):
            return f"server binary not found: {path}"
        if not os.access(path, os.X_OK):
            os.chmod(path, 0o755)
        self.stop_requested = False
        # Pumpkin reads console commands from stdin; a TTY isn't available here.
        self.proc = subprocess.Popen([path], cwd=self.dir, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.STDOUT, bufsize=1, text=True, errors="replace",
                                     start_new_session=True)
        self.started_at = time.time()
        self.log(f"[panel] started {self.binary} (pid {self.proc.pid})")
        threading.Thread(target=self._pump, args=(self.proc,), daemon=True).start()
        return "started"

    def _pump(self, proc):
        for line in proc.stdout:
            self.log(ANSI.sub("", line.rstrip("\n")))
        code = proc.wait()
        self.log(f"[panel] server exited with code {code}")
        if self.restart_pending:
            self.restart_pending = False
            time.sleep(1)
            self.start()

    def command(self, text):
        if not self.running():
            return "not running"
        try:
            self.proc.stdin.write(text.strip() + "\n")
            self.proc.stdin.flush()
            self.log(f"> {text.strip()}")
            return "sent"
        except (BrokenPipeError, ValueError) as e:
            return f"failed: {e}"

    def stop(self, timeout=30):
        if not self.running():
            return "not running"
        self.stop_requested = True
        self.command("stop")
        deadline = time.time() + timeout
        while self.running() and time.time() < deadline:
            time.sleep(0.2)
        if self.running():
            self.log("[panel] server did not stop in time, sending SIGTERM")
            self.proc.terminate()
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.log("[panel] killing server")
                self.proc.kill()
        return "stopped"

    def kill(self):
        if self.running():
            self.proc.kill()
            return "killed"
        return "not running"

    def restart(self):
        if self.running():
            self.restart_pending = True
            self.stop()
            return "restarting"
        return self.start()

    def status(self):
        running = self.running()
        mem = None
        if running:
            try:
                with open(f"/proc/{self.proc.pid}/status") as f:
                    for l in f:
                        if l.startswith("VmRSS:"):
                            mem = int(l.split()[1]) * 1024
            except OSError:
                pass
        return {"running": running, "pid": self.proc.pid if running else None,
                "uptime": int(time.time() - self.started_at) if running else 0, "memory": mem,
                "binary": self.binary, "dir": self.dir}

    def since(self, seq, wait=0.0):
        with self.cond:
            if wait and self.seq <= seq:
                self.cond.wait(wait)
            return self.seq, [(s, l) for s, l in self.lines if s > seq]


ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")


# ---------------------------------------------------------------- auth ----------------------------

class Auth:
    def __init__(self, path):
        self.path = path
        self.sessions = {}
        self.failures = {}  # ip -> [count, last_time]
        self.lock = threading.Lock()
        self.salt = None
        self.hash = None
        if os.path.exists(path):
            with open(path) as f:
                data = json.load(f)
            self.salt, self.hash = bytes.fromhex(data["salt"]), bytes.fromhex(data["hash"])

    def set_password(self, password):
        self.salt = secrets.token_bytes(16)
        self.hash = hashlib.pbkdf2_hmac("sha256", password.encode(), self.salt, 200_000)
        with open(self.path, "w") as f:
            json.dump({"salt": self.salt.hex(), "hash": self.hash.hex()}, f)
        os.chmod(self.path, 0o600)

    def check(self, password):
        if self.hash is None:
            return False
        return hmac.compare_digest(hashlib.pbkdf2_hmac("sha256", password.encode(), self.salt, 200_000), self.hash)

    def throttled(self, ip):
        """True while an address is locked out after too many wrong passwords."""
        with self.lock:
            count, last = self.failures.get(ip, (0, 0))
            if count >= 5 and time.time() - last < 300:
                return True
            if time.time() - last >= 300:
                self.failures.pop(ip, None)
            return False

    def failed(self, ip):
        with self.lock:
            count, _ = self.failures.get(ip, (0, 0))
            self.failures[ip] = (count + 1, time.time())

    def succeeded(self, ip):
        with self.lock:
            self.failures.pop(ip, None)

    def login(self):
        token = secrets.token_urlsafe(32)
        with self.lock:
            self.sessions[token] = time.time()
        return token

    def valid(self, token):
        with self.lock:
            t = self.sessions.get(token)
            if t is None or time.time() - t > 7 * 86400:
                self.sessions.pop(token, None)
                return False
            self.sessions[token] = time.time()
            return True

    def logout(self, token):
        with self.lock:
            self.sessions.pop(token, None)


# ---------------------------------------------------------------- files ---------------------------

class Files:
    def __init__(self, root):
        self.root = os.path.realpath(root)

    def resolve(self, rel):
        rel = rel.lstrip("/")
        path = os.path.realpath(os.path.join(self.root, rel))
        if path != self.root and not path.startswith(self.root + os.sep):
            raise PermissionError("outside the server folder")
        return path

    def list(self, rel):
        path = self.resolve(rel)
        entries = []
        with os.scandir(path) as it:
            for e in it:
                st = e.stat(follow_symlinks=False)
                if e.name == ".panel-auth.json":
                    continue
                entries.append({"name": e.name, "dir": e.is_dir(follow_symlinks=False), "size": st.st_size,
                                "mtime": int(st.st_mtime), "text": self.is_text(e.name)})
        entries.sort(key=lambda x: (not x["dir"], x["name"].lower()))
        return {"path": os.path.relpath(path, self.root).replace(os.sep, "/").replace(".", "", 1) if path != self.root else "", "entries": entries}

    @staticmethod
    def is_text(name):
        return os.path.splitext(name)[1].lower() in TEXT_EXT or name in ("ops.json", "whitelist.json", "banned-players.json")

    def read(self, rel):
        path = self.resolve(rel)
        if os.path.getsize(path) > MAX_EDIT_BYTES:
            raise ValueError("file too large to edit in the browser")
        with open(path, "rb") as f:
            data = f.read()
        try:
            return data.decode("utf-8")
        except UnicodeDecodeError:
            raise ValueError("not a text file")

    def write(self, rel, content):
        path = self.resolve(rel)
        if len(content.encode()) > MAX_EDIT_BYTES:
            raise ValueError("too large")
        tmp = path + ".panel-tmp"
        with open(tmp, "w", encoding="utf-8", newline="") as f:
            f.write(content)
        os.replace(tmp, path)

    def write_bytes(self, rel, data):
        path = self.resolve(rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)

    def mkdir(self, rel):
        os.makedirs(self.resolve(rel), exist_ok=True)

    def delete(self, rel):
        path = self.resolve(rel)
        if path == self.root:
            raise PermissionError("refusing to delete the server folder")
        if os.path.isdir(path) and not os.path.islink(path):
            shutil.rmtree(path)
        else:
            os.remove(path)

    def rename(self, rel, new_rel):
        os.replace(self.resolve(rel), self.resolve(new_rel))


# ---------------------------------------------------------------- http ----------------------------

class Handler(BaseHTTPRequestHandler):
    server_version = "ServerJarPanel/1.0"
    protocol_version = "HTTP/1.1"

    # shared state, set in main()
    srv: Server = None
    auth: Auth = None
    files: Files = None
    page: bytes = b""
    tls: bool = False

    def log_message(self, fmt, *args):
        if "/api/log" not in (args[0] if args else ""):
            sys.stderr.write("%s - %s\n" % (self.address_string(), fmt % args))

    # -- helpers
    def send_json(self, obj, status=200):
        body = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def send_bytes(self, body, ctype, status=200, extra=None):
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def body(self, limit=MAX_UPLOAD_BYTES):
        n = int(self.headers.get("Content-Length") or 0)
        if n > limit:
            raise ValueError("request too large")
        return self.rfile.read(n)

    def json_body(self):
        raw = self.body(MAX_EDIT_BYTES + 4096)
        return json.loads(raw.decode() or "{}")

    def token(self):
        cookie = self.headers.get("Cookie", "")
        for part in cookie.split(";"):
            k, _, v = part.strip().partition("=")
            if k == "panel":
                return v
        return None

    def authed(self):
        t = self.token()
        return bool(t and self.auth.valid(t))

    def same_origin(self):
        # API calls must come from the page itself (blocks cross-site form posts).
        origin = self.headers.get("Origin") or self.headers.get("Referer") or ""
        host = self.headers.get("Host", "")
        return (not origin) or urllib.parse.urlsplit(origin).netloc == host

    # -- routes
    def do_GET(self):
        url = urllib.parse.urlsplit(self.path)
        q = urllib.parse.parse_qs(url.query)
        if url.path == "/":
            self.send_bytes(self.page, "text/html; charset=utf-8", extra={"Cache-Control": "no-store"})
            return
        if url.path == "/api/session":
            self.send_json({"authed": self.authed(), "has_password": self.auth.hash is not None})
            return
        if not self.authed():
            self.send_json({"error": "unauthorized"}, 401)
            return
        try:
            if url.path == "/api/status":
                self.send_json(self.srv.status())
            elif url.path == "/api/log":
                since = int(q.get("since", ["0"])[0])
                seq, lines = self.srv.since(since, wait=20.0 if q.get("wait") else 0.0)
                self.send_json({"seq": seq, "lines": lines, "running": self.srv.running()})
            elif url.path == "/api/files":
                self.send_json(self.files.list(q.get("path", [""])[0]))
            elif url.path == "/api/file":
                self.send_json({"content": self.files.read(q.get("path", [""])[0])})
            elif url.path == "/api/download":
                path = self.files.resolve(q.get("path", [""])[0])
                with open(path, "rb") as f:
                    data = f.read()
                ctype = mimetypes.guess_type(path)[0] or "application/octet-stream"
                self.send_bytes(data, ctype, extra={"Content-Disposition": f'attachment; filename="{os.path.basename(path)}"'})
            else:
                self.send_json({"error": "not found"}, 404)
        except (OSError, ValueError, PermissionError) as e:
            self.send_json({"error": str(e)}, 400)

    def do_POST(self):
        url = urllib.parse.urlsplit(self.path)
        q = urllib.parse.parse_qs(url.query)
        if not self.same_origin():
            self.send_json({"error": "cross-site request blocked"}, 403)
            return
        try:
            if url.path == "/api/login":
                data = self.json_body()
                ip = self.client_address[0]
                if self.auth.throttled(ip):
                    self.send_json({"error": "too many attempts, wait 5 minutes"}, 429)
                    return
                time.sleep(0.3)  # slow down guessing
                if self.auth.check(str(data.get("password", ""))):
                    self.auth.succeeded(ip)
                    token = self.auth.login()
                    self.send_response(200)
                    secure = "; Secure" if self.tls else ""
                    self.send_header("Set-Cookie", f"panel={token}; HttpOnly; SameSite=Strict; Path=/{secure}")
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", "15")
                    self.end_headers()
                    self.wfile.write(b'{"authed":true}')
                else:
                    self.auth.failed(ip)
                    self.send_json({"error": "wrong password"}, 401)
                return
            if not self.authed():
                self.send_json({"error": "unauthorized"}, 401)
                return
            if url.path == "/api/logout":
                self.auth.logout(self.token())
                self.send_json({"ok": True})
            elif url.path in ("/api/start", "/api/stop", "/api/restart", "/api/kill"):
                result = getattr(self.srv, url.path.rsplit("/", 1)[1])()
                self.send_json({"result": result, "status": self.srv.status()})
            elif url.path == "/api/command":
                self.send_json({"result": self.srv.command(str(self.json_body().get("command", "")))})
            elif url.path == "/api/file":
                data = self.json_body()
                self.files.write(data["path"], data["content"])
                self.send_json({"ok": True})
            elif url.path == "/api/mkdir":
                self.files.mkdir(self.json_body()["path"])
                self.send_json({"ok": True})
            elif url.path == "/api/delete":
                self.files.delete(self.json_body()["path"])
                self.send_json({"ok": True})
            elif url.path == "/api/rename":
                data = self.json_body()
                self.files.rename(data["path"], data["new_path"])
                self.send_json({"ok": True})
            elif url.path == "/api/upload":
                self.files.write_bytes(q.get("path", [""])[0], self.body())
                self.send_json({"ok": True})
            else:
                self.send_json({"error": "not found"}, 404)
        except (OSError, ValueError, PermissionError, KeyError, json.JSONDecodeError) as e:
            self.send_json({"error": str(e)}, 400)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--server", default=os.path.join(HERE, "..", "server"), help="server folder (holds pumpkin and pumpkin.toml)")
    ap.add_argument("--binary", default="pumpkin.exe" if os.name == "nt" else "pumpkin", help="server executable inside the folder")
    ap.add_argument("--bind", default="127.0.0.1:8080", help="address:port to listen on")
    ap.add_argument("--set-password", action="store_true", help="change the panel password and exit")
    ap.add_argument("--autostart", action="store_true", help="start the server when the panel starts")
    ap.add_argument("--tls", action="store_true", help="serve HTTPS with a self-signed certificate (created in the server folder)")
    args = ap.parse_args()

    server_dir = os.path.realpath(args.server)
    if not os.path.isdir(server_dir):
        sys.exit(f"server folder not found: {server_dir}")
    auth = Auth(os.path.join(server_dir, ".panel-auth.json"))
    if args.set_password or auth.hash is None:
        pw = os.environ.get("PANEL_PASSWORD")  # for unattended setups (systemd, Docker)
        if not pw:
            pw = getpass.getpass("Panel password: ")
            if pw != getpass.getpass("Repeat: "):
                sys.exit("passwords differ")
        if len(pw) < 8:
            sys.exit("use at least 8 characters")
        auth.set_password(pw)
        print("password saved")
        if args.set_password:
            return

    with open(os.path.join(HERE, "panel.html"), "rb") as f:
        Handler.page = f.read()
    Handler.srv = Server(server_dir, args.binary)
    Handler.auth = auth
    Handler.files = Files(server_dir)
    if args.autostart:
        Handler.srv.start()

    host, _, port = args.bind.rpartition(":")
    httpd = ThreadingHTTPServer((host or "127.0.0.1", int(port)), Handler)
    httpd.daemon_threads = True
    scheme = "http"
    if args.tls:
        cert = os.path.join(server_dir, ".panel-cert.pem")
        key = os.path.join(server_dir, ".panel-key.pem")
        if not (os.path.exists(cert) and os.path.exists(key)):
            if shutil.which("openssl") is None:
                sys.exit("--tls needs the openssl command to create a certificate (apt install openssl)")
            subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650", "-subj", "/CN=serverjar-panel",
                            "-keyout", key, "-out", cert], check=True, capture_output=True)
            os.chmod(key, 0o600)
            print(f"created self-signed certificate {cert}")
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(cert, key)
        httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
        Handler.tls = True
        scheme = "https"

    def shutdown(*_):
        print("\nstopping server...")
        Handler.srv.stop()
        os._exit(0)

    signal.signal(signal.SIGINT, shutdown)
    signal.signal(signal.SIGTERM, shutdown)
    print(f"panel on {scheme}://{args.bind}  (server folder: {server_dir})")
    if not args.tls and not host.startswith("127.") and host not in ("localhost", ""):
        print("WARNING: listening on a public address without --tls; the password travels unencrypted.")
    httpd.serve_forever()


if __name__ == "__main__":
    main()
