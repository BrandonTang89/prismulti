I'm refactoring the code base to support more model types generically via:

```rust
#[derive(Clone, Debug)]
pub struct Ast<M: ModelKind> {
    pub basic: BasicAst,
    pub model: M,
    pub properties: Vec<M::Property>,
}

pub trait ModelKind {
    type Property: Clone + std::fmt::Debug + std::fmt::Display;
}

#[derive(Clone, Debug, Default)]
pub struct Dtmc;

impl ModelKind for Dtmc {
    type Property = DTMCProperty;
}

#[derive(Clone, Debug, Default)]
pub struct Mdp;

impl ModelKind for Mdp {
    type Property = MDPProperty;
}

pub type DTMCAst = Ast<Dtmc>;
pub type MDPAst = Ast<Mdp>;
```

Help me to ensure that the rest of the code base is adapted to this new structure everywhere.

You will also need to implement new parsing logic to be able to parse MDP properties:

```rust
#[derive(Clone, Debug)]
pub enum MDPProperty {
    MaxProbQuery(PathFormula),
    MinProbQuery(PathFormula),
    MaxRewardQuery(PathFormula),
    MinRewardQuery(PathFormula),
}
```

For now, we can ignore parsing of any reward properties, just leave a note there that they are not yet supported. 

The only properties that we want to support as of now are of the form:

```
Pmax=? [psi]
Pmin=? [psi]

psi := X phi                
     | phi_1 U phi_2
     | phi_1 U<=k phi_2
     | phi_1 R phi_2
     | phi_1 R<=k phi_2
     | G<=k phi
     | G phi           
     | F(<=k) phi
     | F phi
```
where `phi` are expressions over state variables.

G and F are just syntactic sugar:
- `Pmax=? [G phi]` is the same as `Pmax=? [phi R false]`
- `Pmax=? [G<=k phi]` is the same as `Pmax=? [phi R<=k false]`
- `Pmax=? [F phi]` is the same as `Pmax=? [true U phi]`
- `Pmax=? [F<=k phi]` is the same as `Pmax=? [true U<=k phi]`

We previously did not have any support for the release operator, so you will need to implement parsing for the release operator and relevant syntactic sugar (G).

Ensure that all the DTMC tests continue to pass and ensure that we can successfully parse and analyze the MDP models in tests/mdp.

Don't worry about implementing model checking for now. This is not important yet.