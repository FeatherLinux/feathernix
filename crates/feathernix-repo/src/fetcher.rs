use std::io::Write;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::RepoError;

#[derive(Debug, Clone)]
pub struct Fetcher {
    client: reqwest::blocking::Client,
}

impl Fetcher {
    pub fn new() -> Result<Self, RepoError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent(format!("feathernix/{}", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client })
    }

    pub fn fetch_bytes(&self, base_url: &str, rel: &str) -> Result<Vec<u8>, RepoError> {
        let url = join_url(base_url, rel);
        if url.starts_with("file://") {
            let p = url.trim_start_matches("file://");
            Ok(std::fs::read(p)?)
        } else if url.starts_with("path:") {
            let p = url.trim_start_matches("path:").trim_start_matches("//");
            Ok(std::fs::read(p)?)
        } else if url.starts_with("http://") || url.starts_with("https://") {
            let resp = self.client.get(&url).send()?.error_for_status()?;
            Ok(resp.bytes()?.to_vec())
        } else {
            Ok(std::fs::read(&url)?)
        }
    }

    /// Like `fetch_bytes` but returns `Ok(None)` if the resource is missing
    /// (HTTP 404 or file-not-found). Other errors still bubble up.
    pub fn try_fetch_bytes(
        &self,
        base_url: &str,
        rel: &str,
    ) -> Result<Option<Vec<u8>>, RepoError> {
        match self.fetch_bytes(base_url, rel) {
            Ok(b) => Ok(Some(b)),
            Err(RepoError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(RepoError::Http(e)) if e.status().map(|s| s.as_u16()) == Some(404) => Ok(None),
            Err(other) => Err(other),
        }
    }

    pub fn fetch_to_file(
        &self,
        base_url: &str,
        rel: &str,
        dest: &Path,
        expected_sha256: Option<&str>,
    ) -> Result<u64, RepoError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let url = join_url(base_url, rel);
        let bytes = if url.starts_with("file://") {
            std::fs::read(url.trim_start_matches("file://"))?
        } else if url.starts_with("path:") {
            std::fs::read(url.trim_start_matches("path:").trim_start_matches("//"))?
        } else if url.starts_with("http://") || url.starts_with("https://") {
            self.client.get(&url).send()?.error_for_status()?.bytes()?.to_vec()
        } else {
            std::fs::read(&url)?
        };

        if let Some(expected) = expected_sha256 {
            let actual = sha256_hex(&bytes);
            if actual != expected {
                return Err(RepoError::HashMismatch {
                    url: url.clone(),
                    expected: expected.to_string(),
                    actual,
                });
            }
        }

        let mut f = std::fs::File::create(dest)?;
        f.write_all(&bytes)?;
        Ok(bytes.len() as u64)
    }
}

pub fn join_url(base: &str, rel: &str) -> String {
    if rel.is_empty() {
        return base.to_string();
    }
    let base = base.trim_end_matches('/');
    let rel = rel.trim_start_matches('/');
    format!("{base}/{rel}")
}

pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_handles_slashes() {
        assert_eq!(join_url("http://x.com", "a"), "http://x.com/a");
        assert_eq!(join_url("http://x.com/", "/a"), "http://x.com/a");
        assert_eq!(join_url("http://x.com/", ""), "http://x.com/");
    }

    #[test]
    fn sha256_known_vector() {
        // "abc" sha256
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
