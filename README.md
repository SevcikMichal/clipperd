# Clipperd

Sync text, images, URLs, and files between an iPhone and Linux using iOS Shortcuts.
Clipperd runs locally and uses HTTPS. No account or cloud service is required.

## Requirements

- Linux with Wayland (`wl-data-control`) or X11
- iPhone with iOS 13 or later and the Shortcuts app
- Rust and Cargo to build from source
- `notify-send` for optional desktop notifications

## Install

```bash
git clone https://github.com/SevcikMichal/clipperd
cd clipperd
cargo install --locked --path .
```

Make sure `~/.cargo/bin` is in your `PATH`.

### NixOS and Home Manager

To try a checkout:

```bash
nix run . -- --version
nix run . -- setup
```

Setup uses your existing configuration in `~/.config/clipperd`, if present.
Git-backed flakes include tracked files only.

For regular use, add the flake input:

```nix
inputs.clipperd.url = "github:SevcikMichal/clipperd";
```

In your Home Manager configuration:

```nix
{
  imports = [ inputs.clipperd.homeManagerModules.default ];
  services.clipperd.enable = true;
  # Optional: start the service manually instead of at login.
  # services.clipperd.autoStart = false;
}
```

Pass `inputs` to Home Manager with `extraSpecialArgs`, or
`home-manager.extraSpecialArgs` when using the NixOS module.
For a local checkout, use `git+file:///home/YOUR_USER/path/to/clipperd` as
the input URL, then run `nix flake update clipperd` in your configuration.

## Pair your iPhone

Stop the daemon before pairing, since setup uses the same HTTPS port:

```bash
systemctl --user stop clipperd
clipperd setup
```

On the phone:

1. Open the HTTP setup URL shown in the terminal and download the CA profile.
2. Verify the downloaded profile as described below, then install it.
3. In **Settings > General > About > Certificate Trust Settings**, trust the Clipperd CA.
4. Scan the HTTPS pairing QR code shown in the terminal and tap **Pair device**.
   You can also open the HTTPS pairing URL and enter the code manually.
5. Follow the instructions to create the two Shortcuts.

Stop setup with Ctrl+C when finished. The pairing code is valid for 10 minutes
and can be used once. Keep the pairing QR code and your Shortcuts private:
the Shortcuts contain your authentication token.

The certificate profile is downloaded over HTTP and can be altered in transit.
Only use setup on a trusted network. Verify the actual downloaded profile
independently before installing it; comparing the HTTP page's fingerprint to
the terminal does not verify the profile. The terminal fingerprint is the first
16 bytes of the DER-encoded certificate's BLAKE3 hash, not a standard SHA-256 fingerprint.
iOS may not display a comparable fingerprint, so verification may require
external tools. Review the profile for unexpected contents as well.

By default, Clipperd uses your LAN address. To use a specific local interface,
such as a Tailscale IP, run `clipperd setup --bind-ip <IP>` on first setup.
Changing the binding on an existing configuration requires removing the config
and running setup again. This generates new credentials and requires re-pairing.
An existing config keeps its saved port; `setup --port` only applies to a new config.

## Run

```bash
clipperd run
```

For the Home Manager service:

```bash
mkdir -p ~/Downloads
systemctl --user start clipperd
systemctl --user status clipperd
```

The service starts with your graphical session by default, after setup has
created a config. To stop it, run `systemctl --user stop clipperd`. For logs,
run `journalctl --user -u clipperd -b`. The Downloads directory must exist
before the service starts if you want to receive files.

To install the included systemd user service without Home Manager:

```bash
mkdir -p ~/.config/systemd/user ~/Downloads
cp systemd/clipperd.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now clipperd
```

You can assign the Shortcuts to Back Tap under **Settings > Accessibility >
Touch > Back Tap**.

## Network and security

- The daemon uses HTTPS on port 7171 by default.
- Setup uses HTTPS on the same port and HTTP on the next port for the CA profile.
- The HTTP page does not serve the authentication token. Verify the downloaded
  profile before trusting the certificate, then enter the pairing code over HTTPS.
- Firewall rules are not configured automatically. Allow the ports on the
  intended interface and restrict access to trusted devices or networks.
  The HTTP setup port is only needed during pairing.
- `--bind-all` listens on all IPv4 interfaces. Use it only when needed.
- The config and keys are stored in `~/.config/clipperd/config.toml` with mode 0600.

## Commands

```text
clipperd setup              Generate credentials and start pairing
clipperd setup --port PORT  Set the HTTPS port
clipperd setup --bind-ip IP Bind to a specific local address
clipperd setup --bind-all   Bind to all IPv4 interfaces
clipperd run                Run the daemon
clipperd status             Show configuration and certificate information
```

## License

MIT
