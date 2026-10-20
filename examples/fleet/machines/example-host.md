id: example-host
role: client
os: linux
arch: x86_64
nix_system: x86_64-linux
ssh: example-host
tailscale: example-host
tailscale_ip: 100.64.0.10
tailscale_tag: tag:client
shell: /bin/zsh
sudo: required
repo_root: /home/user/code
repos: example-org/nix-config,example-org/shared-crates
rebuild_repo: example-org/nix-config
rebuild: home-manager switch --flake .#user@example-host
writes: none
notes: Example only; no real host data.
