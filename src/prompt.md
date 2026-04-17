Currently ProtectedLocal::uses lazy protect for the following reason:

- With eager protect in ProtectedLocal::new, we protect the address inside the constructor.
- Returning Self can move that value, so Sylvan is tracking an old pointer.
- Later nested protect/unprotect stack behavior hits invalid ordering/address assumptions and triggers protect_down assertions (this is the exact failure we saw).
- Lazy protect ensures first protect happens only after the value is in its final local slot (get/set/...), so the protected pointer is stable.

But I don't like having the extra bool within the struct for performance reasons. Intead, we should make a ProtectedLocal that doesn't do this.
What we want is a ProtectedLocal that uses a macro to first construt the ProtectedLocal without protection, then immediately protects it

E.g. 
```rust
protected_bdd!(local, bdd.zero());
```

where protected_bdd! would expand to something like:
```rust
let mut local = ProtectedBddLocal::new(bdd.zero());
local.protect();
```

Now, the second thing is that almost all the functions in dd.rs take a DDManager as an input, but doesn't need to be. Remove that parameter from all the functions that don't need a DDManager. This will make the code cleaner and more efficient, as we won't have to pass around the DDManager unnecessarily. Adjust all the call sites accordingly.

Ensure all tests continue to pass. Good luck!