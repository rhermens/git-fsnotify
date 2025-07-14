use std::path::Path;

use git2::{Cred, CredentialType};

pub fn credentials_callback(
    _remote_url: &str,
    cred: Option<&str>,
    _allowed: CredentialType,
) -> Result<Cred, git2::Error> {
    Cred::ssh_key(
        cred.expect("Failed to get username from url"),
        None,
        Path::new(&format!(
            "{}/.ssh/id_ed25519",
            std::env::var("HOME").expect("Failed to get home directory")
        )),
        None,
    )
}
