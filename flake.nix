{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    nix-fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nix-bochsym = {
      url = "github:luis-hebendanz/bochsym";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, nix-bochsym, utils, nix-fenix, naersk }:
    utils.lib.eachDefaultSystem (system:
      let
        overlays = [ nix-fenix.overlay ];
        pkgs = import nixpkgs { inherit system; inherit overlays; };
        naersk-lib = pkgs.callPackage naersk { };
        bochsym = nix-bochsym.packages.${system}.default;
        fenix = nix-fenix.packages.${system};
        target64 = fenix.targets."x86_64-unknown-none".latest.withComponents [
          "rust-std"
        ];
        myrust = with fenix; fenix.combine [
          (latest.withComponents [
            "rust-src"
            "rustc"
            "rustfmt"
            "llvm-tools-preview"
            "cargo"
            "clippy"
          ])
          target64
        ];

        buildDeps = with pkgs; [
          bochsym
          myrust
          zlib.out
          xorriso
          grub2
        ]  ++ (with pkgs.llvmPackages_latest; [
          lld
          llvm
        ]);
      in
      rec {
        packages.default = naersk-lib.buildPackage {
          src = ./.;
          postInstall = ''
            ln -s $out/bin/glue_gun $out/bin/gg
          '';
        };

        defaultPackage = packages.default;

        apps.default = utils.lib.mkApp {
          drv = self.defaultPackage."${system}";
        };

        devShell = with pkgs; mkShell {
          buildInputs = buildDeps;
          RUST_SRC_PATH = rustPlatform.rustLibSrc;
          shellHook = ''
          unalias gg
          export PATH=$PATH:~/.cargo/bin
          '';
        };
      });
}
