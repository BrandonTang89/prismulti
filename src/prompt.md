Lets now implement the prob0 and prob1 functions that
should be used to determine s_no and s_yes respectively within the unbounded until
checking algorithm.

prob0(phi1, phi2) returns the states with 0 probability that phi1 holds until phi2 holds.
We compute it by finding states that have >0 probability of satisfying phi1 until phi2
and then negating that. 

sol = phi2
loop:
    sol' = sol OR (phi1 AND BDD_EXISTSabstract(T_01 AND ReplaceVars(sol, curr_vars, next_vars), next_vars))
    if sol' == sol:
        break
    sol = sol'

return reachable AND NOT sol

prob1(phi1, phi2, s_no) returns the states with proabbility 1 of stisfying phi1 until phi2. We compute it by finding states that have >0 of reaching a state in s_no and then negating that.

sol = s_no
loop:
    sol' = sol OR ((phi_1 AND NOT phi_2) AND BDD_EXISTSabstract(T_01 AND ReplaceVars(sol, curr_vars, next_vars), next_vars))
    if sol' == sol:
        break
    sol = sol'

return reachable AND NOT sol

Implement this in sym_check and confirm that the existing tests still pass.

Also add the P=? [ F s=34 & d=x ] test for the knuth_two_dice model to the test suite and confrm that it should be 
: 0.0833333320915699 for x = 4
: 0.0555555522441864 for x = 3
: 0.0277777761220932 for x = 2
: 0.0 for x = 1