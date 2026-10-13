#!/usr/bin/env python3
"""Makes data/bench.db: one table `items` with 10,000 rows, the database every
benchmark app reads (the same file, copied in). Deterministic: the same rows
every time."""
import os, random, sqlite3, sys

path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "data", "bench.db")
os.makedirs(os.path.dirname(path), exist_ok=True)
if os.path.exists(path):
    os.remove(path)
db = sqlite3.connect(path)
db.execute("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price INTEGER NOT NULL, stock INTEGER NOT NULL)")
rng = random.Random(42)
words = ["Road", "Trail", "City", "Gravel", "Folding", "Cargo", "Kids", "Touring", "Sport", "Comp", "Pro", "Elite"]
db.executemany(
    "INSERT INTO items (id, name, price, stock) VALUES (?, ?, ?, ?)",
    [(i, f"{rng.choice(words)} {rng.choice(words)} {i}", rng.randint(100, 90_000) * 1000, rng.randint(0, 40)) for i in range(1, 10_001)],
)
db.commit()
db.execute("VACUUM")
db.close()
print(path)
