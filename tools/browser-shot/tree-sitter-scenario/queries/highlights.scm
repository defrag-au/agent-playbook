; Actions
[
  "run"
  "peer"
  "wait"
  "eval"
  "key"
  "pointer"
  "capture"
  "assert"
  "network"
] @keyword

; Fields and their values
(field key: (key_name) @property)
(field value: (value (string) @string))
(field value: (value (bare_value) @string))

; Positional values
(name) @variable
(code) @constant
(flag) @keyword
(key_state) @constant.builtin
(pointer_kind) @constant.builtin
(expression (string) @string.special)
(expression (bare_expression) @string.special)

; Comments
(comment) @comment
