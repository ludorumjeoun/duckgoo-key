#!/usr/bin/env python3
"""Normalize a source image into DuckGooKey's macOS icon assets."""

from __future__ import annotations

import argparse
import io
import os
import struct
import subprocess
import tempfile
from collections import deque
from pathlib import Path

from PIL import Image


CANVAS_SIZE = 1024
ICONSET_SIZES = {
    "icon_16x16.png": 16,
    "icon_16x16@2x.png": 32,
    "icon_32x32.png": 32,
    "icon_32x32@2x.png": 64,
    "icon_128x128.png": 128,
    "icon_128x128@2x.png": 256,
    "icon_256x256.png": 256,
    "icon_256x256@2x.png": 512,
    "icon_512x512.png": 512,
    "icon_512x512@2x.png": 1024,
}
ICNS_ELEMENT_TYPES = {
    "icon_16x16.png": b"icp4",
    "icon_16x16@2x.png": b"ic11",
    "icon_32x32.png": b"icp5",
    "icon_32x32@2x.png": b"ic12",
    "icon_128x128.png": b"ic07",
    "icon_128x128@2x.png": b"ic13",
    "icon_256x256.png": b"ic08",
    "icon_256x256@2x.png": b"ic14",
    "icon_512x512.png": b"ic09",
    "icon_512x512@2x.png": b"ic10",
}


def parse_args() -> argparse.Namespace:
    project_root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(
        description=(
            "Preserve the supplied artwork, remove only edge-connected near-black "
            "canvas pixels, and build PNG/ICNS app icon assets."
        )
    )
    parser.add_argument("source", type=Path, help="Source PNG or other Pillow-readable image")
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=project_root / "assets" / "icons",
        help="Destination directory (default: assets/icons)",
    )
    parser.add_argument(
        "--background-threshold",
        type=int,
        default=12,
        choices=range(0, 65),
        metavar="0..64",
        help="Maximum RGB channel value considered connected black canvas",
    )
    return parser.parse_args()


def is_near_black(pixel: tuple[int, int, int, int], threshold: int) -> bool:
    red, green, blue, alpha = pixel
    return alpha > 0 and max(red, green, blue) <= threshold


def clear_edge_connected_background(image: Image.Image, threshold: int) -> Image.Image:
    """Make only near-black pixels connected to the canvas edge transparent."""

    rgba = image.convert("RGBA")
    width, height = rgba.size
    pixels = rgba.load()
    visited = bytearray(width * height)
    pending: deque[tuple[int, int]] = deque()

    def enqueue(x: int, y: int) -> None:
        index = y * width + x
        if visited[index] or not is_near_black(pixels[x, y], threshold):
            return
        visited[index] = 1
        pending.append((x, y))

    for x in range(width):
        enqueue(x, 0)
        enqueue(x, height - 1)
    for y in range(1, height - 1):
        enqueue(0, y)
        enqueue(width - 1, y)

    while pending:
        x, y = pending.popleft()
        if x > 0:
            enqueue(x - 1, y)
        if x + 1 < width:
            enqueue(x + 1, y)
        if y > 0:
            enqueue(x, y - 1)
        if y + 1 < height:
            enqueue(x, y + 1)

    alpha = bytearray(rgba.getchannel("A").tobytes())
    for index, is_background in enumerate(visited):
        if is_background:
            alpha[index] = 0
    rgba.putalpha(Image.frombytes("L", rgba.size, bytes(alpha)))
    return rgba


