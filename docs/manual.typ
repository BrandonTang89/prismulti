#import "@preview/codelst:2.0.2": sourcecode
#let title = "PRISM-rs Manual"
#let author ="Brandon"
#set document(title: title, author: author)
#set par(justify: true)
#set page(numbering: "1/1", number-align: right,)
#let tut(x) = [#block(x, stroke: blue, radius: 1em, inset: 1.5em, width: 100%)]
#let pblock(x) = [#block(x, stroke: rgb("#e6c5fc") + 0.03em, fill: rgb("#fbf5ff"), radius: 0.3em, inset: 1.5em, width: 100%)]
#let gblock(x) = [#block(x, stroke: rgb("#5eb575") + 0.03em, fill: rgb("#e3fae9"), radius: 0.3em, inset: 1.5em, width: 100%)]
#pdf.attach("manual.typ")
#align(center)[
  #text(weight: "bold", 1.75em, title)
  #v(1em, weak: true)
  #text(weight: "medium", 1.1em, author)
]

= Model Semantics
We informally describe the semantics of the PRISM models here for the sake of implementation.

For a more formal description, see the #link("https://www.prismmodelchecker.org/manual/")[#underline([prism manual])].

== DTMCs
DTMCs in PRISM are defined by a set of modules that execute in parallel. Each module has some local variables and a set of commands. Each command has an action label, a guard (a boolean expression over the module's local variables), and a probabilistic update (a probability distribution over assignments to the module's local variables). 

At each time-step, an action label is chosen *uniformly*. Then, all modules that have an enabled command with that action label are scheduled to execute. We require that at most one command is enabled in each module for each action label. If there are no enabled commands for the chosen action label, then the system remains in the same state.

For the simplicity of modeling, we allow commands without an action label. Such commands are assumed to have an implicit action label that is unique to the module and shared by all commands in the same module without an explicit action label. 

= Implementation Notes
== DTMCs
We will create unique action labels for each module for commands without explicit action labels. This way, we can treat all commands as having an action label when implementing the semantics.