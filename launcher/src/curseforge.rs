//! CurseForge modpacks, as Java instances.
//!
//! A pack's zip names its jars by CurseForge project and file id only (`manifest.json`), and
//! CurseForge's official API wants a key the launcher has none of. Its website's own JSON API
//! answers those two ids without one, and the CDN serves each file at a path built from its id
//! and name, so a pack imports with no account, no key and no search: the manifest is the list.
//!
//! The pack's overrides (`overrides/`, `client-overrides/`) go into the instance's game folder
//! as they are, and every required jar into its `mods/`. Optional files are named and left out:
//! a launcher can't tell which optional a player wants.

use crate::backend;
use crate::instances;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// What a pack's overrides live under, inside its zip.
const OVERRIDES: [&str; 2] = ["overrides/", "client-overrides/"];

/// Files of the player, not of a pack: a pack imported into an existing game folder (the
/// player's own `.minecraft`) leaves them as they are, and its worlds too.
const PLAYER_FILES: [&str; 4] = ["options.txt", "servers.dat", "servers.dat_old", "optionsof.txt"];

/// A CurseForge modpack's `manifest.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub minecraft: Game,
    #[serde(default)]
    pub files: Vec<File>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Game {
    /// The Minecraft version, as Mojang names it.
    pub version: String,
    #[serde(default, rename = "modLoaders")]
    pub loaders: Vec<ModLoader>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModLoader {
    /// `fabric-0.16.9`, `forge-47.2.0`, `neoforge-21.1.77`.
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct File {
    #[serde(rename = "projectID")]
    pub project: u64,
    #[serde(rename = "fileID")]
    pub id: u64,
    #[serde(default = "required_by_default")]
    pub required: bool,
}

fn required_by_default() -> bool {
    true
}

/// A file of a pack, where CurseForge keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub size: u64,
    pub url: String,
}

/// The pack's `manifest.json`.
pub fn manifest(zip: &Path) -> Result<Manifest, String> {
    let file = fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut entry = archive
        .by_name("manifest.json")
        .map_err(|_| format!("{} is not a CurseForge modpack: it has no manifest.json.", zip.display()))?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut entry, &mut text)
        .map_err(|e| format!("{}: {e}", zip.display()))?;
    // Windows tools write a byte-order mark into JSON they edit; it is not part of the document.
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text).map_err(|e| format!("{}: manifest.json: {e}", zip.display()))
}

/// A file's name and CDN link from its two ids: CurseForge's website API, no key.
///
/// The official API (`api.curseforge.com`) needs a key and answers the same two fields; this one
/// is the page a browser's own file list comes from. `shortcut: an unofficial endpoint, swap it
/// for the official API behind a key if CurseForge ever closes it`.
pub fn locate(project: u64, file: u64) -> Result<Found, String> {
    let url = format!("https://www.curseforge.com/api/v1/mods/{project}/files/{file}");
    let body = backend::get_bytes(&url)?;
    let json: Value = serde_json::from_slice(&body)
        .map_err(|e| format!("CurseForge's answer for {project}/{file}: {e}"))?;
    let data = json.get("data").unwrap_or(&json);
    let name = data["fileName"]
        .as_str()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| {
            format!("CurseForge has no file {file} of project {project} (it may be deleted).")
        })?;
    Ok(Found {
        name: name.to_string(),
        size: data["fileLength"].as_u64().unwrap_or(0),
        url: format!(
            "https://mediafilez.forgecdn.net/files/{}/{}/{}",
            file / 1000,
            file % 1000,
            percent_encode(name)
        ),
    })
}

