{
  description = "Audio converter for BigLinux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "big-audio-converter";
        version = "0.1.0";
        src = ../..;
        cargoLock.lockFile = ../../Cargo.lock;
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.gtk4 pkgs.libadwaita pkgs.mpv pkgs.ffmpeg ];
        postInstall = ''
          install -Dm644 resources/applications/br.com.biglinux.AudioConverter.desktop "$out/share/applications/br.com.biglinux.AudioConverter.desktop"
          install -Dm644 resources/metainfo/br.com.biglinux.AudioConverter.metainfo.xml "$out/share/metainfo/br.com.biglinux.AudioConverter.metainfo.xml"
          install -Dm644 resources/icons/hicolor/scalable/apps/big-audio-converter.svg "$out/share/icons/hicolor/scalable/apps/big-audio-converter.svg"
        '';
      };
    };
}
