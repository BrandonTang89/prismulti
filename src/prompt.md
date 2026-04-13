Okay, now we will work on dealing with the next and bounded until property model checking.
This should mostly be implemented in a new sym_check.rs file.

P=? [X phi]

is given by MVMult(T, rename(phi, curr_vars -> next_vars), next_vars)

where T is the transition probability ADD and phi is a BDD converted into a 0-1 ADD representing the states that satisfy phi. The result is an ADD representing the probability of satisfying phi in the next step.

MVMult(A, B, cols) is given by exposing cudd_addMatrixMultiply in ref_manager.rs 

pub unsafe extern "C" fn Cudd_addMatrixMultiply(
    dd: *mut DdManager,
    A: *mut DdNode,
    B: *mut DdNode,
    z: *mut *mut DdNode,
    nz: c_int,
) -> *mut DdNode

Calculates the product of two matrices, A and B, represented as ADDs. This procedure implements the quasiring multiplication algorithm. A is assumed to depend on variables x (rows) and z (columns). B is assumed to depend on variables z (rows) and y (columns). The product of A and B then depends on x (rows) and y (columns). Only the z variables have to be explicitly identified; they are the "summation" variables. Returns a pointer to the result if successful; NULL otherwise. 

We should confirm that the P=? [X s=1] property for the knuth_die model is correctly computed as 0.5, and add this to a new test file called dtmc_sym_check_tests.rs

Then, we can implement the bounded until property check.

To compute P=? [phi1 U<=k phi2], can use the following algorithm:

```
s_no = !(phi1 || phi2)
s_yes = phi
s_? = (s_reachable &&!(phi1 || phi2)) 
    [where s_reachable is the computed set of reachable states from the reachability analysis]
T_? = bdd_to_add(s_?) times T

res_add = bdd_to_add(s_yes)
for i in 1..k:
    res_add = MVMult(T_?, add_rename(res_add, curr_vars -> next_vars), next_vars) + bdd_to_add(s_yes)

return res_add
```

We should ensure that P=? [true U<=3 s=7] has a result of 0.75 and implement this as a test in dtmc_sym_check_tests.rs as well.


As for the behaviour in the main.rs, we should check all the properties that we support (ignoring reward properties and unbounded until properties for now) and call the appropriate checking function for each property type. We can also add a command line flag to specify which properties to check, i.e. --props 1,2,3 to check properties 1, 2 and 3 in the order they are defined in the property file. By default, we check all the properties.

Remember to add sufficient documentation for me to do code review, and suffient logging at runtime with the info!, debug! and trace! macros to allow for debugging and understanding the flow of the program.
