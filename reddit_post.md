
TL;DR:
LLM-assisted coding (vibe coding / context engineering) improves local productivity,
but often collapses global structure.
I built a design-level IR (Surv IR) to make system architecture explicit *before*
implementation, so both humans and LLMs can reason about the same DAG.

---

LLM coding tools like Cursor, Claude Code, and Codex have dramatically improved
local productivity. Writing individual functions or modules has become easy.

But I keep running into one recurring problem: global structure.

Design decisions are usually made through sequential natural-language instructions,
and system-wide dependency structure is only inferred implicitly.
As projects grow, this makes it hard to reason about impact, consistency,
and implementation order.

To address this, I built **Surv IR**: a declarative, TOML-based design IR
for describing schemas, functions, and modules *before* implementation.
The goal is not to replace vibe coding, but to complement it by fixing
its weakest point: global structure.

Surv IR lets you:
- Declare schemas, functions, and module pipelines explicitly
- Validate that a coherent DAG exists at the design stage
- Trace dependencies mechanically (refs, slice, trace)
- Visualize execution flow before writing code

I wrote a detailed article explaining the motivation, concrete syntax,
tooling, and examples:

👉 **[Beyond Vibe Coding: Introducing Survibe](https://github.com/otaku46/Surv-IR/blob/main/note_weblog_english.md)**

---

**Project:** [github.com/otaku46/Surv-IR](https://github.com/otaku46/Surv-IR)
- `surc check` validates design coherence
- `surc slice`, `surc refs`, `surc trace` for dependency analysis
- Working examples in `/examples/`

---

I'm curious:
How do you currently manage global structure in LLM-heavy workflows?
Do you rely on specs, diagrams, refactors, or something else?
