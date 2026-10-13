# renox.rs: the landing page and the blog

**https://renox.rs** says why Renox exists and why it is powerful. It is a Renox app
(`renox-www`) compiled into one binary. The guides live elsewhere, on the docs site at
**https://docs.renox.rs** (`site/`); this site links to them everywhere.

```bash
cd www
cp .env.example .env
cargo run            # or: rnx serve
```

Then open <http://127.0.0.1:3000>.

## Writing a post

Add a Markdown file to [`content/blog/`](content/blog), named `YYYY-MM-DD-the-slug.md`, with
a front matter block:

```markdown
---
title: How a benchmark made Renox 3.3× faster
description: One sentence for search results, social cards and the feed (160 characters at most).
date: 2026-10-07
updated: 2026-10-09          (optional)
author: Arif Rachim          (optional)
tags: performance, rust      (optional, comma separated)
---
The post, in Markdown. Start sections at `##`: the post's title is the page's h1.
```

That one file gives you all of this:
- the page at `/blog/the-slug`, with a table of contents;
- its entry on `/blog`, on the home page and on each tag's page;
- the Atom feed (`/blog/feed.xml`) and the sitemap;
- `llms.txt` and `llms-full.txt`;
- the plain Markdown at `/blog/the-slug.md`.

A post dated in the future stays hidden until its day. Code blocks are coloured like the
docs' (` ```rust `, ` ```bash `, ` ```php `, ` ```toml `…). Raw HTML is shown as text.
`cargo test -p renox-www` checks every post's front matter, its title length (70 characters)
and its description length (160).

## Search engines and language models

Nothing on the site needs JavaScript to be read. The landing page's text, code samples (coloured
on the server) and numbers are in the HTML, and [`public/www.js`](public/www.js) only animates
them (and nothing moves under `prefers-reduced-motion`). On top of that:

| What | Where |
|---|---|
| Title, description, canonical URL, Open Graph and Twitter cards (`img/og.png`), `article:*` on posts | `seo()` in the views |
| JSON-LD: `WebSite` + `SoftwareSourceCode` (home), `Blog`, `BlogPosting` + `BreadcrumbList` (posts) | [src/lib.rs](src/lib.rs) |
| `sitemap.xml` with each page's last change, `robots.txt` allowing every crawler | [src/lib.rs](src/lib.rs), [public/robots.txt](public/robots.txt) |
| An Atom feed | `/blog/feed.xml` |
| `llms.txt` (what Renox is, every docs page with one line, the posts), `llms-full.txt` (the landing page's text and every post) | [src/lib.rs](src/lib.rs) |
| Each post as Markdown | `/blog/{slug}.md` |
| One `h1` per page, descriptive link text, a skip link, `noindex` on the 404 page | the views |
| Fast first paint: fonts preloaded, image sizes set, the script deferred, the layout set before the styles (no shift) | [layouts/app.html](resources/views/layouts/app.html) |

[tests/www.rs](tests/www.rs) checks the tags, the structured data, the feed, the sitemap and
`llms.txt`. It also checks that every link to `docs.renox.rs/docs/…` names a page the docs
site has.

## Benchmarks on the page

The benchmark section shows [`content/benchmarks.json`](content/benchmarks.json), written
from a run of [`benchmarks/run.sh`](../benchmarks). The section is hidden while the file is
`null`. Its shape:

```json
{
  "summary": "One sentence under the heading.",
  "note": "Machine, settings and date of the run.",
  "charts": [
    { "title": "Requests per second", "unit": "/page · HTML from SQLite", "rows": [
      { "name": "Renox", "value": 9935, "label": "9,935" }
    ] }
  ]
}
```

## Where it runs

`renox.rs` runs on the owner's server, like the docs; GitHub holds no key to it.

- [`.github/workflows/release-www.yml`](../.github/workflows/release-www.yml) ("Release build
  (www)") tests and builds `renox-www` on Ubuntu 24.04 when `www/`, the docs or the crates
  change on `main`. It uploads the stripped binary as the artifact `renox-www-<sha>` (with
  `REVISION` and a sha256), kept 30 days. It can also be run by hand from the Actions tab.
- The server pulls each new `renox-www-<sha>` the way it pulls `renox-site-<sha>`. It checks
  the sha256, installs the binary, restarts the service (socket activation: visitors wait,
  nobody is refused) and checks `/health`.

So a new post is live a few minutes after it is merged.

### By hand

```bash
cargo build --release -p renox-www        # from the repository root
sudo useradd --system --home /opt/renox-www renox-www
sudo mkdir -p /opt/renox-www/storage
sudo cp target/release/renox-www /opt/renox-www/
sudo cp www/.env.example /opt/renox-www/.env   # then edit it, below
sudo chown -R renox-www /opt/renox-www
sudo cp www/deploy/renox-www.service www/deploy/renox-www.socket /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now renox-www.socket
sudo systemctl start renox-www
```

Set these in `/opt/renox-www/.env`:
- `APP_ENV=production` and `APP_DEBUG=false`;
- `APP_KEY=`, from `rnx key:generate --show`;
- `APP_URL=https://renox.rs` (canonical links, the sitemap, the feed and the JSON-LD use it);
- `APP_PORT=3090` (the socket's port);
- `TRUSTED_PROXIES=127.0.0.1` and `TRUSTED_HOSTS=renox.rs,www.renox.rs`.
- `GA4_MEASUREMENT_ID` only to use another Google Analytics property: renox.rs's own
  (`G-48GJK3ZM4J`, `landing::GA4`) is the default. The tag is sent in production only, with the
  request's CSP nonce, and the CSP allows Google's script and collection hosts.

Then the proxy, for example Caddy. `www` redirects to the bare domain, so search engines see one
address:

```text
renox.rs {
    reverse_proxy 127.0.0.1:3090
}
www.renox.rs {
    redir https://renox.rs{uri} permanent
}
```
