---
id: no-committed-secrets
title: Never commit a secret
layer: core
activation: always
priority: 74
overrides:
targets:
---

No credential, token, private key, connection string or API key goes into the repository — not
in source, not in a test fixture, not in a config file, not in a commit message, not in a
comment explaining what the value used to be.

If a change needs a secret to run, the secret comes from the environment or a secrets manager,
and the repository holds only the *name* of the variable and an `.env.example` showing its
shape with an obvious placeholder.

```
# .env.example — committed
KOIOS_API_KEY=your-key-here

# .env — gitignored
KOIOS_API_KEY=<real value>
```

## When you need one to test something

Say so and stop. Do not invent a placeholder that looks real, and do not reach for a real key
that is already in the environment — a test that only passes with a live credential is not a
test, it is a scheduled failure.

## If you find one already committed

Report it. Do not quietly delete the line and move on: the value is in the history, so deleting
it from the working tree does not revoke it. **The secret has to be rotated**, and that is a
decision for me, not a cleanup you perform silently.

## The related failure

A secret pasted into a code comment or a `// TODO: replace before merge` is the same leak with
an expiry date that nobody enforces.
