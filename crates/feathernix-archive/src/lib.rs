pub mod manifest;

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use manifest::{DependencyRef, Manifest, PackageBlock};

pub const MANIFEST_FILENAME: &str = "manifest.toml";
pub const PAYLOAD_DIR: &str = "payload";

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("7z error: {0}")]
    SevenZ(#[from] sevenz_rust2::Error),
    #[error("manifest missing from archive {0}")]
    MissingManifest(PathBuf),
    #[error("malformed manifest: {0}")]
    Manifest(#[from] toml::de::Error),
}

pub type ArchiveResult<T> = Result<T, ArchiveError>;

pub fn extract(archive: &Path, dest: &Path) -> ArchiveResult<()> {
    if !dest.exists() {
        std::fs::create_dir_all(dest)?;
    }
    sevenz_rust2::decompress_file(archive, dest)?;
    Ok(())
}

pub fn read_manifest(archive: &Path) -> ArchiveResult<Manifest> {
    let mut found: Option<Vec<u8>> = None;
    let tmp_dest = std::env::temp_dir().join("feathernix-archive-manifest-discard");

    sevenz_rust2::decompress_file_with_extract_fn(archive, &tmp_dest, |entry, reader, _dest| {
        if entry.is_directory() {
            return Ok(true);
        }
        if entry.name() == MANIFEST_FILENAME {
            let mut buf = Vec::with_capacity(entry.size() as usize);
            reader.read_to_end(&mut buf)?;
            found = Some(buf);
        }
        Ok(true)
    })?;

    let bytes = found.ok_or_else(|| ArchiveError::MissingManifest(archive.to_path_buf()))?;
    let text = std::str::from_utf8(&bytes).map_err(|e| {
        ArchiveError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;
    Ok(Manifest::from_str(text)?)
}

pub fn pack(src_dir: &Path, dest_archive: &Path) -> ArchiveResult<()> {
    sevenz_rust2::compress_to_path(src_dir, dest_archive)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MANIFEST: &str = r#"
[package]
name = "hello"
version = "1.0.0"
release = "1"
arch = "x86_64-linux"
summary = "Hello world"
license = "MIT"

[[dependencies.requires]]
name = "glibc"
version_constraint = ">= 2.17"

[[dependencies.provides]]
name = "hello"
version = "1.0.0"

[outputs]
default = "out"
"#;

    fn make_sample_archive(workdir: &Path) -> PathBuf {
        let src = workdir.join("src");
        std::fs::create_dir_all(src.join("payload/bin")).unwrap();
        std::fs::write(src.join(MANIFEST_FILENAME), SAMPLE_MANIFEST).unwrap();
        std::fs::write(src.join("payload/bin/hello"), b"#!/bin/sh\necho hello\n").unwrap();
        let archive = workdir.join("hello-1.0.0-1-x86_64-linux.fnpkg.7z");
        pack(&src, &archive).expect("pack");
        archive
    }

    #[test]
    fn parses_sample_manifest() {
        let m = Manifest::from_str(SAMPLE_MANIFEST).expect("parse");
        assert_eq!(m.package.name, "hello");
        assert_eq!(m.package.version, "1.0.0");
        assert_eq!(m.dependencies.requires.len(), 1);
        assert_eq!(m.dependencies.requires[0].name, "glibc");
        assert_eq!(m.outputs.default, "out");
    }

    #[test]
    fn pack_then_extract_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = make_sample_archive(tmp.path());
        let out = tmp.path().join("extracted");
        extract(&archive, &out).expect("extract");
        assert!(out.join("manifest.toml").exists());
        assert!(out.join("payload/bin/hello").exists());
    }

    #[test]
    fn read_manifest_from_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = make_sample_archive(tmp.path());
        let m = read_manifest(&archive).expect("read manifest");
        assert_eq!(m.package.name, "hello");
        assert_eq!(m.package.arch, "x86_64-linux");
    }
}
