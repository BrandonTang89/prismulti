from __future__ import annotations

import shlex
from dataclasses import dataclass


@dataclass(frozen=True)
class BenchmarkConfig:
    level: int
    runs: int | None = None


@dataclass(frozen=True)
class Benchmark:
    model_path: str
    prop_path: str
    const_overrides: str
    props: str
    bench_config: BenchmarkConfig
    warmup_runs: int = 1

    def command_args(self, binary_path: str) -> list[str]:
        return [
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

    def command(self, binary_path: str) -> str:
        return shlex.join(self.command_args(binary_path))


BENCHMARKS: dict[str, Benchmark] = {
    # ~0.2s (L1)
    "leader3_2_check": Benchmark(
        model_path="tests/dtmc/leader3_2.prism",
        prop_path="tests/dtmc/leader.prop",
        const_overrides="L=3",
        props="1,2",
        bench_config=BenchmarkConfig(level=1, runs=3),
    ),
    # ~1.1s (L1)
    "leader5_7_check": Benchmark(
        model_path="tests/dtmc/leader5_7.prism",
        prop_path="tests/dtmc/leader.prop",
        const_overrides="L=3",
        props="1,2",
        bench_config=BenchmarkConfig(level=1, runs=3),
    ),
    # ~4.0s (L1)
    "leader6_6_check": Benchmark(
        model_path="tests/dtmc/leader6_6.prism",
        prop_path="tests/dtmc/leader.prop",
        const_overrides="L=3",
        props="1,2",
        bench_config=BenchmarkConfig(level=1, runs=3),
    ),
    # 0.9s (L1)
    "brp_n32_max4_all_props": Benchmark(
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=32,MAX=4",
        props="1,2,3,4,5,6",
        bench_config=BenchmarkConfig(level=1, runs=3),
    ),
    # ~5.6s (L2)
    "brp_n1024_max8_all_props": Benchmark(
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=1024,MAX=8",
        props="1,2,3,4,5,6",
        bench_config=BenchmarkConfig(level=2, runs=3),
    ),
    # ~11.8s (L3)
    "brp_n2048_max8_all_props": Benchmark(
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=2048,MAX=8",
        props="1,2,3,4,5,6",
        bench_config=BenchmarkConfig(level=3, runs=3),
    ),
    # ~21.8s (L3)
    "brp_n256_max4_all_props": Benchmark(
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=256,MAX=4",
        props="1,2,3,4,5,6",
        bench_config=BenchmarkConfig(level=3, runs=3),
    ),
    # ~55.7s (L4)
    "brp_n512_max4_all_props": Benchmark(
        model_path="tests/dtmc/brp.prism",
        prop_path="tests/dtmc/brp.prop",
        const_overrides="N=512,MAX=4",
        props="1,2,3,4,5,6",
        bench_config=BenchmarkConfig(level=4, runs=2),
    ),
}
