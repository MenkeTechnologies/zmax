; Highlights for the Rocq/Coq proof assistant.
; Written against lamg/tree-sitter-rocq @ 051e6cf (grammar name `coq`: the
; generated parser still exports `tree_sitter_coq` despite the repo rename).
;
; Note the shape of this grammar: `definition` / `theorem` / `inductive` / … are
; STRUCTURAL sentence nodes carrying `name:` / `type:` / `body:` fields, not
; keyword tokens. Capturing them wholesale (as the grammar's own bundled
; highlights.scm does) paints the whole sentence as a keyword. The vernacular
; keywords are anonymous tokens, so they are matched literally below.
;
; Ordering follows the helix convention: broad captures first, specific ones
; after — a later pattern wins over an earlier one on the same node.

; ------------------------------------------------------------ generic leaves
(identifier) @variable

(number) @constant.numeric.integer
(string) @string
(comment) @comment

; ---------------------------------------------------------------- vernacular
[
  "Definition"
  "Fixpoint"
  "CoFixpoint"
  "Inductive"
] @keyword.function

[
  "Theorem"
  "Lemma"
  "Corollary"
  "Proposition"
  "Remark"
] @keyword

[
  "Section"
  "End"
] @keyword.storage

[
  "Require"
  "Import"
  "Export"
  "From"
] @keyword.control.import

; ------------------------------------------------------------- term language
[
  "forall"
  "fun"
] @keyword.function

[
  "let"
  "in"
  "match"
  "with"
  "end"
] @keyword.control

[
  "as"
  "using"
] @keyword

; ------------------------------------------------------------------- proofs
; `proof` and `proof_terminator` are leaf tokens: the literal `Proof` opener and
; the `Qed` / `Defined` / `Admitted` / `Abort` closer.
(proof) @keyword
(proof_terminator) @keyword

[
  "Qed"
  "Defined"
  "Admitted"
  "Abort"
] @keyword

; `tactic` is a leaf token holding the tactic name — the verbs of a proof script.
(tactic) @function.builtin

[
  "apply"
  "assumption"
  "auto"
  "cbn"
  "cbv"
  "constructor"
  "destruct"
  "easy"
  "exact"
  "induction"
  "intro"
  "intros"
  "inversion"
  "now"
  "reflexivity"
  "rewrite"
  "simpl"
  "split"
  "subst"
  "trivial"
] @function.builtin

; ------------------------------------------------------------------ operators
[
  "->"
  "=>"
  "<-"
  ":="
  "*"
  "|"
  "!"
  "?"
] @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ";"
  ":"
  "."
] @punctuation.delimiter

; ------------------------------------------- names bound by each vernacular
; Last, so these win over the blanket `(identifier) @variable` above.
(require (qualid) @namespace)

(explicit_binder (identifier) @variable.parameter)
(implicit_binder (identifier) @variable.parameter)

(let_in name: (identifier) @variable)

(definition name: (identifier) @function)
(fixpoint name: (identifier) @function)
(cofixpoint name: (identifier) @function)
(theorem name: (identifier) @function)
(lemma name: (identifier) @function)

(inductive name: (identifier) @type)
(constructor_decl name: (identifier) @constructor)

(section name: (identifier) @namespace)
(end name: (identifier) @namespace)
