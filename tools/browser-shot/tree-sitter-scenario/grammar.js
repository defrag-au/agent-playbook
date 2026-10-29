/**
 * Tree-sitter grammar for the browser-shot scenario DSL.
 *
 * Canonical files are produced by `browser-shot fmt`: values containing `#` are
 * quoted, so `#` can be an ordinary comment token here.
 */
module.exports = grammar({
  name: 'browser_shot_scenario',

  extras: $ => [$.comment, /[ \t]+/],

  rules: {
    source_file: $ => repeat(choice(
      $.run,
      $.peer,
      $.wait,
      $.sleep,
      $.eval,
      $.key,
      $.pointer,
      $.capture,
      $.burst,
      $.assert,
      $.network,
      $.spawn,
      $.wait_for,
      $.goto,
      $._blank,
    )),

    _blank: $ => $._newline,
    _newline: _ => /\r?\n/,
    _line_end: $ => $._newline,

    comment: _ => token(/#.*/),

    run: $ => seq('run', repeat($.field), $._line_end),
    peer: $ => seq('peer', $.name, repeat(choice($.flag, $.field)), $._line_end),
    wait: $ => seq('wait', $.peer_list, repeat($.field), $._line_end),
    sleep: $ => seq('sleep', $.name, $.value, $._line_end),
    eval: $ => seq('eval', $.name, $.expression, repeat(choice($.flag, $.field)), $._line_end),
    key: $ => seq('key', $.name, $.code, $.key_state, repeat($.field), $._line_end),
    pointer: $ => seq('pointer', $.name, $.pointer_kind, repeat($.field), $._line_end),
    capture: $ => seq('capture', $.name, repeat($.field), $._line_end),
    burst: $ => seq('burst', $.name, repeat($.field), $._line_end),
    assert: $ => seq('assert', $.name, $.expression, repeat($.field), $._line_end),
    network: $ => seq('network', $.name, repeat($.field), $._line_end),
    spawn: $ => seq('spawn', $.name, repeat($.field), $._line_end),
    wait_for: $ => seq('wait_for', repeat($.field), $._line_end),
    goto: $ => seq('goto', $.name, $.expression, repeat($.field), $._line_end),

    field: $ => seq(field('key', $.key_name), '=', field('value', $.value)),
    key_name: _ => /[A-Za-z_][A-Za-z0-9_]*/,
    value: $ => choice($.string, $.bare_value),
    string: _ => choice(/"(\\.|[^"\\])*"/, /'(\\.|[^'\\])*'/),
    bare_value: _ => /[^\s"'=#]+/,

    name: $ => choice($.bare_name, $.string),
    bare_name: _ => /[^\s,"'=#]+/,

    expression: $ => choice($.string, $.bare_expression),
    bare_expression: _ => /[^\s=#]+/,

    peer_list: $ => seq($.name, repeat(seq(',', $.name))),
    code: _ => /[A-Za-z0-9]+/,

    flag: _ => choice('touch', 'headed', 'await'),
    key_state: _ => choice('down', 'up'),
    pointer_kind: _ => choice('click', 'hover', 'drag', 'tap', 'swipe'),
  },
});
