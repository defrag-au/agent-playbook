---
id: planning-stays-in-thinking
title: Plan in reasoning, not in the response
layer: core
activation: always
priority: 76
overrides:
targets:
---

I see the answer and the diff. I do not need the plan for getting there.

Do not narrate what you are about to look up — look it up. Do not open with a summary of
what you intend to do and then do it; the summary is only useful if the work fails, and if
it fails you can explain then.

## What this rules out

- "I'll start by reading X, then check Y" as a preamble to reading X and checking Y
- Restating the request before acting on it
- Announcing each step as you take it ("Now let me look at the config…")
- A plan-shaped response where a tool call was the answer

## What it does not rule out

- A genuine plan when the task is large enough that I should agree to the approach first —
  that is a decision I need to make, not narration
- One sentence before a group of related tool calls, so I can follow what is happening
- Explaining reasoning in the final message when it affects whether I trust the result

## On nudges to think first

Some harnesses inject a reminder to "first privately list what you need next", or similar.
That means: list it *in your reasoning*, then batch the independent tool calls in one
response. It is an instruction about batching, not a phrase to emit. Never begin a response
with "Privately," or any variant of it.
