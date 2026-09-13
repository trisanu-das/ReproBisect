from __future__ import annotations

import io
import struct
import tarfile
import zipfile
from pathlib import Path

BUILD = Path("build")
BUILD.mkdir(exist_ok=True)


def zip_member(archive: zipfile.ZipFile, name: str, data: bytes) -> None:
    info = zipfile.ZipInfo(name, date_time=(2000, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_STORED
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    archive.writestr(info, data)


with zipfile.ZipFile(BUILD / "demo.jar", "w") as archive:
    zip_member(archive, "META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n")
    zip_member(archive, "demo/Main.class", b"\xca\xfe\xba\xbe\x00\x00\x00\x34")

with zipfile.ZipFile(BUILD / "demo-1.0-py3-none-any.whl", "w") as archive:
    zip_member(archive, "demo/__init__.py", b"VALUE = 1\n")
    zip_member(archive, "demo-1.0.dist-info/WHEEL", b"Wheel-Version: 1.0\n")
    zip_member(archive, "demo-1.0.dist-info/METADATA", b"Metadata-Version: 2.1\nName: demo\nVersion: 1.0\n")
    zip_member(archive, "demo-1.0.dist-info/RECORD", b"")


def ar_member(name: str, data: bytes) -> bytes:
    if not name.endswith("/"):
        name += "/"
    header = (
        f"{name:<16}{946684800:<12}{0:<6}{0:<6}{0o100644:<8o}{len(data):<10}`\n"
    ).encode("ascii")
    assert len(header) == 60
    result = bytearray(header)
    result.extend(data)
    if len(data) % 2:
        result.extend(b"\n")
    return bytes(result)


deb = bytearray(b"!<arch>\n")
deb.extend(ar_member("debian-binary", b"2.0\n"))
deb.extend(ar_member("control.tar", b"control"))
deb.extend(ar_member("data.tar", b"data"))
(BUILD / "demo.deb").write_bytes(deb)


def tar_bytes(name: str, data: bytes) -> tarfile.TarInfo:
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mtime = 946684800
    info.uid = 0
    info.gid = 0
    info.mode = 0o644
    return info

with tarfile.open(BUILD / "demo-oci.tar", "w", format=tarfile.USTAR_FORMAT) as archive:
    for name, data in [
        ("oci-layout", b'{"imageLayoutVersion":"1.0.0"}\n'),
        ("index.json", b'{"schemaVersion":2,"manifests":[]}\n'),
        ("blobs/sha256/abc", b"blob"),
    ]:
        archive.addfile(tar_bytes(name, data), io.BytesIO(data))

pe = bytearray(128)
pe[:2] = b"MZ"
pe[0x3C:0x40] = struct.pack("<I", 64)
pe[64:68] = b"PE\x00\x00"
(BUILD / "demo.exe").write_bytes(pe)

(BUILD / "demo.macho").write_bytes(b"\xcf\xfa\xed\xfe" + b"\x00" * 28)
(BUILD / "demo.wasm").write_bytes(b"\x00asm\x01\x00\x00\x00")
