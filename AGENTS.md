# AGENTS

## Fast Start
- Run all checks with `cargo test -- --nocapture`.
- Run one integration test file with `cargo test --test dtmc_sym_constr_tests -- --nocapture`.
- Run symbolic checking tests with `cargo test --test dtmc_sym_check_tests -- --nocapture`.
- Run one test case with `cargo test --test dtmc_sym_constr_tests dtmc_simple_constr -- --nocapture`.
- Run the CLI on a model with constants: `cargo run -- --model tests/dtmc/brp.prism --model-type dtmc --const N=1,MAX=2`.

## Build / Tooling Facts
- Parser generation is automatic via `build.rs` (`lalrpop::process_root()`), so grammar edits in `src/parser/parser.lalrpop` are picked up by normal Cargo builds.
- Release profile keeps debug symbols (`[profile.release] debug = true`, `strip = "none"`) for profiling.
- `cudd-sys` is patched to the vendored crate at `vendor/cudd-sys` via `[patch.crates-io]` in `Cargo.toml`.
- Default feature `build-cudd` is enabled and wires to `cudd-sys/build_cudd`; disable defaults if you need to link against a system CUDD.
- CI currently runs Cargo tests only (`.github/workflows/ci.yml`); Nix flake checks are present but commented out.
- Toolchain is pinned via `rust-toolchain.toml`.

## Architecture (what matters when editing)
- Main flow: `src/main.rs` -> `parser::parse_dtmc` -> `analyze::analyze_dtmc` -> `constr_symbolic::build_symbolic_dtmc` -> `reachability::compute_reachable_and_filter`.
- Symbolic manager wrapper is `src/ref_manager.rs`; this is the single place that should call CUDD APIs directly.
- `SymbolicDTMC` owns manager roots and is responsible for deref on drop (`src/symbolic_dtmc.rs`).
- `SymbolicDTMC` now lazily caches and owns derived BDDs via `OnceCell`: initial state, reachable set, filtered 0-1 transitions, and `(curr == next)` identity.
- Reachability filtering and dead-end self-loop completion are centralized in `SymbolicDTMC::set_reachable_and_filter`.

## CUDD Type Discipline (critical)
- `BddNode` wraps nodes used with `Cudd_bdd*` operations.
- `AddNode` wraps nodes used with `Cudd_add*` operations.
- Any function that takes or returns `BddNode`/`AddNode` must include an explicit
  doc comment contract in this style:
  - `__Refs__: ...`
  - `__Derefs__: ...`
  This is mandatory so ownership and ref-count behavior stays auditable.
- Convert explicitly when crossing APIs:
  - ADD -> BDD: `add_to_bdd` / `add_to_bdd_pattern`
  - BDD -> ADD: `bdd_to_add`
- `Cudd_addIte` expects an ADD condition; in this repo `add_ite` accepts `BddNode` and converts internally to ADD before calling CUDD.
- Abstraction helpers are explicitly separated by intent: `add_sum_abstract`, `add_or_abstract`, `add_max_abstract`, and `add_min_abstract`.
- Numerical convergence checks use `add_equal_sup_norm(..., mgr.epsilon())` (`EPS = 1e-10`).

## Ref / Leak Checks
- Leak check path is CUDD-based (`Cudd_CheckZeroRef`) through `RefManager::nonzero_ref_count()`.
- `RefManager::debug_check()` wraps `Cudd_DebugCheck`; drop-time debug check is gated by `ENABLE_CUDD_DEBUGCHECK_ON_DROP`.

## Test Expectations You Should Not Accidentally Break
- `tests/dtmc_sym_constr_tests.rs` asserts transition node count, terminal count, minterms, reachable states, and zero nonzero refs via `release_report()`.
- `tests/parser_consts_tests.rs` asserts const parsing supports interspersed `const` declarations and optional initializers.
- `tests/dtmc_sym_check_tests.rs` now includes unbounded-until regression on `knuth_die` (`P=? [phi1 U phi2]`) and still expects zero leaked refs after `release_report()`.

<!-- code-review-graph MCP tools -->
## MCP Tools: code-review-graph

**IMPORTANT: This project has a knowledge graph. ALWAYS use the
code-review-graph MCP tools BEFORE using Grep/Glob/Read to explore
the codebase.** The graph is faster, cheaper (fewer tokens), and gives
you structural context (callers, dependents, test coverage) that file
scanning cannot.

### When to use graph tools FIRST

- **Exploring code**: `semantic_search_nodes` or `query_graph` instead of Grep
- **Understanding impact**: `get_impact_radius` instead of manually tracing imports
- **Code review**: `detect_changes` + `get_review_context` instead of reading entire files
- **Finding relationships**: `query_graph` with callers_of/callees_of/imports_of/tests_for
- **Architecture questions**: `get_architecture_overview` + `list_communities`

Fall back to Grep/Glob/Read **only** when the graph doesn't cover what you need.

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

1. The graph auto-updates on file changes (via hooks).
2. Use `detect_changes` for code review.
3. Use `get_affected_flows` to understand impact.
4. Use `query_graph` pattern="tests_for" to check coverage.
