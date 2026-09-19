#!/usr/bin/env python3
"""Walk the whole image looking for APK/APEX entries matching a pattern."""
import sys
sys.path.insert(0, "/tmp/gsi")
from ext4ls import Ext4

fs = Ext4(sys.argv[1])
needle = sys.argv[2].lower()
matches = []


def walk(ino, path, depth=0):
    if depth > 7:
        return
    try:
        ents = fs.listdir(ino)
    except Exception:
        return
    for n, num, ft in ents:
        if n in (".", ".."):
            continue
        p = path + "/" + n
        if needle in n.lower():
            try:
                matches.append((p, fs.inode(num)["size"]))
            except Exception:
                matches.append((p, -1))
        if ft == 2:
            walk(num, p, depth + 1)


walk(2, "")
if matches:
    for p, s in matches:
        print(f"{s:>14,}  {p}")
else:
    print(f"(no match for {needle!r})")
