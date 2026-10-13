#!/usr/bin/env python3
"""Process e2e (#270): the app binary and `rnx` as real processes.

Uses the browser fixture (tests/browser/fixture), which tests/process/run.sh
builds first. Each check starts what it needs on a free port with its own
database and stops it again; nothing is left running.

  python3 tests/process/process.py            # every check
  python3 tests/process/process.py signals queue

With PROCESS_POSTGRES naming a PostgreSQL server (postgres://user:pw@host:port,
no database), each app gets a database of its own there, made and dropped by
the check, and the checks that are about the database engine run (the
fixture must be built with `--features renox/postgres`; tests/process/run.sh
postgres does that). `rnx build` (a release build) runs only with RNX_BUILD=1.
"""

import base64
import json
import re
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TARGET = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
FIXTURE = os.path.join(TARGET, "debug", "browser-fixture")
FIXTURE_DIR = os.path.join(ROOT, "tests", "browser", "fixture")
RNX = os.path.join(TARGET, "debug", "rnx")
# One target directory for the apps `rnx new` makes here, so the second
# build reuses Renox's compiled crates; two build jobs, as everywhere.
RNX_APPS = {"CARGO_TARGET_DIR": os.path.join(TARGET, "rnx-apps"), "CARGO_BUILD_JOBS": "2"}
POSTGRES = os.environ.get("PROCESS_POSTGRES", "").rstrip("/")
_DATABASES = iter(range(1_000_000))


def pg_database(exe, cwd, env, create=True, name=None):
    """Makes (or drops) a database on the PROCESS_POSTGRES server through the
    binary's own db:shell on the server's `postgres` database."""
    name = name or f"renox_process_{os.getpid()}_{int(time.time() * 1000)}_{next(_DATABASES)}"
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


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class App:
    """The fixture with its own port, database, storage and log file."""

    def __init__(self, **extra):
        self.dir = tempfile.mkdtemp(prefix="renox-process-")
        self.port = free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        self.log = os.path.join(self.dir, "fixture.log")
        self.env = {
            "PATH": os.environ["PATH"],
            "HOME": os.environ.get("HOME", "/tmp"),
            "APP_ENV": "local",
            "APP_DEBUG": "true",
            "APP_HOST": "127.0.0.1",
            "APP_PORT": str(self.port),
            "APP_KEY": "base64:" + base64.b64encode(os.urandom(32)).decode(),
            "DATABASE_URL": f"sqlite://{self.dir}/app.db",
            "STORAGE_PATH": os.path.join(self.dir, "storage"),
            "VIEWS_PATH": "views",
            "QUEUE_WORKERS": "0",
            "SCHEDULER": "false",
            "FIXTURE_LOG": self.log,
            "RUST_LOG": "info",
        }
        self.env.update(extra)
        self.database = None
        if POSTGRES:
            self.database = pg_database(FIXTURE, FIXTURE_DIR, self.env)
            self.env["DATABASE_URL"] = f"{POSTGRES}/{self.database}"

    def run(self, *args, stdin=None, check=True, env=None):
        """Runs a command of the binary to the end."""
        result = subprocess.run(
            [FIXTURE, *args],
            cwd=FIXTURE_DIR,
            env={**self.env, **(env or {})},
            input=stdin,
            capture_output=True,
            text=True,
            timeout=120,
        )
        if check and result.returncode != 0:
            raise AssertionError(f"{args} failed ({result.returncode}):\n{result.stdout}\n{result.stderr}")
        return result

    def start(self, *args, prefix=(), env=None, pass_fds=()):
        """Starts a long-running command; returns the process. With
        `prefix=["sh"]`, `args` is `-c <script>` and the fixture is `$0`."""
        out = open(os.path.join(self.dir, f"out-{len(os.listdir(self.dir))}.txt"), "w+")
        argv = [*prefix, *args[:2], FIXTURE] if prefix == ["sh"] else [*prefix, FIXTURE, *args]
        proc = subprocess.Popen(
            argv,
            cwd=FIXTURE_DIR,
            env={**self.env, **(env or {})},
            stdout=out,
            stderr=subprocess.STDOUT,
            text=True,
            pass_fds=pass_fds,
        )
        proc.output = out
        return proc

    def wait_health(self, proc=None, timeout=30):
        until = time.time() + timeout
        while time.time() < until:
            if proc is not None and proc.poll() is not None:
                raise AssertionError(f"it exited ({proc.returncode}):\n{output(proc)}")
            try:
                with urllib.request.urlopen(f"{self.url}/health", timeout=2) as res:
                    if res.status == 200:
                        return
            except (urllib.error.URLError, ConnectionError, OSError):
                pass
            time.sleep(0.1)
        raise AssertionError(f"no /health within {timeout}s")

    def noted(self):
        try:
            with open(self.log) as f:
                return [line.strip() for line in f]
        except FileNotFoundError:
            return []

    def close(self):
        if self.database:
            pg_database(FIXTURE, FIXTURE_DIR, self.env, create=False, name=self.database)
            self.database = None
        shutil.rmtree(self.dir, ignore_errors=True)


