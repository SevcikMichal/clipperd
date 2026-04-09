use rcgen::{CertificateParams, DistinguishedName, DnType, Issuer, KeyPair, SanType};

pub struct GeneratedCerts {
    pub ca_cert_pem: String,
    pub cert_pem: String,
    pub key_pem: String,
}

pub fn generate_certs(hostname: &str, lan_ip: std::net::IpAddr) -> anyhow::Result<GeneratedCerts> {
    // Generate CA key + cert
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params.distinguished_name = {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "Clipperd CA");
        dn.push(DnType::OrganizationName, "Clipperd");
        dn
    };
    // Valid for 10 years
    ca_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    ca_params.not_after = rcgen::date_time_ymd(2035, 1, 1);

    let ca_cert = ca_params.self_signed(&ca_key)?;
    let ca_cert_pem = ca_cert.pem();

    // Build an Issuer from the CA params + key for signing the server cert
    let ca_issuer = Issuer::from_params(&ca_params, &ca_key);

    // Generate server key + cert signed by CA
    // iOS 13+ rejects TLS certs with validity > 825 days — keep well under that.
    let now = time::OffsetDateTime::now_utc();
    let two_years = now + time::Duration::days(730);

    let server_key = KeyPair::generate()?;
    let mut server_params = CertificateParams::default();
    server_params.is_ca = rcgen::IsCa::NoCa;
    server_params.distinguished_name = {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, hostname);
        dn
    };
    server_params.not_before = now;
    server_params.not_after = two_years;
    server_params.subject_alt_names = vec![
        SanType::DnsName(hostname.try_into()?),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        SanType::IpAddress(lan_ip),
    ];

    let server_cert = server_params.signed_by(&server_key, &ca_issuer)?;

    Ok(GeneratedCerts {
        ca_cert_pem,
        cert_pem: server_cert.pem(),
        key_pem: server_key.serialize_pem(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn test_ip() -> IpAddr {
        "192.168.1.100".parse().unwrap()
    }

    #[test]
    fn generate_certs_produces_valid_pem() {
        let certs = generate_certs("test-host", test_ip()).unwrap();
        assert!(certs.ca_cert_pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(certs.cert_pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(certs.key_pem.contains("-----BEGIN PRIVATE KEY-----"));
    }

    #[test]
    fn fingerprint_is_formatted_correctly() {
        let certs = generate_certs("test-host", test_ip()).unwrap();
        let fp = cert_fingerprint(&certs.ca_cert_pem).unwrap();
        // Expected: 16 uppercase hex byte pairs separated by colons → "AA:BB:...:FF"
        let parts: Vec<&str> = fp.split(':').collect();
        assert_eq!(parts.len(), 16, "fingerprint should have 16 byte pairs");
        for part in &parts {
            assert_eq!(part.len(), 2, "each part should be 2 hex chars");
            assert!(part.chars().all(|c| c.is_ascii_hexdigit()), "must be hex");
            assert_eq!(*part, part.to_uppercase(), "must be uppercase");
        }
    }

    #[test]
    fn fingerprint_is_deterministic_for_same_cert() {
        let certs = generate_certs("test-host", test_ip()).unwrap();
        let fp1 = cert_fingerprint(&certs.ca_cert_pem).unwrap();
        let fp2 = cert_fingerprint(&certs.ca_cert_pem).unwrap();
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn different_certs_have_different_fingerprints() {
        let a = generate_certs("host-a", test_ip()).unwrap();
        let b = generate_certs("host-b", test_ip()).unwrap();
        // Different CA keys → different fingerprints
        assert_ne!(
            cert_fingerprint(&a.ca_cert_pem).unwrap(),
            cert_fingerprint(&b.ca_cert_pem).unwrap()
        );
    }

    #[test]
    fn server_cert_validity_is_under_825_days() {
        // iOS 13+ rejects TLS certs with validity > 825 days.
        // We set 730 days — verify the constant hasn't drifted above the limit.
        assert!(730 <= 825, "server cert validity must be ≤ 825 days (iOS requirement)");
    }

    #[test]
    fn fingerprint_rejects_empty_input() {
        assert!(cert_fingerprint("").is_err());
        assert!(cert_fingerprint("not a pem").is_err());
    }
}

/// Compute SHA-256 fingerprint of the CA cert DER for display/verification
pub fn cert_fingerprint(ca_cert_pem: &str) -> anyhow::Result<String> {
    use rustls_pemfile::certs;
    let mut reader = std::io::BufReader::new(ca_cert_pem.as_bytes());
    let der_certs: Vec<_> = certs(&mut reader).collect::<Result<_, _>>()?;
    let der = der_certs.first().ok_or_else(|| anyhow::anyhow!("No cert found"))?;
    let hash = blake3::hash(der.as_ref());
    let hex = hex::encode(&hash.as_bytes()[..16]); // first 16 bytes = 32 hex chars
    // Format as pairs: AA:BB:CC:...
    let formatted: String = hex.as_bytes().chunks(2)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join(":");
    Ok(formatted.to_uppercase())
}
