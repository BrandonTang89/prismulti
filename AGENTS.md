# AGENTS

## Fast Start
- Run all tests with `cargo test -- --nocapture`.
- All tests run well under 10 seconds, so use that as a timeout, some changes may cause timeouts if they cause deadlocks/livelocks, etc.
- Run one integration test file with `cargo test --test dtmc_sym_constr_tests -- --nocapture`.
- Run symbolic checking tests with `cargo test --test dtmc_sym_check_tests -- --nocapture`.
- Run parser integration tests with `cargo test --test parser_tests -- --nocapture`.
- Run one test case with `cargo test --test dtmc_sym_constr_tests dtmc_simple_constr -- --nocapture`.
- Run the CLI on a model with constants: `cargo run -- --model tests/dtmc/brp.prism --model-type dtmc --const N=1,MAX=2`.
- Run the CLI with a property file: `cargo run -- --model tests/dtmc/knuth_die.prism --model-type dtmc --prop-file tests/dtmc/knuth_die.prop --props 2,3 --const x=4`.

## Build / Tooling Facts
- Parser generation is automatic via `build.rs` (`lalrpop::process_root()`), so grammar edits in `src/parser/parser.lalrpop` are picked up by normal Cargo builds.
- Release profile keeps debug symbols (`[profile.release] debug = true`, `strip = "none"`) for profiling.
- Default feature `build-sylvan` is enabled and wires to `sylvan-sys/build_sylvan` in `Cargo.toml`.
- `sylvan-sys` builds and links static `lace`, `sylvan`, and wrapper libraries via CMake/FetchContent when `build-sylvan` is enabled.
- Runtime tuning is env-driven in `DDManager`: `PRISM_SYLVAN_WORKERS` (default `0`), `PRISM_SYLVAN_MEMORY_CAP` (default `1<<30`), `PRISM_SYLVAN_TABLE_RATIO`, `PRISM_SYLVAN_INITIAL_RATIO`, and `PRISM_SYLVAN_GRANULARITY`.
- Toolchain is pinned via `rust-toolchain.toml`.

## Architecture (what matters when editing)
- Main flow: `src/main.rs` -> `parser::parse_dtmc` (+ optional `parser::parse_dtmc_props`) -> `analyze::analyze_dtmc` -> `constr_symbolic::build_symbolic_dtmc` -> `reachability::compute_reachable_and_filter` -> `sym_check::evaluate_property_at_initial_state`.
- Sylvan API usage should stay inside `src/dd_manager.rs` (runtime init/config) and `src/dd_manager/dd.rs` (BDD/ADD operations).
- `SymbolicDTMC` owns the DD manager, variable roots, and cached derived nodes (`src/symbolic_dtmc.rs`).
- `SymbolicDTMC` now lazily caches and owns derived BDDs via `OnceCell`: initial state, reachable set, filtered 0-1 transitions, and `(curr == next)` identity.
- Reachability filtering and dead-end self-loop completion are centralized in `SymbolicDTMC::set_reachable_and_filter`.

## Sylvan Type Discipline (critical)
- `BddNode` wraps nodes used with `Sylvan_*` boolean operations.
- `AddNode` wraps nodes used with `Sylvan_mtbdd_*` numeric operations.
- Convert explicitly when crossing APIs:
  - ADD -> BDD: `add_to_bdd` / `add_to_bdd_pattern`
  - BDD -> ADD: `bdd_to_add`
- `add_ite` accepts a `BddNode` condition and uses Sylvan MTBDD ITE (`Sylvan_mtbdd_ite`) directly.
- Abstraction helpers are explicitly separated by intent: `add_sum_abstract`, `add_or_abstract`, `add_max_abstract`, and `add_min_abstract`.
- Numerical convergence checks use `add_equal_sup_norm(..., dd::epsilon(&mgr))` (`EPS = 1e-10`).

## Test Expectations You Should Not Accidentally Break
- `tests/dtmc_sym_constr_tests.rs` asserts transition ADD node count, terminal count, minterms, and reachable states for regression models.
- `tests/parser_tests.rs` covers interspersed `const` declarations, optional constant initializers, renamed-module expansion, and property-file parsing.
- `tests/dtmc_sym_check_tests.rs` includes regressions for unbounded/next/bounded-until on `knuth_die`, plus `knuth_two_dice`, `brp`, and `leader` property evaluations.

<!-- code-review-graph MCP tools -->
## MCP Tools: code-review-graph

**IMPORTANT: If code-review-graph MCP tools are available in your client,
use them before broad Grep/Glob/Read scans.** They are usually faster,
cheaper (fewer tokens), and provide structural context (callers,
dependents, test coverage).

### When to use graph tools FIRST

- **Exploring code**: `semantic_search_nodes` or `query_graph` instead of Grep
- **Understanding impact**: `get_impact_radius` instead of manually tracing imports
- **Code review**: `detect_changes` + `get_review_context` instead of reading entire files
- **Finding relationships**: `query_graph` with callers_of/callees_of/imports_of/tests_for
- **Architecture questions**: `get_architecture_overview` + `list_communities`

Fall back to Grep/Glob/Read when graph tools are unavailable or don't cover what you need.

### Key Tools

| Tool | Use when |
|------|----------|
| `detect_changes` | Reviewing code changes — gives risk-scored analysis |
| `get_review_context` | Need source snippets for review — token-efficient |
| `get_impact_radius` | Understanding blast radius of a change |
| `get_affected_flows` | Finding which execution paths are impacted |
| `query_graph` | Tracing callers, callees, imports, tests, dependencies |
| `semantic_search_nodes` | Finding functions/classes by name or keyword |
| `get_architecture_overview` | Understanding high-level codebase structure |
| `refactor_tool` | Planning renames, finding dead code |

### Workflow

1. If the graph is available, start with `detect_changes` during review.
2. Use `get_affected_flows` to understand impact.
3. Use `query_graph` with pattern `tests_for` to check coverage.
4. Use Grep/Glob/Read for gaps or deeper file-level details.
