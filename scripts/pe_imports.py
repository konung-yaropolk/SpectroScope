#!/usr/bin/env python3
"""List (or assert on) the DLLs a Windows PE file imports.

    python scripts/pe_imports.py target/release/spectroscope.exe
    python scripts/pe_imports.py target/release/spectroscope.exe \
        --forbid VCRUNTIME --forbid api-ms-win-crt

With `--forbid`, any imported DLL whose name contains one of the given
substrings (case-insensitively) makes the script exit 1. That is how CI checks
that the release .exe still links the C runtime statically: VCRUNTIME140.dll
ships with the Visual C++ Redistributable rather than with Windows, and a
static import of a DLL the machine has not got stops the program *starting*,
with a dialog naming a file the user has never heard of.

Standard library only, and it reads the file rather than running it, so it
works on any host -- including the Linux runner, if a cross-built .exe ever
needs checking there.
"""

from __future__ import annotations

import argparse
import struct
import sys


def _section_table(data: bytes, opt_start: int, opt_size: int, count: int):
    """(virtual_size, virtual_address, raw_size, raw_address) per section."""
    out = []
    base = opt_start + opt_size
    for i in range(count):
        off = base + i * 40
        out.append(struct.unpack_from("<IIII", data, off + 8))
    return out


def _to_offset(sections, rva: int):
    """File offset for a relative virtual address, or None if unmapped."""
    for vsize, vaddr, rsize, raddr in sections:
        if vaddr <= rva < vaddr + max(vsize, rsize):
            return raddr + (rva - vaddr)
    return None


def _cstring(data: bytes, off: int) -> str:
    return data[off : data.index(b"\0", off)].decode("ascii", "replace")


def imported_dlls(path: str) -> list[str]:
    """Every DLL named in the PE's import and delay-import directories."""
    data = open(path, "rb").read()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe : pe + 4] != b"PE\0\0":
        raise SystemExit(f"{path}: not a PE file")

    section_count = struct.unpack_from("<H", data, pe + 6)[0]
    opt_size = struct.unpack_from("<H", data, pe + 20)[0]

    opt = pe + 24
    pe32plus = struct.unpack_from("<H", data, opt)[0] == 0x20B
    dirs = opt + (112 if pe32plus else 96)
    dir_count = struct.unpack_from("<I", data, opt + (108 if pe32plus else 92))[0]
    sections = _section_table(data, opt, opt_size, section_count)

    names: list[str] = []

    # Directory 1 is the import table: 20-byte descriptors, name RVA at +12,
    # terminated by an all-zero entry.
    if dir_count > 1:
        rva = struct.unpack_from("<I", data, dirs + 8)[0]
        off = _to_offset(sections, rva) if rva else None
        while off is not None:
            fields = struct.unpack_from("<IIIII", data, off)
            if not any(fields):
                break
            names.append(_cstring(data, _to_offset(sections, fields[3])))
            off += 20

    # Directory 13 is the delay-load table: 32-byte descriptors, name at +4.
    # Delay-loaded imports fail later than static ones but fail just as hard,
    # so they count for this check.
    if dir_count > 13:
        rva = struct.unpack_from("<I", data, dirs + 13 * 8)[0]
        off = _to_offset(sections, rva) if rva else None
        while off is not None:
            fields = struct.unpack_from("<IIIIIIII", data, off)
            if not any(fields):
                break
            names.append(_cstring(data, _to_offset(sections, fields[1])))
            off += 32

    return names


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("binary")
    ap.add_argument(
        "--forbid",
        action="append",
        default=[],
        metavar="SUBSTRING",
        help="fail if an imported DLL name contains this (case-insensitive)",
    )
    args = ap.parse_args()

    names = imported_dlls(args.binary)
    print(f"{args.binary} imports {len(names)} DLLs:")
    for name in sorted(names, key=str.lower):
        print(f"  {name}")

    # A --forbid check that found nothing is only good news if the parse
    # worked. Both directories can come back empty -- an unmapped RVA, a
    # rewritten header, a bug in the walk above -- and the result would be an
    # empty list and a cheerful exit 0, which is exactly the "the CRT is
    # static now" answer the caller is hoping for. Every Windows executable
    # imports KERNEL32, so its absence means the parse failed, not that the
    # binary is unusually self-contained.
    if not any(n.lower().startswith("kernel32") for n in names):
        print(
            f"::error::{args.binary}: no KERNEL32 import found, so the import "
            "table was not parsed -- the result below would be meaningless"
        )
        return 2

    bad = [
        name
        for name in names
        if any(f.lower() in name.lower() for f in args.forbid)
    ]
    if bad:
        print()
        for name in bad:
            print(f"::error::{args.binary} must not import {name}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