def output(proc):
    proc.output.flush()
    proc.output.seek(0)
    return proc.output.read()


def stop(proc, sig=signal.SIGTERM, timeout=15):
    proc.send_signal(sig)
    try:
        return proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        raise AssertionError(f"it didn't stop within {timeout}s after {sig.name}:\n{output(proc)}")


def get(url, timeout=10):
    with urllib.request.urlopen(url, timeout=timeout) as res:
        return res.status, res.read().decode()


def wait_until(check, timeout=20, what="the condition"):
    until = time.time() + timeout
    while time.time() < until:
        if check():
            return
        time.sleep(0.1)
    raise AssertionError(f"timed out waiting for {what}")


def browser(app):
    """An opener with cookies, and the page's CSRF token read from `path`."""
    import http.cookiejar
    import re

    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def token(path):
        with opener.open(f"{app.url}{path}", timeout=10) as res:
            page = res.read().decode()
        return re.search(r'name="csrf-token" content="([^"]+)"', page).group(1)

    return opener, token


def register(app, opener, token):
    """Signs up on the fixture (Auth::new()), which logs in."""
    data = urllib.parse.urlencode(
        {
            "_token": token("/register"),
            "name": "Ana",
            "email": "ana@example.com",
            "password": "password123",
            "password_confirmation": "password123",
        }
    ).encode()
    with opener.open(f"{app.url}/register", data=data, timeout=10) as res:
        assert res.status == 200, res.status


class Stream(threading.Thread):
    """Reads an event stream until the server ends it."""

    def __init__(self, opener, url):
        super().__init__(daemon=True)
        self.opener, self.url = opener, url
        self.opened = threading.Event()
        self.ended = False
        self.error = None

    def run(self):
        try:
            with self.opener.open(self.url, timeout=30) as res:
                assert res.headers.get("Content-Type", "").startswith("text/event-stream"), res.headers
                self.opened.set()
                while res.read(1024):
                    pass
            self.ended = True
        except Exception as error:  # noqa: BLE001
            self.error = error
            self.opened.set()


# ---------- checks ----------


def signals():
    """serve stops on SIGTERM and Ctrl-C (SIGINT): the request in flight
    finishes, the queue worker finishes its job, the scheduler stops, and the
    notifications and live-reload streams end."""
    for sig in (signal.SIGTERM, signal.SIGINT):
        app = App(QUEUE_WORKERS="1", SCHEDULER="true", FIXTURE_TICK="1")
        try:
            app.run("migrate")
            proc = app.start("serve")
            app.wait_health(proc)
            opener, token = browser(app)
            register(app, opener, token)
            streams = [
                Stream(opener, f"{app.url}/notifications/stream"),
                Stream(opener, f"{app.url}/_renox/live"),
            ]
            for stream in streams:
                stream.start()
                assert stream.opened.wait(10) and stream.error is None, (stream.url, stream.error)
            app.run("jobs:nap")
            wait_until(lambda: "nap started" in app.noted(), what="the job to start")
            answer = {}

            def slow():
                try:
                    answer["value"] = get(f"{app.url}/pause")
                except Exception as error:  # noqa: BLE001
                    answer["value"] = error

            thread = threading.Thread(target=slow)
            thread.start()
            time.sleep(0.4)  # the request is running
            started = time.time()
            code = stop(proc, sig)
            thread.join(10)
            text = output(proc)
            assert code == 0, f"{sig.name}: exit code {code}\n{text}"
            assert time.time() - started < 10, f"{sig.name}: took {time.time() - started:.1f}s"
            assert answer.get("value") == (200, "finished"), f"{sig.name}: the request in flight got {answer}"
            # The job that was running when the signal came finished.
            assert "nap done" in app.noted(), app.noted()
            assert "queue workers started" in text and "scheduler started" in text, text
            assert "background work was still running" not in text, text
            assert "Renox stopped" in text, text
            for stream in streams:
                stream.join(5)
                assert stream.ended, f"{sig.name}: {stream.url} wasn't ended ({stream.error})"
            ticks = app.noted().count("tick")
            time.sleep(1.5)
            assert app.noted().count("tick") == ticks, "the scheduler kept running"
        finally:
            app.close()


