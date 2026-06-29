use std::process::Command;

/// Credentials Claude Code stores in the macOS Keychain.
pub struct Credentials {
    pub access_token: String,
    pub subscription_type: Option<String>,
}

/// Read Claude Code's live OAuth credentials from the macOS Keychain.
///
/// Claude Code keeps the canonical (refreshed) token here under the
/// `Claude Code-credentials` generic-password item. Reading it fresh on every
/// fetch means we always use a valid token and never have to run the OAuth
/// refresh flow ourselves.
pub fn read_credentials() -> Result<Credentials, String> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .map_err(|e| format!("failed to run `security`: {e}"))?;

    if !output.status.success() {
        return Err(
            "Claude Code credentials not found in Keychain. Open Claude Code and sign in.".into(),
        );
    }

    let blob = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(blob.trim()).map_err(|e| format!("could not parse credentials: {e}"))?;

    let oauth = &json["claudeAiOauth"];
    let access_token = oauth["accessToken"]
        .as_str()
        .ok_or("accessToken missing from credentials")?
        .to_string();
    let subscription_type = oauth["subscriptionType"].as_str().map(|s| s.to_string());

    Ok(Credentials {
        access_token,
        subscription_type,
    })
}
