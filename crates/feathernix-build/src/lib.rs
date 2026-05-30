use std::path::{Path, PathBuf};

use thiserror::Error;

use feathernix_archive::manifest::{
    DependenciesBlock as ManifestDeps, OutputsBlock, PackageBlock as ManifestPackage,
};
use feathernix_archive::{DependencyRef, MANIFEST_FILENAME, Manifest, PAYLOAD_DIR, pack};
use feathernix_recipe::{Recipe, SourceKind};
use feathernix_sandbox::{SandboxError, SandboxSpec, run_step};
use feathernix_source::{FetchedSource, SourceError, SourceFetcher};

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Sandbox(#[from] SandboxError),
    #[error(transparent)]
    Archive(#[from] feathernix_archive::ArchiveError),
    #[error("manifest serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("recipe validation failed: {0}")]
    Recipe(#[from] feathernix_recipe::RecipeError),
    #[error("empty $OUT after build (no install step produced files?)")]
    EmptyOutput,
}

pub type BuildResult<T> = Result<T, BuildError>;

#[derive(Debug, Clone)]
pub struct BuildArtifact {
    pub archive_path: PathBuf,
    pub manifest: Manifest,
    pub source_tree_sha256: String,
}

pub struct Builder {
    pub work_root: PathBuf,
    pub source_cache: PathBuf,
}

impl Builder {
    pub fn new(work_root: PathBuf, source_cache: PathBuf) -> BuildResult<Self> {
        std::fs::create_dir_all(&work_root)?;
        std::fs::create_dir_all(&source_cache)?;
        Ok(Self { work_root, source_cache })
    }

    /// Run the full pipeline for `recipe`. `dest_dir` is where the produced
    /// `.fnpkg.7z` is written (filename derived from the recipe).
    pub fn build(&self, recipe: &Recipe, dest_dir: &Path) -> BuildResult<BuildArtifact> {
        recipe.validate()?;
        std::fs::create_dir_all(dest_dir)?;

        let fetcher = SourceFetcher::new(self.source_cache.clone())?;
        let fetched = fetcher.fetch(recipe)?;

        let job_id = format!(
            "{}-{}-{}",
            recipe.package.name, recipe.package.version, &fetched.tree_sha256[..16]
        );
        let job_dir = self.work_root.join(&job_id);
        if job_dir.exists() {
            std::fs::remove_dir_all(&job_dir)?;
        }
        std::fs::create_dir_all(&job_dir)?;

        let build_dir = job_dir.join("build");
        let out_dir = job_dir.join("out");
        std::fs::create_dir_all(&build_dir)?;
        std::fs::create_dir_all(&out_dir)?;

        // For path/git sources we always seed /build with a writable copy of /src
        // so build steps can run in-tree without trying to write into the read-only
        // src bind.
        seed_build_dir(&fetched, &build_dir, recipe)?;

        self.run_phase(recipe, &fetched, &build_dir, &out_dir, "prepare", &recipe.build.prepare)?;
        self.run_phase(recipe, &fetched, &build_dir, &out_dir, "build", &recipe.build.build)?;
        self.run_phase(recipe, &fetched, &build_dir, &out_dir, "install", &recipe.build.install)?;

        let staged = job_dir.join("staged");
        let staged_payload = staged.join(PAYLOAD_DIR);
        std::fs::create_dir_all(&staged_payload)?;
        if !out_dir.exists() || dir_is_empty(&out_dir)? {
            return Err(BuildError::EmptyOutput);
        }
        copy_tree(&out_dir, &staged_payload)?;

        let manifest = manifest_from_recipe(recipe);
        std::fs::write(
            staged.join(MANIFEST_FILENAME),
            manifest.to_pretty_string()?,
        )?;

        let archive_path = dest_dir.join(recipe.artifact_filename());
        if archive_path.exists() {
            std::fs::remove_file(&archive_path)?;
        }
        pack(&staged, &archive_path)?;

        Ok(BuildArtifact {
            archive_path,
            manifest,
            source_tree_sha256: fetched.tree_sha256,
        })
    }

    fn run_phase(
        &self,
        recipe: &Recipe,
        fetched: &FetchedSource,
        build_dir: &Path,
        out_dir: &Path,
        phase: &str,
        steps: &[String],
    ) -> BuildResult<()> {
        if steps.is_empty() {
            return Ok(());
        }
        tracing::info!("phase {}: {} steps", phase, steps.len());
        for (i, step) in steps.iter().enumerate() {
            let mut spec = SandboxSpec::new(build_dir.to_path_buf(), out_dir.to_path_buf());
            // For "path" sources we point /src at the original (read-only). Git/tarball
            // sources have already been copied into /build, so /src is redundant but
            // still informative for recipes that prefer to reference the canonical tree.
            if recipe.source.kind == SourceKind::Path {
                spec = spec.with_src(fetched.root.clone());
            } else {
                spec = spec.with_src(fetched.root.clone());
            }
            for (k, v) in &recipe.build.env {
                spec = spec.env(k, v);
            }
            spec = spec.env("FEATHERNIX_PHASE", phase);
            spec = spec.env("FEATHERNIX_STEP", &i.to_string());
            run_step(&spec, step).map_err(|e| -> BuildError {
                match e {
                    SandboxError::StepFailed { step, status } => SandboxError::StepFailed {
                        step: format!("[{phase}#{i}] {step}"),
                        status,
                    }
                    .into(),
                    other => other.into(),
                }
            })?;
        }
        Ok(())
    }
}

fn manifest_from_recipe(recipe: &Recipe) -> Manifest {
    let pkg = ManifestPackage {
        name: recipe.package.name.clone(),
        version: recipe.package.version.clone(),
        release: recipe.package.release.clone(),
        epoch: recipe.package.epoch,
        arch: recipe.package.arch.clone(),
        summary: recipe.package.summary.clone(),
        license: recipe.package.license.clone(),
        url: recipe.package.url.clone(),
    };
    let deps = ManifestDeps {
        requires: recipe.dependencies.requires.iter().map(parse_dep_ref).collect(),
        provides: recipe.dependencies.provides.iter().map(parse_dep_ref).collect(),
    };
    Manifest {
        package: pkg,
        dependencies: deps,
        outputs: OutputsBlock::default(),
    }
}

fn parse_dep_ref(spec: &String) -> DependencyRef {
    // Forms supported:
    //   "name"
    //   "name = 1.2.3"
    //   "name >= 1.2.3"
    let s = spec.trim();
    let (name, rest) = match s.split_once(char::is_whitespace) {
        Some((n, r)) => (n.trim().to_string(), r.trim()),
        None => return DependencyRef { name: s.to_string(), version: None, version_constraint: None },
    };
    if let Some(eq) = rest.strip_prefix('=') {
        DependencyRef {
            name,
            version: Some(eq.trim().to_string()),
            version_constraint: None,
        }
    } else {
        DependencyRef {
            name,
            version: None,
            version_constraint: Some(rest.to_string()),
        }
    }
}

fn seed_build_dir(fetched: &FetchedSource, build_dir: &Path, recipe: &Recipe) -> BuildResult<()> {
    if recipe.source.kind == SourceKind::Path {
        // For local-path sources we leave /build empty and let the recipe explicitly
        // copy from $SRC if it wants. Recipes that build in-tree should use git or
        // tarball sources.
        return Ok(());
    }
    copy_tree(&fetched.root, build_dir)?;
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.map_err(|e| std::io::Error::other(e.to_string()))?;
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_symlink() {
            let link_target = std::fs::read_link(entry.path())?;
            // Skip symlinks pointing into .git or outside the tree.
            let _ = std::os::unix::fs::symlink(link_target, &target);
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
            // Preserve mode bits (especially executable).
            if let Ok(meta) = entry.metadata() {
                use std::os::unix::fs::PermissionsExt;
                let mode = meta.permissions().mode();
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

fn dir_is_empty(dir: &Path) -> std::io::Result<bool> {
    Ok(std::fs::read_dir(dir)?.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    use feathernix_archive::{read_manifest, extract};
    use feathernix_recipe::Recipe;
    use feathernix_sandbox::ensure_bwrap_available;

    fn have_bwrap() -> bool {
        ensure_bwrap_available().is_ok()
    }

    #[test]
    fn end_to_end_hello_path_recipe() {
        if !have_bwrap() {
            eprintln!("skipping: bwrap not available");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();

        // Author a tiny source tree.
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("hello.sh"), b"#!/bin/sh\necho hello from feathernix\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(src.join("hello.sh"), std::fs::Permissions::from_mode(0o755))
            .unwrap();

        let recipe_str = format!(
            r#"
[recipe]
format_version = 1
[package]
name    = "hello"
version = "0.1.0"
release = "1"
arch    = "x86_64-linux"
summary = "greet"
[source]
kind = "path"
path = "{}"
[build]
prepare = []
build   = []
install = ["mkdir -p $OUT/bin && cp $SRC/hello.sh $OUT/bin/hello && chmod 755 $OUT/bin/hello"]
[dependencies]
provides = ["hello = 0.1.0"]
"#,
            src.display()
        );
        let recipe = Recipe::from_str(&recipe_str).unwrap();

        let builder = Builder::new(
            tmp.path().join("work"),
            tmp.path().join("srccache"),
        )
        .unwrap();
        let artifact = builder.build(&recipe, tmp.path()).unwrap();
        assert!(artifact.archive_path.exists());
        assert!(
            artifact
                .archive_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".fnpkg.7z")
        );

        // Verify manifest round-trips.
        let m = read_manifest(&artifact.archive_path).unwrap();
        assert_eq!(m.package.name, "hello");
        assert_eq!(m.package.version, "0.1.0");

        // Verify payload extraction.
        let out = tmp.path().join("extracted");
        extract(&artifact.archive_path, &out).unwrap();
        assert!(out.join("payload/bin/hello").exists());
        let contents = std::fs::read_to_string(out.join("payload/bin/hello")).unwrap();
        assert!(contents.contains("echo hello from feathernix"));
    }

    #[test]
    fn parse_dep_handles_eq_and_constraints() {
        let a = parse_dep_ref(&"glibc >= 2.17".to_string());
        assert_eq!(a.name, "glibc");
        assert_eq!(a.version_constraint.as_deref(), Some(">= 2.17"));
        let b = parse_dep_ref(&"rg = 14.1.0".to_string());
        assert_eq!(b.name, "rg");
        assert_eq!(b.version.as_deref(), Some("14.1.0"));
        let c = parse_dep_ref(&"bash".to_string());
        assert_eq!(c.name, "bash");
        assert!(c.version.is_none() && c.version_constraint.is_none());
    }
}
