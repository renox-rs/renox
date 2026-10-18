# Security policy

## Reporting a vulnerability

Please report security problems privately, not in a public issue:

- use GitHub's **Report a vulnerability** button on the
  [Security tab](https://github.com/renox-rs/renox/security/advisories/new) of this
  repository.

Include what you found, how to reproduce it (a small app or a failing test is ideal), and
what an attacker could do with it. You'll get an answer within 7 days. Once there's a fix,
it's released with an advisory that credits you, unless you'd rather not be named.

Please give us a reasonable time to ship a fix before you publish details.

## Supported versions

Renox is before 1.0: only the latest commit on `main` (and, once published, the latest
release on crates.io) gets security fixes. From 1.0, the latest minor release of the
current major version does, and so does the minor release before it for six months after
its successor ships (see [docs/stability.md](docs/stability.md)).

Apps made by `rnx new` depend on a Renox version (or, for an `rnx` built from Git, a
commit). To get a fix, move that version (or the commit pin) forward and run
`cargo update -p renox`.

## What's in scope

Anything in this repository: the `renox`, `renox-core`, `renox-macros`, `renox-cli`,
`renox-2fa`, `renox-editors` (including the HTML cleaning of rich text), `renox-oauth`
(social login: `state`, PKCE, linking accounts by email), `renox-admin` (the admin panel:
who may open it, the policy checks before every page and action) and `renox-billing`
(subscriptions: the payment gateways' webhook signatures, who may change a subscription)
and `renox-pdf` (PDFs from Typst templates: template paths stay inside the template root,
no package is downloaded) crates, the
templates and assets they ship, the files `rnx new` and `rnx make:*` write
(Dockerfile, systemd unit, …), and the examples when they show a pattern apps would copy.

Problems in a dependency (axum, sqlx, rustls, …) should go to that project. If Renox uses
it in an unsafe way, or its defaults expose apps, report it here too.

## What Renox already defends against

So you can tell a new problem from a known limit, `docs/audit/2026-09-pre-1.0.md` lists
what was audited before 1.0 and how each finding was fixed. In short:

- CSRF tokens on every unsafe request, sessions in an encrypted and signed cookie (with
  `SESSION_DRIVER=database` the cookie holds only an id, and the session is in the database),
  `SameSite=Lax`, `Secure` behind `https://`;
- a Content-Security-Policy with nonces, security headers and HSTS;
- Argon2id passwords (imported bcrypt hashes are rehashed at login), a password policy,
  password confirmation for sensitive pages, a login lock per email and per IP,
  constant-time comparisons for tokens, signatures and webhook MACs;
- logout per device and "log out other devices", sessions that end on a password change,
  API tokens with abilities and expiry;
- parameterized SQL, with column names and operators checked against the model; the raw
  fragments (`where_raw`, `order_by_raw`, `having_raw`, `select_as`, `sql()`) take SQL as
  written, so identifiers in them are the app's to check, and values go through `?`;
- uploads sniffed by content and served from a sandbox with `nosniff`;
- signed URLs with expiry, and redirects that never leave the site;
- `TRUSTED_PROXIES` so `X-Forwarded-For` is believed only from your proxy.

`docs/operations.md` covers running an app safely in production.