def socket_activation():
    """With systemd's socket (LISTEN_FDS) the app serves on it."""
    socket_restart()
    launcher = shutil.which("systemd-socket-activate")
    if not launcher:
        print("  (systemd-socket-activate part skipped: it isn't installed)")
        return
    app = App()
    try:
        app.run("migrate")
        setenv = [f"--setenv={k}" for k in app.env]
        proc = app.start("serve", prefix=[launcher, "-l", f"127.0.0.1:{app.port}", *setenv])
        app.wait_health(proc)
        assert get(f"{app.url}/")[0] == 200
        assert stop(proc) == 0
        assert "using the socket systemd passed" in output(proc), output(proc)
    finally:
        app.close()


def socket_restart():
    """What systemd does on a restart: it keeps the listening socket, so a
    request sent while no app runs waits in the queue and the next app
    (given the same socket) answers it; none is refused."""
    app = App()
    listener = socket.socket()
    try:
        app.run("migrate")
        listener.bind(("127.0.0.1", app.port))
        listener.listen(16)
        fd = listener.fileno()

        def serve():
            # fd 3 is the socket, LISTEN_PID the app's own pid (exec keeps it).
            return app.start(
                "-c",
                f'exec 3<&{fd}; LISTEN_FDS=1 LISTEN_PID=$$ exec "$0" serve',
                prefix=["sh"],
                pass_fds=(fd,),
            )

        first = serve()
        app.wait_health(first)
        assert stop(first) == 0, output(first)
        # Nothing runs now; the socket still accepts.
        answer = {}

        def wait():
            try:
                answer["value"] = get(f"{app.url}/health", timeout=60)
            except Exception as error:  # noqa: BLE001
                answer["value"] = error

        thread = threading.Thread(target=wait)
        thread.start()
        time.sleep(0.5)
        second = serve()
        thread.join(60)
        assert answer.get("value", (0,))[0] == 200, f"the request sent between the two apps got {answer}"
        assert "using the socket systemd passed" in output(second), output(second)
        assert stop(second) == 0
    finally:
        listener.close()
        app.close()


def queue():
    """queue:work --once drains; a long-running worker pool stops cleanly."""
    app = App()
    try:
        app.run("migrate")
        app.run("jobs:push", "3")
        result = app.run("queue:work", "--once", "--queue", "default,mail")
        assert "Ran 3 job(s)" in result.stdout, result.stdout
        assert sorted(app.noted()) == ["job 1", "job 2", "job 3"], app.noted()

        worker = app.start("queue:work", "--workers", "2")
        time.sleep(0.5)
        app.run("jobs:push", "2")
        wait_until(lambda: len(app.noted()) == 5, what="the worker to run two more jobs")
        assert stop(worker) == 0, output(worker)
    finally:
        app.close()


def scheduler():
    """schedule:work runs a task each second; two of them on one database run it once each time."""
    app = App(FIXTURE_TICK="1")
    try:
        app.run("migrate")
        first = app.start("schedule:work")
        second = app.start("schedule:work")
        time.sleep(4.5)
        assert stop(first) == 0 and stop(second) == 0
        ticks = app.noted().count("tick")
        assert 2 <= ticks <= 6, f"{ticks} ticks in about four seconds from two schedulers"
    finally:
        app.close()


