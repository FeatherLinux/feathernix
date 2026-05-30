use std::path::{Path, PathBuf};

use pgp::composed::{
    ArmorOptions, Deserializable, DetachedSignature, EncryptionCaps, KeyType,
    SecretKeyParamsBuilder, SignedPublicKey, SignedSecretKey, SubkeyParamsBuilder,
};
use pgp::crypto::hash::HashAlgorithm;
use pgp::types::{KeyDetails, Password};
use rand::rngs::OsRng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("openpgp error: {0}")]
    OpenPgp(String),
    #[error("invalid key armor: {0}")]
    Armor(String),
}

pub type SignResult<T> = Result<T, SignError>;

/// A loaded secret key plus its (cached) ASCII-armored public counterpart.
/// Construct via [`SecretKey::from_armored_bytes`] or [`SecretKey::load`].
pub struct SecretKey {
    pub key: SignedSecretKey,
    pub password: Password,
}

impl SecretKey {
    /// Load a secret key from disk (armored). Pass `password=""` for an unencrypted key.
    pub fn load(path: &Path, password: &str) -> SignResult<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_armored_bytes(&bytes, password)
    }

    pub fn from_armored_bytes(bytes: &[u8], password: &str) -> SignResult<Self> {
        let s = std::str::from_utf8(bytes).map_err(|e| SignError::Armor(e.to_string()))?;
        let (key, _h) = SignedSecretKey::from_string(s)
            .map_err(|e| SignError::OpenPgp(format!("parse secret key: {e}")))?;
        let password = if password.is_empty() {
            Password::empty()
        } else {
            Password::from(password)
        };
        Ok(Self { key, password })
    }

    /// Sign `data` and return an ASCII-armored detached signature.
    pub fn sign_detached_armor(&self, data: &[u8]) -> SignResult<Vec<u8>> {
        let sig = DetachedSignature::sign_binary_data(
            OsRng,
            &self.key.primary_key,
            &self.password,
            HashAlgorithm::Sha256,
            data,
        )
        .map_err(|e| SignError::OpenPgp(format!("sign: {e}")))?;
        sig.to_armored_bytes(ArmorOptions::default())
            .map_err(|e| SignError::OpenPgp(format!("armor: {e}")))
    }

    /// Write a `.sig` next to `data_file` with the armored detached signature of its contents.
    pub fn sign_file(&self, data_file: &Path) -> SignResult<PathBuf> {
        let bytes = std::fs::read(data_file)?;
        let sig = self.sign_detached_armor(&bytes)?;
        let mut sig_path = data_file.as_os_str().to_owned();
        sig_path.push(".sig");
        let sig_path = PathBuf::from(sig_path);
        std::fs::write(&sig_path, &sig)?;
        Ok(sig_path)
    }

    /// Export the public key as an ASCII-armored buffer (for distribution via
    /// `gpg_key_url`).
    pub fn export_public_armor(&self) -> SignResult<Vec<u8>> {
        let pk = SignedPublicKey::from(self.key.clone());
        let mut armor = Vec::new();
        pk.to_armored_writer(&mut armor, ArmorOptions::default())
            .map_err(|e| SignError::OpenPgp(format!("armor public: {e}")))?;
        Ok(armor)
    }

    pub fn fingerprint_hex(&self) -> String {
        format!("{}", self.key.fingerprint())
    }
}

/// Generate an ephemeral Ed25519 keypair (no passphrase). Used by tests and
/// `fnmaint key gen` (when we add it). Returns `(SecretKey, armored_public)`.
pub fn generate_ephemeral(user_id: &str) -> SignResult<(SecretKey, Vec<u8>)> {
    let mut signkey = SubkeyParamsBuilder::default();
    signkey
        .key_type(KeyType::Ed25519Legacy)
        .can_sign(true)
        .can_encrypt(EncryptionCaps::None)
        .can_authenticate(false);
    let mut key_params = SecretKeyParamsBuilder::default();
    key_params
        .key_type(KeyType::Ed25519Legacy)
        .can_certify(true)
        .can_sign(true)
        .can_encrypt(EncryptionCaps::None)
        .primary_user_id(user_id.into())
        .subkeys(vec![signkey.build().map_err(|e| SignError::OpenPgp(e.to_string()))?]);
    let params = key_params
        .build()
        .map_err(|e| SignError::OpenPgp(e.to_string()))?;
    let sk = params
        .generate(OsRng)
        .map_err(|e| SignError::OpenPgp(format!("generate: {e}")))?;
    let pk = SignedPublicKey::from(sk.clone());
    let mut armor = Vec::new();
    pk.to_armored_writer(&mut armor, ArmorOptions::default())
        .map_err(|e| SignError::OpenPgp(format!("armor: {e}")))?;
    Ok((
        SecretKey {
            key: sk,
            password: Password::empty(),
        },
        armor,
    ))
}

/// Convenience: write the secret key out as an ASCII-armored file.
pub fn save_secret_key(sk: &SecretKey, dest: &Path) -> SignResult<()> {
    let mut armor = Vec::new();
    sk.key
        .to_armored_writer(&mut armor, ArmorOptions::default())
        .map_err(|e| SignError::OpenPgp(format!("armor secret: {e}")))?;
    std::fs::write(dest, armor)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgp::composed::DetachedSignature;

    #[test]
    fn round_trip_sign_and_verify() {
        let (sk, pk_armor) = generate_ephemeral("FeatherNix Test <t@example.com>").unwrap();
        let data = b"sign me";
        let sig = sk.sign_detached_armor(data).unwrap();

        let pk_str = std::str::from_utf8(&pk_armor).unwrap();
        let (pk, _) = SignedPublicKey::from_string(pk_str).unwrap();
        let sig_str = std::str::from_utf8(&sig).unwrap();
        let (parsed_sig, _) = DetachedSignature::from_string(sig_str).unwrap();
        let mut ok = parsed_sig.verify(&pk, data).is_ok();
        if !ok {
            for sub in &pk.public_subkeys {
                if parsed_sig.verify(sub, data).is_ok() {
                    ok = true;
                    break;
                }
            }
        }
        assert!(ok, "signature should verify against generated public key");
    }

    #[test]
    fn sign_file_writes_sibling_sig() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("payload.bin");
        std::fs::write(&f, b"abc123").unwrap();
        let (sk, _) = generate_ephemeral("t@example.com").unwrap();
        let sig_path = sk.sign_file(&f).unwrap();
        assert_eq!(sig_path, f.with_extension("bin.sig"));
        assert!(sig_path.exists());
    }

    #[test]
    fn save_and_reload_secret_key() {
        let tmp = tempfile::tempdir().unwrap();
        let (sk, _) = generate_ephemeral("t@example.com").unwrap();
        let f = tmp.path().join("sk.asc");
        save_secret_key(&sk, &f).unwrap();
        let sk2 = SecretKey::load(&f, "").unwrap();
        assert_eq!(sk.fingerprint_hex(), sk2.fingerprint_hex());
    }
}
