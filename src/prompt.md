
Using local roots guard is inefficient since we very frequently allocate memory on the heap with the use of Vec.
We will replace its functionality entirely with the use of protected local.

The idea is that for each temporary we make, we store it within a protected local.

The current API is also quite messy on using local roots guard is also very messy which I dislike. It is not clear who supposed to protect what. So lets make this clear

For EVERY function of the form

fn foo(a: BddNode, b: BddNode, ...) -> BddNode

it is the CALLER's responsibility to protect a and b (and the rest of the parameters) as well as the result.
Within foo, we only need to protect temporaries we create. So most of the functions currently in ref_manger.rs should be changed to assume this.
We should also inspect each and every call site to fix the API contract and ensure that the caller is protecting the parameters and result as required.

Also help me to move all methods that don't actually depend on ref_manager.rs out of the impl of RefManager.

Ensure that all tests continue to pass after this change.

Good luck