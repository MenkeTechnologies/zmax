// tree-sitter grammar for strykelang (`.stk`). A pragmatic Perl-family grammar
// tuned for syntax highlighting rather than exhaustive parsing: it recognises
// the token categories that matter (sigil variables, strings, numbers,
// comments, keywords, sub/package names, calls, operators) with a permissive
// expression layer so real-world scripts parse with few ERRORs.

module.exports = grammar({
  name: 'stryke',

  extras: $ => [/[ \t\r\n]/, $.comment],

  word: $ => $.identifier,

  conflicts: $ => [
    [$.block, $.hash],
  ],

  rules: {
    source_file: $ => repeat($._statement),

    comment: _ => token(seq('#', /[^\n]*/)),

    _statement: $ => choice(
      $.package_statement,
      $.use_statement,
      $.subroutine_definition,
      $.if_statement,
      $.while_statement,
      $.for_statement,
      $.return_statement,
      $.control_statement,
      $.block,
      $.expression_statement,
      ';',
    ),

    package_statement: $ => seq(
      'package',
      field('name', $.identifier),
      choice($.block, ';'),
    ),

    use_statement: $ => seq(
      choice('use', 'no', 'require'),
      repeat($._primary),
      ';',
    ),

    subroutine_definition: $ => seq(
      'sub',
      field('name', $.identifier),
      optional($.prototype),
      $.block,
    ),

    prototype: _ => token(seq('(', /[^)]*/, ')')),

    if_statement: $ => seq(
      choice('if', 'unless'),
      $._paren_expr,
      $.block,
      repeat(seq('elsif', $._paren_expr, $.block)),
      optional(seq('else', $.block)),
    ),

    while_statement: $ => seq(choice('while', 'until'), $._paren_expr, $.block),

    for_statement: $ => seq(
      choice('for', 'foreach'),
      optional(seq(optional(choice('my', 'our', 'local')), $.variable)),
      '(',
      optional($._expression),
      ')',
      $.block,
    ),

    return_statement: $ => seq('return', optional($._expression), ';'),

    control_statement: $ => seq(choice('last', 'next', 'redo'), optional($.identifier), ';'),

    _paren_expr: $ => seq('(', $._expression, ')'),

    block: $ => seq('{', repeat($._statement), '}'),

    expression_statement: $ => seq(
      $._expression,
      optional($.statement_modifier),
      ';',
    ),

    statement_modifier: $ => seq(
      choice('if', 'unless', 'while', 'until', 'for', 'foreach'),
      $._expression,
    ),

    // ---- expressions ------------------------------------------------------

    _expression: $ => choice(
      $.assignment,
      $.binary_expression,
      $.unary_expression,
      $.ternary_expression,
      $._primary,
    ),

    assignment: $ => prec.right(1, seq(
      field('left', $._primary),
      field('operator', choice('=', '+=', '-=', '*=', '/=', '.=', '//=', '||=', '&&=', '%=', '**=', 'x=')),
      field('right', $._expression),
    )),

    ternary_expression: $ => prec.right(2, seq($._expression, '?', $._expression, ':', $._expression)),

    binary_expression: $ => {
      const table = [
        // stryke pipe / threading operators (not in Perl). From strykelang
        // token.rs: `|>` pipe-forward; `~>`/`~>>` thread-first/last; `~s>`/`~s>>`
        // streaming; `~p>`/`~p>>` parallel-chunk; `~d>`/`~d>>` distributed;
        // `->>` thread-last (arrow form); `~|>`, `||>`, `|then|` boundary markers.
        [2, choice(
          '|>', '~>', '~>>', '~s>', '~s>>', '~p>', '~p>>', '~d>', '~d>>',
          '->>', '~|>', '||>', '|then|',
        )],
        [3, choice('or', 'and', 'xor', '||', '&&', '//')],
        [4, choice('==', '!=', '<=>', '<', '>', '<=', '>=', 'eq', 'ne', 'cmp', 'lt', 'gt', 'le', 'ge')],
        [5, choice('=~', '!~')],
        [6, choice('+', '-', '.')],
        [7, choice('*', '/', '%', 'x')],
        [8, '**'],
        [9, choice('..', '...')],
      ];
      return choice(...table.map(([p, op]) =>
        prec.left(p, seq($._expression, field('operator', op), $._expression))));
    },

    unary_expression: $ => prec(10, seq(
      field('operator', choice('!', '-', '+', '\\', 'not', 'defined', 'ref', 'my', 'our', 'local')),
      $._expression,
    )),

    _primary: $ => choice(
      $.list_operator,
      $.function_call,
      $.method_call,
      $.variable,
      $.element_access,
      $.number,
      $.string,
      $.interpolated_string,
      $.command_string,
      $.qw_list,
      $.regex,
      $.substitution,
      $.array,
      $.hash,
      $.parenthesized_expression,
      $.identifier,
    ),

    // Perl-style list operators / builtins that take a list without parens:
    // `print $x, "y"`, `push @a, 1`, `map { … } @list`.
    builtin: _ => choice(
      'print', 'say', 'printf', 'sprintf', 'warn', 'die', 'push', 'pop',
      'shift', 'unshift', 'splice', 'join', 'split', 'map', 'grep', 'sort',
      'reverse', 'keys', 'values', 'each', 'exists', 'delete', 'defined',
      'scalar', 'wantarray', 'chomp', 'chop', 'bless', 'ref', 'open', 'close',
      'read', 'write', 'chdir', 'system', 'exec',
      'pmap', 'pgrep', 'pfor', 'psort', 'preduce',
    ),

    list_operator: $ => prec.right(0, seq(
      field('function', $.builtin),
      optional(seq(optional($.block), $._list)),
    )),

    parenthesized_expression: $ => seq('(', optional($._list), ')'),
    array: $ => seq('[', optional($._list), ']'),
    hash: $ => seq('{', optional($._list), '}'),

    _list: $ => repeat1(seq($._expression, optional(choice(',', '=>')))),

    function_call: $ => prec(11, seq(
      field('name', $.identifier),
      seq('(', optional($._list), ')'),
    )),

    method_call: $ => prec.left(12, seq(
      field('invocant', choice($.variable, $.identifier)),
      '->',
      field('method', $.identifier),
      optional(prec(13, seq('(', optional($._list), ')'))),
    )),

    element_access: $ => prec(12, seq(
      $.variable,
      repeat1(choice(
        seq('[', $._expression, ']'),
        seq('{', $._expression, '}'),
        seq('->', choice(seq('[', $._expression, ']'), seq('{', $._expression, '}'))),
      )),
    )),

    // ---- terminals --------------------------------------------------------

    variable: _ => token(choice(
      seq(choice('$', '@', '%', '&'), optional('#'), /[A-Za-z_][A-Za-z0-9_]*/),
      seq(choice('$', '@', '%', '&'), optional('#'), '{', /[A-Za-z_][A-Za-z0-9_]*/, '}'),
      seq('$', choice('_', '0', '!', '@', '/', '\\', '&', '`', "'", '.', ',', ';', '#', '$', /[1-9][0-9]*/)),
      seq('@', choice('_', 'ARGV', 'INC')),
      seq('$#', /[A-Za-z_][A-Za-z0-9_]*/),
    )),

    identifier: _ => /[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*/,

    number: _ => token(choice(
      /0[xX][0-9a-fA-F_]+/,
      /0[bB][01_]+/,
      /[0-9][0-9_]*(\.[0-9_]+)?([eE][-+]?[0-9]+)?/,
      /\.[0-9_]+([eE][-+]?[0-9]+)?/,
    )),

    string: _ => token(seq("'", repeat(choice(/[^'\\]/, /\\./)), "'")),
    interpolated_string: _ => token(seq('"', repeat(choice(/[^"\\]/, /\\./)), '"')),
    command_string: _ => token(seq('`', repeat(choice(/[^`\\]/, /\\./)), '`')),

    qw_list: _ => token(choice(
      seq('qw(', /[^)]*/, ')'),
      seq('qw{', /[^}]*/, '}'),
      seq('qw[', /[^\]]*/, ']'),
      seq('qw/', /[^/]*/, '/'),
    )),

    regex: _ => token(choice(
      seq('m/', repeat(choice(/[^/\\]/, /\\./)), '/', /[a-z]*/),
      seq('qr/', repeat(choice(/[^/\\]/, /\\./)), '/', /[a-z]*/),
    )),

    substitution: _ => token(choice(
      seq('s/', repeat(choice(/[^/\\]/, /\\./)), '/', repeat(choice(/[^/\\]/, /\\./)), '/', /[a-z]*/),
      seq('tr/', repeat(choice(/[^/\\]/, /\\./)), '/', repeat(choice(/[^/\\]/, /\\./)), '/', /[a-z]*/),
      seq('y/', repeat(choice(/[^/\\]/, /\\./)), '/', repeat(choice(/[^/\\]/, /\\./)), '/', /[a-z]*/),
    )),
  },
});
