{ self }:

{ config, lib, pkgs, ... }:

let
  cfg = config.services.clipperd;
in
{
  options.services.clipperd = {
    enable = lib.mkEnableOption "clipperd clipboard sync daemon";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.clipperd;
      defaultText = lib.literalExpression "clipperd.packages.\${system}.clipperd";
      description = "The clipperd package to use.";
    };

    autoStart = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Start clipperd automatically with the graphical session.
        Set to false to install the unit but start it on demand with
        `systemctl --user start clipperd`.
      '';
    };

    logLevel = lib.mkOption {
      type = lib.types.str;
      default = "clipperd=info,warn";
      description = "Value for the RUST_LOG environment variable.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    systemd.user.services.clipperd = {
      Unit = {
        Description = "Clipperd — iPhone clipboard sync daemon";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };

      Service = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/clipperd run";
        Restart = "on-failure";
        RestartSec = 5;
        Environment = [ "RUST_LOG=${cfg.logLevel}" ];

        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = "read-only";
        ReadWritePaths = [ "%h/.config/clipperd" "%h/Downloads" ];
      };

      Install = lib.mkIf cfg.autoStart {
        WantedBy = [ "graphical-session.target" ];
      };
    };
  };
}
