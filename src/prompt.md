
We now move onto parsing a small set of properties.

We are interested in properties of the form

`P=? [psi]` and `R=? [psi]` i.e. prob_query and reward_query

where `psi` is a path formula of the form 
- `psi` := `phi1 U phi2` | `X phi` | `phi1 U<=k phi2` | `F phi`

`F phi` is simply a shorthand for `true U phi` so should be parsed into the same AST node as `phi1 U phi2` with `phi1` set to `true`.

`X phi` is the next operator, `phi1 U phi2` is the until operator, and `phi1 U<=k phi2` is the bounded until operator.

For now, `phi` is just an expression over state variables.

Implement these in the ast.rs file as relevant enums, and the implement the parsing of property files such as knuth_die.prop and knuth_two_dice.prop. We should allow both property declarations and constant declarations.

Expose the relevant new parsing functionality which should give us a `DTMCProps` struct as a result of parsing a property file.

Within analyze.rs, perform type checking and constant folding on the parsed properties, similar to what we do for the DTMC model. We should ensure that `phi` is a boolean expression over state variables and that all constants used are actually defined in the constant overrides provided. The same constant overrides are used for overriding both constants in the model and in the property file.

The intended final result of this is that we can do 

`cargo run -- --model tests/dtmc/knuth_two_dice.prism --model-type dtmc --props tests/dtmc/knuth_two_dice.prop --const x=5` 

We don't worry about implementing the actual model checking for now, just print the parsed and type-checked property to the console to show that we have successfully parsed and type-checked it. Doing this as debugging output for now is okay.

Add relevant parsing tests to tests/parser_tests.rs for both knuth_two_dice and knuth_die property files.