def normalize_canvas(image: Image.Image) -> Image.Image:
    """Center the artwork on a square canvas and resize without distortion."""

    width, height = image.size
    side = max(width, height)
    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    canvas.alpha_composite(image, ((side - width) // 2, (side - height) // 2))
    if side == CANVAS_SIZE:
        return canvas
    return canvas.resize((CANVAS_SIZE, CANVAS_SIZE), Image.Resampling.LANCZOS)


def save_png_atomic(image: Image.Image, destination: Path) -> None:
    temporary = destination.with_name(f".{destination.stem}.tmp.png")
    image.save(temporary, format="PNG", optimize=True)
    os.replace(temporary, destination)


def build_icns(image: Image.Image, destination: Path) -> None:
    iconutil = Path("/usr/bin/iconutil")
    if not iconutil.is_file():
        raise RuntimeError(
            "macOS /usr/bin/iconutil is required to validate DuckGooKey.icns"
        )
    if ICONSET_SIZES.keys() != ICNS_ELEMENT_TYPES.keys():
        raise RuntimeError("ICNS element mapping does not match the iconset sizes")

    elements = []
    for name, size in ICONSET_SIZES.items():
        resized = image.resize((size, size), Image.Resampling.LANCZOS)
        encoded = io.BytesIO()
        resized.save(encoded, format="PNG", optimize=True)
        png = encoded.getvalue()
        elements.append(
            ICNS_ELEMENT_TYPES[name] + struct.pack(">I", len(png) + 8) + png
        )

    payload = b"".join(elements)
    family = b"icns" + struct.pack(">I", len(payload) + 8) + payload
    temporary = destination.with_name(f".{destination.stem}.tmp.icns")
    try:
        temporary.write_bytes(family)
        validate_icns_with_iconutil(temporary, iconutil)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def validate_icns_with_iconutil(icns: Path, iconutil: Path) -> None:
    """Have macOS decode the generated family and verify every representation."""

    with tempfile.TemporaryDirectory(prefix="duckgookey-icns-") as directory:
        iconset = Path(directory) / "DuckGooKey.iconset"
        result = subprocess.run(
            [
                str(iconutil),
                "-c",
                "iconset",
                "-o",
                str(iconset),
                str(icns),
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            details = result.stderr.strip() or result.stdout.strip()
            raise RuntimeError(f"macOS rejected generated ICNS: {details}")

        for name, expected_size in ICONSET_SIZES.items():
            extracted = iconset / name
            if not extracted.is_file():
                raise RuntimeError(
                    f"macOS ICNS validation did not extract {name}"
                )
            with Image.open(extracted) as representation:
                if representation.size != (expected_size, expected_size):
                    raise RuntimeError(
                        f"macOS extracted {name} at {representation.size}; "
                        f"expected {expected_size}x{expected_size}"
                    )
                representation.verify()


def validate(image: Image.Image) -> None:
    if image.size != (CANVAS_SIZE, CANVAS_SIZE):
        raise RuntimeError(f"normalized icon must be {CANVAS_SIZE}x{CANVAS_SIZE}")
    alpha_minimum, alpha_maximum = image.getchannel("A").getextrema()
    if alpha_minimum != 0 or alpha_maximum != 255:
        raise RuntimeError("normalized icon must contain transparent and opaque pixels")
    if image.getbbox() is None:
        raise RuntimeError("normalized icon contains no visible artwork")


def main() -> None:
    args = parse_args()
    if not args.source.is_file():
        raise SystemExit(f"source image does not exist: {args.source}")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    with Image.open(args.source) as source:
        normalized = normalize_canvas(
            clear_edge_connected_background(source, args.background_threshold)
        )
    validate(normalized)

    source_output = args.output_dir / "duckgoo-key.png"
    preview_output = args.output_dir / "duckgoo-key-128.png"
    icns_output = args.output_dir / "DuckGooKey.icns"
    save_png_atomic(normalized, source_output)
    save_png_atomic(
        normalized.resize((128, 128), Image.Resampling.LANCZOS),
        preview_output,
    )
    build_icns(normalized, icns_output)

    print(f"Normalized PNG: {source_output}")
    print(f"In-app PNG:    {preview_output}")
    print(f"macOS ICNS:    {icns_output}")


if __name__ == "__main__":
    main()
