//! OCIOZ archive support.
//!
//! Implements OCIO config archiving and extraction using ZIP.

use crate::{OcioError, OcioResult};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::ZipWriter;
use zip::read::ZipArchive;
use zip::write::SimpleFileOptions;

const CONFIG_ARCHIVE_NAME: &str = "config.ocio";

/// Returns the standard config filename inside an OCIOZ archive.
pub fn config_archive_name() -> &'static str {
    CONFIG_ARCHIVE_NAME
}

/// Extracts an OCIOZ archive into the destination directory.
pub fn extract_ocioz_archive(archive_path: &Path, destination_dir: &Path) -> OcioResult<()> {
    let file = File::open(archive_path)?;
    let mut zip = ZipArchive::new(file).map_err(|e| OcioError::Io(std::io::Error::other(e)))?;

    fs::create_dir_all(destination_dir)?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| OcioError::Io(std::io::Error::other(e)))?;
        let Some(name) = entry.enclosed_name() else {
            return Err(OcioError::Validation(
                "archive entry has unsafe path".into(),
            ));
        };
        let out_path = destination_dir.join(name);

        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }

    Ok(())
}

/// Reads the embedded config.ocio contents from an OCIOZ archive.
pub fn read_config_from_archive(archive_path: &Path) -> OcioResult<String> {
    let file = File::open(archive_path)?;
    let mut zip = ZipArchive::new(file).map_err(|e| OcioError::Io(std::io::Error::other(e)))?;

    let mut config_file =
        zip.by_name(CONFIG_ARCHIVE_NAME)
            .map_err(|_| OcioError::ConfigNotFound {
                path: archive_path.to_path_buf(),
            })?;

    let mut buf = String::new();
    config_file.read_to_string(&mut buf)?;
    Ok(buf)
}

/// Archives the config contents and LUT files to a .ocioz file path.
pub fn archive_config_to_path(
    output_path: &Path,
    config_contents: &str,
    working_dir: &Path,
    lut_extensions: &[&str],
) -> OcioResult<()> {
    let file = File::create(output_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file(CONFIG_ARCHIVE_NAME, options)
        .map_err(|e| OcioError::Io(std::io::Error::other(e)))?;
    zip.write_all(config_contents.as_bytes())?;

    if working_dir.exists() {
        add_lut_files_to_zip(&mut zip, working_dir, working_dir, lut_extensions, options)?;
    }

    zip.finish()
        .map_err(|e| OcioError::Io(std::io::Error::other(e)))?;
    Ok(())
}

fn add_lut_files_to_zip(
    zip: &mut ZipWriter<File>,
    root: &Path,
    current: &Path,
    lut_extensions: &[&str],
    options: SimpleFileOptions,
) -> OcioResult<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            add_lut_files_to_zip(zip, root, &path, lut_extensions, options)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());
        let Some(ext) = ext else {
            continue;
        };
        if ext == "ocio" {
            continue;
        }
        if !lut_extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
            continue;
        }

        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel,
            Err(_) => continue,
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        zip.start_file(rel_str, options)
            .map_err(|e| OcioError::Io(std::io::Error::other(e)))?;
        let mut file = File::open(&path)?;
        std::io::copy(&mut file, zip)?;
    }

    Ok(())
}

/// Creates a temporary directory for extracting OCIOZ archives: created atomically under a
/// unique name (the old `ocioz_<pid>_<n>` was predictable and checked-then-created). The caller
/// owns it (the config keeps it as `archive_dir`).
pub fn create_temp_archive_dir() -> OcioResult<PathBuf> {
    Ok(tempfile::Builder::new().prefix("ocioz_").tempdir()?.keep())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each call makes its own new directory.
    #[test]
    fn temp_archive_dirs_are_distinct_and_created() {
        let (a, b) = (
            create_temp_archive_dir().unwrap(),
            create_temp_archive_dir().unwrap(),
        );
        assert_ne!(a, b);
        assert!(a.is_dir() && b.is_dir());
        fs::remove_dir_all(a).ok();
        fs::remove_dir_all(b).ok();
    }
}
