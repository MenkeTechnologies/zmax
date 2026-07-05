# Language Injection

"Language injection" (JetBrains' term) is treating a region of one file as a
*different* language: CSS inside an HTML `<style>`, JavaScript inside `<script>`,
SQL inside a `db.Query("…")` string, and so on. zemacs resolves the effective
language at any position through tree-sitter **injection layers**, and a growing
set of editor features consult that instead of the file's top-level language.

This document is the matrix of what we detect, how it's wired, and how to add
more.

## The one detection primitive

Everything funnels through a single call:

```rust
// zemacs-view/src/document.rs
Document::language_config_at(&self, loader: &syntax::Loader, byte_pos: usize)
    -> Option<&LanguageConfiguration>
```

It finds the smallest tree-sitter injection layer containing `byte_pos` and
returns that layer's `LanguageConfiguration` (whose `.language_id` is the
grammar name, e.g. `"css"`, `"sql"`). With no syntax tree it falls back to the
document's root language. The loader comes from `editor.syn_loader.load()`.

```rust
let loader = editor.syn_loader.load();
let byte = text.char_to_byte(cursor);
let lang = doc.language_config_at(&loader, byte).map(|c| c.language_id.as_str());
if lang == Some("sql") { /* … */ }
```

Injection layers are built by tree-sitter from `runtime/queries/<host>/injections.scm`
plus the `sql`/`css`/… grammars. The engine supports static (`#set!
injection.language "css"`), dynamic (`@injection.language` captured from text —
markdown code fences, tagged-template tags), and predicate-gated
(`#match?`/`#any-of?`/`#eq?`) injection.

## The injection engine (config-driven rules)

Most detection is expressed as tree-sitter injection queries in
`runtime/queries/<host>/injections.scm` (structural: `<script>` → js, `sql`
tagged templates, `@Language("SQL")`, `.query()` call sites). Those are precise
but baked into grammar files and not end-user-editable.

On top of them there is a small **config-driven rule engine** (`zemacs-core/src/injection.rs`),
modelled on JetBrains' IntelliLang "Language Injections" registry. Rules live in
TOML and merge from two scopes on top of built-in defaults:

- built-in defaults (`injection::default_rules()`)
- global `~/.zemacs/injections.toml`
- project `.zemacs/injections.toml` (VCS-shareable, like JetBrains'
  `.idea/IntelliLang.xml`)

At grammar-compile time the engine expands the rules that apply to a host into
tree-sitter injection-query text and appends it to that host's `injections.scm`
(`Loader::injection_rules()` → `injection::generate()` in
`syntax.rs::compile_syntax_config`). Reusing tree-sitter for the actual matching
means engine-injected regions get highlighting for free and flow through the same
`language_config_at` path.

```toml
# ~/.zemacs/injections.toml or <project>/.zemacs/injections.toml
[[injection]]
language = "sql"            # guest language id
hosts    = ["python", "go"] # host grammars (omit = all supported)
content  = "(?i)^\\s*SELECT\\s+\\*"   # content trigger (regex; #match? semantics)
enabled  = true            # turn a rule off without deleting it

[[injection]]
language = "graphql"
hosts    = ["typescript"]
content  = "(?i)^\\s*(query|mutation)\\s"
```

Three triggers are generated:

- **`content`** — the tunable form of "sniffing": a `#match?` regex on a
  string's inner text. Built-in classifiers ship for **SQL, JSON, HTML/XML,
  GraphQL, and CSS** (`injection::{SQL,JSON,HTML,GRAPHQL,CSS}_CONTENT_REGEX`), each tuned
  against a positive+negative battery to keep false positives on
  prose/JS-object/placeholder strings low. Each is a normal `content` rule, so
  you can disable one (`enabled = false`) or add your own.
