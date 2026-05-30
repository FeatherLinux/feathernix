use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub const HASH_LEN: usize = 40;

pub fn hash_for_payload(name: &str, version: &str, payload_sha256: &str) -> String {
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    h.update([0]);
    h.update(version.as_bytes());
    h.update([0]);
    h.update(payload_sha256.as_bytes());
    let digest = h.finalize();
    hex::encode(&digest[..HASH_LEN / 2])
}

pub fn store_path(store_root: &Path, hash: &str, name: &str, version: &str) -> PathBuf {
    store_root.join(format!("{hash}-{name}-{version}"))
}

pub fn compute_store_path(
    store_root: &Path,
    name: &str,
    version: &str,
    payload_sha256: &str,
) -> (String, PathBuf) {
    let hash = hash_for_payload(name, version, payload_sha256);
    let path = store_path(store_root, &hash, name, version);
    (hash, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let h1 = hash_for_payload("hello", "1.0.0", "abcd");
        let h2 = hash_for_payload("hello", "1.0.0", "abcd");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), HASH_LEN);
    }

    #[test]
    fn store_path_format() {
        let (h, p) = compute_store_path(Path::new("/fnix/store"), "hello", "1.0.0", "abcd");
        assert!(p.starts_with("/fnix/store"));
        let leaf = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(leaf.starts_with(&h));
        assert!(leaf.ends_with("-hello-1.0.0"));
    }

    #[test]
    fn different_payload_yields_different_path() {
        let (h1, _) = compute_store_path(Path::new("/fnix/store"), "a", "1.0", "aaaa");
        let (h2, _) = compute_store_path(Path::new("/fnix/store"), "a", "1.0", "bbbb");
        assert_ne!(h1, h2);
    }
}
