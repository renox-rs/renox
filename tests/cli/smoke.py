#!/usr/bin/env python3
"""Drives an app made by `rnx` over HTTP, the way a person in a browser does (#142).

    tests/cli/smoke.py resources <base url> <sqlite file> <app binary>
    tests/cli/smoke.py starter   <base url> <sqlite file> <app binary>

`resources`: the app tests/cli/run.sh makes with every generator. Every GET page answers
without a server error, as a guest and logged in; the `products` module from
`make:module products --resource --fields …` is created, shown, edited (unticking a checkbox
must store false) and deleted through its forms, with CSRF, validation errors and toasts.

`starter`: an app from `rnx new --starter`. Sign-up lands on email verification; the seeded
admin sees the dashboard, the users page and the activity log and changes a member's roles;
the member gets 403 on the admin pages.

Standard library only. A failure says which step and shows the response.
"""

import http.cookiejar
import re
import sqlite3
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request


def fail(step, response=None):
    detail = ""
    if response is not None:
        detail = f"\n  status {response.status}, location {response.location!r}\n  body: {response.text[:600]!r}"
    sys.exit(f"smoke: {step}{detail}")


class Response:
    def __init__(self, status, headers, body):
        self.status = status
        self.location = headers.get("Location")
        self.text = body.decode("utf-8", "replace")

    def path(self):
        """The redirect's path (Location may be absolute)."""
        return urllib.parse.urlparse(self.location or "").path


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


class Browser:
    """One visitor: a cookie jar, and the CSRF token of the last page."""

    def __init__(self, base):
        self.base = base.rstrip("/")
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()), NoRedirect)
        self.token = None
        self.page = None  # the last page shown, sent as Referer like a browser does

    def request(self, method, path, form=None):
        data = None
        if form is not None:
            pairs = list(form) if isinstance(form, list) else list(form.items())
            if self.token:
                pairs.append(("_token", self.token))
            data = urllib.parse.urlencode(pairs).encode()
        req = urllib.request.Request(self.base + path, data=data, method=method)
        if data is not None:
            req.add_header("Content-Type", "application/x-www-form-urlencoded")
        if self.page:
            req.add_header("Referer", self.page)
        try:
            with self.opener.open(req, timeout=30) as res:
                response = Response(res.status, res.headers, res.read())
        except urllib.error.HTTPError as err:
            response = Response(err.code, err.headers, err.read())
        if method == "GET" and response.status == 200:
            self.page = self.base + path
        token = re.search(r'name="csrf-token" content="([^"]+)"', response.text)
        if token:
            self.token = token.group(1)
        return response

    def get(self, path):
        return self.request("GET", path)

    def post(self, path, form):
        return self.request("POST", path, form)

    def follow(self, response):
        """GET where a redirect points (as the browser would next)."""
        return self.get(response.path() + (f"?{urllib.parse.urlparse(response.location).query}"
                                           if "?" in (response.location or "") else ""))


def expect(step, response, status, location=None, see=None, dont_see=None):
    if response.status != status:
        fail(f"{step}: expected {status}", response)
    if location is not None and response.path() != location:
        fail(f"{step}: expected a redirect to {location}", response)
    for text in [see] if isinstance(see, str) else (see or []):
        if text not in response.text:
            fail(f"{step}: the page should show {text!r}", response)
    for text in [dont_see] if isinstance(dont_see, str) else (dont_see or []):
        if text in response.text:
            fail(f"{step}: the page shouldn't show {text!r}", response)
    print(f"ok   {step}")
    return response


def get_routes(binary):
    """GET routes without parameters, from the app's own `route:list`."""
    out = subprocess.run([binary, "route:list"], capture_output=True, text=True, check=True).stdout
    routes = []
    for line in out.splitlines()[1:]:
        cells = line.split()
        if len(cells) >= 2 and "GET" in cells[0] and "{" not in cells[1]:
            routes.append(cells[1])
    if not routes:
        sys.exit(f"smoke: route:list listed no GET routes:\n{out}")
    return routes


def every_page_answers(browser, routes, who):
    for path in routes:
        # Renox's own pages, and server-sent event streams (they never end).
        if path.startswith("/_renox/") or path.endswith("/stream") or path == "/logout":
            continue
        response = browser.get(path)
        if response.status >= 500:
            fail(f"GET {path} as {who}: a server error", response)
    print(f"ok   every GET page answers {who} without a server error ({len(routes)} routes)")


def register(browser, name, email, home):
    browser.get("/register")
    response = browser.post("/register", {
        "name": name, "email": email,
        "password": "secret-password-42", "password_confirmation": "secret-password-42"})
    return expect(f"register {email}", response, 303, location=home)


def login(browser, email, password, home):
    browser.get("/login")
    response = browser.post("/login", {"email": email, "password": password})
    return expect(f"log in as {email}", response, 303, location=home)


