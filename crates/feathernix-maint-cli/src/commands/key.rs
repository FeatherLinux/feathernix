use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use feathernix_sign::{generate_ephemeral, save_secret_key};

pub fn generate(user_id: &str, secret_out: &Path, public_out: &Path) -> Result<()> {
    let (sk, pub_armor) =
        generate_ephemeral(user_id).context("generate signing key")?;
    if let Some(parent) = secret_out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = public_out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    save_secret_key(&sk, secret_out).context("write secret key")?;
    std::fs::write(public_out, &pub_armor).context("write public key")?;
    println!(
        "{} fingerprint {} ({})",
        "generated".green().bold(),
        &sk.fingerprint_hex()[..16],
        sk.fingerprint_hex(),
    );
    println!("           secret: {}", secret_out.display());
    println!("           public: {}", public_out.display());
    Ok(())
}
