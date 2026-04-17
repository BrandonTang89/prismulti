# DD Manager Usage Notes

This document explains how to use `src/dd_manager/`, with emphasis on
`protected_local` and `protected_slot`.

## Why protection exists

Sylvan uses garbage collection with a mark-and-sweep style reachability pass.
In practice, nodes that are reachable from protected roots are kept alive, and
unreachable nodes may be reclaimed during GC.

`Sylvan_mtbdd_protect(ptr)` registers a pointer to an `MTBDD` slot as a root.
`Sylvan_mtbdd_unprotect(ptr)` removes that root registration.

If an intermediate/result node is not protected when needed, later DD calls may
trigger GC and reclaim it.

## Two protection mechanisms

### `protected_local` (stack-local, temporary roots)

Source: `src/dd_manager/protected_local.rs`

- `ProtectedLocal` stores an `MTBDD` in a local variable slot.
- `protect()` registers that slot with Sylvan.
- `Drop` calls `unprotect()`.
- Typed wrappers exist for domain types:
  - `ProtectedBddLocal`
  - `ProtectedAddLocal`
  - `ProtectedMapLocal`
  - `ProtectedVarSetLocal`

Important: do not rely on calling `new(...)` alone. Protection must happen only
after the value is in its final local slot. Use the macros:

- `protected_bdd!(name, expr)`
- `protected_add!(name, expr)`
- `protected_map!(name, expr)`
- `protected_var_set!(name, expr)`

Each macro expands to:

```rust
let mut name = ProtectedXLocal::new(expr);
name.protect();
```

This avoids protecting a pointer before a move of the local binding.

### `protected_slot` (owned, long-lived roots)

Source: `src/dd_manager/protected_slot.rs`

- `ProtectedSlot` stores `MTBDD` inside a `Box<MTBDD>`.
- It protects eagerly in `new(...)` and unprotects in `Drop`.
- Because the value is boxed, the protected pointer stays stable even if the
  wrapper itself is moved.
- Typed wrappers:
  - `ProtectedBddSlot`
  - `ProtectedAddSlot`
  - `ProtectedVarSetSlot`

Use this for fields that must stay rooted across many operations (for example,
roots stored in `SymbolicDTMC`).

## Calling convention in this codebase

This repository follows a strict convention for DD operations:

- The caller is responsible for protecting all argument nodes before calling a
  function.
- The caller is also responsible for protecting the returned node if it must
  survive subsequent DD operations.
- Callees should only protect their own internal temporaries.

In other words: ownership of root-liveness is at call boundaries.

### What this means in practice

- Public functions in `src/dd_manager/dd.rs` mostly pass and return plain
  `BddNode`/`AddNode`/`VarSet` values.
- Those values are not automatically rooted by the function API.
- Functions use `protected_*!` only for internal intermediate values that must
  remain alive while building the final result.

## Example patterns

### Caller-side protection

```rust
crate::protected_bdd!(x, dd::bdd_var(&mgr, x_idx));
crate::protected_bdd!(not_x, dd::bdd_not(x.get()));

let combined = dd::bdd_and(x.get(), not_x.get());
crate::protected_bdd!(combined_root, combined);
```

### Callee-side temporaries only

```rust
pub fn add_equals(a: AddNode, b: AddNode) -> BddNode {
    crate::protected_bdd!(gt, add_greater_than(a, b));
    crate::protected_bdd!(lt, add_less_than(a, b));
    crate::protected_bdd!(neq, bdd_or(gt.get(), lt.get()));
    bdd_not(neq.get())
}
```

The callee protects only `gt`, `lt`, `neq` (its internal temporaries). The
caller must root the returned value if needed beyond immediate use.

## Rule of thumb

- Use `protected_local` macros for short-lived locals in expression-building
  code.
- In iterative/hot loops, predeclare reusable `protected_local` slots before the
  loop and update them with `.replace(...)`/`.set(...)` each iteration, instead
  of creating new protected locals inside the loop body.
- Use `protected_slot` for long-lived owned roots in structs.
- Never assume a plain returned node stays alive unless you root it on the
  caller side.
- Never assume that temporaries between DD calls stay alive unless you protect them by storing the intermediate result in a `protected_local` or `protected_slot`.