- **`methods` / `arg`** — the IntelliLang core trigger: inject the `arg`-th
  string argument of a call to one of `methods` (e.g. `db.query("…")`,
  `createNativeQuery("…")`, Go's `ExecContext(ctx, "…")` with `arg = 1`). The
  built-in SQL query-method sets (JDBC/JPA/Spring, ADO.NET/Dapper/EF, DB-API,
  `database/sql`/sqlx) ship as defaults.

```toml
[[injection]]
language = "sql"
hosts    = ["go"]
methods  = ["MyQuery", "MyExec"]   # inject the string arg of these calls
arg      = 0

[[injection]]
language = "json"
hint     = true    # `/* language=json */ "…"` comment hint forces json
```

- **`hint`** — the manual/JetBrains form: a `language=<lang>` /
  `@Language("<lang>")` comment immediately before a string injects `<lang>`
  (block-comment hosts). Built-in hint rules ship for ~15 common languages; the
  `:inject-language` command inserts the comment for you.

### Engine API and commands

The rule set and its operations are wrapped in `injection::InjectionEngine`
(`with_rules`/`load`/`generate`/`describe`/`rules`), owned by the syntax
`Loader`. Inspect it at runtime:

- **`:injections`** — list the active rules (language, trigger, hosts, enabled)
  in a scratch buffer.
- **`:injection-info`** — report the effective (possibly injected) language at
  the cursor.
- **`:edit-fragment`** — JetBrains "Edit Fragment": open the injected region at
  point in a fresh buffer of the **guest language**, so it gets that language's
  highlighting, completion, and LSP. `commands::injected_fragment_at` finds the
  region via the injection layer's tree
  (`syntax.tree_for_byte_range(byte,byte).root_node()` span).
- **`:apply-fragment`** — write the fragment buffer's text back into its host
  string, **escaping for the host delimiter** (`commands::escape_fragment`):
  backslash/quote/newline for `"`/`'` strings, backtick + `${` for JS/TS template
  literals; triple-quoted text blocks and Go raw strings are written verbatim.

- **`:inject-language <lang>`** — JetBrains "Inject language here": insert a
  `/* language=<lang> */` hint before the string at point (block-comment hosts),
  which the engine's `hint` trigger then injects. Persisted in source, so it
  survives reopen/VCS.

**Reloading rules:** editing `injections.toml` takes effect on `:config-reload`
(which rebuilds the whole syntax loader, re-reading the file) — no separate
command needed.

The default content-sniff and query-method rules live in `default_rules()`
(no longer hand-written in the grammar files). Only genuinely *structural* rules
stay in `injections.scm`: tagged templates (`` sql`…` ``), the `/* sql */`
comment hint, the Java `@Language("SQL")` annotation, HTML `<script>`/`<style>`
and inline attributes, `<script type>` differentiation. `annotation`/`tag`
triggers and `prefix`/`suffix` wrapping are the next increment.

### Design note: why not free-form sniffing?

Researching JetBrains (IntelliLang) confirmed it **does not sniff string content
to guess a language** — its engine is a registry of *injection places*
(class+method+arg, annotation, tag → language) with an optional `value-pattern`
regex that only *filters* an already-matched place. We follow the same shape: the
engine's real leverage is a user-editable, call-site/annotation-keyed rule table;
content matching is kept as an explicit, conservative, per-rule filter — never a
standalone "guess the language" pass.

## Feature consumers

Features that already switch on the injected language at the cursor:

| Feature | Where | Behaviour |
|---|---|---|
| Syntax highlighting | tree-sitter | Embedded languages highlight natively. |
| Emmet (Tab) | `commands.rs::try_emmet_expand` | CSS emmet in `<style>`, none in `<script>`. |
| **SQL completion** | `handlers/completion/sql.rs` | SQL keywords when the cursor is in an injected `sql` region. |
| Comment tokens / join-lines | `commands.rs` | Uses the injected layer's comment tokens. |
| Indentation | indent queries | Follows the injected language. |

Adding a consumer is just "call `language_config_at` at the cursor and branch."

## SQL injection (featured)

SQL strings are highlighted and offered SQL keyword completion. Signals by host:

| Host | Signals |
|---|---|
| **JavaScript / TypeScript** (`ecma`) | `` sql`…` `` tagged template · `/* sql */ "…"` comment hint · `.query()/.execute()/.prepare()/.raw()` methods |
| **Python** | `cursor.execute()/.executemany()/.executescript()/.execute_batch()` · SQLAlchemy `text("…")` |
| **Go** | `db.Query()/QueryRow()/Exec()/Prepare()/NamedExec()/NamedQuery()/MustExec()` (query = 1st arg) · `*Context()` variants (query = 2nd arg) |
| **Java** | JDBC/JPA/Spring method calls (`executeQuery/executeUpdate/prepareStatement/createQuery/createNativeQuery/query/queryForObject/queryForList/update/batchUpdate`, 1st arg) · **`@Language("SQL")`** annotation on a field or local variable |
| **C#** | ADO.NET/Dapper/EF Core: `ExecuteReader/ExecuteNonQuery/ExecuteScalar(+Async)/FromSqlRaw/FromSqlInterpolated/ExecuteSqlRaw/Query/QueryAsync/QueryFirst/…/Execute` (1st arg) |
| **Kotlin** | Android/JDBC: `rawQuery/execSQL/query/prepareStatement/createStatement/createQuery/createNativeQuery` (1st arg) |
| **Rust** | built-in grammar rules (macros/strings tagged sql) |
| **PHP**, **Ruby**, **Scala**, **Crystal**, **V**, **PRQL**, **Nix** | built-in grammar rules |

The completion source (`sql.rs`) is gated purely on `language_config_at ==
"sql"`, so it works for *every* host above with no per-host code. It is a static
keyword list, not a schema-aware SQL LSP.

### SQL content auto-detection (no tag/hint needed)

On top of the structural signals above, JS/TS, Python, Go, Java, C#, and Kotlin
**auto-detect SQL from the string's content** — any string (including multiline
text blocks / raw strings) whose text is a recognisable SQL statement is treated
as SQL with no tag, comment hint, or query method required.

