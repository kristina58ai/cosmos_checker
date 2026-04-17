#!/usr/bin/env python3
"""Создать минимальные валидные плейсхолдер-иконки для Tauri bundle.

Продакшн-иконки позже генерируются через `cargo tauri icon source.png`.
Этот скрипт нужен только для того, чтобы `cargo tauri build` не падал
на этапе компиляции Windows-ресурсов.

Использование:
    python scripts/gen_placeholder_icons.py
"""
from __future__ import annotations

import struct
import zlib
from pathlib import Path

ICONS_DIR = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"


def make_png(size: int) -> bytes:
    """Сгенерировать валидный квадратный PNG size×size (полупрозрачный синий)."""
    # R=30, G=58, B=138 (slate-800), A=255
    pixel = bytes((30, 58, 138, 255))
    scanline = b"\x00" + pixel * size  # filter type 0 + row
    raw = scanline * size
    compressed = zlib.compress(raw, 9)

    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data)
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        sig
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", compressed)
        + chunk(b"IEND", b"")
    )


def make_ico(size: int) -> bytes:
    """Сгенерировать ICO-файл с одним 32-bit BMP-слоем."""
    pixel = bytes((138, 58, 30, 255))  # BGRA
    # BMP внутри ICO: bitmap stored bottom-up, height = 2×size (image + mask).
    pixels = pixel * (size * size)
    # AND mask: size × size бит, выравнивание каждой строки до 4 байт.
    row_bytes = ((size + 31) // 32) * 4
    mask = b"\x00" * (row_bytes * size)

    image_size = 40 + len(pixels) + len(mask)

    # ICONDIR (6 bytes)
    icondir = struct.pack("<HHH", 0, 1, 1)
    # ICONDIRENTRY (16 bytes)
    w = 0 if size == 256 else size
    h = 0 if size == 256 else size
    entry = struct.pack("<BBBBHHLL", w, h, 0, 0, 1, 32, image_size, 22)
    # BITMAPINFOHEADER (40 bytes)
    bih = struct.pack(
        "<LllHHLLllLL",
        40,
        size,
        size * 2,  # height doubled (image + mask)
        1,
        32,
        0,
        len(pixels) + len(mask),
        0,
        0,
        0,
        0,
    )
    return icondir + entry + bih + pixels + mask


def main() -> None:
    ICONS_DIR.mkdir(parents=True, exist_ok=True)

    outputs = {
        "32x32.png": make_png(32),
        "128x128.png": make_png(128),
        "128x128@2x.png": make_png(256),
        "icon.png": make_png(512),
        "icon.ico": make_ico(32),
    }
    for name, data in outputs.items():
        path = ICONS_DIR / name
        path.write_bytes(data)
        print(f"  wrote {path.relative_to(ICONS_DIR.parent.parent)}  ({len(data)} bytes)")

    # macOS icns — опционально. tauri.conf.json ссылается на него, но для
    # Windows-сборки он не нужен; Tauri будет ругаться только если target=dmg/app.
    # Создаём stub-файл, чтобы CI с multi-target bundle не падал.
    icns_stub = ICONS_DIR / "icon.icns"
    if not icns_stub.exists():
        icns_stub.write_bytes(b"icns\x00\x00\x00\x08")  # минимальный ICNS header
        print(f"  wrote {icns_stub.name}  (stub)")


if __name__ == "__main__":
    main()
