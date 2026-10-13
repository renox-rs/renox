# Results

The latest run of [`run.sh`](run.sh) on `main` (after #334, which made `AppState` one `Arc`).
How it is measured, and what it doesn't say: [README.md](README.md). The raw numbers are in
[`results/`](results).

Run 2026-10-07T17-41-10Z on Intel(R) Core(TM) i7-6700 CPU @ 3.40GHz (8 cores, Linux 7.0.0-34-generic).
Each server on cores 0,1 with 1g of memory; oha 1.16.0, 64 connections, 15s per endpoint after a 5s warm-up.

**Requests per second** (higher is better)

| | `/plaintext` | `/json` | `/db` | `/page` |
|---|---:|---:|---:|---:|
| Renox | 29,287 | 28,164 | 15,100 | 9,630 |
| Axum (bare) | 106,011 | 101,252 | 29,642 | 15,137 |
| Laravel (PHP-FPM + nginx) | 719 | 722 | 611 | 581 |
| Laravel Octane (FrankenPHP) | 1,398 | 1,390 | 1,216 | 1,066 |

**Latency, p50 / p99 in ms** (lower is better)

| | `/plaintext` | `/json` | `/db` | `/page` |
|---|---:|---:|---:|---:|
| Renox | 2.23 / 3.45 | 2.33 / 3.55 | 4.28 / 7.46 | 6.67 / 11.63 |
| Axum (bare) | 0.61 / 0.91 | 0.64 / 0.95 | 2.12 / 3.49 | 4.17 / 6.91 |
| Laravel (PHP-FPM + nginx) | 89.95 / 116.34 | 89.11 / 119.73 | 105.03 / 136.01 | 111.99 / 146.12 |
| Laravel Octane (FrankenPHP) | 45.35 / 62.16 | 45.63 / 61.97 | 52.11 / 68.05 | 59.72 / 75.39 |

**Resources**

| | Cold start | Memory at rest | Peak memory under load | What you deploy |
|---|---:|---:|---:|---:|
| Renox | 655 ms | 3 MiB | 12 MiB | 18.8 MiB, one binary |
| Axum (bare) | 493 ms | 5 MiB | 11 MiB | 5.7 MiB, one binary |
| Laravel (PHP-FPM + nginx) | 2,694 ms | 36 MiB | 106 MiB | 26.5 MiB, app + vendor (plus PHP) |
| Laravel Octane (FrankenPHP) | 1,120 ms | 93 MiB | 128 MiB | 26.5 MiB, app + vendor (plus PHP) |

## In short

- **A real page** (`/page`: 20 rows from SQLite through a template): Renox serves **9× Laravel
  Octane** and **16× Laravel on PHP-FPM**, at about a tenth of the latency.
- **Memory:** 12 MiB under load against 106–128 MiB for Laravel.
- **Deploy:** one 19 MiB file.
- **Against bare Axum:** Renox's defaults (security headers with a CSP nonce, the request id,
  an encrypted session cookie, CSRF, the user, the locale, the view layer, error pages and the
  timeout) cost about a third of `/page`'s throughput. On a route that does nothing, they cost
  more, because there they are all the work there is.

## Before #334

The first run, on 2026-10-07 before #334, had Renox at 9,265 requests a second on
`/plaintext` and 6,237 on `/page`. Every middleware layer cloned a 35-field `AppState`.
[`results/2026-10-07T15-49-18Z-before-334.json`](results/2026-10-07T15-49-18Z-before-334.json)
keeps that run, and the blog post
[How a benchmark made Renox 3.3× faster](https://renox.rs/blog/how-a-benchmark-made-renox-faster)
tells the story.
