/// Generate iOS Shortcut files as XML plist (.shortcut format).
///
/// iOS imports them via the deep link:
///   shortcuts://import-workflow/?url=<encoded_url>&name=<name>
///
/// The plist format is the standard Shortcuts workflow format used since iOS 12.

fn new_uuid() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    format!(
        "{:08X}-{:04X}-{:04X}-{:04X}-{:012X}",
        u32::from_be_bytes(bytes[0..4].try_into().unwrap()),
        u16::from_be_bytes(bytes[4..6].try_into().unwrap()),
        u16::from_be_bytes(bytes[6..8].try_into().unwrap()),
        u16::from_be_bytes(bytes[8..10].try_into().unwrap()),
        {
            let mut n: u64 = 0;
            for &b in &bytes[10..16] { n = (n << 8) | b as u64; }
            n
        }
    )
}

fn header_dict(key: &str, value: &str) -> String {
    format!(r#"
                            <dict>
                                <key>WFItemType</key>
                                <integer>0</integer>
                                <key>WFKey</key>
                                <dict>
                                    <key>Value</key>
                                    <dict>
                                        <key>string</key>
                                        <string>{key}</string>
                                    </dict>
                                    <key>WFSerializationType</key>
                                    <string>WFTextTokenString</string>
                                </dict>
                                <key>WFValue</key>
                                <dict>
                                    <key>Value</key>
                                    <dict>
                                        <key>string</key>
                                        <string>{value}</string>
                                    </dict>
                                    <key>WFSerializationType</key>
                                    <string>WFTextTokenString</string>
                                </dict>
                            </dict>"#,
        key = key,
        value = value
    )
}

fn http_headers_dict(headers: &str) -> String {
    format!(r#"
                <key>WFHTTPHeaders</key>
                <dict>
                    <key>Value</key>
                    <dict>
                        <key>WFDictionaryFieldValueItems</key>
                        <array>{headers}
                        </array>
                    </dict>
                    <key>WFSerializationType</key>
                    <string>WFDictionaryFieldValue</string>
                </dict>"#,
        headers = headers
    )
}

fn plist_wrapper(actions: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>WFWorkflowActions</key>
    <array>{actions}
    </array>
    <key>WFWorkflowClientVersion</key>
    <string>1141.2</string>
    <key>WFWorkflowMinimumClientVersion</key>
    <integer>900</integer>
    <key>WFWorkflowTypes</key>
    <array/>
    <key>WFWorkflowInputContentItemClasses</key>
    <array/>
    <key>WFWorkflowImportQuestions</key>
    <array/>
</dict>
</plist>"#,
        actions = actions
    )
}

