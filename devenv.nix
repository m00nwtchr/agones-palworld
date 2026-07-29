{
  pkgs,
  lib,
  config,
  inputs,
  ...
}: {
  # https://devenv.sh/packages/
  packages = with pkgs; [
    git
    shellcheck
    kubernetes-helm
    kubectl
    kubectx
    cargo-audit
    cargo-deny
    cargo-nextest
    cargo-watch
    rust-analyzer
    protobuf
  ];

  # https://devenv.sh/languages/
  languages.rust = {
    enable = true;
    channel = "stable";
  };

  # https://devenv.sh/scripts/

  # https://devenv.sh/tests/

  # https://devenv.sh/git-hooks/
  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };
}
