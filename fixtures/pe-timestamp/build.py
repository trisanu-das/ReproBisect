from __future__ import annotations

import os
import struct
from pathlib import Path

# A minimal PE/COFF-shaped artifact whose COFF TimeDateStamp is intentionally
# sourced from SOURCE_DATE_EPOCH. ReproBisect should localize the byte delta to
# the semantic COFF timestamp field rather than merely reporting changed bytes.
epoch = int(os.environ["SOURCE_DATE_EPOCH"]) & 0xFFFFFFFF
image = bytearray(128)
image[:2] = b"MZ"
image[0x3C:0x40] = struct.pack("<I", 64)
image[64:68] = b"PE\0\0"
coff = 68
image[coff:coff + 2] = struct.pack("<H", 0x8664)  # AMD64
image[coff + 2:coff + 4] = struct.pack("<H", 0)    # sections
image[coff + 4:coff + 8] = struct.pack("<I", epoch)
image[coff + 16:coff + 18] = struct.pack("<H", 0)  # optional header size
image[coff + 18:coff + 20] = struct.pack("<H", 0x0002)
Path("out.exe").write_bytes(image)
