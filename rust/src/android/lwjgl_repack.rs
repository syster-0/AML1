//! JPMS-clean LWJGL repack for modular loaders (Forge ≥ 1.21, NeoForge).
//!
//! Vanilla/Fabric launches prepend the fat `lwjgl-glfw-classes.jar` stub to the
//! classpath so its Android-patched classes shadow the desktop LWJGL jars
//! (first-wins). Modern Forge instead turns EVERY classpath jar into a JPMS
//! module via SecureModuleFinder: the fat jar becomes the automatic module
//! `lwjgl.glfw.classes` exporting every package, which collides with the real
//! `org.lwjgl.glfw` module (`ResolutionException: Modules lwjgl.glfw.classes
//! and org.lwjgl.glfw export package org.lwjgl.glfw`).
//!
//! The fix keeps the exact same effective class set but packages it per JPMS
//! module: each real `org.lwjgl*` jar is rewritten into a patched copy that
//! keeps its own metadata (manifest / module-info) while every class that the
//! stub overrides is replaced with the stub's bytes, and stub classes living
//! in packages the module already owns are added. `android/util` (referenced
//! by the stub GLFW classes) is folded into the org.lwjgl.glfw module jar so
//! intra-module access stays legal. Remaining stub packages that no real
//! module owns (jsr305 annotations, LWJGL2 legacy, nanovg) go into one
//! supplemental automatic-module jar so their runtime availability matches
//! the fat-jar behavior.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use zip::write::FileOptions;
use zip::{ZipArchive, ZipWriter};

/// Stub packages with no home in any real LWJGL module keep working (as in
/// the fat-jar setup) via a supplemental automatic-module jar, whose packages
/// must stay unique across the whole module graph.
const EXTRA_JAR: &str = "lwjgl-android-extras.jar";

/// Rewrite `classpath` for a modular-loader launch: real org.lwjgl module jars
/// are replaced by patched copies carrying the stub's classes, the fat stub
/// itself disappears, and the supplemental extras jar is appended.
pub(crate) fn modular_lwjgl_classpath(files_root: &Path, classpath: &str) -> Result<String> {
    let stub_path = files_root.join("components/lwjgl3/lwjgl-glfw-classes.jar");
    let stub_sig = signature(&stub_path)
        .ok_or_else(|| anyhow::anyhow!("LWJGL stub jar missing: {}", stub_path.display()))?;
    let stub = StubJar::read(&stub_path)?;

    let out_dir = files_root.join("components/lwjgl3/patched");
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("create {}", out_dir.display()))?;

    let sep = crate::launcher::args::classpath_separator();
    let mut out: Vec<String> = Vec::new();
    let mut module_pkgs: HashSet<String> = HashSet::new();
    let mut repacked = false;
    for entry in classpath.split(sep) {
        if is_lwjgl_module_jar(entry) {
            let src_sig = signature(Path::new(entry))
                .with_context(|| format!("lwjgl module jar missing: {entry}"))?;
            let file_name = Path::new(entry)
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("bad classpath entry: {entry}"))?;
            let patched = out_dir.join(file_name);
            let owned = repack_module(Path::new(entry), &stub, &patched, &(stub_sig, src_sig))?;
            module_pkgs.extend(owned);
            repacked = true;
            out.push(patched.to_string_lossy().into_owned());
        } else {
            out.push(entry.to_string());
        }
    }

    if repacked {
        let extras = out_dir.join(EXTRA_JAR);
        write_extras(&stub, &module_pkgs, &extras, &stub_sig)?;
        out.push(extras.to_string_lossy().into_owned());
    }

    Ok(out.join(&sep.to_string()))
}

/// `.../org/lwjgl/lwjgl-glfw/3.3.3/lwjgl-glfw-3.3.3.jar` → true; natives and
/// the fat stub itself → false.
fn is_lwjgl_module_jar(entry: &str) -> bool {
    let name = Path::new(entry)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    name.starts_with("lwjgl")
        && name.ends_with(".jar")
        && !name.contains("natives")
        && name != "lwjgl-glfw-classes.jar"
        && entry.contains("/org/lwjgl/")
}

/// (len, mtime) of a file for cheap cache validation.
fn signature(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((meta.len(), mtime))
}

struct StubJar {
    /// package ("org/lwjgl/glfw") → entries (jar path, bytes)
    by_package: HashMap<String, Vec<(String, Vec<u8>)>>,
}

