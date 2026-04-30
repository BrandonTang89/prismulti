
## Markov Decision Processes (MDPs)
### Model Structure and Semantics
MDPs are single-agent models where at every state, the agent selects one of the enabled actions, and the system transitions to the next state based on the probabilistic update of that action.

Like in DTMCs, a MDP model consists of modules, each with local variables and commands. Each command contains a single action label, a guard, and a probabilistic update. 

Each action label corresponds to a group of agent-actions that can be taken. We let $\overline{M}_a$ be the set of modules that have at least one command with action label $a$. At each state $s$, the agent can choose any action $a$ such that for every module $m \in \overline{M}_a$, there is at least one enabled command in $m$ with action label $a$. For each $m \in \overline{M}_a$, it can choose which enabled command in $m$ with action label $a$ to execute. The update is then the combination of the updates of the chosen commands in each module. For modules $m \notin \overline{M}_a$, that do not have any command with action label $a$, they remain unchanged.

As syntactic sugar, we allow commands without an action label. Such commands are assumed to have an implicit action label that is unique to the module and shared by all commands in the same module without an explicit action label.

States with no available actions are called "dead-end" states. We add a self-loop with probability 1 to each dead-end state to ensure that the transition relation is total.

### Properties
We support the following types of properties for MDPs:
```
Pmax=? [psi]                [Maximum probability of satisfying psi]
Pmin=? [psi]                [Minimum probability of satisfying psi]

psi := X phi                [Next state satisfies phi]
     | phi_1 U phi_2        [phi_2 is eventually satisfied, 
                             and phi_1 is satisfied until then]
     | phi_1 U<=k phi_2     [phi_2 is satisfied within the next k steps, 
                             and phi_1 is satisfied until then]
     | G phi                [G phi = !F !phi, i.e. phi is always satisfied]
     | F phi                [F phi = true U phi, i.e. phi is eventually satisfied]
```
where `phi` are expressions over state variables.

### Algorithms
