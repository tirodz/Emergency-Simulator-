#!/usr/bin/env python3
"""Minimal binary AndroidManifest.xml (AXML) extractor.

We only need enough to confirm which <protected-broadcast> entries the
framework declares. Rather than a full parser, walk the chunk tree and
collect string pools plus start/end element names in order.
"""
import struct
import sys
import zipfile

RES_STRING_POOL = 0x0001
RES_XML_START_ELEMENT = 0x0102
RES_XML_END_ELEMENT = 0x0103
UTF8_FLAG = 1 << 8


def parse_string_pool(data, off):
    _type, header_size, size = struct.unpack_from("<HHI", data, off)
    string_count, _style_count, flags, strings_start, _styles_start = struct.unpack_from(
        "<IIIII", data, off + 8)
    is_utf8 = bool(flags & 0x100)
    offsets = struct.unpack_from(f"<{string_count}I", data, off + header_size)
    base = off + strings_start
    out = []
    for o in offsets:
        p = base + o
        if is_utf8:
            # two lengths, each 1 or 2 bytes
            n1 = data[p]
            if n1 & 0x80:
                n1 = ((n1 & 0x7F) << 8) | data[p + 1]
                p += 2
            else:
                p += 1
            n2 = data[p]
            if n2 & 0x80:
                n2 = ((n2 & 0x7F) << 8) | data[p + 1]
                p += 2
            else:
                p += 1
            out.append(data[p:p + n2].decode("utf-8", "replace"))
        else:
            n = struct.unpack_from("<H", data, p)[0]
            p += 2
            raw = data[p:p + n * 2]
            out.append(raw.decode("utf-16-le", "replace").split("\x00")[0])
    return out


def main(path):
    if path.endswith(".apk"):
        with zipfile.ZipFile(path) as z:
            data = z.read("AndroidManifest.xml")
    else:
        data = open(path, "rb").read()

    off = 8  # skip file header
    strings = []
    elements = []
    while off + 8 <= len(data):
        ctype, header_size, size = struct.unpack_from("<HHI", data, off)
        if size == 0:
            break
        if ctype == RES_STRING_POOL:
            strings = parse_string_pool(data, off)
        elif ctype == RES_XML_START_ELEMENT:
            # ResXMLTree_node: header(8) + lineNumber(4) + comment(4)
            # then ResXMLTree_attrExt
            ns_idx, name_idx = struct.unpack_from("<iI", data, off + 16)
            elements.append(("start", strings[name_idx] if name_idx < len(strings) else f"#{name_idx}"))
        elif ctype == RES_XML_END_ELEMENT:
            ns_idx, name_idx = struct.unpack_from("<iI", data, off + 16)
            elements.append(("end", strings[name_idx] if name_idx < len(strings) else f"#{name_idx}"))
        off += size

    # Report protected-broadcast entries and receiver declarations
    cur_tag = None
    print(f"total elements: {len(elements)}")
    for kind, name in elements:
        if kind == "start":
            cur_tag = name
            if name in ("protected-broadcast", "receiver", "activity", "service", "provider"):
                print(f"  <{name}>")
                if name == "protected-broadcast":
                    # the immediately following strings are attributes
                    idx = elements.index((kind, name))
                    print("       ...", elements[idx + 1:idx + 4])
        else:
            cur_tag = None
    _ = cur_tag


if __name__ == "__main__":
    main(sys.argv[1])