/// Clipperd Send: reads iPhone clipboard → POST to Linux
pub fn generate_send_shortcut(host_url: &str, token: &str) -> String {
    let clipboard_uuid = new_uuid();
    let auth_header = header_dict("Authorization", &format!("Bearer {}", token));
    let headers = http_headers_dict(&auth_header);

    let actions = format!(r#"
        <dict>
            <key>WFWorkflowActionIdentifier</key>
            <string>is.workflow.actions.getclipboard</string>
            <key>WFWorkflowActionParameters</key>
            <dict>
                <key>UUID</key>
                <string>{clipboard_uuid}</string>
            </dict>
        </dict>
        <dict>
            <key>WFWorkflowActionIdentifier</key>
            <string>is.workflow.actions.downloadurl</string>
            <key>WFWorkflowActionParameters</key>
            <dict>
                <key>WFHTTPMethod</key>
                <string>POST</string>
                <key>WFURL</key>
                <string>{host_url}/v1/clipboard</string>
                <key>ShowHeaders</key>
                <true/>{headers}
                <key>WFHTTPBodyType</key>
                <string>File</string>
                <key>WFRequestVariable</key>
                <dict>
                    <key>Value</key>
                    <dict>
                        <key>OutputUUID</key>
                        <string>{clipboard_uuid}</string>
                        <key>Type</key>
                        <string>ActionOutput</string>
                    </dict>
                    <key>WFSerializationType</key>
                    <string>WFTokenAttachmentParameterState</string>
                </dict>
            </dict>
        </dict>"#,
        clipboard_uuid = clipboard_uuid,
        host_url = host_url,
        headers = headers,
    );

    plist_wrapper(&actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://192.168.1.100:7171";
    const TOKEN: &str = "abc123token";

    #[test]
    fn send_shortcut_contains_url_and_token() {
        let xml = generate_send_shortcut(URL, TOKEN);
        assert!(xml.contains(&format!("{}/v1/clipboard", URL)), "must contain endpoint URL");
        assert!(xml.contains(&format!("Bearer {}", TOKEN)), "must contain Bearer token");
        assert!(xml.contains("is.workflow.actions.getclipboard"), "must include Get Clipboard action");
        assert!(xml.contains("is.workflow.actions.downloadurl"), "must include URL download action");
        assert!(xml.contains("POST"), "must use POST method");
    }

    #[test]
    fn get_shortcut_contains_url_and_token() {
        let xml = generate_get_shortcut(URL, TOKEN);
        assert!(xml.contains(&format!("{}/v1/clipboard", URL)), "must contain endpoint URL");
        assert!(xml.contains(&format!("Bearer {}", TOKEN)), "must contain Bearer token");
        assert!(xml.contains("is.workflow.actions.downloadurl"), "must include URL download action");
        assert!(xml.contains("is.workflow.actions.setclipboard"), "must include Set Clipboard action");
        assert!(xml.contains("GET"), "must use GET method");
    }

    #[test]
    fn shortcuts_are_valid_plist_xml() {
        for xml in [generate_send_shortcut(URL, TOKEN), generate_get_shortcut(URL, TOKEN)] {
            assert!(xml.starts_with("<?xml"), "must start with XML declaration");
            assert!(xml.contains("<plist"), "must contain plist element");
            assert!(xml.contains("</plist>"), "plist must be closed");
            assert!(xml.contains("WFWorkflowActions"), "must have actions key");
        }
    }

    #[test]
    fn shortcuts_have_different_uuids() {
        // Each call generates fresh UUIDs — two shortcuts shouldn't share them
        let send1 = generate_send_shortcut(URL, TOKEN);
        let send2 = generate_send_shortcut(URL, TOKEN);
        // Extract first UUID occurrence — they should differ across calls
        // (with overwhelming probability given 128-bit random UUIDs)
        assert_ne!(send1, send2, "regenerated shortcuts should have different UUIDs");
    }

    #[test]
    fn shortcut_output_references_clipboard_action_uuid() {
        let xml = generate_send_shortcut(URL, TOKEN);
        // The download action must reference the clipboard action's UUID as its body source
        assert!(xml.contains("OutputUUID"), "POST body must reference clipboard action output");
    }
}

/// Clipperd Get: GET Linux clipboard → write to iPhone clipboard
pub fn generate_get_shortcut(host_url: &str, token: &str) -> String {
    let download_uuid = new_uuid();
    let auth_header = header_dict("Authorization", &format!("Bearer {}", token));
    let headers = http_headers_dict(&auth_header);

    let actions = format!(r#"
        <dict>
            <key>WFWorkflowActionIdentifier</key>
            <string>is.workflow.actions.downloadurl</string>
            <key>WFWorkflowActionParameters</key>
            <dict>
                <key>UUID</key>
                <string>{download_uuid}</string>
                <key>WFHTTPMethod</key>
                <string>GET</string>
                <key>WFURL</key>
                <string>{host_url}/v1/clipboard</string>
                <key>ShowHeaders</key>
                <true/>{headers}
            </dict>
        </dict>
        <dict>
            <key>WFWorkflowActionIdentifier</key>
            <string>is.workflow.actions.setclipboard</string>
            <key>WFWorkflowActionParameters</key>
            <dict>
                <key>WFInput</key>
                <dict>
                    <key>Value</key>
                    <dict>
                        <key>OutputUUID</key>
                        <string>{download_uuid}</string>
                        <key>Type</key>
                        <string>ActionOutput</string>
                    </dict>
                    <key>WFSerializationType</key>
                    <string>WFTokenAttachmentParameterState</string>
                </dict>
            </dict>
        </dict>"#,
        download_uuid = download_uuid,
        host_url = host_url,
        headers = headers,
    );

    plist_wrapper(&actions)
}
