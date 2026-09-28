//! Android platform queries.
//!
//! Non-Android targets compile these to inert stubs so callers keep a single
//! call site across platforms.

/// Read-only information about a JRE staged in the app files dir.
#[derive(Clone, Debug)]
pub struct StagedJreDto {
    /// Absolute JRE root path.
    pub java_home: String,
    /// Raw version string from the JRE `release` file.
    pub version: String,
    /// Resolved major version (e.g. 17).
    pub major_version: u32,
    /// Target architecture (e.g. "amd64").
    pub arch: String,
}

/// Probe the JRE staged at `<files_dir>/jre` without executing it. Returns
/// `None` when no valid JRE is present.
pub fn probe_staged_jre(files_dir: String) -> Option<StagedJreDto> {
    #[cfg(target_os = "android")]
    {
        crate::android::runtime::jre::probe(&files_dir).map(StagedJreDto::from)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = files_dir;
        None
    }
}

/// Select the lowest installed staged JRE whose major version satisfies
/// `required_major`, scanning `<files_dir>/runtimes/<major>` plus the legacy
/// `<files_dir>/jre` location. Returns `None` when nothing qualifies.
pub fn select_staged_jre(
    files_dir: String,
    required_major: u32,
) -> Option<StagedJreDto> {
    #[cfg(target_os = "android")]
    {
        crate::android::runtime::jre::select(&files_dir, required_major)
            .map(StagedJreDto::from)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (files_dir, required_major);
        None
    }
}

#[cfg(target_os = "android")]
impl From<crate::android::runtime::jre::StagedJre> for StagedJreDto {
    fn from(jre: crate::android::runtime::jre::StagedJre) -> Self {
        StagedJreDto {
            java_home: jre.java_home,
            version: jre.version,
            major_version: jre.major_version,
            arch: jre.arch,
        }
    }
}
