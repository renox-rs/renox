#!/usr/bin/env python3
"""Both example binaries as real processes (#270): migrate, seed twice,
serve, then ask for every GET route without parameters (from `route:list`)
as a guest; a 5xx anywhere fails. tests/process/run.sh builds the binaries
first.

  python3 tests/process/examples.py              # both
  python3 tests/process/examples.py bikeshop     # one

Both are crawled a second time, logged in: the bike shop with its seeded
owner, hello with an account signed up for the run. With PROCESS_POSTGRES
naming a PostgreSQL server (postgres://user:pw@host:port), the bike shop
runs there instead, in a database of its own (built with
`--features renox/postgres`; tests/process/run.sh postgres).
"""

import base64
import http.cookiejar
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TARGET = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))

# Package (binary) → directory.
EXAMPLES = {
    "hello": "hello",
    "bikeshop": "bikeshop",
}

# The account to log in with: seeded ones, or None to sign up.
ACCOUNTS = {
    "hello": None,
    "bikeshop": ("owner@bikeshop.test", "password"),
}

# Variables an example needs beyond the common ones: the bike shop's staff
# must set up two-factor login before the staff pages open, which a crawl
# can't do.
EXTRA_ENV = {
    "bikeshop": {"BIKESHOP_STAFF_2FA": "optional"},
}

POSTGRES = os.environ.get("PROCESS_POSTGRES", "").rstrip("/")
POSTGRES_EXAMPLES = {"bikeshop": "bikeshop"}


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Guest(urllib.request.HTTPRedirectHandler):
    """Doesn't follow redirects: a 302 to /login is an answer, not an error."""

    def redirect_request(self, *args, **kwargs):
        return None


def get(url, opener=None):
    opener = opener or urllib.request.build_opener(Guest)
    try:
        with opener.open(url, timeout=15) as res:
            return res.status
    except urllib.error.HTTPError as error:
        return error.code
    except (urllib.error.URLError, ConnectionError, OSError):
        return 0  # not listening (yet)


def log_in(url, binary):
    """A cookie-keeping opener logged in to the example, or None if its form
    doesn't take the account."""
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar), Guest)

    def token(path):
        with opener.open(f"{url}{path}", timeout=15) as res:
            page = res.read().decode()
        found = re.search(r'name="csrf-token" content="([^"]+)"', page)
        if not found:
            raise AssertionError(f"no CSRF token on {path}")
        return found.group(1)

    account = ACCOUNTS[binary]
    if account is None:
        email, password = f"process-{binary}@example.com", "password123"
        fields = {"name": "Process", "email": email, "password": password, "password_confirmation": password}
        path = "/register"
    else:
        email, password = account
        fields = {"email": email, "password": password}
        path = "/login"
    fields["_token"] = token(path)
    try:
        with opener.open(f"{url}{path}", data=urllib.parse.urlencode(fields).encode(), timeout=15) as res:
            status, location = res.status, res.headers.get("Location", "")
    except urllib.error.HTTPError as error:
        status, location = error.code, error.headers.get("Location", "")
    if status not in (302, 303) or location.rstrip("/").endswith(path):
        raise AssertionError(f"{path} as {email}: {status} to {location!r}")
    return opener


def pg_database(exe, cwd, env, create=True, name=None):
    """Makes (or drops) a database on the PROCESS_POSTGRES server through the
    binary's own db:shell."""
    name = name or f"renox_examples_{os.getpid()}_{int(time.time() * 1000)}"
    statement = f"CREATE DATABASE {name};" if create else f"DROP DATABASE IF EXISTS {name} WITH (FORCE);"
    result = subprocess.run(
        [exe, "db:shell"],
        cwd=cwd,
        env={**env, "DATABASE_URL": f"{POSTGRES}/postgres"},
        input=statement + "\n",
        capture_output=True,
        text=True,
        timeout=60,
    )
    if result.returncode != 0 or "Error:" in result.stdout:
        raise AssertionError(f"{statement} failed:\n{result.stdout}\n{result.stderr}")
    return name


