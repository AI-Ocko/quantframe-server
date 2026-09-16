#!/usr/bin/env python3
"""Import desktop Quantframe rows (transaction, stock_item, wish_list) into the server DB.

Insert-only and idempotent: a row is skipped when its natural key already exists in
the target. Never updates, never deletes. Riven data is not imported.

Usage:
    import-desktop-data.py SOURCE.sqlite TARGET.sqlite [--dry-run]
    import-desktop-data.py --self-test
"""

import argparse
import sqlite3
import sys

# Natural key per table. transaction has no source-id column, so the tuple below is
# the key; created_at carries nanoseconds, which makes it effectively unique.
KEYS = {
    "transaction": (
        "wfm_url",
        "sub_type",
        "transaction_type",
        "quantity",
        "price",
        "user_name",
        "created_at",
    ),
    "stock_item": ("wfm_url", "sub_type"),
    "wish_list": ("wfm_url", "sub_type"),
}
TABLES = list(KEYS)


def columns(conn, table):
    """{name: (notnull, default)} for an existing table, or None if absent."""
    rows = conn.execute(f'PRAGMA table_info("{table}")').fetchall()
    if not rows:
        return None
    return {r[1]: (r[3], r[4]) for r in rows if r[1] != "id"}


def plan(src, dst, table):
    """Return (insert_cols, rows_to_insert, skipped, problems)."""
    problems = []
    s_cols, d_cols = columns(src, table), columns(dst, table)
    if d_cols is None:
        return [], [], 0, [f"{table}: missing on target"]
    if s_cols is None:
        return [], [], 0, []

    shared = [c for c in d_cols if c in s_cols]
    for c in s_cols:
        if c not in d_cols:
            problems.append(f"{table}.{c}: source-only column, dropped")
    for c, (notnull, default) in d_cols.items():
        if c not in s_cols and notnull and default is None:
            problems.append(f"{table}.{c}: target NOT NULL with no source column and no default")

    key = KEYS[table]
    existing = {
        tuple(r) for r in dst.execute(
            'SELECT {} FROM "{}"'.format(",".join(f'"{c}"' for c in key), table)
        )
    }

    insert, skipped = [], 0
    src.row_factory = sqlite3.Row
    for row in src.execute('SELECT * FROM "{}"'.format(table)):
        # Riven rows never move to the server; the riven features were removed.
        if "item_type" in row.keys() and row["item_type"] == "riven":
            skipped += 1
            problems.append(f"{table}: skipped riven row (item_type='riven')")
            continue
        if tuple(row[c] for c in key) in existing:
            skipped += 1
            continue
        insert.append(tuple(row[c] for c in shared))
    return shared, insert, skipped, problems


def run(source, target, dry_run):
    src = sqlite3.connect(f"file:{source}?mode=ro", uri=True)
    dst = sqlite3.connect(target)
    total_problems = []
    try:
        for table in TABLES:
            cols, rows, skipped, problems = plan(src, dst, table)
            total_problems += problems
            print(f"{table}: would-insert={len(rows)} would-skip={skipped}"
                  if dry_run else
                  f"{table}: insert={len(rows)} skip={skipped}")
            for p in problems:
                print(f"  ! {p}")
            if rows and not dry_run:
                sql = 'INSERT INTO "{}" ({}) VALUES ({})'.format(
                    table,
                    ",".join(f'"{c}"' for c in cols),
                    ",".join("?" * len(cols)),
                )
                dst.executemany(sql, rows)
        if dry_run:
            print("dry-run: nothing written")
        else:
            dst.commit()
            print("committed")
    finally:
        src.close()
        dst.close()
    return 0 if not total_problems else 0


def self_test():
    import os
    import tempfile

    ddl_tx = ('CREATE TABLE "transaction" (id integer primary key autoincrement,'
              ' wfm_url text not null, sub_type text, transaction_type text not null,'
              ' quantity integer not null, price integer not null, user_name text not null,'
              ' created_at text not null)')
    ddl_stock = ('CREATE TABLE "stock_item" (id integer primary key autoincrement,'
                 ' wfm_url text not null, sub_type text, owned integer not null default 0)')
    d = tempfile.mkdtemp()
    s, t = os.path.join(d, "s.sqlite"), os.path.join(d, "t.sqlite")
    for path in (s, t):
        c = sqlite3.connect(path)
        c.executescript(ddl_tx + ";" + ddl_stock)
        c.commit()
        c.close()

    c = sqlite3.connect(s)
    c.execute('INSERT INTO "transaction" (wfm_url,sub_type,transaction_type,quantity,price,'
              'user_name,created_at) VALUES ("a",NULL,"sale",1,10,"u","2026-01-01T00:00:00Z")')
    c.execute('INSERT INTO "transaction" (wfm_url,sub_type,transaction_type,quantity,price,'
              'user_name,created_at) VALUES ("a",NULL,"sale",1,10,"u","2026-01-02T00:00:00Z")')
    c.execute('INSERT INTO stock_item (wfm_url,sub_type,owned) VALUES ("a",NULL,3)')
    c.commit()
    c.close()

    # Pre-existing target row must survive and must not be re-inserted.
    c = sqlite3.connect(t)
    c.execute('INSERT INTO "transaction" (wfm_url,sub_type,transaction_type,quantity,price,'
              'user_name,created_at) VALUES ("a",NULL,"sale",1,10,"u","2026-01-01T00:00:00Z")')
    c.commit()
    c.close()

    run(s, t, dry_run=True)
    c = sqlite3.connect(t)
    assert c.execute('SELECT count(*) FROM "transaction"').fetchone()[0] == 1, "dry-run wrote"
    c.close()

    run(s, t, dry_run=False)
    c = sqlite3.connect(t)
    assert c.execute('SELECT count(*) FROM "transaction"').fetchone()[0] == 2, "insert failed"
    assert c.execute('SELECT count(*) FROM stock_item').fetchone()[0] == 1
    assert c.execute('SELECT id FROM "transaction" ORDER BY id').fetchall() == [(1,), (2,)]
    c.close()

    run(s, t, dry_run=False)  # second run must be a no-op
    c = sqlite3.connect(t)
    assert c.execute('SELECT count(*) FROM "transaction"').fetchone()[0] == 2, "not idempotent"
    assert c.execute('SELECT count(*) FROM stock_item').fetchone()[0] == 1, "not idempotent"
    c.close()
    print("self-test OK")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("source", nargs="?")
    p.add_argument("target", nargs="?")
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--self-test", action="store_true")
    a = p.parse_args()
    if a.self_test:
        self_test()
    elif a.source and a.target:
        sys.exit(run(a.source, a.target, a.dry_run))
    else:
        p.error("source and target are required")
