---
id: no-committed-secrets
title: Never commit a secret
layer: core
activation: always
priority: 74
overrides:
targets:
---

## Directive

- No credential, token, private key, connection string or API key in the repository — not in
  source, a test fixture, a config file, a commit message, or a comment explaining what the
  value used to be.
- A change that needs a secret gets it from the environment or a secrets manager. The
  repository holds the variable *name* and an `.env.example` with an obvious placeholder:

```
# .env.example — committed
KOIOS_API_KEY=your-key-here

# .env — gitignored
KOIOS_API_KEY=<real value>
```

- Need one to test something → say so and stop. Do not invent a placeholder that looks real,
  and do not reach for a real key already in the environment; a test that only passes with a
  live credential is not a test, it is a scheduled failure.
- Find one already committed → **report it**, do not quietly delete the line. It is in the
  history, so deleting it from the working tree does not revoke it. The secret has to be
  rotated, and that is my decision rather than a cleanup you perform silently.
- A secret in a code comment or a `// TODO: replace before merge` is the same leak with an
  expiry date that nobody enforces.

## Rationale

The reporting rule is the one most likely to be got wrong in a well-meaning way. Deleting the
line feels like fixing the problem and actually hides it: the value is still in the history,
still valid, and now nobody knows it needs rotating.