/// Every byte but an unreserved one as `%XX`: a file name is a path segment, not a query.
fn percent_encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `f` into `to`, unless it is there already; returns the jar.
pub fn download(f: &File, to: &Path) -> Result<PathBuf, String> {
    let found = locate(f.project, f.id)?;
    let dest = to.join(&found.name);
    if dest.is_file() {
        return Ok(dest);
    }
    let body = backend::get_bytes(&found.url)?;
    if found.size != 0 && body.len() as u64 != found.size {
        return Err(format!(
            "{}: got {} bytes, CurseForge says {}",
            found.name,
            body.len(),
            found.size
        ));
    }
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let part = dest.with_extension("jar.part");
    fs::write(&part, &body).map_err(|e| format!("{}: {e}", part.display()))?;
    fs::rename(&part, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest)
}

/// Imports the pack zip as a Java instance playing its Minecraft version and loader, with its
/// required jars and its overrides. `home` (`Some`) plays in a game folder the player already
/// has, as [`instances::create_java_at`] does.
pub fn import(
    zip: &Path,
    name: Option<&str>,
    home: Option<&Path>,
    log: &dyn Fn(String),
) -> Result<instances::Instance, String> {
    let pack = manifest(zip)?;
    let version = pack.minecraft.version.trim();
    if version.is_empty() {
        return Err("This pack's manifest names no Minecraft version.".into());
    }
    let primary = pack
        .minecraft
        .loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| pack.minecraft.loaders.first());
    let loader = primary.map(loader_of).transpose()?;
    let name = name.map(str::to_string).or_else(|| pack_title(&pack)).unwrap_or_else(|| "CurseForge pack".into());
    log(format!(
        "{name}: Minecraft {version}{}, {} files",
        loader
            .as_ref()
            .map(|l| format!(", {} {}", l.kind.name(), l.version))
            .unwrap_or_default(),
        pack.files.len()
    ));
    let instance = match home {
        Some(dir) => instances::create_java_at(&name, version, dir, Some(dir), loader)?,
        None => {
            let made = instances::create_java(&name, version)?;
            match loader {
                Some(loader) => instances::edit(&made.id, |m| {
                    m.java.get_or_insert_with(Default::default).loader = Some(loader)
                })?,
                None => made,
            }
        }
    };
    let mods = instance.home.join("mods");
    fs::create_dir_all(&mods).map_err(|e| format!("{}: {e}", mods.display()))?;
    let required: Vec<&File> = pack.files.iter().filter(|f| f.required).collect();
    let mut left_off = 0;
    for (i, f) in required.iter().enumerate() {
        log(format!("{}/{} project {}", i + 1, required.len(), f.project));
        if let Err(e) = download(f, &mods) {
            log(format!("{}/{}: {e}", i + 1, required.len()));
        }
    }
    left_off += pack.files.len().saturating_sub(required.len());
    // The player's own settings and worlds stay as they are when the pack lands in a folder
    // that already has them.
    let unpacked = overrides(zip, &instance.home, home.is_some(), log)?;
    if left_off > 0 {
        log(format!(
            "{left_off} optional files left out: add the ones you want on its Mods page."
        ));
    }
    log(format!("{unpacked} files from the pack's overrides"));
    Ok(instance)
}

/// The pack's title for the instance: its name, its version when it has one.
fn pack_title(pack: &Manifest) -> Option<String> {
    let name = pack.name.trim();
    if name.is_empty() {
        return None;
    }
    let version = pack.version.trim();
    Some(if version.is_empty() {
        name.to_string()
    } else {
        format!("{name} {version}")
    })
}

/// `fabric-0.16.9` as a loader, for the loaders the launcher installs.
fn loader_of(loader: &ModLoader) -> Result<crate::loaders::Loader, String> {
    let id = loader.id.trim();
    let (kind, version) = id.split_once('-').ok_or_else(|| {
        format!("{id}: a mod loader is named like fabric-0.16.9 or forge-47.2.0.")
    })?;
    let kind = match crate::loaders::Kind::parse(kind) {
        Ok(kind) => kind,
        // An older pack may name a loader the launcher doesn't install (liteloader, rift).
        Err(e) => return Err(format!("{e} This pack plays {id}.")),
    };
    Ok(crate::loaders::Loader {
        kind,
        version: version.to_string(),
    })
}

