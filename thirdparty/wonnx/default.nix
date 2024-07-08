{ pkgs, ... }:
pkgs.rustPlatform.buildRustPackage
rec {
  pname = "wonnx";
  version = "v0.5.1";

  nativeBuildInputs = with pkgs; [ pkg-config ];
  buildInputs = [ ] ++ pkgs.lib.optionals (pkgs.stdenv.isDarwin) (with pkgs; with darwin.apple_sdk.frameworks; [
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

  src = pkgs.fetchFromGitHub {
    owner = "webonnx";
    repo = pname;
    rev = version;
    hash = "sha256-1h9Sif7eDTouwFssEN8bPxFLGMakXLm0nM75tN2nnJ4=";
  };

  cargoHash = "sha256-tQ0mREfUG3gY+nPNg15BJB6SrvnP7cqCd4OZJvhyH1M=";
}