impl StubJar {
    fn read(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("open stub jar {}", path.display()))?;
        let mut archive = ZipArchive::new(file)?;
        let mut by_package: HashMap<String, Vec<(String, Vec<u8>)>> = HashMap::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if entry.is_dir() || !name.ends_with(".class") || name.starts_with("META-INF/") {
                continue;
            }
            let Some((pkg, _)) = name.rsplit_once('/') else {
                continue;
            };
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            by_package
                .entry(pkg.to_string())
                .or_default()
                .push((name, bytes));
        }
        Ok(Self { by_package })
    }
}

/// Build `out` as a copy of `module_jar` whose classes are overridden/extended
/// by stub classes living in packages the module already contains. Returns the
/// set of packages this module owns. Cached via a `<name>.sig` sidecar holding
/// the (stub, source) signatures.
fn repack_module(
    module_jar: &Path,
    stub: &StubJar,
    out: &Path,
    sig: &((u64, u64), (u64, u64)),
) -> Result<HashSet<String>> {
    let file =
        std::fs::File::open(module_jar).with_context(|| format!("open {}", module_jar.display()))?;
    let mut archive = ZipArchive::new(file)?;

    // Packages owned by this module (root classes only; MR versioned entries
    // stay metadata-internal to the jar).
    let mut module_pkgs: HashSet<String> = HashSet::new();
    for name in archive.file_names() {
        if name.ends_with(".class") && !name.starts_with("META-INF/") {
            if let Some((pkg, _)) = name.rsplit_once('/') {
                module_pkgs.insert(pkg.to_string());
            }
        }
    }

    if cache_valid(out, sig) {
        return Ok(module_pkgs);
    }

    let tmp = out.with_extension("jar.tmp");
    let tmp_file =
        std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    let mut writer = ZipWriter::new(tmp_file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut written: HashSet<String> = HashSet::new();

    // Copy the module jar; entries the stub overrides get the stub bytes.
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            continue;
        }
        let replacement = if name.ends_with(".class") && !name.starts_with("META-INF/") {
            name.rsplit_once('/')
                .filter(|(pkg, _)| module_pkgs.contains(*pkg))
                .and_then(|(pkg, _)| stub.by_package.get(pkg))
                .and_then(|entries| entries.iter().find(|(n, _)| *n == name))
                .map(|(_, bytes)| bytes)
        } else {
            None
        };
        writer.start_file(&name, options)?;
        if let Some(bytes) = replacement {
            writer.write_all(bytes)?;
        } else {
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            writer.write_all(&bytes)?;
        }
        written.insert(name);
    }

    // Add stub classes in module packages that the original jar lacks
    // (e.g. org.lwjgl.glfw.GLFWWindowProperties).
    for pkg in &module_pkgs {
        if let Some(entries) = stub.by_package.get(pkg) {
            for (name, bytes) in entries {
                if written.insert(name.clone()) {
                    writer.start_file(name, options)?;
                    writer.write_all(bytes)?;
                }
            }
        }
    }

    // The stub's GLFW implementation references android/util helpers
    // (verified: org.lwjgl.glfw.GLFW is the only stub class referencing
    // android/util). JPMS has no unnamed-module escape hatch, so those
    // classes must live in the SAME module as their caller — fold them into
    // the org.lwjgl.glfw module jar. (A supplemental jar would form its own
    // automatic module, which org.lwjgl.glfw does not read.)
    if module_pkgs.contains("org/lwjgl/glfw") {
        if let Some(entries) = stub.by_package.get("android/util") {
            for (name, bytes) in entries {
                if written.insert(name.clone()) {
                    writer.start_file(name, options)?;
                    writer.write_all(bytes)?;
                }
            }
        }
    }

    writer.finish()?;
    drop(archive);
    std::fs::rename(&tmp, out).with_context(|| format!("rename {}", tmp.display()))?;
    write_sig(out, sig)?;
    Ok(module_pkgs)
}

