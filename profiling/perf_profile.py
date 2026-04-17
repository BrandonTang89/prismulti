#!/usr/bin/env python3

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
from datetime import datetime
from pathlib import Path

from benches.benchmarks import BENCHMARKS

REPO_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

DEFAULT_BENCHMARK_NAMES: list[str] = [
    "leader3_2_check",
    "leader5_7_check",
    "leader6_6_check",
    "brp_n32_max4_all_props",
]
DEFAULT_RECORD_ARGS: list[str] = ["--call-graph", "dwarf,16384", "--freq", "999"]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run selected prismulti benchmarks under perf record."
    )
    parser.add_argument(
        "benchmarks",
        nargs="*",
        help=(
            "Benchmark names to run. If omitted, DEFAULT_BENCHMARK_NAMES is used. "
            "Benchmark definitions live in benches/benchmarks.py."
        ),
    )
    parser.add_argument(
        "--binary",
        default="target/release/prismulti",
        help="Path to prismulti binary (default: target/release/prismulti).",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="Skip cargo build --release before profiling.",
    )
    parser.add_argument(
        "--output-dir",
        default="profiling/perf_output",
        help="Directory for perf output files (default: profiling/perf_output).",
    )
    parser.add_argument(
        "--record-args",
        default="",
        help=(
            "Extra arguments appended to perf record, for example: "
            "'--event cycles:u --sample-cpu'."
        ),
    )
    parser.add_argument(
        "--no-default-record-args",
        action="store_true",
        help=(
            "Disable default perf record args (--call-graph dwarf,16384 --freq 999)."
        ),
    )
    return parser.parse_args()


def ensure_binary(binary_path: str, skip_build: bool) -> None:
    if not skip_build:
        subprocess.run(["cargo", "build", "--release"], check=True)
    if not Path(binary_path).exists():
        raise FileNotFoundError(f"Expected binary at {binary_path}")


def select_benchmark_names(requested_names: list[str]) -> list[str]:
    names = requested_names if requested_names else DEFAULT_BENCHMARK_NAMES
    unknown = sorted(set(names) - BENCHMARKS.keys())
    if unknown:
        available = ", ".join(sorted(BENCHMARKS.keys()))
        unknown_list = ", ".join(unknown)
        raise ValueError(
            f"Unknown benchmark(s): {unknown_list}. Available benchmarks: {available}"
        )
    return names


def output_paths_for_benchmark(
    output_dir: Path, benchmark_name: str
) -> tuple[Path, Path, str]:
    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    data_path = output_dir / f"{benchmark_name}-{timestamp}.perf"
    log_path = output_dir / f"{benchmark_name}-{timestamp}.record.log"
    return data_path, log_path, timestamp


def run_perf_for_benchmark(
    benchmark_name: str,
    binary_path: str,
    output_dir: Path,
    record_args: list[str],
) -> tuple[Path, Path]:
    benchmark = BENCHMARKS[benchmark_name]
    data_path, log_path, timestamp = output_paths_for_benchmark(
        output_dir, benchmark_name
    )
    command = [
        "perf",
        "record",
        "-o",
        str(data_path),
        *record_args,
        "--",
        *benchmark.command_args(binary_path),
    ]
    result = subprocess.run(command, capture_output=True, text=True, check=False)

    output = [
        f"benchmark: {benchmark_name}\n",
        f"timestamp: {timestamp}\n",
        f"command: {shlex.join(command)}\n",
        f"perf_data: {data_path}\n",
        f"return_code: {result.returncode}\n",
        "\n=== stdout ===\n",
        result.stdout,
        "\n=== stderr ===\n",
        result.stderr,
    ]
    log_path.write_text("".join(output), encoding="utf-8")

    if result.returncode != 0:
        raise subprocess.CalledProcessError(
            returncode=result.returncode,
            cmd=command,
            output=result.stdout,
            stderr=result.stderr,
        )
    if not data_path.exists():
        raise FileNotFoundError(f"Expected perf data at {data_path}")
    return data_path, log_path


def run_perf(
    benchmark_names: list[str],
    binary_path: str,
    output_dir: Path,
    record_args: list[str],
) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    for benchmark_name in benchmark_names:
        data_path, log_path = run_perf_for_benchmark(
            benchmark_name=benchmark_name,
            binary_path=binary_path,
            output_dir=output_dir,
            record_args=record_args,
        )
        print(f"Wrote perf data for {benchmark_name} to {data_path}")
        print(f"Wrote perf record log for {benchmark_name} to {log_path}")


def main() -> None:
    args = parse_args()
    ensure_binary(binary_path=args.binary, skip_build=args.skip_build)
    benchmark_names = select_benchmark_names(args.benchmarks)
    record_args: list[str] = []
    if not args.no_default_record_args:
        record_args.extend(DEFAULT_RECORD_ARGS)
    record_args.extend(shlex.split(args.record_args))
    run_perf(
        benchmark_names=benchmark_names,
        binary_path=args.binary,
        output_dir=Path(args.output_dir),
        record_args=record_args,
    )


if __name__ == "__main__":
    main()
