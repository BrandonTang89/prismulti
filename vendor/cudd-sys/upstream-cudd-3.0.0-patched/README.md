# Patched CUDD 3.0.0 Snapshot

This directory stores the final patched upstream files for CUDD `3.0.0` used by this repository.

Files:

- `cudd/cuddAddAbs.c`
- `cudd/cudd.h`
- `cudd/cuddInt.h`

Why this exists:

- `vendor/cudd-sys/patches/cudd-add-max-min.patch` is the canonical patch used by both:
  - `vendor/cudd-sys/build.rs` (when building bundled CUDD), and
  - `flake.nix` (when overriding Nixpkgs `cudd`).
- Keeping a snapshot of the patched target files makes future patch maintenance easier.

How to regenerate patch from this snapshot:

1. Download and unpack upstream `cudd-3.0.0` tarball.
2. Create a diff between upstream and this snapshot's `cudd/` directory.
3. Save the result to `vendor/cudd-sys/patches/cudd-add-max-min.patch`.

Example:

```bash
mkdir -p /tmp/cudd-orig /tmp/cudd-snap
tar xf cudd-3.0.0.tar.gz --strip-components=1 -C /tmp/cudd-orig
cp -r vendor/cudd-sys/upstream-cudd-3.0.0-patched/cudd /tmp/cudd-snap/
diff -uNr /tmp/cudd-orig/cudd /tmp/cudd-snap/cudd > vendor/cudd-sys/patches/cudd-add-max-min.patch
```