The classifier is deliberately conservative to avoid false-positives on prose:

- **Unambiguous DML/DDL** (`INSERT INTO`, `UPDATE … SET`, `DELETE FROM`,
  `CREATE/ALTER/DROP TABLE`, `TRUNCATE TABLE`, `WITH … AS (`) matches outright —
  these essentially never appear in plain English.
- **`SELECT`** matches only with real SQL structure: `SELECT *`, a comma column
  list, or `SELECT … FROM …` **followed by a clause keyword**
  (`WHERE`/`JOIN`/`GROUP BY`/`ORDER BY`/`HAVING`/`LIMIT`/`UNION`). So
  `"select an option from the settings menu"` is **not** matched, while
  `"SELECT id, name FROM users WHERE …"` is.

This is now emitted by the **injection engine** (a built-in `content` rule with
`injection::SQL_CONTENT_REGEX`), not hand-written per grammar — so it can be
tuned or disabled in `injections.toml`. Bare `SELECT … FROM table` with no
clause is intentionally *not* auto-detected (too English-ambiguous); those are
still caught by the query-method/tag/`@Language` signals.

## Full cross-language matrix

Host → guest languages (excludes the universal `comment`/`regex` baseline; see
below). `+dyn` = language inferred dynamically from text.

### Web / templating
| Host | Injects |
|---|---|
| `html` | css (`<style>` + inline `style="…"`), javascript (`<script>` + inline `on*="…"`), json (`<script type="application/json"|"importmap"|"ld+json">`), typescript (`<script type="…typescript">`) |
| `ecma` (js/ts/jsx/tsx) | css, html, graphql, sql, bash `+dyn` |
| `vue` `+dyn`, `svelte` `+dyn` | css, scss, sass, less, javascript, typescript |
| `astro` | tsx, typescript |
| `glimmer`/`_gjs` `+dyn`, `qml`, `ripple`, `templ`, `vento`, `dot`, `pug`, `nearley`, `github-action` | css / javascript / typescript / html |
| `erb`, `eex`, `heex`, `ejs`, `embedded-perl`, `blade`, `php`, `twig`, `jinja`, `htmldjango`, `gotmpl`, `rshtml` | html + ruby/elixir/js/perl/php/rust |

### SQL
`ecma`, `python`, `go`, `java`, `c-sharp`, `kotlin`, `rust`, `php`, `ruby`, `scala`, `crystal`, `v`, `prql`, `nix`.

### GraphQL
`ecma`, `rescript`, `ruby`.

### Shell / bash
`bash`, `make`, `dockerfile`, `docker-bake`, `go`, `julia`, `ruby` (heredoc), `yaml`, `just`, `earthfile`, `gitlab-ci`, `github-action`, `hyprlang`, `tilt`, `woodpecker-ci`, `miseconfig`, `cross-config`, `git-rebase`, `git-cliff-config`.

### Markdown / docstrings (mostly `+dyn` code-fence language)
`markdown`, `markdown-rustdoc` (→ rust), and markdown-in-comments for `elixir`, `gleam`, `julia`, `amber`, `erlang`, `lean`, `nickel`, `pkl`, `unison`, `markdoc`.

### The everything-injector
`nix` injects ~24 languages (bash, c, clojure, css, fish, haskell, html, javascript, json, lua, nginx, nim, nu, perl, python, ruby, rust, scheme, sql, toml, typescript, xml, yaml).

### Other notable
`rust` → html/json/slint/rust-format-args/markdown-rustdoc/sql · `elixir` → heex/json/zig · `vim` → lua/python/ruby/vim · `caddyfile`/`spicedb` → cel · `nginx` → lua · `elm` → glsl · `fsharp` → xml · `hurl` → json/xml.

## Universal baseline (not real embedding)
- **`comment`** — ~150 grammars inject a pseudo-`comment` language to highlight `TODO`/`FIXME`/tags inside comments.
- **`regex`** — ~30 grammars inject `regex` into regex literals.

