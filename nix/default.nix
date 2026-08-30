{ pkgs ? import <nixpkgs> { } }:

let
  ferrisSaysSource = pkgs.callPackage ./ferris-says-source.nix { };
in
{
  ferris-says-source = ferrisSaysSource;
  ferris-says-blueprint = pkgs.callPackage ./ferris-says-blueprint.nix {
    inherit ferrisSaysSource;
  };
}