def routes(exe, cwd, env):
    """GET paths without parameters, on the default host, from `route:list`."""
    listing = subprocess.run([exe, "route:list"], cwd=cwd, env=env, capture_output=True, text=True, timeout=60)
    if listing.returncode != 0:
        raise AssertionError(f"route:list failed:\n{listing.stdout}\n{listing.stderr}")
    lines = listing.stdout.splitlines()
    with_domain = bool(lines) and lines[0].startswith("DOMAIN")
    paths = []
    for line in lines[1:]:
        cells = re.split(r"\s{2,}", line.strip())
        if with_domain and not line.startswith(" ") and not cells[0].startswith(("GET", "POST", "PUT", "PATCH", "DELETE", "*")):
            continue  # a route of another host
        for i, cell in enumerate(cells):
            if cell.startswith("/"):
                method = cells[i - 1] if i else ""
                if "GET" in method and "{" not in cell and "*" not in cell:
                    paths.append(cell)
                break
    return sorted(set(paths))


def smoke(binary, directory):
    exe = os.path.join(TARGET, "debug", binary)
    cwd = os.path.join(ROOT, "examples", directory)
    data = tempfile.mkdtemp(prefix=f"renox-{binary}-")
    port = free_port()
    env = {
        "PATH": os.environ["PATH"],
        "HOME": os.environ.get("HOME", "/tmp"),
        "APP_ENV": "local",
        "APP_DEBUG": "true",
        "APP_HOST": "127.0.0.1",
        "APP_PORT": str(port),
        "APP_URL": f"http://127.0.0.1:{port}",
        "APP_KEY": "base64:" + base64.b64encode(os.urandom(32)).decode(),
        "DATABASE_URL": f"sqlite://{data}/app.db",
        "STORAGE_PATH": os.path.join(data, "storage"),
        "MAIL_MAILER": "log",
        "QUEUE_WORKERS": "1",
        "RUST_LOG": "warn",
        **EXTRA_ENV.get(binary, {}),
    }
    proc = None
    database = None
    if POSTGRES:
        database = pg_database(exe, cwd, env)
        env["DATABASE_URL"] = f"{POSTGRES}/{database}"
    try:
        for command in ("migrate", "db:seed", "db:seed"):
            run = subprocess.run([exe, command], cwd=cwd, env=env, capture_output=True, text=True, timeout=300)
            if run.returncode != 0:
                raise AssertionError(f"{command} failed:\n{run.stdout}\n{run.stderr}")
        out = open(os.path.join(data, "serve.txt"), "w+")
        proc = subprocess.Popen([exe, "serve"], cwd=cwd, env=env, stdout=out, stderr=subprocess.STDOUT)
        url = f"http://127.0.0.1:{port}"
        until = time.time() + 30
        while get(f"{url}/health") != 200:
            if proc.poll() is not None or time.time() > until:
                out.seek(0)
                raise AssertionError(f"serve didn't answer:\n{out.read()}")
            time.sleep(0.2)
        broken = []
        paths = routes(exe, cwd, env)
        as_guest = {}
        for path in paths:
            status = get(f"{url}{path}")
            as_guest[path] = status
            if status >= 500:
                broken.append(f"{path} → {status}")
        # Again, logged in (never logging out half way): at least one page
        # must answer differently, or the login didn't take.
        if binary in ACCOUNTS:
            opener = log_in(url, binary)
            changed = 0
            for path in paths:
                if "logout" in path or path.endswith("/stream"):
                    continue
                status = get(f"{url}{path}", opener)
                changed += status != as_guest[path]
                if status >= 500:
                    broken.append(f"{path} (logged in) → {status}")
            if not changed:
                broken.append("logged in, every page answered as for a guest")
        if broken:
            out.seek(0)
            log = out.read()[-3000:]
            raise AssertionError(f"{len(broken)} of {len(paths)} pages failed: {broken}\n{log}")
        return len(paths)
    finally:
        if proc is not None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                proc.kill()
        if database:
            pg_database(exe, cwd, env, create=False, name=database)
        shutil.rmtree(data, ignore_errors=True)


def main():
    examples = POSTGRES_EXAMPLES if POSTGRES else EXAMPLES
    names = sys.argv[1:] or list(examples)
    failed = []
    for name in names:
        started = time.time()
        try:
            count = smoke(name, examples[name])
            print(f"ok   {name}: {count} pages ({time.time() - started:.1f}s)")
        except Exception as error:  # noqa: BLE001
            failed.append(name)
            print(f"FAIL {name}: {error}")
    if failed:
        print(f"{len(failed)} failed: {', '.join(failed)}")
        sys.exit(1)
    print(f"all {len(names)} examples answered")


if __name__ == "__main__":
    main()