These are noise for language-injection purposes; consumers should ignore
`comment`/`regex`.

## Adding a new injection

1. Edit `runtime/queries/<host>/injections.scm` (grammars inherit via
   `; inherits: <base>` — e.g. JS/TS both inherit `ecma`).
2. Capture the region as `@injection.content` and set the guest language:
   `(#set! injection.language "sql")`, or capture `@injection.language`
   dynamically.
3. Add an injection test in `zemacs-term/tests/test/injection.rs` asserting
   `language_config_at` resolves the guest at the right byte (and does **not**
   over-match a plain string).

### Gotchas (learned the hard way)

- **Capture the inner content node, not the whole string literal.** Injecting a
  node that includes the delimiters can silently fail to register the layer.
  Right: `(raw_string_literal (raw_string_literal_content) @injection.content)`
  and `(string (string_content) @injection.content)` / `(template_string
  (string_fragment) @injection.content)`. Wrong: `(raw_string_literal)
  @injection.content`.
- **Match method calls on the full selector text**, mirroring the built-in
  `regexp` rule: `(call_expression (selector_expression) @_fn (#match? @_fn
  "\\.(Query|Exec)$") (argument_list . [...] @injection.content))`. This avoids
  fragile per-grammar field-name assumptions.
- **Anchor the argument position.** `. (string …)` = first argument;
  `(_) . (string …)` = second (e.g. Go `*Context(ctx, query)`).
- **`#match?` is a search, not a full match** (`^//go:generate` matches a longer
  comment). `#any-of?`/`#eq?` compare the whole captured text.
- Keep heuristics **low-false-positive**: prefer explicit signals (tagged tag,
  `/* sql */`, SQL-specific method names) over generic ones (`.Get`, `.query` on
  arbitrary objects) that would light up non-SQL strings.

To confirm a grammar's node names, add a temporary test that walks
`doc.syntax().descendant_for_byte_range(b, b)` up via `.parent()` and panics with
the `.kind()` chain.

## Gaps / roadmap
- **Engine triggers.** `content`, `methods`/`arg` (call-site), and `hint`
  (comment) are generated; the SQL defaults have migrated out of the grammar
  `.scm` files. Call-site matching is name+arg only (no receiver-type/import
  resolution) — most of IntelliLang's value without a type system. Still
  optional: a `tag`/attribute trigger and `prefix`/`suffix` fragment wrapping.
  The `:inject-language` *command* inserts a `/* … */` comment, so it targets
  block-comment hosts; Python hint *detection* works (a `# language=…` on the
  line above the assignment), but you write that comment by hand.
- **Content sniffers cover SQL, JSON, HTML/XML, GraphQL, CSS.** Regex classifiers
  are inherently heuristic — the built-ins are conservative (require distinctive
  structure: `{"key":` for JSON, a matching `</tag>` for HTML, an operation
  header + `{` for GraphQL, a `{ … : … }` block for CSS) but not perfect.
  `regex`/`YAML`/`TOML` are omitted: their content is too ambiguous to sniff
  safely. Tune or disable any of them in `injections.toml`.
- **Content sniffers run on the code-string hosts** (js/ts, python, go, java, c#,
  kotlin); Ruby/PHP/Rust/Scala use upstream structural rules only — a `hosts`
  override adds them.
- **SQL hosts not yet covered:** Dart (its `selector`/`argument_part` call
  grammar is awkward; low priority).
- **Generalize `@Language("…")`** — Java's annotation hint is wired for SQL only;
  it could inject any named language (JSON/HTML/RegExp), and Kotlin/C# have
  equivalent annotation/comment idioms not yet wired.
- **Injected LSP** — `injection::FragmentView` (guest language + host offset +
  text, with `to_host`/`from_host` mapping) is the built, unit-tested *foundation*
  — the single-range analogue of IntelliJ's `DocumentWindow`. `commands::injected_fragment_at`
  returns one. What remains is the **transport layer** on top: `didOpen` the
  fragment to the guest language server under a synthetic URI, translate the
  request position with `from_host`, and map results back with `to_host`. That
  needs zemacs-lsp client plumbing + a live server to verify, so it's the one
  piece not yet wired. In the meantime `:edit-fragment` gives the guest
  language's full LSP by opening a real buffer.
- **Comment-hint injection** (`// language=sql`) is only wired for JS/TS
  (`/* sql */`); generalizing it per host is straightforward but per-grammar.

HTML is now fully covered: `<style>`/`<script>`, inline `style="…"` and `on*="…"`
attributes, and `<script type>` differentiation (json / typescript / javascript).
