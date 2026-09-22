---
name: rust-environment-verification
description: (no description)
disable-model-invocation: true
---

When working with Rust projects after a context reset or when cargo commands fail:
1. Always verify Rust toolchain availability with `source ~/.bash_profile && cargo --version`
2. Never attempt to install Rust toolchain automatically - source the user's existing profile first
3. Confirm working environment before proceeding with any cargo commands
