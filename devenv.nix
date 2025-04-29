{
  pkgs,
  config,
  lib,
  ...
}:
let
  cfg = config.custom;
in
{
  config = {
    custom = {
      common.project_name = "rustify";
      rust.enable = true;
    };
  };
}
