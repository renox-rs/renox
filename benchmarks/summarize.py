#!/usr/bin/env python3
"""Prints a results file of run.sh as Markdown tables (what RESULTS.md shows)."""
import json
import sys

NAMES = {
    "renox": "Renox",
    "axum": "Axum (bare)",
    "laravel-fpm": "Laravel (PHP-FPM + nginx)",
    "laravel-octane": "Laravel Octane (FrankenPHP)",
}
ENDPOINTS = ["plaintext", "json", "db", "page"]


def main(path):
    d = json.load(open(path))
    servers = d["servers"]
    s = d["settings"]
    print(f"Run {d['date']} on {d['machine']['cpu']} ({d['machine']['cores']} cores, Linux {d['machine']['kernel']}).")
    print(f"Each server on cores {s['server_cpus']} with {s['memory']} of memory; oha {s['oha']}, "
          f"{s['connections']} connections, {s['duration']} per endpoint after a {s['warmup']} warm-up.\n")

    print("**Requests per second** (higher is better)\n")
    print("| | " + " | ".join(f"`/{e}`" for e in ENDPOINTS) + " |")
    print("|---|" + "---:|" * len(ENDPOINTS))
    for key, v in servers.items():
        cells = [f"{v['endpoints'][e]['rps']:,.0f}" for e in ENDPOINTS]
        print(f"| {NAMES.get(key, key)} | " + " | ".join(cells) + " |")

    print("\n**Latency, p50 / p99 in ms** (lower is better)\n")
    print("| | " + " | ".join(f"`/{e}`" for e in ENDPOINTS) + " |")
    print("|---|" + "---:|" * len(ENDPOINTS))
    for key, v in servers.items():
        cells = [f"{v['endpoints'][e]['p50_ms']:.2f} / {v['endpoints'][e]['p99_ms']:.2f}" for e in ENDPOINTS]
        print(f"| {NAMES.get(key, key)} | " + " | ".join(cells) + " |")

    print("\n**Resources**\n")
    print("| | Cold start | Memory at rest | Peak memory under load | What you deploy |")
    print("|---|---:|---:|---:|---:|")
    for key, v in servers.items():
        peak = max(v["endpoints"][e]["peak_mib"] for e in ENDPOINTS)
        size = v["artifact_bytes"] / 1048576
        what = "one binary" if key in ("renox", "axum") else "app + vendor (plus PHP)"
        print(f"| {NAMES.get(key, key)} | {v['cold_start_ms']:,} ms | {v['idle_mib']:.0f} MiB | {peak:.0f} MiB | {size:.1f} MiB, {what} |")

    errors = {k: sum(v["endpoints"][e]["errors"] for e in ENDPOINTS) for k, v in servers.items()}
    if any(errors.values()):
        print("\nErrors (non-200 answers or failed requests): " + ", ".join(f"{NAMES.get(k, k)} {n}" for k, n in errors.items() if n))


if __name__ == "__main__":
    main(sys.argv[1])
