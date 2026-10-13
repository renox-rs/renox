# learn.renox.rs

The Renox course: one app (a product catalog with stock) built stage by stage, from `rnx new`
to a live component with tests. Each stage will have its chapter, screenshots, a short video
and a challenge solved in an editor that knows Renox and checked by compiling and testing the
student's code. This directory starts with its skeleton: the layout, the course's
outline on the home page, the sitemap, `robots.txt` and `llms.txt`.

It is a Renox app compiled into one binary (views and public files embedded), like
[`site/`](../site) (docs.renox.rs) and [`www/`](../www) (renox.rs).

| Where | What |
|---|---|
| [`src/lib.rs`](src/lib.rs) | the app: routes, JSON-LD (`Course`), sitemap, `llms.txt` |
| [`src/course.rs`](src/course.rs) | the stages: number, title, summary, topics, the guide that goes deeper |
| [`resources/views/`](resources/views) | the layout, the home page, the 404 page |
| [`public/learn.css`](public/learn.css) | the styles (light and dark) |
| [`tests/learn.rs`](tests/learn.rs) | every route, the tags, the stages' order, links into the docs |
| [`deploy/`](deploy) | the systemd socket and service |

## Run it

```bash
cd learn
cp .env.example .env
cargo run            # or: rnx serve
```

Then open <http://127.0.0.1:3000>.

[tests/learn.rs](tests/learn.rs) checks the tags and the structured data, that the stages are
numbered in order, and that every link to `docs.renox.rs/docs/…` names a page the docs site has
(`renox_site::content::PAGES`).

## Where it runs

`learn.renox.rs` runs on the owner's server, like the docs and renox.rs; GitHub holds no key
to it.

- [`.github/workflows/release-learn.yml`](../.github/workflows/release-learn.yml) ("Release
  build (learn)") tests and builds `renox-learn` on Ubuntu 24.04 when `learn/`, the docs site's
  code or the crates change on `main`. It uploads the stripped binary as the artifact
  `renox-learn-<sha>` (with `REVISION` and a sha256), kept 30 days. It can also be run by hand
  from the Actions tab.
- The server pulls each new `renox-learn-<sha>` the way it pulls `renox-site-<sha>` and
  `renox-www-<sha>`: it checks the sha256, installs the binary, restarts the service (socket
  activation: visitors wait, nobody is refused) and checks `/health`.

### By hand

```bash
cargo build --release -p renox-learn        # from the repository root
sudo useradd --system --home /opt/renox-learn renox-learn
sudo mkdir -p /opt/renox-learn/storage
sudo cp target/release/renox-learn /opt/renox-learn/
sudo cp learn/.env.example /opt/renox-learn/.env   # then edit it, below
sudo chown -R renox-learn /opt/renox-learn
sudo cp learn/deploy/renox-learn.service learn/deploy/renox-learn.socket /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now renox-learn.socket
sudo systemctl start renox-learn
```

Set these in `/opt/renox-learn/.env`:
- `APP_ENV=production` and `APP_DEBUG=false`;
- `APP_KEY=`, from `rnx key:generate --show`;
- `APP_URL=https://learn.renox.rs` (canonical links, the sitemap and the JSON-LD use it);
- `APP_PORT=3100` (the socket's port);
- `TRUSTED_PROXIES=127.0.0.1` and `TRUSTED_HOSTS=learn.renox.rs`.

Then the proxy, for example Caddy:

```text
learn.renox.rs {
    reverse_proxy 127.0.0.1:3100
}
```
