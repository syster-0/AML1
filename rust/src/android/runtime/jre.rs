//! Staged JRE probe (Phase 4).
//!
//! Validates the OpenJDK image shipped in the app files dir without executing
//! anything: the VM library and the run-time module image must exist, and the
//! `release` metadata file supplies the version and architecture. The
//! launcher process runs this as a readiness gate before writing a launch
//! manifest; the in-process loader (`loader.rs`) consumes the same tree in
//! the game process.

use std::path::PathBuf;

/// Read-only description of a validated staged JRE.
pub(crate) struct StagedJre {
    /// Absolute JRE root (`<files>/jre`).
    pub java_home: String,
    /// Raw `JAVA_VERSION` from the `release` file (e.g. "17-internal").
    pub version: String,
    /// Resolved major version (e.g. 17, 8).
    pub major_version: u32,
    /// `OS_ARCH` from the `release` file.
    pub arch: String,
}

/// Probe the JRE at `<files_dir>/jre`. Returns `None` when the tree is missing
/// any boot-critical piece or its `release` metadata cannot be parsed.
pub(crate) fn probe(files_dir: &str) -> Option<StagedJre> {
    let home = PathBuf::from(files_dir.trim_end_matches('/')).join("jre");
    // libjvm.so is the VM the game process dlopens; `lib/modules` is the run-time
    // image HotSpot opens during boot. Both are mandatory.
    if !home.join("lib/server/libjvm.so").is_file() {
        return None;
    }
    if !home.join("lib/modules").is_file() {
        return None;
    }

    let release = std::fs::read_to_string(home.join("release")).ok()?;
    let mut version = None;
    let mut arch = None;
    for line in release.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "JAVA_VERSION" => version = Some(value.to_string()),
            "OS_ARCH" => arch = Some(value.to_string()),
            _ => {}
        }
    }

    let version = version?;
    let major_version = parse_major(&version)?;
    Some(StagedJre {
        java_home: home.to_string_lossy().into_owned(),
        version,
        major_version,
        arch: arch.unwrap_or_else(|| std::env::consts::ARCH.to_string()),
    })
}

/// Map a raw `JAVA_VERSION` to its major version: legacy `1.x` numbering for
/// Java 8 and earlier ("1.8.0_…"), modern scheme otherwise ("17-internal").
fn parse_major(version: &str) -> Option<u32> {
    let mut components = version.split(['.', '-', '_']);
    let first: u32 = components.next()?.parse().ok()?;
    if first == 1 {
        components
            .next()
            .and_then(|value| value.parse().ok())
            .or(Some(first))
    } else {
        Some(first)
    }
}
