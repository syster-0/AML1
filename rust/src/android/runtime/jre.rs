//! Staged JRE probe (Phase 4) and multi-runtime selection (Phase 7).
//!
//! Bionic OpenJDK images live in the app files dir; nothing is executed to
//! validate them. The VM library and run-time module image must exist, and the
//! `release` metadata file supplies the version and architecture. The launcher
//! process runs this as a readiness gate before writing a launch manifest; the
//! in-process loader (`loader.rs`) consumes the same tree in the game process.
//!
//! Runtime layout:
//! - `<files>/runtimes/<major>` — versioned runtimes (e.g. `runtimes/21`),
//!   the production download/import target;
//! - `<files>/jre` — legacy single-runtime location (Phase 3–5, Java 17),
//!   kept readable so existing installs keep working.

use std::path::PathBuf;

/// Read-only description of a validated staged JRE.
pub(crate) struct StagedJre {
    /// Absolute JRE home (`<files>/runtimes/21` or legacy `<files>/jre`).
    pub java_home: String,
    /// Raw `JAVA_VERSION` from the `release` file (e.g. "21.0.1").
    pub version: String,
    /// Resolved major version (e.g. 17, 21, 8).
    pub major_version: u32,
    /// `OS_ARCH` from the `release` file.
    pub arch: String,
}

/// Probe every known runtime location. Results are sorted by major version;
/// malformed/incomplete trees are skipped silently (readiness is re-checked
/// when a launch is attempted).
pub(crate) fn probe_all(files_dir: &str) -> Vec<StagedJre> {
    let root = PathBuf::from(files_dir.trim_end_matches('/'));
    let mut runtimes = Vec::new();

    // Legacy single-runtime location.
    if let Some(jre) = probe_home(root.join("jre")) {
        runtimes.push(jre);
    }

    let runtimes_root = root.join("runtimes");
    if let Ok(entries) = std::fs::read_dir(&runtimes_root) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            if let Some(jre) = probe_home(entry.path()) {
                runtimes.push(jre);
            }
        }
    }

    runtimes.sort_by_key(|jre| jre.major_version);
    runtimes
}

/// Pick the lowest installed major version that satisfies `required_major`.
/// The lowest qualifying version (not the highest installed) is preferred:
/// running an old MC on a much newer JDK works often enough but breaks more
/// mod stacks than it helps.
pub(crate) fn select(files_dir: &str, required_major: u32) -> Option<StagedJre> {
    probe_all(files_dir)
        .into_iter()
        .filter(|jre| jre.major_version >= required_major)
        .min_by_key(|jre| jre.major_version)
}

/// Probe the legacy `<files>/jre` tree. Retained for callers that only care
/// about the single-runtime layout; new code should use [select].
pub(crate) fn probe(files_dir: &str) -> Option<StagedJre> {
    probe_home(PathBuf::from(files_dir.trim_end_matches('/')).join("jre"))
}

fn probe_home(home: PathBuf) -> Option<StagedJre> {
    // libjvm.so is the VM the game process dlopens; `lib/modules` is the
    // run-time image HotSpot opens during boot. Both are mandatory.
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
/// Java 8 and earlier ("1.8.0_…"), modern scheme otherwise ("21.0.1").
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
