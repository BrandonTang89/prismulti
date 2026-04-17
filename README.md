# prismulti

`prismulti` is a multi-threaded rust implementation of a subset of the
[PRISM](https://www.prismmodelchecker.org/) model checker.

## Current status

Working today:
- PRISM DTMC model parsing and symbolic construction
- symbolic probabilistic checking for:
  - `P=? [X phi]`
  - `P=? [phi1 U<=k phi2]`
  - `P=? [phi1 U phi2]`

Not implemented yet:
- reward model checking (`R=? [...]`)
- broader PRISM language coverage (MDPs, TSGs, CSGs, etc)

## Differences from Prism
Apart from some differences in supported features (see other [docs](docs/)), there are some differences in the internal design.

PRISM uses the CUDD library which has better single-threaded performance than Sylvan but does not support multi-threading. In contrast, we use Sylvan which has worse single-threaded performance but supports multi-threading and is more actively maintained.

In the Prism codebase, only JDDNode is used, which internally wrap CUDD ADD nodes. BDDs are then just represented as 0-1 ADDs. In prismulti, we explicitly differentiate between BDDs and ADDs in the codebase with the BddNode and AddNode types. These wrap sylvan BDD and ADD nodes respectively. This lack of type strictness is more prone to errors and less efficient. ADDs in Sylvan do not implement complementary edges and therefore make negation more expensive. Furthermore, operations that assume to operate on BDDs can be performed more efficiently than that same operation on ADDs, so it is important to differentiate between the two and use the correct type for the correct purpose.

This type strictness is taken further by the usage of `VarSet` and `BddMap` types which represent BDD cubes and specialized Sylvan maps that are used for permuting variables efficiently in abstraction operations.

CUDD uses value-based reference counting, while Sylvan allows the use of pointer based protection, which is used throughout the codebase for efficiency and ergonomics. By using pointer-based protection we avoid the overhead of incrementing/decrementing reference counts in hot loops by predeclaring several temporary protected variables which act as GC roots to protect intermediate values. 

## Build and Run
### Stable Rust
With the stable version of rust, you can build, run and test the project in the usual way with cargo:

```bash
cargo build --release
cargo run -- [options]
```

### Nix
We also support building with Nix for easier packaging in the future.

```bash
nix build
./result/bin/prismulti [options]
```

For development, we recommend using `nix develop` to get a shell with all the relevant tools installed.

### Using the Binary
General form:

```bash
cargo run -- --model-type dtmc --model <path-to-model.prism> [options]
```

Options:
- `--model-type dtmc` model type selector (currently only DTMC)
- `--model <path>` model file
- `--const NAME=VALUE,...` constant overrides
- `--prop-file <path>` property file to load
- `--props 1,2,3` evaluate only selected property indices (1-based, in file order)
- `-v, --verbose` enable debug-level logging

#### Example
```bash
cargo run -- --model-type dtmc --model tests/dtmc/knuth_die.prism --prop-file tests/dtmc/knuth_die.prop --props 2,3 --const x=4
```

This parses the model and properties, constructs the symbolic DTMC, and checks
properties 2 and 3 from the property file.

## Testing
We mostly use rust integration tests on small results. Simply run `cargo test` to run all tests. 

## Benchmarks
We provide both macro-benchmarking and micro-benchmarking infrastructure within `benches/`.

Micro-benchmarking is provided by the `criterion` crate and allows for fine-grained benchmarking of individual functions or operations. See `dtmc_benches.rs`. (However, this is not currently used for much)

Macro-benchmarking is done via `hyperfine` in a Python script that runs the entire binary on a set of models and properties and measures end-to-end runtime. Benchmark definitions are centralized in `benches/benchmarks.py` so they can also be reused by other profiling scripts.

### Hyperfine benchmarks
Install `hyperfine` and run:

```bash
python -m benches.hyperfine_bench.py
```

Useful options:
- `--max-level <N>` only runs benchmarks with level `<= N`.
- `--skip-build` skips `cargo build --release`.
- `--binary <path>` points to a custom `prismulti` binary.
- `--export-json <path>` sets the base JSON output path (one file per benchmark).

Example:

```bash
python -m benches.hyperfine_bench.py --max-level 3 --export-json target/hyperfine-checking.json
```

## Perf profiling
For profiling data that you can open in Hotspot (and use for flamegraph-style call-stack analysis), use `profiling/perf_profile.py`.

By default it runs a built-in list of benchmark names, and you can override that list by passing benchmark names as positional arguments.

```bash
python3 -m profiling.perf_profile.py
python3 -m profiling.perf_profile.py leader6_6_check brp_n1024_max8_all_props
```

Useful options:
- `--skip-build` skips `cargo build --release`.
- `--binary <path>` points to a custom `prismulti` binary.
- `--output-dir <path>` changes where profile outputs are written (default: `profiling/perf_output`).
- `--record-args "..."` appends custom flags to `perf record`.
- `--no-default-record-args` disables default callgraph sampling args (`--call-graph dwarf,16384 --freq 999`).

Each run writes:
- `profiling/perf_output/<benchmark>-<timestamp>.perf` (open this in Hotspot)
- `profiling/perf_output/<benchmark>-<timestamp>.record.log` (command, return code, stdout/stderr)

Open one result in Hotspot:

```bash
hotspot profiling/perf_output/leader6_6_check-<timestamp>.perf
```

Unfortunately, due to use of lace by Sylvan, we cannot really see where the epensive ADD operations are being called, but we can at least see which are the expensive ADD operations.

## Sylvan Tuning
 
 The symbolic backend can be tuned via environment variables:
 
 - `PRISM_SYLVAN_WORKERS` (default `0`): Lace worker threads. 
 - `PRISM_SYLVAN_GRANULARITY`: override Sylvan task granularity (optional).
 - `PRISM_SYLVAN_MEMORY_CAP`: memory cap passed to `sylvan_set_limits` (bytes).
 - `PRISM_SYLVAN_TABLE_RATIO`: table/cache ratio passed to `sylvan_set_limits`.
 - `PRISM_SYLVAN_INITIAL_RATIO`: initial table ratio passed to `sylvan_set_limits`.

## Docs and Code Conventions
For deeper DTMC semantics and symbolic checking notes, see
([docs/dtmc_details.md](docs/dtmc_details.md)).

For internal conventions around DD usage and protection, see
([docs/dd_usage.md](docs/dd_usage.md)).
