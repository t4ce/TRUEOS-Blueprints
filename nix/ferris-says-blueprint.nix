{
  ferrisSaysSource,
  lib,
  stdenvNoCC,
}:

stdenvNoCC.mkDerivation {
  pname = "trueos-ferris-says-blueprint";
  version = "0.1.0";

  src = ../apps/ferris-says-nix/dist/ferris-says-nix.bp;
  dontUnpack = true;

  installPhase = ''
    runHook preInstall
    install -Dm0444 "$src" "$out/share/trueos/blueprints/ferris-says-nix.bp"
    runHook postInstall
  '';

  passthru = {
    targetConfig = "x86_64-unknown-trueos";
    blueprintSelector = "ferris-says-nix";
    upstream = {
      repository = "https://github.com/rust-lang/ferris-says";
      source = ferrisSaysSource;
      inherit (ferrisSaysSource) outputHash rev;
    };
  };

  meta = {
    description = "Pinned ferris-says GitHub package compiled as a TRUEOS Blueprint";
    homepage = "https://github.com/rust-lang/ferris-says";
    license = with lib.licenses; [
      asl20
      mit
    ];
    platforms = lib.platforms.linux;
  };
}
