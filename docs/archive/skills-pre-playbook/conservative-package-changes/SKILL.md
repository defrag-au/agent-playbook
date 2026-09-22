---
name: conservative-package-changes
description: (no description)
disable-model-invocation: true
---

CONSTRAINT: Before making any of the following changes, you MUST:
1. Present the problem clearly
2. Offer 2-3 specific options with pros/cons  
3. Explicitly ask "Which approach would you prefer?"
4. Wait for user response before implementing

Triggering changes:
- Modifying Cargo.toml, package.json, or other config files
- Switching frameworks/libraries (smlang -> rust-fsm, etc.)
- Replacing entire modules or implementations  
- Making architectural decisions (FSM patterns, database choices, etc.)
- Any change that affects the fundamental approach to solving a problem

Exception: Simple bug fixes and direct user requests ("change X to Y") don't require this process.