/// Supplemental automatic-module jar holding every stub package no real
/// module owns (jsr305 annotations, LWJGL2 legacy, nanovg). android/util is
/// NOT here: it is folded into the org.lwjgl.glfw module, its only consumer.
fn write_extras(
    stub: &StubJar,
    module_pkgs: &HashSet<String>,
    out: &Path,
    stub_sig: &(u64, u64),
) -> Result<()> {
    // The orphan set depends on which packages the real modules own, so the
    // cache key must include the module package set, not just the stub.
    let mut sorted: Vec<&String> = module_pkgs.iter().collect();
    sorted.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for pkg in sorted {
        std::hash::Hash::hash(pkg, &mut hasher);
    }
    let pkgs_hash = std::hash::Hasher::finish(&hasher);
    let sig = (*stub_sig, (pkgs_hash, module_pkgs.len() as u64));
    if cache_valid(out, &sig) {
        return Ok(());
    }
    let tmp = out.with_extension("jar.tmp");
    let tmp_file =
        std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    let mut writer = ZipWriter::new(tmp_file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (pkg, entries) in &stub.by_package {
        // android/util is folded into the org.lwjgl.glfw module jar (its only
        // consumer); emitting it here too would split the package across two
        // modules, which JPMS rejects.
        if module_pkgs.contains(pkg) || pkg == "android/util" {
            continue;
        }
        for (name, bytes) in entries {
            writer.start_file(name, options)?;
            writer.write_all(bytes)?;
        }
    }
    writer.finish()?;
    std::fs::rename(&tmp, out).with_context(|| format!("rename {}", tmp.display()))?;
    write_sig(out, &sig)?;
    Ok(())
}

/// Bump when the repack rules change; the signature files embed it so stale
/// patched jars from older app builds are regenerated.
const REPACK_VERSION: u32 = 2;

fn sig_path(out: &Path) -> PathBuf {
    out.with_extension("sig")
}

fn sig_text(sig: &((u64, u64), (u64, u64))) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        REPACK_VERSION, sig.0 .0, sig.0 .1, sig.1 .0, sig.1 .1
    )
}

fn cache_valid(out: &Path, sig: &((u64, u64), (u64, u64))) -> bool {
    let Ok(text) = std::fs::read_to_string(sig_path(out)) else {
        return false;
    };
    text.trim() == sig_text(sig) && out.exists()
}

fn write_sig(out: &Path, sig: &((u64, u64), (u64, u64))) -> Result<()> {
    std::fs::write(sig_path(out), sig_text(sig))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Fixture-driven test: expects the Pojav stub at /tmp/stub.jar and the
    //! real LWJGL 3.3.3 jars at /tmp/lwj/<artifact>.jar (pulled from a
    //! device). Skips silently when the fixtures are absent.
    use super::*;

    fn jar_names(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        ZipArchive::new(file)
            .unwrap()
            .file_names()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn repack_produces_jpms_clean_modules() {
        let stub = PathBuf::from("/tmp/stub.jar");
        let lwj = PathBuf::from("/tmp/lwj");
        if !stub.exists() || !lwj.exists() {
            eprintln!("fixtures missing, skipping");
            return;
        }
        let out_dir = std::env::temp_dir().join("lwjgl_repack_test");
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir).unwrap();

        let stub_jar = StubJar::read(&stub).unwrap();
        let stub_sig = signature(&stub).unwrap();
        let mut module_pkgs = HashSet::new();
        for entry in std::fs::read_dir(&lwj).unwrap() {
            let src = entry.unwrap().path();
            if src.extension().and_then(|e| e.to_str()) != Some("jar") {
                continue;
            }
            let patched = out_dir.join(src.file_name().unwrap());
            let sig = (stub_sig, signature(&src).unwrap());
            let owned = repack_module(&src, &stub_jar, &patched, &sig).unwrap();
            let names = jar_names(&patched);
            // Original metadata preserved: manifest + module-info present.
            assert!(names.iter().any(|n| n == "META-INF/MANIFEST.MF"), "{src:?}");
            assert!(
                names.iter().any(|n| n == "module-info.class" || n.contains("module-info")),
                "{src:?} lost module-info"
            );
            // No duplicate entries (would confuse JarFile readers).
            let unique: HashSet<&String> = names.iter().collect();
            assert_eq!(names.len(), unique.len(), "{src:?} has duplicate entries");
            // Stub's glfw classes really replaced the originals, and the
            // android/util helpers ride along in the same module.
            if src.file_name().unwrap() == "lwjgl-glfw.jar" {
                assert!(names.iter().any(|n| n == "org/lwjgl/glfw/GLFW.class"));
                assert!(names.iter().any(|n| n == "android/util/ArrayMap.class"));
            }
            module_pkgs.extend(owned);
        }

        let extras = out_dir.join(EXTRA_JAR);
        write_extras(&stub_jar, &module_pkgs, &extras, &stub_sig).unwrap();
        let names = jar_names(&extras);
        assert!(!names.iter().any(|n| n.starts_with("android/util/")));
        assert!(names.iter().any(|n| n.starts_with("javax/annotation/")));
        // No overlap between extras and module-owned packages.
        for n in &names {
            if let Some((pkg, _)) = n.rsplit_once('/') {
                assert!(!module_pkgs.contains(pkg), "extras overlap module pkg {pkg}");
            }
        }
    }
}
