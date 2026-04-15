## Discrete Time Markov Chains (DTMCs)
### Model Structure and Semantics
DTMCs are completely probabilistic, i.e. they have no nondeterminism/choices. Variables are organised into modules. Each module has some local variables and a set of commands. Each command has an action label, a guard (a boolean expression over the module's local variables), and a probabilistic update (a probability distribution over assignments to the module's local variables). 

We say that a command is "enabled" in a state $s$ if its guard is satisfied by $s$.

For each action label $a$, we let the set of modules that have at least one command with action label $a$ be $M_a$. The rest of the modules are then $M_{\neg a}$. At any state, action $a$ is "enabled" if and only if for every module $m \in M_a$, there is at least one "enabled" command in $m$ with action label $a$. It is not allowed for a module to have more than one "enabled" command with the same action label in any (reachable) state. 

For a given state $s$, if $a$ is runs, then
- For each module $m \in M_a$, $m$ is updated based on the unique enabled command in $m$. For local variables that are not mentioned by the command's update, they remain unchanged.
- For each module $m in M_{\neg a}$, $m$ remains unchanged.

At each time-step/state, there can be multiple "enabled" actions. We assume that each of the "enabled" actions is chosen with equal probability, i.e. the next action is chosen *uniformly*.

As syntactic sugar, we allow commands without an action label. Such commands are assumed to have an implicit action label that is unique to the module and shared by all commands in the same module without an explicit action label. 

Compared to normal prism, we do not support global variables and require that the model have a unique initial state. For variables without an `init` value, the lowest value/false is the default initial value.

Lastly, if there are any states with no enabled actions, we call them "dead-end" states and add a self-loop with probability 1 to them. This is to ensure that the transition relation is total, i.e. the sum of probabilities of outgoing transitions from any state is 1.

### Property Semantics
We support the following numerical properties for DTMCs:
- `P=? [X phi]`: The probability that the next state satisfies `phi`. 
- `P=? [phi1 U<=k phi2]`: the probability that `phi2` is satisfied within the next `k` steps, and that `phi1` is satisfied at all preceding steps.
- `P=? [phi1 U phi2]`: the probability that `phi2` is eventually satisfied, and that `phi1` is satisfied at all preceding steps until then.

Apart from lines of properties, we also allow declaration of constants in the property file.

### Parsing and Semantic Analysis Implementation Notes
DTMCs are parsed by [parse_dtmc](../src/parser.rs) and represented by [DTMCAst](../src/ast.rs). We then pass them through semantic analysis to do type checking, constant folding, and desugaring of commands without action labels. This modifies the AST and produces [DTMCModelInfo](../src/analyze.rs) which is later used for model checking.

Specifically regarding desugarinig if module `M` has commands without action labels, we will desugar them into commands with the same unique action label `__M_action__`.

Properties are parsed by [parse_dtmc_props](../src/parser.rs) and represented by [ParsedProps](../src/parser.rs). We merge this together with the model AST within DTMCAst, also merging the declared constants, before passing it to semantic analysis. This allows us to do constant folding and resolution that incorporates constants from both the model and property files.

### Symbolic Representation
We represent the transition relation of a DTMC an algebraic decision diagram (ADD) over current state variables $S$ and next state variables $S'$. All the information for this is stored in [SymbolicDTMC](../src/symbolic_dtmc.rs).

We construct it compositionally from the commands and actions in the model. After which, we perform reachability analysis to filter out unreachable states and add self-loops to dead-end states (see [../src/reachability.rs](../src/reachability.rs) for details).

### Symbolic Model Checking

#### Next
For `P=? [X phi]`, we construct an ADD that maps each current state `s` to
`sum_{s'} P(s,s') * I_phi(s')`, where `I_phi` is the indicator of states satisfying
`phi`.

Implementation outline (see `src/sym_check.rs`):

```text
phi_bdd  := state_formula_to_bdd(phi)              // over current vars
phi_add  := bdd_to_add(phi_bdd)                    // 0/1 ADD over current vars
phi_next := swap_vars(phi_add, curr -> next)       // now over next vars

// Matrix-vector multiply over next-state variables:
// result(s) = sum_{s'} P(s,s') * phi_next(s')
result_add := add_matrix_multiply(transitions, phi_next, next_var_indices)

return evaluate_in_initial_state(result_add)
```

`evaluate_in_initial_state` evaluates the result ADD at the unique initial state.
The initial state BDD is precomputed during symbolic construction and stored in
`SymbolicDTMC::init`.

#### Bounded Until
For `P=? [phi1 U<=k phi2]`,

Let:
- `S_yes = { s | phi2(s) }`
- `S_question = { s | reachable(s) and phi1(s) and not phi2(s) }`

`S_question` is restricted to reachable states to avoid propagating probability
mass through unreachable encodings.

The recurrence encoded by the implementation is:

```text
V_0(s) = I_{S_yes}(s)
V_i(s) = I_{S_yes}(s)
         + I_{S_question}(s) * sum_{s'} P(s,s') * V_{i-1}(s')
```

Implementation outline (see `src/sym_check.rs`):

```text
s_yes_add      := bdd_to_add(phi2_bdd)
phi1_not_phi2  := phi1_bdd and (not phi2_bdd)
s_question     := reachable and phi1_not_phi2
s_question_add := bdd_to_add(s_question)

// Keep only transitions from S_question states
t_question := s_question_add * transitions

res := s_yes_add                               // V_0
for i in 1..=k:
    renamed := swap_vars(res, curr -> next)    // V_{i-1}(s')
    stepped := add_matrix_multiply(t_question, renamed, next_var_indices)
    res     := s_yes_add + stepped             // V_i

return evaluate_in_initial_state(res)
```

#### Unbounded Until
For `P=? [phi1 U phi2]`, we follow the standard decomposition into certainty
regions and an unknown region:

- `S_no`: probability 0 of satisfying `phi1 U phi2`
- `S_yes`: probability 1 of satisfying `phi1 U phi2`
- `S_question = reachable \ (S_no \cup S_yes)`

`S_no` and `S_yes` are computed with fixpoints over the filtered 0-1 transition
relation `T_01` (see `SymbolicDTMC::get_transitions_01`).

`prob0(phi1, phi2)`:

```text
sol := phi2
loop:
  sol' := sol OR (phi1 AND Exists_{next}(T_01 AND swap_curr_to_next(sol)))
  if sol' == sol: break
  sol := sol'

S_no := reachable AND (NOT sol)
```

`prob1(phi1, phi2, S_no)`:

```text
sol := S_no
loop:
  sol' := sol OR ((phi1 AND NOT phi2)
                  AND Exists_{next}(T_01 AND swap_curr_to_next(sol)))
  if sol' == sol: break
  sol := sol'

S_yes := reachable AND (NOT sol)
```

After obtaining `S_yes` and `S_question`, we solve only on `S_question`:

```text
A := I - P_question
b := I_{S_yes}

solve A x = b using Jacobi until sup-norm <= EPS
```

Where `P_question` is the transition ADD masked by `S_question` over current
state variables. This avoids spending iterations on regions already known to be
exactly 0 or 1.