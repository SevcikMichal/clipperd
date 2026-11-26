{ lib
, rustPlatform
, makeWrapper
, libnotify
, cmake
, pkg-config
, wayland
}:

rustPlatform.buildRustPackage {
  pname = "clipperd";

  version = (fromTOML (builtins.readFile ../Cargo.toml)).package.version;

  src = ../.;

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ makeWrapper cmake pkg-config ];
  buildInputs = [ wayland ];

  postInstall = ''
    wrapProgram $out/bin/clipperd \
      --prefix PATH : ${lib.makeBinPath [ libnotify ]}
  '';

  meta = with lib; {
    description = "Seamless iPhone ↔ Linux clipboard sync over your LAN";
    homepage = "https://github.com/SevcikMichal/clipperd";
    license = licenses.mit;
    mainProgram = "clipperd";
    platforms = platforms.linux;
  };
}
