Pipeline (recommended)
	1.	Parse → AST (untyped)
	•	Exact reflection of source (keep tokens, Span, and NodeId).
	•	No cross-node pointers; keep it immutable and cheap to clone via arenas/IDs.
	2.	Desugar → HIR (typed-ready)
	•	Normalize syntactic sugar (e.g., else if, for → while, default params).
	•	Canonicalize method-ish sugar: string.length / a.contains(b) → explicit call form.
	•	HIR nodes carry DefId/SymbolId placeholders but are not typed yet.
	3.	Name/Module Resolution (on HIR)
	•	Build symbol tables + scope graph; assign DefId to each reference.
	•	Import/prelude injection here (see stdlib below).
	4.	Type Inference & Checking (TAST)
	•	Produce a Typed AST/HIR (TAST): every expr has a TypeId.
	•	Constraint solver/unifier; produce typed method targets (after lookup).
	•	Emit diagnostics with Span from original AST.
	5.	Codegen (e.g., WASM)
	•	MIR → target.
