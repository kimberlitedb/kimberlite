#!/usr/bin/env python3
"""Build wheel with bundled FFI library.

This script:
1. Builds the kimberlite-ffi library in release mode
2. Copies the library to kimberlite/lib/
3. Builds the Python wheel with the bundled library
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path


def main():
    """Build wheel with bundled FFI library."""
    # Find project root
    script_dir = Path(__file__).parent
    project_root = script_dir.parent.parent

    print("Building kimberlite-ffi library...")

    # Build FFI library in release mode
    result = subprocess.run(
        ["cargo", "build", "-p", "kimberlite-ffi", "--release"],
        cwd=project_root,
        capture_output=True,
        text=True,
    )

    if result.returncode != 0:
        print(f"Error building FFI library:\n{result.stderr}", file=sys.stderr)
        return 1

    print("✓ FFI library built")

    # Determine library name based on platform
    if sys.platform == "darwin":
        lib_name = "libkimberlite_ffi.dylib"
    elif sys.platform == "win32":
        lib_name = "kimberlite_ffi.dll"
    else:  # Linux
        lib_name = "libkimberlite_ffi.so"

    # Copy library to package
    lib_src = project_root / "target" / "release" / lib_name
    lib_dest_dir = script_dir / "kimberlite" / "lib"
    lib_dest_dir.mkdir(parents=True, exist_ok=True)
    lib_dest = lib_dest_dir / lib_name

    if not lib_src.exists():
        print(f"Error: Library not found at {lib_src}", file=sys.stderr)
        return 1

    shutil.copy2(lib_src, lib_dest)
    print(f"✓ Copied {lib_name} to package")

    # Build wheel
    print("Building wheel...")
    result = subprocess.run(
        ["python3", "-m", "build"],
        cwd=script_dir,
        capture_output=True,
        text=True,
    )

    if result.returncode != 0:
        print(f"Error building wheel:\n{result.stderr}", file=sys.stderr)
        return 1

    print("✓ Wheel built successfully")
    print(f"\nWheel location: {script_dir / 'dist'}/")

    return 0


if __name__ == "__main__":
    sys.exit(main())
