# The documentation site

Renox's documentation site, built with Renox: the guides in `docs/`, the
tutorial, the "Coming from Laravel" guide, the cheat sheet, the changelog and
the project's pages, rendered from this repository's Markdown and compiled
into one binary. Pages carry an `ETag` (`Routes::etag`), there's a search
(`/search?q=`) and a sitemap.

## Run it

```bash
cd site
cargo run          # or: rnx serve
```

Then open http://127.0.0.1:3000. Edit a guide in `docs/` and rebuild: the
pages are compiled in (`include_str!`), so the site always shows the docs of
the commit it was built from.

| What | Where |
|---|---|
| The pages and the sidebar's order | [src/content.rs](src/content.rs) |
| Markdown to HTML: heading anchors, the table of contents, doctest setup lines hidden, links between files turned into site addresses | [src/render.rs](src/render.rs) |
| Routes, search, sitemap | [src/lib.rs](src/lib.rs) |
| The kit's shell with the guides in the sidebar | [resources/views/layouts/app.html](resources/views/layouts/app.html) |
| Long-form text styles | [public/site.css](public/site.css) |
| Every page renders, links land on headings that exist | [tests/site.rs](tests/site.rs) |

## Where it runs

The site runs at **https://docs.renox.rs**, on the owner's Ubuntu 24.04 server, which
pulls its releases; GitHub holds no key to the server.

- [`.github/workflows/release-site.yml`](../.github/workflows/release-site.yml) ("Release
  build (site)") builds `renox-site` on Ubuntu 24.04 (the server's release, so glibc matches)
  when `site/`, `docs/`, `crates/` or the root Markdown change on `main`, and uploads the
  stripped binary as the artifact `renox-site-<sha>` (with `REVISION` and a sha256), kept 30
  days. It also runs from the Actions tab (Run workflow).
- Every five minutes the server looks for a newer `renox-site-<sha>` artifact from `main`,
  checks its sha256, installs it next to the previous releases, restarts the site (socket
  activation: visitors wait, nobody is refused) and checks `/health`. When `/health` fails it
  goes back to the previous release and skips that commit until a newer one is built.

So a change to the docs is live a few minutes after it's merged.

## By hand

The site keeps no data. On a Linux server with systemd, behind Caddy or nginx:

1. Build on a machine like the server: `cargo build --release -p renox-site`
   (from the repository root) → `target/release/renox-site`.
2. On the server:

   ```bash
   sudo useradd --system --home /opt/renox-site renox-site
   sudo mkdir -p /opt/renox-site/storage
   sudo cp renox-site /opt/renox-site/
   sudo cp .env.example /opt/renox-site/.env   # then edit it, below
   sudo chown -R renox-site /opt/renox-site
   sudo cp deploy/renox-site.service deploy/renox-site.socket /etc/systemd/system/
   sudo systemctl daemon-reload
   sudo systemctl enable --now renox-site.socket
   sudo systemctl start renox-site
   ```

3. In `/opt/renox-site/.env`: `APP_ENV=production`, `APP_DEBUG=false`,
   `APP_KEY=` (from `rnx key:generate --show`), `APP_URL=https://your-domain`
   (links in the sitemap use it), `TRUSTED_PROXIES=127.0.0.1` and
   `TRUSTED_HOSTS=your-domain`, `APP_PORT=3080` (the socket's port; 3000 is often taken by
   another app on the same server).
4. The proxy, e.g. Caddy:

   ```text
   docs.example.com {
       reverse_proxy 127.0.0.1:3080
   }
   ```

5. A new version of the docs: build again, copy the binary over and
   `sudo systemctl restart renox-site` (the socket keeps the port open).
