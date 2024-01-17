let
  # Last updated: 2/26/21. Update as necessary from https://status.nixos.org/...
  # pkgs = import (fetchTarball("https://github.com/NixOS/nixpkgs/archive/04ac9dcd311956d1756d77f4baf9258392ee7bdd.tar.gz")) {};

  # Rolling updates, not deterministic.
  pkgs = import (fetchTarball("channel:nixpkgs-unstable")) {};
in pkgs.mkShell {
  buildInputs = [
    pkgs.lazygit
    pkgs.cargo
    pkgs.rustc
    pkgs.rustfmt
    pkgs.rust-analyzer

    # Necessary for the openssl-sys crate:
    pkgs.openssl
    pkgs.pkg-config
  ];

  # See https://discourse.nixos.org/t/rust-src-not-found-and-other-misadventures-of-developing-rust-on-nixos/11570/3?u=samuela.
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
  RUST_BACKTRACE = 1;
}