/// Every file under the pack's overrides, into `home`; none leaves it.
fn overrides(
    zip: &Path,
    home: &Path,
    keep_player_files: bool,
    log: &dyn Fn(String),
) -> Result<usize, String> {
    let file = fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    let count = archive.len();
    let mut written = 0;
    for i in 0..count {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // A zip made on Windows may name its files with backslashes; a pack's own paths are "/".
        let name = entry
            .name()
            .map_err(|e| e.to_string())?
            .replace('\\', "/");
        let Some(relative) = OVERRIDES.iter().find_map(|o| name.strip_prefix(o)) else {
            continue;
        };
        if keep_player_files
            && (PLAYER_FILES.contains(&relative) || relative.starts_with("saves/"))
        {
            continue;
        }
        let Some(to) = inside(home, relative) else {
            continue;
        };
        if entry.is_dir() {
            fs::create_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
            continue;
        }
        fs::create_dir_all(to.parent().unwrap_or(home))
            .map_err(|e| format!("{}: {e}", to.display()))?;
        let mut out = fs::File::create(&to).map_err(|e| format!("{}: {e}", to.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("{}: {e}", to.display()))?;
        written += 1;
        log(format!("Copying {relative}"));
    }
    Ok(written)
}

/// `relative` as a path inside `home`, or None when it would leave it.
fn inside(home: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    if relative.ends_with('/')
        || !path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(home.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_names_its_version_loader_and_files() {
        let text = r#"{
            "minecraft": {"version": "26.3", "modLoaders": [{"id": "fabric-0.16.9", "primary": true}]},
            "name": "Testers", "version": "1.0",
            "files": [{"projectID": 306612, "fileID": 9792808, "required": true},
                      {"projectID": 342439, "fileID": 5313542, "required": false}]
        }"#;
        let pack: Manifest = serde_json::from_str(text).unwrap();
        assert_eq!(pack.minecraft.version, "26.3");
        assert_eq!(pack_title(&pack).as_deref(), Some("Testers 1.0"));
        let loader = loader_of(&pack.minecraft.loaders[0]).unwrap();
        assert_eq!(loader.kind.id(), "fabric");
        assert_eq!(loader.version, "0.16.9");
        assert_eq!(pack.files.len(), 2);
        assert!(!pack.files[1].required);
    }

    #[test]
    fn a_manifest_with_a_byte_order_mark_still_reads() {
        // Windows editors write a BOM into JSON they touch; the manifest of a pack edited that
        // way is still a CurseForge pack.
        let path = std::env::temp_dir().join(format!(
            "horizon-cf-bom-{}.zip",
            std::process::id()
        ));
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(
            &mut zip,
            "\u{feff}{\"minecraft\": {\"version\": \"26.3\"}, \"files\": []}".as_bytes(),
        )
        .unwrap();
        zip.finish().unwrap();
        let pack = manifest(&path).unwrap();
        assert_eq!(pack.minecraft.version, "26.3");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_loader_this_launcher_cannot_install_says_so() {
        let err = loader_of(&ModLoader {
            id: "liteloader-1.12.2".into(),
            primary: true,
        })
        .unwrap_err();
        assert!(err.contains("liteloader-1.12.2"), "{err}");
    }

    #[test]
    fn overrides_stay_inside_the_game_folder() {
        let home = Path::new("C:/game");
        assert_eq!(inside(home, "config/a.toml"), Some(home.join("config/a.toml")));
        assert_eq!(inside(home, "../../steal.txt"), None);
        assert_eq!(inside(home, "/etc/passwd"), None);
        assert_eq!(inside(home, "C:/Windows/x.dll"), None);
        assert_eq!(inside(home, "mods/"), None);
    }

    #[test]
    fn a_file_name_is_a_path_segment_on_the_cdn() {
        assert_eq!(
            percent_encode("fabric api-1.0+build 2.jar"),
            "fabric%20api-1.0%2Bbuild%202.jar"
        );
    }
}
