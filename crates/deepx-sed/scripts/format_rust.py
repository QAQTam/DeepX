#!/usr/bin/env python3

# Copyright (c) 2026 Red Authors
# License: MIT
#

"""
Rust code formatter helper.

Usage:
  - Fix formatting in-place:
      python3 red/scripts/format_rust.py

  - Check formatting without changing files (CI-friendly):
      python3 red/scripts/format_rust.py --check

This script locates the crate root relative to its own location and runs
`cargo fmt --all` (or with `-- --check` in check mode).

Project convention: all code comments must be in English.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path


def get_crate_root() -> Path:
    """Return the crate root directory that contains Cargo.toml.

    The script lives under <repo>/red/scripts/, so the crate root is its parent.
    """
    script_path = Path(__file__).resolve()
    crate_root = script_path.parent.parent
    cargo_toml = crate_root / "Cargo.toml"
    if not cargo_toml.is_file():
        raise FileNotFoundError(
            f"Cargo.toml not found at expected location: {cargo_toml}"
        )
    return crate_root


def ensure_tools_available(preferred_toolchain: str | None) -> None:
    """Ensure required tools are available in PATH and rustfmt component exists.

    If a specific toolchain is preferred (e.g., "nightly" for edition 2024),
    verify rustfmt for that toolchain.
    """
    if shutil.which("cargo") is None:
        raise RuntimeError(
            "cargo not found in PATH. Install Rust (https://rustup.rs) and retry."
        )

    cargo_cmd: list[str] = ["cargo"]
    if preferred_toolchain:
        cargo_cmd += [f"+{preferred_toolchain}"]

    try:
        subprocess.run(
            [*cargo_cmd, "fmt", "--", "--version"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=True,
        )
    except subprocess.CalledProcessError as exc:
        if preferred_toolchain == "nightly":
            raise RuntimeError(
                "rustfmt for nightly is not installed. Install it via:\n"
                "  rustup toolchain install nightly\n"
                "  rustup component add rustfmt --toolchain nightly"
            ) from exc
        raise RuntimeError(
            "rustfmt is not installed. Install it via: rustup component add rustfmt"
        ) from exc
    except FileNotFoundError as exc:
        raise RuntimeError("cargo executable is not available") from exc


def run_cargo_fmt(check: bool, preferred_toolchain: str | None) -> int:
    """Run cargo fmt in the crate root. Return the process exit code."""
    crate_root = get_crate_root()
    cmd: list[str] = ["cargo"]
    if preferred_toolchain:
        cmd += [f"+{preferred_toolchain}"]
    cmd += ["fmt", "--all"]
    if check:
        cmd += ["--", "--check"]

    process = subprocess.run(cmd, cwd=str(crate_root))
    return process.returncode


def read_edition(crate_root: Path) -> str | None:
    """Read edition from Cargo.toml if present (e.g., "2021", "2024")."""
    cargo_toml = crate_root / "Cargo.toml"
    try:
        content = cargo_toml.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return None
    import re

    match = re.search(r"^\s*edition\s*=\s*\"(\d{4})\"\s*$", content, re.MULTILINE)
    if match:
        return match.group(1)
    return None


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Format Rust code using cargo fmt",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Check formatting without writing changes (non-zero exit on diff)",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)

    crate_root = get_crate_root()
    edition = read_edition(crate_root)
    preferred_toolchain = "nightly" if edition and edition >= "2024" else None

    try:
        ensure_tools_available(preferred_toolchain)
    except Exception as error:
        print(f"[format_rust] Error: {error}", file=sys.stderr)
        return 2

    exit_code = run_cargo_fmt(check=args.check, preferred_toolchain=preferred_toolchain)
    if exit_code == 0:
        action = "checked" if args.check else "formatted"
        print(f"[format_rust] Successfully {action} Rust code.")
    else:
        if args.check:
            print(
                "[format_rust] Formatting check failed. Run without --check to fix.",
                file=sys.stderr,
            )
        else:
            print(
                "[format_rust] Formatting command failed. See output above.",
                file=sys.stderr,
            )
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