def logs():
    """LOG_FORMAT=json, LOG_FILE, and LOG_FILE that can't be opened."""
    # The request's trace (tower_http, debug) carries the request span.
    app = App(LOG_FORMAT="json", RUST_LOG="info,tower_http=debug")
    try:
        app.run("migrate")
        proc = app.start("serve")
        app.wait_health(proc)
        get(f"{app.url}/")
        time.sleep(0.3)
        stop(proc)
        lines = [line for line in output(proc).splitlines() if line.strip()]
        parsed = [json.loads(line) for line in lines]
        assert parsed, "no log lines"
        assert any(entry.get("span", {}).get("uri") == "/" for entry in parsed), lines[-5:]
    finally:
        app.close()

    app = App()
    try:
        app.run("migrate")
        path = os.path.join(app.dir, "renox.log")
        proc = app.start("serve", env={"LOG_FILE": path})
        app.wait_health(proc)
        stop(proc)
        with open(path) as f:
            text = f.read()
        assert "Renox stopped" in text and "\x1b[" not in text, text
    finally:
        app.close()

    # Without RUST_LOG: serve with debug on logs Renox's debug lines; other
    # commands only warnings.
    app = App()
    try:
        del app.env["RUST_LOG"]
        app.run("migrate")
        quiet = app.run("jobs:push", "1")
        plain = re.sub(r"\x1b\[[0-9;]*m", "", quiet.stdout + quiet.stderr)
        assert " INFO " not in plain and " DEBUG " not in plain, plain
        proc = app.start("serve", env={"QUEUE_WORKERS": "1"})
        app.wait_health(proc)
        wait_until(lambda: "job 1" in app.noted(), what="the job")
        stop(proc)
        text = re.sub(r"\x1b\[[0-9;]*m", "", output(proc))
        assert " INFO renox_core" in text and " DEBUG renox_core" in text, text
    finally:
        app.close()

    app = App()
    try:
        app.run("migrate")
        proc = app.start("serve", env={"LOG_FILE": "/nonexistent-dir/renox.log"})
        app.wait_health(proc)
        stop(proc)
        assert "logging to stdout" in output(proc), output(proc)
    finally:
        app.close()


def app_key():
    """Production refuses to start without APP_KEY."""
    app = App(APP_ENV="production", APP_DEBUG="false")
    try:
        del app.env["APP_KEY"]
        result = app.run("serve", check=False)
        assert result.returncode != 0, result.stdout
        assert "APP_KEY" in result.stdout + result.stderr, result.stdout + result.stderr
    finally:
        app.close()
    # Locally it starts, with a temporary key and a warning.
    app = App()
    try:
        del app.env["APP_KEY"]
        app.run("migrate")
        proc = app.start("serve")
        app.wait_health(proc)
        stop(proc)
        assert "APP_KEY is not set; using a temporary key" in output(proc), output(proc)
    finally:
        app.close()


def shell():
    """db:shell reads statements from a pipe."""
    app = App()
    try:
        app.run("migrate")
        result = app.run(
            "db:shell",
            stdin=".tables\nSELECT 41 + 1 AS answer;\nSELECT * FROM no_such_table;\nSELECT 'still here' AS after;\n.quit\nSELECT 'not run';\n",
        )
        assert "jobs" in result.stdout, result.stdout
        assert "answer" in result.stdout and "42" in result.stdout, result.stdout
        assert "Error: " in result.stdout and "no_such_table" in result.stdout, result.stdout
        assert "still here" in result.stdout and "not run" not in result.stdout, result.stdout
    finally:
        app.close()


