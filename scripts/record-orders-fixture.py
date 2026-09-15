#!/usr/bin/env python3
"""Record trimmed GET /v2/orders/item/{slug} responses as collector test fixtures.

Usage: scripts/record-orders-fixture.py arcane_energize axi_a1_relic ayatan_anasa_sculpture
"""
import json
import pathlib
import sys
import time
import urllib.request

out = pathlib.Path(__file__).resolve().parent.parent / "crates/qf_core/tests/fixtures"
for slug in sys.argv[1:]:
    request = urllib.request.Request(
        f"https://api.warframe.market/v2/orders/item/{slug}",
        headers={"Platform": "pc", "Language": "en", "Crossplay": "true", "User-Agent": "quantframe-server-fixtures"},
    )
    body = json.load(urllib.request.urlopen(request))
    orders = body["data"]
    keep, seen = [], set()
    for order in orders:
        key = (order["type"], order.get("rank"), order.get("charges"), order.get("subtype"),
               order.get("amberStars"), order.get("cyanStars"))
        if key not in seen or len(keep) < 60:
            seen.add(key)
            keep.append(order)
    # Anonymise other players: stable per-file ids, no names, slugs or avatars.
    aliases: dict[str, str] = {}
    for order in keep:
        user = order["user"]
        alias = aliases.setdefault(user["id"], f"user{len(aliases) + 1}")
        order["user"] = {"id": alias, "ingameName": alias, "status": user.get("status", "offline")}
    body["data"] = keep
    (out / f"orders_{slug}.json").write_text(json.dumps(body, indent=1) + "\n")
    print(f"{slug}: {len(orders)} orders, kept {len(keep)}")
    time.sleep(1)
