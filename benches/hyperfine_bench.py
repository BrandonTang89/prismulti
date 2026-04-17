#!/usr/bin/env python3

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path

from benchmarks import BENCHMARKS


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run DTMC model-checking benchmarks with hyperfine."
    )
    parser.add_argument(
        "--binary",
        default="target/release/prismulti",
        help="Path to prismulti binary (default: target/release/prismulti).",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="Skip cargo build --release before benchmarking.",
    )
    parser.add_argument(
        "--max-level",
        type=int,
        choices=range(1, 6),
        default=4,
        help=(
            "Run benchmark levels up to this level (1-5). "
            "Example: --max-level 3 runs levels 1, 2, and 3."
        ),
    )
    parser.add_argument(
        "--export-json",
        default="target/hyperfine-checking.json",
        help=(
            "Base output path for hyperfine JSON results. "
            "One file per benchmark is written."
        ),
    )
    return parser.parse_args()


def ensure_binary(binary_path: str, skip_build: bool) -> None:
    if skip_build:
        return
    subprocess.run(["cargo", "build", "--release"], check=True)
    if not Path(binary_path).exists():
        raise FileNotFoundError(f"Expected binary at {binary_path}")


def json_path_for_benchmark(base_path: str, benchmark_name: str) -> str:
    base = Path(base_path)
    return str(base.with_name(f"{base.stem}-{benchmark_name}{base.suffix}"))


def run_hyperfine(binary_path: str, export_json_base: str, max_level: int) -> None:
    selected_benchmarks = tuple(
        (name, benchmark)
        for name, benchmark in BENCHMARKS.items()
        if benchmark.bench_config.level <= max_level
    )
    if not selected_benchmarks:
        raise ValueError(f"No benchmarks configured for level <= {max_level}")

    for benchmark_name, benchmark in selected_benchmarks:
        command: list[str] = [
            "hyperfine",
            "--warmup",
            str(benchmark.warmup_runs),
            "--export-json",
            json_path_for_benchmark(export_json_base, benchmark_name),
            "-n",
            benchmark_name,
            benchmark.command(binary_path),
        ]

        if benchmark.bench_config.runs is not None:
            command[1:1] = ["--runs", str(benchmark.bench_config.runs)]

        subprocess.run(command, check=True)


def main() -> None:
    args = parse_args()
    ensure_binary(binary_path=args.binary, skip_build=args.skip_build)
    run_hyperfine(
        binary_path=args.binary,
        export_json_base=args.export_json,
        max_level=args.max_level,
    )


if __name__ == "__main__":
    main()
