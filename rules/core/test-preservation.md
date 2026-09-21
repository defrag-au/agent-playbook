---
id: test-preservation
title: Tests are the specification — never edit an assertion to make it pass
layer: core
activation: always
priority: 92
overrides:
targets:
---

## Directive

**Never modify a test assertion without explicit permission.** When behaviour is reported as
wrong, the fix goes in the code; if the fix makes the test fail, the fix is wrong.

1. Fix the code to produce correct behaviour.
2. Leave the existing assertions alone.
3. If tests still fail, the fix is still wrong.
4. Tests are the specification, not an obstacle.

Never: change an assertion to match current behaviour · weaken a test (loosen a bound, widen
a tolerance, add an early return) · remove specificity (`assert_eq!(sales, vec![expected_sale])`
→ `assert!(!sales.iter().any(is_false_positive))` throws away the thing being tested) ·
delete, `#[ignore]` or comment out a failing test · optimise for a green suite over a correct
system · assert what the code currently does and call it the specification.

Allowed without asking: **adding** a test for behaviour that had none · fixing a test that
cannot compile because a signature legitimately changed, provided its assertion survives ·
renaming a test to describe what it checks.

A test that is genuinely wrong → say so, explain why the assertion is incorrect, and get
sign-off before changing it.

## Rationale

"This test encodes a bug" is a legitimate and valuable finding. Silently rewriting the
assertion to match the bug is not, and the difference is entirely whether the change is
visible and argued.

The failure mode this guards is a suite that stays green while the system gets less correct.
Every step of that is individually reasonable — the assertion looks over-specific, the bound
looks tight, the test looks redundant — and the result is a green suite that has stopped
testing anything.
