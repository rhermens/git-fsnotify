{
    description = "fsnotify-git";

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
            in
        {
            devShells.default = pkgs.mkShell {
                packages = [ pkgs.pkg-config pkgs.openssl ];
            };

            packages.default = craneLib.buildPackage {
                src = craneLib.cleanCargoSource ./.;
                nativeBuildInputs = [ pkgs.pkg-config pkgs.openssl ];
            };
        });
}
