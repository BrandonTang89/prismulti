#!/usr/bin/env python3

from __future__ import annotations

import argparse
import shlex
import subprocess
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Benchmark:
    name: str
    model_path: str
    prop_path: str
    const_overrides: str
    props: str
    level: int
    runs: int | None = None
    warmup_runs: int = 1

    def command(self, binary_path: str) -> str:
        args = [
            binary_path,
            "--model-type",
            "dtmc",
            "--model",
            self.model_path,
            "--prop-file",
            self.prop_path,
            "--props",
            self.props,
            "--const",
            self.const_overrides,
        ]
        return shlex.join(args)


BENCHMARKS: tuple[Benchmark, ...] = (
    # ~0.2s (L1)
    Benchmark(
        name="leader3_2_check",
        model_path="tests/dtmc/leader3_2.prism",
        prop_path="tests/dtmc/leader.prop",
        const_overrides="L=3",
        props="1,2",
        level=1,
        runs=3,
    ),
    # ~1.1s (L1)
    Benchmark(
        name="leader5_7_check",
        model_path="tests/dtmc/leader5_7.prism",
        prop_path="tests/dtmc/leader.prop",
        const_overrides="L=3",
        props="1,2",
        level=1,
        runs=3,
    ),
    # ~4.0s (L1)
    Benchmark(
        name="leader6_6_check",
        model_path="tests/dtmc/leader6_6.prism",
        prop_path="tests/dtmc/leader.prop",
        const_overrides="L=3",
        props="1,2",
        level=1,
        runs=3,
    ),
    # 0.9s (L1)
    Benchmark(
        name="brp_n512_max4_all_props",
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=32,MAX=4",
        props="1,2,3,4,5,6",
        level=1,
        runs=3,
    ),
    # ~5.6s (L2)
    Benchmark(
        name="brp_n1024_max8_all_props",
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=1024,MAX=8",
        props="1,2,3,4,5,6",
        level=2,
        runs=3,
    ),
    # ~11.8s (L3)
    Benchmark(
        name="brp_n2048_max8_all_props",
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=2048,MAX=8",
        props="1,2,3,4,5,6",
        level=3,
        runs=3,
    ),
    # ~21.8s (L3)
    Benchmark(
        name="brp_n256_max4_all_props",
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=256,MAX=4",
        props="1,2,3,4,5,6",
        level=3,
        runs=3,
    ),
    # ~55.7s (L4)
    Benchmark(
        name="brp_n512_max4_all_props",
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=512,MAX=4",
        props="1,2,3,4,5,6",
        level=4,
        runs=2,
    ),
)


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
    selected_benchmarks = tuple(b for b in BENCHMARKS if b.level <= max_level)
    if not selected_benchmarks:
        raise ValueError(f"No benchmarks configured for level <= {max_level}")

    for benchmark in selected_benchmarks:
        command: list[str] = [
            "hyperfine",
            "--warmup",
            str(benchmark.warmup_runs),
            "--export-json",
            json_path_for_benchmark(export_json_base, benchmark.name),
            "-n",
            benchmark.name,
            benchmark.command(binary_path),
        ]

        if benchmark.runs is not None:
            command[1:1] = ["--runs", str(benchmark.runs)]

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