def prompts():
    """renox::prompt: answers read in turn from a pipe (a wrong one is an
    error there, for scripts); in a terminal a wrong one is asked again and
    a secret isn't echoed."""
    app = App()
    try:
        result = app.run("ask:me", stdin="Ana\ny\nlarge\ns3cret\n")
        assert "answers: Ana | true | large | 6 characters" in result.stdout, result.stdout + result.stderr
        result = app.run("ask:me", stdin="Ana\nmaybe\n", check=False)
        assert result.returncode != 0
        assert '"maybe" isn\'t yes or no' in result.stdout + result.stderr, result.stdout + result.stderr

        script = shutil.which("script")
        if not script:
            print("  (terminal part skipped: `script` isn't installed)")
            return
        # A pseudo-terminal through `script`: each answer goes when asked.
        command = f"{FIXTURE} ask:me"
        proc = subprocess.Popen(
            [script, "-qec", command, "/dev/null"],
            cwd=FIXTURE_DIR,
            env=app.env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        for answer in ["", "Ana", "maybe", "y", "medium", "large", "s3cret"]:
            time.sleep(0.4)
            proc.stdin.write(answer.encode() + b"\n")
            proc.stdin.flush()
        out, _ = proc.communicate(timeout=30)
        text = out.decode(errors="replace")
        assert "An answer is needed." in text, text
        assert "Answer yes or no." in text, text
        assert "Pick one of the options." in text, text
        assert "answers: Ana | true | large | 6 characters" in text, text
        assert "s3cret" not in text, f"the secret was echoed:\n{text}"
    finally:
        app.close()


def rnx_serve():
    """rnx serve: builds and migrates, restarts on a change, keeps the old app on a failed build."""
    if not os.path.exists(RNX):
        raise AssertionError(f"{RNX} isn't built")
    work = tempfile.mkdtemp(prefix="renox-rnx-serve-")
    if os.environ.get("KEEP_WORK"):
        print(f"  work: {work}")
    try:
        subprocess.run([RNX, "new", "watched", "--renox-path", ROOT], cwd=work, check=True, capture_output=True, text=True, timeout=300)
        app_dir = os.path.join(work, "watched")
        port = free_port()
        env = {
            **os.environ,
            **RNX_APPS,
            "APP_PORT": str(port),
            "APP_HOST": "127.0.0.1",
            "DATABASE_URL": f"sqlite://{work}/watched.db",
            "QUEUE_WORKERS": "0",
            "SCHEDULER": "false",
        }
        out = open(os.path.join(work, "rnx.txt"), "w+")
        # Its own process group, as in a terminal: Ctrl-C goes to the group.
        proc = subprocess.Popen(
            [RNX, "serve"], cwd=app_dir, env=env, stdout=out, stderr=subprocess.STDOUT, text=True, start_new_session=True
        )
        proc.output = out
        url = f"http://127.0.0.1:{port}"
        app = App()
        app.url = url
        app.wait_health(proc, timeout=600)
        # The migrations ran before the app started.
        import sqlite3

        db = sqlite3.connect(os.path.join(work, "watched.db"))
        ran = db.execute("SELECT COUNT(*) FROM renox_migrations").fetchone()[0]
        db.close()
        assert ran > 0, "no migrations ran before the start"
        # view:data ran after the build: the editor's component data exists.
        assert os.path.exists(os.path.join(app_dir, ".vscode", "renox-components.json")), "view:data wrote no file"
        # Reading files (an OPEN event) restarts nothing.
        with open(os.path.join(app_dir, "src", "lib.rs")) as f:
            f.read()
        time.sleep(2)
        assert "change detected" not in output(proc), output(proc)
        # A source change: rebuilt and restarted.
        lib = os.path.join(app_dir, "src", "lib.rs")
        with open(lib, "a") as f:
            f.write("\n// changed by tests/process\n")
        wait_until(lambda: "change detected, rebuilding" in output(proc), timeout=60, what="the change to be seen")
        app.wait_health(proc, timeout=600)
        # A build that fails: the app that runs keeps answering.
        with open(lib, "a") as f:
            f.write("\nthis is not rust\n")
        wait_until(
            lambda: "build failed; the previous version keeps running" in output(proc),
            timeout=600,
            what="the failed build",
        )
        assert get(f"{url}/health")[0] == 200, "the old app stopped"
        # Ctrl-C (SIGINT to the terminal's process group) stops rnx and the
        # app it runs.
        os.killpg(proc.pid, signal.SIGINT)
        proc.wait(timeout=30)
        try:
            get(f"{url}/health", timeout=3)
            raise AssertionError("the app still answers after Ctrl-C")
        except (urllib.error.URLError, ConnectionError, OSError):
            pass
        app.close()

        # The source fixed again for what follows.
        with open(lib) as f:
            source = f.read()
        with open(lib, "w") as f:
            f.write(source.replace("\nthis is not rust\n", ""))
        # Stopped on its own (`kill`, an editor's stop button): the app goes too,
        # so it doesn't keep the port for the next `rnx serve`.
        out = open(os.path.join(work, "rnx-2.txt"), "w+")
        proc = subprocess.Popen(
            [RNX, "serve"], cwd=app_dir, env=env, stdout=out, stderr=subprocess.STDOUT, text=True, start_new_session=True
        )
        proc.output = out
        app = App()
        app.url = url
        try:
            app.wait_health(proc, timeout=600)
            proc.send_signal(signal.SIGTERM)
            proc.wait(timeout=30)
            wait_until(lambda: not answers(f"{url}/health"), timeout=10, what="the app to stop with rnx")
        finally:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            app.close()

        # Commands rnx doesn't know run in the app (cargo run -- <command>),
        # with the app's exit code.

        def rnx(*args):
            return subprocess.run([RNX, *args], cwd=app_dir, env=env, capture_output=True, text=True, timeout=900)

        result = rnx("migrate")
        assert result.returncode == 0 and "Nothing to do." in result.stdout, result.stdout + result.stderr
        result = rnx("db:seed")
        assert result.returncode == 0 and "Seeded." in result.stdout, result.stdout + result.stderr
        result = rnx("foo:bar")
        assert result.returncode != 0 and "foo:bar" in result.stdout + result.stderr, result.stdout + result.stderr
        outside = subprocess.run([RNX, "migrate"], cwd=work, env=env, capture_output=True, text=True, timeout=60)
        assert outside.returncode != 0 and "no Cargo.toml here" in outside.stderr, outside.stderr
    finally:
        if not os.environ.get("KEEP_WORK"):
            shutil.rmtree(work, ignore_errors=True)


# A stand-in for the Tailwind binary (TAILWIND_BIN): writes the output file
# from the input, notes `--minify`, and with `--watch=always` writes it again
# whenever the input changes, until it is stopped. It records its pid.
TAILWIND_STUB = """#!/usr/bin/env python3
import os, sys, time
args = sys.argv[1:]
src = args[args.index("--input") + 1]
dst = args[args.index("--output") + 1]
open(os.environ["STUB_PIDS"], "a").write(f"{os.getpid()}\\n")
def build():
    text = open(src).read()
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    open(dst, "w").write(f"/* stub{' minified' if '--minify' in args else ''} */\\n" + text)
build()
if "--watch=always" in args:
    seen = os.stat(src).st_mtime_ns
    while True:
        time.sleep(0.2)
        now = os.stat(src).st_mtime_ns
        if now != seen:
            seen = now
            build()
"""


def answers(url):
    try:
        return get(url, timeout=2)[0] == 200
    except (urllib.error.URLError, ConnectionError, OSError):
        return False


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    # A zombie that its parent hasn't reaped counts as stopped.
    try:
        with open(f"/proc/{pid}/stat") as f:
            return f.read().split()[2] != "Z"
    except FileNotFoundError:
        return False


def rnx_tailwind():
    """rnx new --tailwind, rnx serve's Tailwind watcher and, with RNX_BUILD=1,
    rnx build, with TAILWIND_BIN pointing at a stand-in (no download)."""
    if not os.path.exists(RNX):
        raise AssertionError(f"{RNX} isn't built")
    work = tempfile.mkdtemp(prefix="renox-rnx-tailwind-")
    try:
        stub = os.path.join(work, "tailwind")
        with open(stub, "w") as f:
            f.write(TAILWIND_STUB)
        os.chmod(stub, 0o755)
        pids = os.path.join(work, "pids")
        port = free_port()
        env = {
            **os.environ,
            **RNX_APPS,
            "TAILWIND_BIN": stub,
            "STUB_PIDS": pids,
            "APP_PORT": str(port),
            "APP_HOST": "127.0.0.1",
            "DATABASE_URL": f"sqlite://{work}/inked.db",
            "QUEUE_WORKERS": "0",
            "SCHEDULER": "false",
        }
        subprocess.run([RNX, "new", "inked", "--tailwind", "--renox-path", ROOT], cwd=work, env=env, check=True, capture_output=True, text=True, timeout=300)
        app_dir = os.path.join(work, "inked")
        css = os.path.join(app_dir, "public", "css", "app.css")
        source = os.path.join(app_dir, "resources", "css", "app.css")
        with open(css) as f:
            assert f.read().startswith("/* stub */"), "rnx new built the CSS"

        out = open(os.path.join(work, "rnx.txt"), "w+")
        proc = subprocess.Popen([RNX, "serve"], cwd=app_dir, env=env, stdout=out, stderr=subprocess.STDOUT, text=True)
        proc.output = out
        app = App()
        app.url = f"http://127.0.0.1:{port}"
        try:
            app.wait_health(proc, timeout=900)
            with open(source, "a") as f:
                f.write("\n.from-the-test { color: red; }\n")
            wait_until(lambda: ".from-the-test" in open(css).read(), timeout=20, what="the watcher to rebuild the CSS")
            stop(proc, signal.SIGINT, timeout=30)
            watchers = [int(line) for line in open(pids) if line.strip()]
            wait_until(lambda: not any(alive(pid) for pid in watchers), timeout=10, what="Tailwind's watcher to stop with rnx")
        finally:
            if proc.poll() is None:
                proc.kill()
            app.close()

        if os.environ.get("RNX_BUILD") != "1":
            print("  (rnx build skipped: set RNX_BUILD=1 for the release build)")
            return
        result = subprocess.run([RNX, "build"], cwd=app_dir, env=env, capture_output=True, text=True, timeout=1800)
        assert result.returncode == 0, result.stdout + result.stderr
        exe = os.path.join(app_dir, "dist", "inked")
        assert os.access(exe, os.X_OK), os.listdir(os.path.join(app_dir, "dist"))
        with open(css) as f:
            assert f.read().startswith("/* stub minified */"), "rnx build minifies the CSS first"
        help = subprocess.run([exe, "help"], cwd=app_dir, env=env, capture_output=True, text=True, timeout=60)
        assert help.returncode == 0 and "migrate" in help.stdout, help.stdout + help.stderr
    finally:
        shutil.rmtree(work, ignore_errors=True)


def commands():
    """The binary's built-in commands and what they print (#249)."""
    import sqlite3

    app = App()
    try:
        out = app.run("migrate:status").stdout
        assert "  pending          00010101000100_create_jobs_table" in out, out
        out = app.run("migrate").stdout
        assert "Migrated: 00010101000100_create_jobs_table" in out, out
        assert app.run("migrate").stdout.strip() == "Nothing to do."

        # An applied migration whose file is gone, and one edited after it ran.
        db = sqlite3.connect(os.path.join(app.dir, "app.db"))
        db.execute(
            "INSERT INTO renox_migrations (name, batch, applied_at, checksum) "
            "VALUES ('29990101000000_gone', 1, 'then', NULL)"
        )
        db.execute(
            "UPDATE renox_migrations SET checksum = 'edited' "
            "WHERE name = '00010101000200_create_cache_table'"
        )
        # A failed job and a failed webhook call to list.
        db.execute(
            "INSERT INTO failed_jobs (queue, job, payload, max_attempts, error, failed_at) "
            "VALUES ('default', 'fixture-touch', '{\"n\":1}', 3, 'it broke', 0)"
        )
        db.execute(
            "INSERT INTO webhook_calls (provider, event_id, payload, status, error, received_at) "
            "VALUES ('pay', 'evt_1', ?, 'failed', 'the shop is closed\nat line 2', 0)",
            (b"{}",),
        )
        db.commit()
        db.close()
        out = app.run("migrate:status").stdout
        assert "29990101000000_gone  (applied, but its file is gone)" in out, out
        assert "00010101000200_create_cache_table  (edited after it ran; the edit won't run)" in out, out
        assert "  #1 fixture-touch (default): it broke" in app.run("queue:failed").stdout
        out = app.run("webhook:failed").stdout
        assert "  #1 pay evt_1: the shop is closed\n" in out, out
        assert "Webhook call #1 queued again." in app.run("webhook:retry", "1").stdout
        assert "No failed webhook calls." in app.run("webhook:failed").stdout
        for args, says in [
            (("webhook:retry", "99"), "there is no webhook call #99"),
            (("webhook:retry", "abc"), "usage: webhook:retry <id>"),
            (("migrate:rollback", "--step", "x"), "--step needs a number"),
            (("schedule:run",), "usage: schedule:run <task>"),
        ]:
            failed = app.run(*args, check=False)
            assert failed.returncode != 0 and says in failed.stdout + failed.stderr, (args, failed.stderr)
        # `--queue` without a value works every queue.
        app.run("queue:work", "--once", "--queue")

        # The scheduler's list, without and with a task.
        assert "No scheduled tasks." in app.run("schedule:list").stdout
        out = app.run("schedule:list", env={"FIXTURE_TICK": "1"}).stdout
        assert "tick" in out and "UTC" in out, out
        out = app.run("schedule:list", env={"FIXTURE_NEVER": "1"}).stdout
        assert any(line.strip().startswith("never") and "never-on-the-hour" in line for line in out.splitlines()), out

        # Maintenance: the bypass and Retry-After, and `up` twice.
        out = app.run("down", "--secret", "let-me-in", "--retry", "60").stdout
        assert "The app is down. Visit /let-me-in to bypass it." in out, out
        proc = app.start("serve")
        try:
            app.wait_health(proc)
            try:
                urllib.request.urlopen(f"{app.url}/", timeout=10)
                raise AssertionError("not down")
            except urllib.error.HTTPError as down:
                assert down.code == 503 and down.headers.get("Retry-After") == "60", down.headers
        finally:
            stop(proc)
        assert "The app is up." in app.run("up").stdout
        assert "The app was not down." in app.run("up").stdout

        # The kit copied into the app (into a temporary place), then kept
        # without --force and replaced with it.
        place = {"VIEWS_PATH": os.path.join(app.dir, "views"), "PUBLIC_PATH": os.path.join(app.dir, "public")}
        assert "Wrote " in app.run("ui:publish", env=place).stdout
        kept = app.run("ui:publish", check=False, env=place)
        assert kept.returncode != 0 and "add --force to replace it" in kept.stderr + kept.stdout
        assert "Wrote " in app.run("ui:publish", "--force", env=place).stdout

        # The route list with the DOMAIN column; help with the app's commands.
        out = app.run("route:list").stdout
        assert out.splitlines()[0].startswith("DOMAIN") and "{team}.fixture.test" in out, out
        out = app.run("help").stdout
        assert "App commands:" in out and "jobs:push" in out and "Queues Touch jobs" in out, out

        # Rolling back by steps: everything ran in one batch; the migration
        # whose file is gone is forgotten.
        out = app.run("migrate:rollback", "--step", "2").stdout
        assert "Rolled back: 00010101000100_create_jobs_table" in out, out
        assert "Rolled back: 29990101000000_gone" in out, out
        assert "Nothing to do." in app.run("migrate:rollback").stdout
    finally:
        app.close()


CHECKS = {
    "commands": commands,
    "signals": signals,
    "socket": socket_activation,
    "queue": queue,
    "scheduler": scheduler,
    "logs": logs,
    "app_key": app_key,
    "shell": shell,
    "prompts": prompts,
    "rnx_serve": rnx_serve,
    "rnx_tailwind": rnx_tailwind,
}

# The checks about the database engine, run again with PROCESS_POSTGRES.
POSTGRES_CHECKS = ["signals", "queue", "scheduler", "shell"]


def main():
    if POSTGRES:
        names = sys.argv[1:] or POSTGRES_CHECKS
    else:
        slow = ("rnx_serve", "rnx_tailwind")
        names = sys.argv[1:] or [n for n in CHECKS if n not in slow or os.environ.get("RNX_SERVE") == "1"]
    failed = []
    for name in names:
        started = time.time()
        try:
            CHECKS[name]()
            print(f"ok   {name} ({time.time() - started:.1f}s)")
        except Exception as error:  # noqa: BLE001
            failed.append(name)
            print(f"FAIL {name}: {error}")
    if failed:
        print(f"{len(failed)} failed: {', '.join(failed)}")
        sys.exit(1)
    print(f"all {len(names)} passed")


if __name__ == "__main__":
    main()
