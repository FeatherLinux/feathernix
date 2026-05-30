use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use feathernix_sign::SecretKey;

pub fn run(archive: &Path, key: &Path) -> Result<()> {
    let sk = SecretKey::load(key, "").context("load signing key")?;
    let sig_path = sk.sign_file(archive).context("sign archive")?;
    println!(
        "{} {} (key {})",
        "signed".green().bold(),
        sig_path.display(),
        &sk.fingerprint_hex()[..16]
    );
    Ok(())
}
