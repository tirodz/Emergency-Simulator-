#!/usr/bin/env python3
"""Minimal read-only ext4 walker.

Written because this environment has no root, no loop devices and no
debugfs/simg2img, but we still need to answer Experiment 3 empirically:
does an AOSP system image ship the Cell Broadcast components?

Reads the filesystem directly out of the image file using only the
superblock, group descriptors, inode table and directory entries.
"""
import struct
import sys

BLOCK = 4096
INCOMPAT_64BIT = 0x80
INCOMPAT_FILETYPE = 0x2
FEATURE_INCOMPAT_OFFSET = 0x60
FEATURE_INCOMPAT_SIZE = 4
S_IFMT = 0o170000
S_IFDIR = 0o040000
S_IFLNK = 0o120000
DIR_ENTRY_STRUCT = struct.Struct("<IHBB")


class Ext4:
    def __init__(self, path):
        self.f = open(path, "rb")
        self.block_size = BLOCK
        self._read_super()

    def _read_super(self):
        self.f.seek(1024)
        sb = self.f.read(1024)
        assert struct.unpack_from("<H", sb, 0x38)[0] == 0xEF53, "not ext4"
        self.log_block_size = struct.unpack_from("<I", sb, 0x18)[0]
        self.block_size = 1024 << self.log_block_size
        self.blocks_count = struct.unpack_from("<I", sb, 0x04)[0]
        inodes_count = struct.unpack_from("<I", sb, 0x00)[0]
        inodes_per_group = struct.unpack_from("<I", sb, 0x28)[0]
        blocks_per_group = struct.unpack_from("<I", sb, 0x20)[0]
        first_ino = struct.unpack_from("<I", sb, 0x54)[0]
        inode_size = struct.unpack_from("<H", sb, 0x58)[0]
        feat_incompat = struct.unpack_from("<I", sb, FEATURE_INCOMPAT_OFFSET)[0]
        self.feat_incompat = feat_incompat
        if inode_size == 0:
            inode_size = 128
        self.inode_size = inode_size
        desc_size = 32
        if feat_incompat & INCOMPAT_64BIT:
            desc_size = struct.unpack_from("<H", sb, 0xFE)[0] or 32
        self.desc_size = desc_size
        groups = (self.blocks_count + blocks_per_group - 1) // blocks_per_group
        gd_start_block = 2 if self.block_size == 1024 else 1
        gd_off = gd_start_block * self.block_size
        self.f.seek(gd_off)
        gd = self.f.read(desc_size * groups)
        self.inode_tables = []
        for g in range(groups):
            base = g * desc_size
            blk = struct.unpack_from("<I", gd, base + 0x08)[0]
            if desc_size > 32:
                blk |= struct.unpack_from("<I", gd, base + 0x28)[0] << 32
            self.inode_tables.append(blk)
        self.inodes_per_group = inodes_per_group
        self.first_ino = first_ino
        _ = inodes_count

    def read_block(self, blk, count=1):
        self.f.seek(blk * self.block_size)
        return self.f.read(count * self.block_size)

    def inode(self, ino):
        group = (ino - 1) // self.inodes_per_group
        index = (ino - 1) % self.inodes_per_group
        tbl_blk = self.inode_tables[group]
        off = tbl_blk * self.block_size + index * self.inode_size
        self.f.seek(off)
        raw = self.f.read(self.inode_size)
        mode = struct.unpack_from("<H", raw, 0x00)[0]
        size_lo = struct.unpack_from("<I", raw, 0x04)[0]
        size_hi = struct.unpack_from("<I", raw, 0x6C)[0]
        size = size_lo | (size_hi << 32) if (self.feat_incompat & INCOMPAT_64BIT) else size_lo
        block = raw[0x28:0x28 + 60]
        return {"mode": mode, "size": size, "block": block}

    def _extents(self, inode):
        """Yield (start_block, num_blocks) for an extent-mapped inode."""
        raw = inode["block"]
        if struct.unpack_from("<H", raw, 0x00)[0] != 0xF30A:
            raise NotImplementedError("only extent-mapped files supported")
        depth = struct.unpack_from("<H", raw, 0x06)[0]
        entries = struct.unpack_from("<H", raw, 0x02)[0]
        nodes = [(raw, entries, depth)]
        out = []
        while nodes:
            node, n, d = nodes.pop()
            for i in range(n):
                if d == 0:
                    base = 0x0C + i * 12
                    start_hi = struct.unpack_from("<H", node, base + 0x06)[0]
                    start_lo = struct.unpack_from("<I", node, base + 0x08)[0]
                    ln = struct.unpack_from("<H", node, base + 0x04)[0]
                    out.append(((start_hi << 32) | start_lo, ln))
                else:
                    base = 0x0C + i * 12
                    blk = struct.unpack_from("<I", node, base + 0x04)[0]
                    child = self.read_block(blk)
                    cn = struct.unpack_from("<H", child, 0x02)[0]
                    cd = struct.unpack_from("<H", child, 0x06)[0]
                    nodes.append((child, cn, cd))
        return out

    def listdir(self, ino):
        node = self.inode(ino)
        data = self.read_file(ino)
        entries = []
        pos = 0
        while pos + 8 <= len(data):
            inode_num, rec_len, name_len, ftype = struct.unpack_from("<IHBB", data, pos)
            if rec_len == 0:
                break
            name = data[pos + 8:pos + 8 + name_len]
            if inode_num != 0:
                entries.append((name.decode("utf-8", "replace"), inode_num, ftype))
            pos += rec_len
        return entries

    def read_file(self, ino):
        node = self.inode(ino)
        size = node["size"]
        out = bytearray()
        for start, ln in self._extents(node):
            out += self.read_block(start, ln)
        return bytes(out[:size])

    def lookup(self, path):
        ino = 2  # root
        for part in path.strip("/").split("/"):
            if not part:
                continue
            found = None
            for name, num, _ft in self.listdir(ino):
                if name == part:
                    found = num
                    break
            if found is None:
                return None
            ino = found
        return ino


def main():
    fs = Ext4(sys.argv[1])
    for path in sys.argv[2:]:
        ino = fs.lookup(path)
        if ino is None:
            print(f"{path}: NOT FOUND")
            continue
        node = fs.inode(ino)
        kind = "dir" if (node["mode"] & S_IFMT) == S_IFDIR else "file"
        print(f"{path}: ino={ino} {kind} size={node['size']}")
        if kind == "dir":
            for name, num, ft in sorted(fs.listdir(ino)):
                print(f"    {name} (ino {num})")


if __name__ == "__main__":
    main()
