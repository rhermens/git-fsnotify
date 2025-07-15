{
  description = "git-fsnotify";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, crane }:
    flake-utils.lib.eachDefaultSystem (system: 
      let 
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;
        git-fsnotify = craneLib.buildPackage {
          src = craneLib.cleanCargoSource ./.;
          nativeBuildInputs = [ pkgs.pkg-config pkgs.openssl ];
        };
      in
        {
        devShells.default = pkgs.mkShell {
          packages = [ pkgs.pkg-config pkgs.openssl ];
        };

        homeManagerModules.git-fsnotify = { config, lib, pkgs, ... }: {
          options = {
            services.git-fsnotify = {
              enable = lib.mkEnableOption "git-fsnotify";
              path = lib.mkOption {
                type = lib.types.path;
                default = lib.mkHomeDirPath "git-fsnotify";
                description = "Path to watch";
              };
            };
          };

          config = lib.mkIf config.services.git-fsnotify.enable {
            home.packages = [ git-fsnotify ];

            systemd.user.services.git-fsnotify = {
              Unit = {
                Description = "Git fsnotify service";
                After = [ "network-online.target" ];
                Wants = [ "network-online.target" ];
              };

              Install = {
                WantedBy = [ "default.target" ];
              };

              Service = {
                ExecStart = "${git-fsnotify}/bin/git-fsnotify --path ${config.services.git-fsnotify.path}";
                Restart = "on-failure";
              };
            };
          };
        };

        packages.default = git-fsnotify;
      });
}
