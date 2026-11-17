use std::path::{Path, PathBuf};

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::types::KeyDetails;

use crate::RepoError;
use crate::fetcher::Fetcher;

#[derive(Debug, Clone)]
pub struct Keyring {
    pub key: SignedPublicKey,
    pub fingerprint: String,
}

impl Keyring {
    pub fn from_armored_bytes(repo_name: &str, bytes: &[u8]) -> Result<Self, RepoError> {
        let s = std::str::from_utf8(bytes).map_err(|e| RepoError::KeyloadFailed {
            repo: repo_name.to_string(),
            reason: format!("key armor is not utf-8: {e}"),
        })?;
        let (key, _headers) =
            SignedPublicKey::from_string(s).map_err(|e| RepoError::KeyloadFailed {
                repo: repo_name.to_string(),
                reason: format!("parse: {e}"),
            })?;
        let fingerprint = format!("{}", key.fingerprint());
        Ok(Self { key, fingerprint })
    }

    pub fn verify_detached(
        &self,
        url: &str,
        data: &[u8],
        sig_armor: &[u8],
    ) -> Result<(), RepoError> {
        let s = std::str::from_utf8(sig_armor).map_err(|e| RepoError::SignatureInvalid {
            url: url.to_string(),
            reason: format!("signature armor is not utf-8: {e}"),
        })?;
        let (sig, _h) =
            DetachedSignature::from_string(s).map_err(|e| RepoError::SignatureInvalid {
                url: url.to_string(),
                reason: format!("parse: {e}"),
            })?;
        if sig.verify(&self.key, data).is_ok() {
            return Ok(());
        }
        for subkey in &self.key.public_subkeys {
            if sig.verify(subkey, data).is_ok() {
                return Ok(());
            }
        }
        Err(RepoError::SignatureInvalid {
            url: url.to_string(),
            reason: "no key or subkey accepted the signature".into(),
        })
    }
}

pub fn keyring_cache_path(keyrings_dir: &Path, repo_name: &str) -> PathBuf {
    keyrings_dir.join(format!("{repo_name}.asc"))
}

pub fn fingerprint_cache_path(keyrings_dir: &Path, repo_name: &str) -> PathBuf {
    keyrings_dir.join(format!("{repo_name}.fpr"))
}

/// TOFU: load the cached public key for `repo_name`, or fetch from `gpg_key_url`
/// (and cache) if not yet present.
pub fn load_or_fetch(
    fetcher: &Fetcher,
    repo_name: &str,
    gpg_key_url: Option<&str>,
    keyrings_dir: &Path,
) -> Result<Keyring, RepoError> {
    let cache_path = keyring_cache_path(keyrings_dir, repo_name);
    if cache_path.exists() {
        let bytes = std::fs::read(&cache_path)?;
        return Keyring::from_armored_bytes(repo_name, &bytes);
    }

    let url = gpg_key_url.ok_or_else(|| RepoError::GpgCheckButNoKeyUrl {
        repo: repo_name.to_string(),
    })?;
    let bytes = fetcher.fetch_bytes(url, "")?;
    let kr = Keyring::from_armored_bytes(repo_name, &bytes)?;

    std::fs::create_dir_all(keyrings_dir)?;
    std::fs::write(&cache_path, &bytes)?;
    std::fs::write(
        fingerprint_cache_path(keyrings_dir, repo_name),
        &kr.fingerprint,
    )?;
    Ok(kr)
}

#[cfg(test)]
mod tests {
    use super::*;

    use pgp::composed::{
        ArmorOptions, EncryptionCaps, KeyType, SecretKeyParamsBuilder, SignedPublicKey,
        SignedSecretKey, SubkeyParamsBuilder,
    };
    use pgp::crypto::hash::HashAlgorithm;
    use pgp::types::Password;
    use feathernix_crypto::CompatOsRng;

    fn make_keypair() -> (SignedSecretKey, Vec<u8>) {
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
            .primary_user_id("FeatherNix Test <test@example.com>".into())
            .subkeys(vec![signkey.build().unwrap()]);
        let params = key_params.build().unwrap();
        let sk = params.generate(CompatOsRng).unwrap();
        let pk = SignedPublicKey::from(sk.clone());
        let mut armor = Vec::new();
        pk.to_armored_writer(&mut armor, ArmorOptions::default())
            .unwrap();
        (sk, armor)
    }

    fn sign_detached_armor(data: &[u8], sk: &SignedSecretKey) -> Vec<u8> {
        let sig = DetachedSignature::sign_binary_data(
            CompatOsRng,
            &sk.primary_key,
            &Password::empty(),
            HashAlgorithm::Sha256,
            data,
        )
        .unwrap();
        sig.to_armored_bytes(ArmorOptions::default()).unwrap()
    }

    #[test]
    fn ephemeral_key_signs_and_verifies() {
        let (sk, pk_armor) = make_keypair();
        let data = b"the quick brown fox";
        let sig_armor = sign_detached_armor(data, &sk);
        let kr = Keyring::from_armored_bytes("test", &pk_armor).unwrap();
        kr.verify_detached("test://sig", data, &sig_armor).unwrap();
        assert!(!kr.fingerprint.is_empty());
    }

    #[test]
    fn wrong_key_fails_verification() {
        let (sk_a, _) = make_keypair();
        let (_, pk_b_armor) = make_keypair();
        let data = b"signed-with-a";
        let sig = sign_detached_armor(data, &sk_a);
        let kr = Keyring::from_armored_bytes("test", &pk_b_armor).unwrap();
        let err = kr.verify_detached("test://sig", data, &sig).unwrap_err();
        assert!(matches!(err, RepoError::SignatureInvalid { .. }));
    }

    #[test]
    fn tampered_data_fails_verification() {
        let (sk, pk_armor) = make_keypair();
        let data = b"original";
        let sig = sign_detached_armor(data, &sk);
        let tampered = b"tampered";
        let kr = Keyring::from_armored_bytes("test", &pk_armor).unwrap();
        let err = kr.verify_detached("test://sig", tampered, &sig).unwrap_err();
        assert!(matches!(err, RepoError::SignatureInvalid { .. }));
    }

    #[test]
    fn tofu_caches_key_after_first_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, pk_armor) = make_keypair();
        let key_file = tmp.path().join("devrepo.asc");
        std::fs::write(&key_file, &pk_armor).unwrap();
        let cache_dir = tmp.path().join("keyrings");

        let fetcher = Fetcher::new().unwrap();
        let url = format!("file://{}", key_file.display());

        let kr1 = load_or_fetch(&fetcher, "devrepo", Some(&url), &cache_dir).unwrap();
        assert!(cache_dir.join("devrepo.asc").exists());
        assert!(cache_dir.join("devrepo.fpr").exists());

        // Delete the source: subsequent calls must still succeed from cache.
        std::fs::remove_file(&key_file).unwrap();
        let kr2 = load_or_fetch(&fetcher, "devrepo", Some(&url), &cache_dir).unwrap();
        assert_eq!(kr1.fingerprint, kr2.fingerprint);
    }
}