def resources(base, database, binary):
    routes = get_routes(binary)
    guest = Browser(base)
    every_page_answers(guest, routes, "a guest")
    expect("a guest is sent to log in", guest.get("/products"), 303, location="/login")

    me = Browser(base)
    register(me, "Ana", "ana@example.com", "/")
    every_page_answers(me, routes, "a logged-in user")

    expect("the list starts empty", me.get("/products"), 200, see="Nothing here yet")
    expect("the create form", me.get("/products/new"), 200, see=['name="name"', 'name="price"'])

    # A form with errors goes back, with the messages and what was typed.
    bad = me.post("/products", {"name": "", "price": "abc", "notes": "kept", "due_on": "2026-12-01"})
    expect("an invalid form goes back", bad, 303, location="/products/new")
    expect("…and shows the errors and the old input", me.follow(bad), 200,
           see=["The name field is required.", "kept"])

    saved = me.post("/products", {"name": "Widget", "price": "1250", "notes": "First one",
                                  "active": "on", "due_on": "2026-12-01"})
    expect("a valid form saves", saved, 303, location="/products")
    listing = expect("…and the list shows it, with a toast", me.follow(saved), 200,
                     see=["Widget", "Saved."])
    edit = re.search(r'href="(?:[^"]*?)/products/(\d+)/edit"', listing.text)
    if not edit:
        fail("the list has no edit link", listing)
    record = edit.group(1)

    expect("the record's page", me.get(f"/products/{record}"), 200, see="Widget")
    expect("the edit form is filled in", me.get(f"/products/{record}/edit"), 200,
           see=['value="Widget"', 'name="_method"'])

    # Unticked: the checkbox isn't sent at all; it must be stored as false (#160).
    updated = me.post(f"/products/{record}", [("_method", "PUT"), ("name", "Gadget"),
                                               ("price", "700"), ("notes", "Second thoughts"),
                                               ("due_on", "2027-01-15")])
    expect("the edit saves", updated, 303, location=f"/products/{record}")
    expect("…and the page shows the change", me.follow(updated), 200,
           see=["Gadget", "Changes saved."], dont_see="Widget")
    row = sqlite3.connect(database).execute(
        "SELECT name, price, active, due_on FROM products WHERE id = ?", (record,)).fetchone()
    if row is None or row[0] != "Gadget" or row[1] != 700 or row[2] not in (0, "0") \
            or not str(row[3]).startswith("2027-01-15"):
        sys.exit(f"smoke: the row after the edit is {row!r}: expected Gadget, 700, the unticked box false, 2027-01-15")
    print("ok   …and the row has the new values, the unticked box stored as false")

    deleted = me.post(f"/products/{record}", {"_method": "DELETE"})
    expect("delete", deleted, 303, location="/products")
    expect("…and the list is empty again", me.follow(deleted), 200,
           see=["Deleted.", "Nothing here yet"], dont_see="Gadget")
    expect("a record that doesn't exist is a 404", me.get(f"/products/{record}"), 404)

    # Another --resource module, made without --fields.
    expect("the tags module answers", me.get("/tags"), 200)
    expect("its create form", me.get("/tags/new"), 200)


def starter(base, database, binary):
    routes = get_routes(binary)
    every_page_answers(Browser(base), routes, "a guest")

    newcomer = Browser(base)
    register(newcomer, "Nia", "nia@example.com", "/dashboard")
    expect("a new account verifies its email first", newcomer.get("/dashboard"), 303,
           location="/verify-email")
    expect("the verification page", newcomer.get("/verify-email"), 200)

    subprocess.run([binary, "db:seed"], check=True, capture_output=True)
    # APP_ENV=local (.env): the login page lists the seeded people to log in with a tap.
    expect("the login page lists the seeded accounts", Browser(base).get("/login"), 200,
           see=["Seeded accounts", 'data-demo-email="admin@example.com"'])
    expect("the home page's patterns", Browser(base).get("/"), 200,
           see=["<em>ready to grow.</em>", "rx-tabbar", "app-showcase"])
    admin = Browser(base)
    login(admin, "admin@example.com", "password123", "/dashboard")
    every_page_answers(admin, routes, "the admin")
    expect("the admin's dashboard", admin.get("/dashboard"), 200, see="Welcome back")
    users = expect("the users page", admin.get("/users"), 200, see="member@example.com")
    expect("the activity log", admin.get("/activity"), 200, see="auth.login")

    member_id = sqlite3.connect(database).execute(
        "SELECT id FROM users WHERE email = 'member@example.com'").fetchone()[0]
    if f"/users/{member_id}/roles" not in users.text:
        fail("the users page has no roles form for the member", users)
    changed = admin.post(f"/users/{member_id}/roles", [("_method", "PUT"), ("roles", "member")])
    if changed.status not in (200, 303):
        fail("the admin changes a member's roles", changed)
    print("ok   the admin changes a member's roles")
    expect("…and the change is in the activity log", admin.get("/activity"), 200,
           see="user.roles_changed")

    member = Browser(base)
    login(member, "member@example.com", "password123", "/dashboard")
    expect("a member's dashboard", member.get("/dashboard"), 200)
    expect("a member can't open the users page", member.get("/users"), 403)
    expect("…or the activity log", member.get("/activity"), 403)


if __name__ == "__main__":
    if len(sys.argv) != 5 or sys.argv[1] not in ("resources", "starter"):
        sys.exit(__doc__)
    {"resources": resources, "starter": starter}[sys.argv[1]](*sys.argv[2:])
    print(f"smoke: {sys.argv[1]} passed")
