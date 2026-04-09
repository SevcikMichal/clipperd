use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rustls_pemfile::certs;

pub fn generate_mobileconfig(ca_cert_pem: &str) -> anyhow::Result<String> {
    let mut reader = std::io::BufReader::new(ca_cert_pem.as_bytes());
    let der_certs: Vec<_> = certs(&mut reader).collect::<Result<_, _>>()?;
    let der = der_certs.first().ok_or_else(|| anyhow::anyhow!("No cert in PEM"))?;
    let cert_b64 = BASE64.encode(der.as_ref());

    let uuid = generate_uuid();
    let profile_uuid = generate_uuid();

    Ok(format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>PayloadContent</key>
    <array>
        <dict>
            <key>PayloadCertificateFileName</key>
            <string>clipperd-ca.cer</string>
            <key>PayloadContent</key>
            <data>{cert_b64}</data>
            <key>PayloadDescription</key>
            <string>Clipperd CA Certificate — allows your iPhone to securely connect to your Linux machine</string>
            <key>PayloadDisplayName</key>
            <string>Clipperd CA</string>
            <key>PayloadIdentifier</key>
            <string>com.clipperd.ca.{uuid}</string>
            <key>PayloadType</key>
            <string>com.apple.security.root</string>
            <key>PayloadUUID</key>
            <string>{uuid}</string>
            <key>PayloadVersion</key>
            <integer>1</integer>
        </dict>
    </array>
    <key>PayloadDescription</key>
    <string>Installs the Clipperd CA certificate so your iPhone can sync clipboard with Linux over HTTPS</string>
    <key>PayloadDisplayName</key>
    <string>Clipperd Setup</string>
    <key>PayloadIdentifier</key>
    <string>com.clipperd.profile</string>
    <key>PayloadRemovalDisallowed</key>
    <false/>
    <key>PayloadType</key>
    <string>Configuration</string>
    <key>PayloadUUID</key>
    <string>{profile_uuid}</string>
    <key>PayloadVersion</key>
    <integer>1</integer>
</dict>
</plist>
"#))
}

fn generate_uuid() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::rng().random();
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes(bytes[0..4].try_into().unwrap()),
        u16::from_be_bytes(bytes[4..6].try_into().unwrap()),
        u16::from_be_bytes(bytes[6..8].try_into().unwrap()),
        u16::from_be_bytes(bytes[8..10].try_into().unwrap()),
        {
            let mut n: u64 = 0;
            for &b in &bytes[10..16] {
                n = (n << 8) | b as u64;
            }
            n
        }
    )
}
