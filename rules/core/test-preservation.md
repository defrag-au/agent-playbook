---
id: test-preservation
title: Tests are the specification — never edit an assertion to make it pass
layer: core
activation: always
priority: 92
overrides:
targets:
---

**Never modify a test assertion without explicit permission.**

When behaviour is reported as wrong, the fix goes in the code. The test says what the system
is supposed to do; if the fix makes the test fail, the fix is wrong.

1. Fix the code to produce correct behaviour.
2. Leave the existing assertions alone.
3. If tests still fail, the fix is still wrong.
4. Tests are the specification, not an obstacle.

## Never

- Change an assertion so it matches the behaviour the code currently has
- Weaken a test when fixing a bug — loosening a bound, widening a tolerance, adding an early
  return
- Remove specificity: `assert_eq!(sales, vec![expected_sale])` becoming
  `assert!(!sales.iter().any(|s| s.is_false_positive()))` throws away the thing being tested
- Delete, `#[ignore]`, or comment out a failing test
- Optimise for "the suite is green" instead of "the system is correct"
- Add a test that asserts what the code currently does, then call the behaviour specified

## Permitted without asking

- **Adding** a test for behaviour that had none
- Fixing a test that cannot compile because a signature legitimately changed — but the
  assertion it makes must survive the change
- Renaming a test to describe what it checks

## If the test is genuinely wrong

Say so explicitly, explain why the assertion is incorrect, and **get sign-off before
changing it**. "This test encodes a bug" is a legitimate finding. Silently rewriting the
assertion to match the bug is not. The difference is whether the change is visible and
argued.
