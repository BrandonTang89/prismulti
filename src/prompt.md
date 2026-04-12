Okay lets try to see if we can get the debug check to work...

One thing that I see is that one_bdd and one_add, zero_add, zero_bdd are all not referenced.

I'm also not sure why we need a refresh_nonzero_ref_baseline. Just before we drop the manager,
we should hygienically clean up all the refs first, so we shouldnl't have any ned for that if we are doing it correctly.

The next thing is to improve the way we get statistics:
    num_nodes no longer has to be done manually. We can just wrap Cudd_DagSize.

    We also can wrap Cudd_CountMinTerm to determine the number of min_terms

    We should also expose Cudd_ForeachNode which may be helpful later on.

    We can then use Cudd_ForeachNode to get a vector of terminals (should also be exposed as a function). This can then be used to implement a get_num_terminals function.

Once this works, remove the existing manual recursiion stuff that we have to collate ADD statistics.
