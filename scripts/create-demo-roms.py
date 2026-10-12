#!/usr/bin/env python3
"""Create non-playable, zero-filled ROM files for local RomM testing."""

import argparse
from pathlib import Path
import sys


# Representative cartridge sizes, not dumps or format-valid game images.
PLATFORMS = (
    ("gb", "gb", 32 * 1024),
    ("gbc", "gbc", 1024 * 1024),
    ("nes", "nes", 256 * 1024),
    ("snes", "sfc", 2 * 1024 * 1024),
    ("gba", "gba", 8 * 1024 * 1024),
    ("n64", "z64", 32 * 1024 * 1024),
)
ZERO_CHUNK = bytes(1024 * 1024)


def positive_int(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return number


def create_rom(path, size):
    """Create exclusively, or verify and reuse an identical existing file."""
    # Refuse symbolic links in the file path or its parent directories.
    if any(parent.is_symlink() for parent in (path, *path.parents)):
        raise ValueError(f"refusing symlink: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        output = path.open("xb")
    except FileExistsError:
        if not path.is_file() or path.stat().st_size != size:
            raise ValueError(f"existing path has a different type or size; preserved: {path}")
        with path.open("rb") as existing:
            while chunk := existing.read(len(ZERO_CHUNK)):
                if any(chunk):
                    raise ValueError(f"existing file contains nonzero data; preserved: {path}")
        return "Verified"
    with output:
        remaining = size
        while remaining:
            count = min(remaining, len(ZERO_CHUNK))
            output.write(ZERO_CHUNK[:count])
            remaining -= count
    return "Created"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[1] / "romm",
        help="library root; files go under ROOT/roms (default: repository/romm)",
    )
    parser.add_argument(
        "--games-per-platform", type=positive_int, default=3,
        help="number of files for each of six platforms (default: 3)",
    )
    parser.add_argument(
        "--size-mib", type=positive_int,
        help="override every file size in MiB; default: platform-specific sizes",
    )
    args = parser.parse_args()
    total = sum(
        (args.size_mib * 1024 * 1024 if args.size_mib else size)
        * args.games_per_platform
        for _, _, size in PLATFORMS
    )
    count = len(PLATFORMS) * args.games_per_platform
    print(f"Library: {args.root.absolute()} ({total:,} bytes across {count} files)")
    for platform, extension, default_size in PLATFORMS:
        size = args.size_mib * 1024 * 1024 if args.size_mib else default_size
        for number in range(1, args.games_per_platform + 1):
            filename = f"Demo {platform.upper()} {number:03d}.{extension}"
            path = args.root / "roms" / platform / filename
            action = create_rom(path, size)
            print(f"{action}: {path} ({size:,} bytes)")
    print("Ready. Scan the library in RomM. These files cannot run in an emulator.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        print(
            "Cancelled; completed files remain. Check the last file before rerunning.",
            file=sys.stderr,
        )
        sys.exit(130)
