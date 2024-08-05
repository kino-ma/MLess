{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, fenix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ fenix.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        rust-components = with pkgs.fenix; combine [
          default.rustc
          default.cargo
          default.rust-std
          default.rust-docs
          default.rustfmt-preview
          default.clippy-preview
          latest.rust-src
        ];
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rust-components;
          rustc = rust-components;
        };

        wonnx = pkgs.callPackage ./thirdparty/wonnx/default.nix { };
        ultralytics = pkgs.callPackage ./thirdparty/ultralytics/default.nix { };

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

      in
      {
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs;
            [
              llvmPackages.libclang.lib
              clang
              bison
              flex
              fontforge
              makeWrapper
              pkg-config
              gnumake
              gcc
              libiconv
              autoconf
              automake
              libtool

              rust-components
              rust-analyzer-nightly
              protobuf
              iconv
              grpcurl
              pkg-config
              python311
              python311Packages.grpcio-tools
              python311Packages.onnx
              python311Packages.onnxruntime
              jq
              gnuplot

              cargo-generate
              wasm-pack
              wonnx
              (builtins.trace ultralytics.outPath ultralytics)
            ]
            ++ pkgs.lib.optionals (system == "x86_64-linux") [ pkgs.radeontop ]
            ++ pkgs.lib.optionals (pkgs.stdenv.isDarwin) (with pkgs; with darwin.apple_sdk.frameworks; [
              llvmPackages.libcxxStdenv
              llvmPackages.libcxxClang
              llvmPackages.libcxx
              darwin.libobjc
              darwin.libiconv
              libiconv
              Security
              SystemConfiguration
              AppKit
              WebKit
              CoreFoundation
            ]);


          RUST_SRC_PATH = "${pkgs.fenix.complete.rust-src}/lib/rustlib/src/rust/";
          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.stdenv.cc.cc pkgs.libclang ];
        };

        packages = rec {
          mless = rustPlatform.buildRustPackage
            {
              inherit (cargoToml.package) name version;
              src = ./.;

              cargoLock.lockFile = ./Cargo.lock;

              # Inputs for both of build&runtime environment
              nativeBuildInputs = with pkgs; [ libclang libclang.lib clang protobuf pkg-config ];
              buildInputs = with pkgs; [ stdenv.cc.cc stdenv.cc.cc.lib lld ];

              RUST_SRC_PATH = "${pkgs.fenix.complete.rust-src}/lib/rustlib/src/rust/";
              PROTOC = "${pkgs.protobuf}/bin/protoc";
              LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.stdenv.cc.cc ];
              CLANG_PATH = "${pkgs.clang}/bin/clang";
            };

          default = mless;
        };

        apps = rec {
          mless = {
            type = "app";
            program = "${self.packages.${system}.mless}/bin/mless";
          };

          default = mless;
        };
      });
}
