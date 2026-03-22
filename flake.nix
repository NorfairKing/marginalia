{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    crane.url = "github:ipetkov/crane";
    pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
    tokei = {
      url = "github:XAMPPRocky/tokei";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, crane, pre-commit-hooks, tokei }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      craneLib = crane.mkLib pkgs;

      languagesJson = "${tokei}/languages.json";

      commonArgs = {
        src = ./.;
        strictDeps = true;

        MARGINALIA_LANGUAGES = languagesJson;

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = with pkgs; [
          libgit2
          openssl
          zlib
        ];
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      marginalia = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        doCheck = false;
      });

      pre-commit = pre-commit-hooks.lib.${system}.run {
        src = ./.;
        hooks = {
          marginalia = {
            enable = true;
            name = "marginalia";
            description = "Show [check] annotations near changed lines";
            package = marginalia;
            entry = "${marginalia}/bin/marginalia";
            language = "system";
            pass_filenames = false;
            stages = [ "pre-commit" ];
            verbose = true;
          };
        };
      };
    in
    {
      packages.${system}.default = marginalia;

      checks.${system} = {
        inherit pre-commit;

        build = marginalia;

        clippy = craneLib.cargoClippy (commonArgs // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        });

        tests = craneLib.cargoNextest (commonArgs // {
          inherit cargoArtifacts;
          nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.git ];
        });
      };

      devShells.${system}.default = craneLib.devShell {
        checks = self.checks.${system};

        MARGINALIA_LANGUAGES = languagesJson;

        packages = pre-commit.enabledPackages;

        shellHook = pre-commit.shellHook;
      };
    };
}
