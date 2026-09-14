#!/usr/bin/env python3
"""Fail if the web app invokes any command the server does not expose."""
import pathlib
import re
import sys

root = pathlib.Path(__file__).resolve().parent.parent
table = (root / "crates/qf_core/src/commands/rpc.rs").read_text()
server = set(re.findall(r"^\s*(\w+)\s*=>\s*\w+::\w+", table, re.M))

pattern = re.compile(r"""(?:sendInvoke|invoke)(?:<.*?>)?\(\s*["'](\w+)["']""")
used: dict[str, list[str]] = {}
for path in (root / "web/src").rglob("*.ts*"):
    for name in pattern.findall(path.read_text()):
        used.setdefault(name, []).append(str(path.relative_to(root)))

missing = {name: files for name, files in used.items() if name not in server}
for name, files in sorted(missing.items()):
    print(f"{name}: {', '.join(sorted(set(files)))}")
print(f"{len(server)} server commands, {len(used)} used by web, {len(missing)} missing")
sys.exit(1 if missing else 0)
