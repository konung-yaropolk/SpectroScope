#!/usr/bin/env python3
"""Decode an APK's binary AndroidManifest.xml, and optionally assert it launches.

    python scripts/apk_manifest.py target/release/apk/SpectroScope.apk
    python scripts/apk_manifest.py <apk> --require-launcher --require-icon

`--require-launcher` exits 1 unless the manifest has an `<activity>` carrying
an intent filter with both `android.intent.action.MAIN` and
`android.intent.category.LAUNCHER`, and an `android.app.lib_name` meta-data
entry. `--require-icon` additionally insists on `android:icon` on the
application.

This exists because cargo-apk2 treats a missing activity as a warning rather
than an error: with no `[[package.metadata.android.application.activity]]` in
Cargo.toml it emits an `<application>` with no children, and the APK installs,
shows no launcher icon and cannot be started. Release v0.1.0 shipped one.
Nothing in the build fails on that, so the release workflow reads the manifest
back out of the finished APK instead of trusting it.

Standard library only, so it runs on any runner with no `pip install` and no
Android SDK -- `aapt2 dump badging` would need the latter.
"""

from __future__ import annotations

import argparse
import struct
import sys
import zipfile

RES_STRING_POOL = 0x0001
RES_XML_START_ELEMENT = 0x0102
RES_XML_END_ELEMENT = 0x0103

TYPE_REFERENCE = 0x01
TYPE_STRING = 0x03
TYPE_FLOAT = 0x04
TYPE_INT_HEX = 0x11
TYPE_INT_BOOLEAN = 0x12

UTF8_FLAG = 1 << 8


def _string_pool(data: bytes, off: int) -> list[str]:
    """The chunk's strings, in index order."""
    header_size = struct.unpack_from("<H", data, off + 2)[0]
    count, _styles, flags, strings_start, _ = struct.unpack_from("<IIIII", data, off + 8)
    utf8 = bool(flags & UTF8_FLAG)
    offsets = struct.unpack_from(f"<{count}I", data, off + header_size)
    base = off + strings_start
    out: list[str] = []
    for rel in offsets:
        p = base + rel
        if utf8:
            # Two lengths -- characters then bytes -- each one or two bytes,
            # the high bit of the first marking the wide form.
            p += 2 if data[p] & 0x80 else 1
            n = data[p]
            if n & 0x80:
                n, p = ((n & 0x7F) << 8) | data[p + 1], p + 2
            else:
                p += 1
            out.append(data[p : p + n].decode("utf-8", "replace"))
        else:
            n = struct.unpack_from("<H", data, p)[0]
            if n & 0x8000:
                n = ((n & 0x7FFF) << 16) | struct.unpack_from("<H", data, p + 2)[0]
                p += 4
            else:
                p += 2
            out.append(data[p : p + n * 2].decode("utf-16-le", "replace"))
    return out


def _value(vtype: int, vdata: int, strings: list[str]) -> str:
    if vtype == TYPE_STRING:
        return strings[vdata] if vdata < len(strings) else f"<str {vdata}>"
    if vtype == TYPE_INT_BOOLEAN:
        return "true" if vdata else "false"
    if vtype == TYPE_REFERENCE:
        return f"@0x{vdata:08x}"
    if vtype == TYPE_INT_HEX:
        return f"0x{vdata:x}"
    if vtype == TYPE_FLOAT:
        return str(struct.unpack("<f", struct.pack("<I", vdata))[0])
    return str(vdata if vdata < 0x80000000 else vdata - 0x100000000)


class Element:
    __slots__ = ("tag", "attrs", "children", "parent")

    def __init__(self, tag: str, attrs: dict[str, str], parent=None):
        self.tag = tag
        self.attrs = attrs
        self.children: list[Element] = []
        self.parent = parent

    def find_all(self, tag: str):
        for c in self.children:
            if c.tag == tag:
                yield c
            yield from c.find_all(tag)

    def dump(self, depth: int = 0) -> str:
        pad = "  " * depth
        a = "".join(f' {k}="{v}"' for k, v in self.attrs.items())
        if not self.children:
            return f"{pad}<{self.tag}{a} />"
        inner = "\n".join(c.dump(depth + 1) for c in self.children)
        return f"{pad}<{self.tag}{a}>\n{inner}\n{pad}</{self.tag}>"


def parse(data: bytes) -> Element:
    total = struct.unpack_from("<I", data, 4)[0]
    off, strings = 8, []
    root = Element("", {})
    cur = root
    while off < min(total, len(data)):
        ctype, hsize, csize = struct.unpack_from("<HHI", data, off)
        if csize == 0:
            break
        if ctype == RES_STRING_POOL:
            strings = _string_pool(data, off)
        elif ctype == RES_XML_START_ELEMENT:
            p = off + hsize
            name = struct.unpack_from("<I", data, p + 4)[0]
            attr_start, _size, attr_count = struct.unpack_from("<HHH", data, p + 8)
            attrs: dict[str, str] = {}
            ap = p + attr_start
            for _ in range(attr_count):
                a_ns, a_name, a_raw, a_typed, a_val = struct.unpack_from("<iiiII", data, ap)
                key = strings[a_name] if 0 <= a_name < len(strings) else f"0x{a_name:x}"
                if not key:
                    key = f"0x{a_name:x}"
                if a_ns >= 0:
                    key = "android:" + key
                a_type = a_typed >> 24
                if a_raw >= 0 and a_type == TYPE_STRING:
                    attrs[key] = strings[a_raw]
                else:
                    attrs[key] = _value(a_type, a_val, strings)
                ap += 20
            node = Element(strings[name], attrs, cur)
            cur.children.append(node)
            cur = node
        elif ctype == RES_XML_END_ELEMENT:
            cur = cur.parent or root
        off += csize
    if not root.children:
        raise SystemExit("AndroidManifest.xml decoded to nothing -- not an AXML file?")
    return root.children[0]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("apk")
    ap.add_argument("--require-launcher", action="store_true")
    ap.add_argument("--require-icon", action="store_true")
    args = ap.parse_args()

    with zipfile.ZipFile(args.apk) as z:
        manifest = parse(z.read("AndroidManifest.xml"))
    print(manifest.dump())
    print()

    problems: list[str] = []

    if args.require_launcher:
        launchable = []
        for activity in manifest.find_all("activity"):
            filters = list(activity.find_all("intent-filter"))
            main = any(
                any(
                    a.attrs.get("android:name") == "android.intent.action.MAIN"
                    for a in f.find_all("action")
                )
                and any(
                    c.attrs.get("android:name") == "android.intent.category.LAUNCHER"
                    for c in f.find_all("category")
                )
                for f in filters
            )
            if main:
                launchable.append(activity)

        if not launchable:
            problems.append(
                "no <activity> with a MAIN/LAUNCHER intent filter: the APK would "
                "install, show no launcher icon and refuse to start"
            )
        else:
            a = launchable[0]
            if not a.attrs.get("android:name"):
                problems.append("the launcher activity has an empty android:name")
            lib = [
                m
                for m in a.find_all("meta-data")
                if m.attrs.get("android:name") == "android.app.lib_name"
            ]
            if not lib:
                problems.append(
                    "the launcher activity has no android.app.lib_name meta-data, so "
                    "NativeActivity would not know which .so to load"
                )
            elif not lib[0].attrs.get("android:value"):
                problems.append("android.app.lib_name has no value")

    if args.require_icon:
        app = next(manifest.find_all("application"), None)
        if app is None or not app.attrs.get("android:icon"):
            problems.append("<application> declares no android:icon")

    for p in problems:
        print(f"::error::{args.apk}: {p}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
