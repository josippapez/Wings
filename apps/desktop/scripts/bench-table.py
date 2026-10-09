"""Prints the renderer benchmark reports (bench-renderers.sh, or the Chromium stub) as Markdown tables.

python3 scripts/bench-table.py <report.json>...
"""
import json
import sys


def fmt(v, unit=""):
    return "-" if v is None else f"{v:.0f}{unit}" if isinstance(v, float) and v >= 100 else f"{v}{unit}"


def mem(p, key):
    return None if not p else p.get(key)


rows = []
empty = []
for path in sys.argv[1:]:
    with open(path) as f:
        r = json.load(f)
    if "sidebarOpen" in r:
        empty.append(r)
        continue
    for run in r["runs"]:
        m = run.get("memory") or {}
        wc, wings, gpu = m.get("webContent"), m.get("wings"), m.get("gpu")
        cpu = run["paced"].get("cpuPercent") or {}
        dcpu = run["drain"].get("cpuPercent") or {}
        rows.append(
            [
                r["webview"],
                r["renderer"],
                f"{run['panes']}/{run['visible']}",
                fmt(mem(wc, "footprintMb"), " MB"),
                fmt(mem(wc, "ownedGraphicsMb"), " MB") + (f" (+{fmt(wc['ownedGraphicsSwappedMb'])} swapped, {wc['ownedGraphicsRegions']} regions)" if wc and wc.get("ownedGraphicsMb") is not None else ""),
                fmt(mem(gpu, "footprintMb"), " MB"),
                fmt(mem(wings, "footprintMb"), " MB"),
                f"{fmt(cpu.get('webContent'))} / {fmt(cpu.get('wings'))} / {fmt(cpu.get('gpu'))}",
                f"{run['paced']['frameMs']['p50']} / {run['paced']['frameMs']['p95']} / {run['paced']['frameMs']['max']}",
                f"{run['drain']['ms']} ms",
                f"{run['drain']['frameMs']['p95']} / {run['drain']['frameMs']['max']}",
                f"{fmt(dcpu.get('webContent'))} / {fmt(dcpu.get('wings'))}",
                f"{run['paced']['channelMbPerSec']} / {run['drain']['channelMbPerSec']}",
            ]
        )

head = [
    "webview", "renderer", "panes/shown", "WebContent footprint", "owned unmapped (graphics)", "GPU proc footprint",
    "Wings footprint", "replay CPU % WebContent / Wings / GPU", "replay frame ms p50 / p95 / max", "drain time",
    "drain frame ms p95 / max", "drain CPU % WebContent / Wings", "channel MB/s replay / drain",
]
if rows:
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for row in rows:
        print("| " + " | ".join(row) + " |")
for r in empty:
    print()
    for label, s in [("blank page", r["blankPage"]), ("UI, sidebar open", r["sidebarOpen"]["memory"]), ("UI, sidebar closed", r["sidebarClosed"]["memory"])]:
        wc = s.get("webContent") or {}
        print(f"- {label}: WebContent {fmt(wc.get('footprintMb'), ' MB')}, owned graphics {fmt(wc.get('ownedGraphicsMb'), ' MB')} "
              f"(+{fmt(wc.get('ownedGraphicsSwappedMb'))} swapped, {wc.get('ownedGraphicsRegions')} regions), "
              f"GPU process {fmt((s.get('gpu') or {}).get('footprintMb'), ' MB')}, Wings {fmt(s['wings'].get('footprintMb'), ' MB')}")
    print(f"- idle CPU with sidebar open: {r['sidebarOpen']['idleCpuPercent']}, frames {r['sidebarOpen']['frameMs']}")
