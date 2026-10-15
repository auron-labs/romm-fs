use super::setup::ROMM_FOLDER;
use crate::catalogue::Catalogue;
use crate::romm::RomDto;
use crate::save_sync::{ensure_no_reparse_components, path_is_reparse_point};
use anyhow::Context;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::Path;

/// Stable identities remain outside gamelist XML: the frontend rewrites XML
/// without preserving custom attributes. Removed games retain their state here.
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct MetadataIndex {
    schema: u32,
    server_id: String,
    account_id: i64,
    #[serde(default)]
    pending_publication: bool,
    games: BTreeMap<String, IndexedGame>,
}

impl MetadataIndex {
    /// Persist this before publishing XML so interruption cannot lose the
    /// captured frontend state or associate swapped paths with the wrong game.
    pub fn staged(&self) -> Self {
        let mut index = self.clone();
        index.pending_publication = true;
        index
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct IndexedGame {
    /// `<system>/<file>` within the managed library.
    path: String,
    frontend_fields: Vec<String>,
    #[serde(default = "initially_active")]
    active: bool,
}

fn initially_active() -> bool {
    true
}

/// RetroBat's own `roms/<system>/gamelist.xml` files, as read before merging.
#[derive(Clone, Default)]
pub struct FrontendGamelists {
    files: BTreeMap<String, String>,
    /// Systems whose gamelist exists but could not be read; never rewritten.
    unreadable: Vec<(String, String)>,
}

impl FrontendGamelists {
    pub fn new(files: BTreeMap<String, String>) -> Self {
        Self {
            files,
            unreadable: Vec::new(),
        }
    }
}

/// Gamelists to write into RetroBat, built without reading any ROM body.
pub struct MetadataPublication {
    /// `<system>/gamelist.xml` (relative to RetroBat's roms folder) → bytes,
    /// only for gamelists whose content changes.
    pub(crate) gamelists: BTreeMap<String, Vec<u8>>,
    pub(crate) index: MetadataIndex,
    /// Systems left untouched, with the reason.
    pub(crate) skipped: Vec<(String, String)>,
}
impl MetadataPublication {
    pub fn gamelists(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.gamelists
    }
    pub fn index(&self) -> &MetadataIndex {
        &self.index
    }
    pub fn skipped(&self) -> &[(String, String)] {
        &self.skipped
    }
}

const GENERATED: &[&str] = &[
    "path",
    "name",
    "desc",
    "genre",
    "developer",
    "publisher",
    "releasedate",
    "players",
    "rating",
    "image",
    "thumbnail",
];
const ARTWORK_ROLES: [&str; 2] = ["image", "thumbnail"];

/// What an existing entry for one of our games carries forward.
#[derive(Default)]
struct Captured {
    /// Frontend fields (favourite, play count, last played, hidden, …).
    fields: Vec<String>,
    /// Existing artwork elements, kept until fresh artwork is available.
    artwork: Vec<(String, String)>,
}

fn element_text(node: &str) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct Value {
        #[serde(rename = "$text", default)]
        text: String,
    }
    Ok(quick_xml::de::from_str::<Value>(node)?.text)
}

/// The managed path (`<system>/<file>`) of a gamelist path, when it points
/// into our folder (`./RomM/<file>` in RetroBat, `./<file>` in a legacy
/// managed gamelist).
fn managed_path(system: &str, path: &str, legacy: bool) -> Option<String> {
    let relative = path.strip_prefix("./").unwrap_or(path);
    if relative.contains('\\') {
        return None;
    }
    let rest = if legacy {
        relative
    } else {
        let (first, rest) = relative.split_once('/')?;
        if !first.eq_ignore_ascii_case(ROMM_FOLDER) {
            return None;
        }
        rest
    };
    let safe = !rest.is_empty()
        && rest
            .split('/')
            .all(|part| crate::save_sync::validate_windows_path_component(part).is_ok());
    safe.then(|| format!("{system}/{rest}"))
}

/// Split a gamelist into the nodes we keep verbatim and the entries for our
/// games (captured by managed path). Duplicate entries keep the first.
fn split_gamelist(
    system: &str,
    xml: &str,
    legacy: bool,
    captured: &mut HashMap<String, Captured>,
) -> anyhow::Result<Vec<String>> {
    let mut kept = Vec::new();
    for (kind, node) in xml_children(xml, "gameList")? {
        let ours = (kind == "game")
            .then(|| -> anyhow::Result<Option<(String, Captured)>> {
                let mut path = None;
                let mut entry = Captured::default();
                for (name, field) in xml_children(&node, "game")? {
                    if name == "path" {
                        path = path.or(Some(element_text(&field)?));
                    } else if ARTWORK_ROLES.contains(&name.as_str()) {
                        // Only our own artwork; legacy paths don't resolve from RetroBat.
                        let text = element_text(&field)?;
                        let ours = !legacy
                            && text
                                .strip_prefix("./")
                                .and_then(|t| t.split_once('/'))
                                .is_some_and(|(first, rest)| {
                                    first.eq_ignore_ascii_case(ROMM_FOLDER)
                                        && rest.starts_with(".rommfs-artwork/")
                                });
                        if ours {
                            entry.artwork.push((name, field));
                        }
                    } else if !GENERATED.contains(&name.as_str()) {
                        entry.fields.push(field);
                    }
                }
                Ok(path
                    .and_then(|path| managed_path(system, &path, legacy))
                    .map(|key| (key.to_lowercase(), entry)))
            })
            .transpose()?
            .flatten();
        match ours {
            Some((key, entry)) => {
                captured.entry(key).or_insert(entry);
            }
            None if legacy => {}
            None => kept.push(node),
        }
    }
    Ok(kept)
}

/// Merge RomM games into RetroBat's gamelists under `./RomM/`. Every entry and
/// node RomMFS doesn't own is kept as it is. Frontend fields on our entries
/// (favourites, play history, hidden flags, …) are carried over by identity,
/// or by path when no identity is known, and survive removal and re-addition.
///
/// `legacy` holds gamelists an earlier build kept in the managed folder; they
/// are only read for frontend fields. A system whose gamelist can't be read is
/// skipped and left untouched; it never stops the others.
#[allow(clippy::too_many_arguments)]
pub fn generate_gamelists(
    catalogue: &Catalogue,
    roms: &[RomDto],
    server_id: &str,
    account_id: i64,
    previous: &MetadataIndex,
    frontend: &FrontendGamelists,
    legacy: &BTreeMap<String, String>,
    artwork: &HashMap<(i64, &'static str), String>,
) -> anyhow::Result<MetadataPublication> {
    anyhow::ensure!(account_id > 0, "metadata requires a verified account");
    // An index for another server or account says nothing about these games.
    let same_owner = previous.schema == 0
        || (previous.schema == 1
            && previous.server_id == server_id
            && previous.account_id == account_id);
    let mut index = if same_owner {
        previous.clone()
    } else {
        MetadataIndex::default()
    };
    let pending = index.pending_publication;
    index.schema = 1;
    index.server_id = server_id.into();
    index.account_id = account_id;

    let mut skipped = frontend.unreadable.clone();
    let mut captured = HashMap::new();
    let mut kept = BTreeMap::new();
    for (system, xml) in &frontend.files {
        let result = crate::save_sync::validate_windows_path_component(system)
            .map_err(anyhow::Error::msg)
            .and_then(|()| split_gamelist(system, xml, false, &mut captured));
        match result {
            Ok(nodes) => {
                kept.insert(system.clone(), nodes);
            }
            Err(error) => skipped.push((
                system.clone(),
                format!("couldn't read roms\\{system}\\gamelist.xml: {error:#}"),
            )),
        }
    }
    let had_ours: HashSet<String> = captured
        .keys()
        .filter_map(|key| key.split_once('/').map(|(system, _)| system.to_owned()))
        .collect();
    for (system, xml) in legacy {
        // Best effort: an unreadable legacy file just has nothing to carry over.
        if crate::save_sync::validate_windows_path_component(system).is_ok() {
            let _ = split_gamelist(system, xml, true, &mut captured);
        }
    }
    let skipped_systems: HashSet<String> = skipped.iter().map(|(s, _)| s.clone()).collect();

    // Capture all old identities before updating any paths, including games
    // that disappeared in the new catalogue. This also survives name swapping.
    if !pending {
        for game in index.games.values_mut().filter(|g| {
            g.active
                && !g
                    .path
                    .split_once('/')
                    .is_some_and(|(system, _)| skipped_systems.contains(system))
        }) {
            if let Some(entry) = captured.get(&game.path.to_lowercase()) {
                game.frontend_fields = entry.fields.clone();
            }
        }
    }
    // Paths another known identity occupied: a new game must not take their state.
    let indexed_paths: HashSet<String> = index
        .games
        .values()
        .filter(|g| g.active)
        .map(|g| g.path.to_lowercase())
        .collect();
    // An interrupted system that is unreadable may still have XML at its old
    // paths. Keep recovery mode until it can be read and republished too.
    index.pending_publication = pending && !skipped_systems.is_empty();
    for game in index.games.values_mut() {
        // No XML was read or will be published for a skipped system. Its
        // identities must still describe the gamelist left on disk.
        if !game
            .path
            .split_once('/')
            .is_some_and(|(system, _)| skipped_systems.contains(system))
        {
            game.active = false;
        }
    }

    let roms: HashMap<_, _> = roms.iter().map(|r| (r.id, r)).collect();
    let mut games: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
    for entry in catalogue
        .entries
        .iter()
        .filter(|e| catalogue.is_launchable(&e.key))
    {
        anyhow::ensure!(
            entry.key.server_id == server_id,
            "metadata catalogue belongs to another server"
        );
        if entry.platform_dir == "switchupdates" || skipped_systems.contains(&entry.platform_dir) {
            continue;
        }
        let rom = roms
            .get(&entry.key.rom_id)
            .context("launchable file has no metadata source")?;
        let path = format!("{}/{}", entry.platform_dir, entry.file_name);
        let key = path.to_lowercase();
        let identity = format!("{}:{}", entry.key.rom_id, entry.key.file_id);
        let state = index.games.entry(identity).or_insert_with(|| IndexedGame {
            path: path.clone(),
            frontend_fields: if indexed_paths.contains(&key) {
                Vec::new()
            } else {
                captured
                    .get(&key)
                    .map(|c| c.fields.clone())
                    .unwrap_or_default()
            },
            active: true,
        });
        state.path = path;
        state.active = true;

        let mut writer = Writer::new(Vec::new());
        writer.write_event(Event::Start(BytesStart::new("game")))?;
        field(
            &mut writer,
            "path",
            &format!("./{ROMM_FOLDER}/{}", entry.file_name),
        )?;
        field(
            &mut writer,
            "name",
            rom.metadata
                .name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&rom.fs_name),
        )?;
        if let Some(summary) = &rom.metadata.summary {
            field(&mut writer, "desc", summary)?;
        }
        if let Some(metadata) = &rom.metadata.metadatum {
            for (name, values) in [
                ("genre", &metadata.genres),
                ("developer", &metadata.developers),
                ("publisher", &metadata.publishers),
            ] {
                if !values.is_empty() {
                    field(&mut writer, name, &values.join(", "))?;
                }
            }
            if let Some(players) = &metadata.player_count {
                field(&mut writer, "players", players)?;
            }
            if let Some(date) = metadata
                .first_release_date
                .and_then(|d| chrono::DateTime::from_timestamp(d, 0))
            {
                field(
                    &mut writer,
                    "releasedate",
                    &date.format("%Y%m%dT000000").to_string(),
                )?;
            }
            if let Some(rating) = metadata.average_rating.filter(|r| r.is_finite()) {
                field(
                    &mut writer,
                    "rating",
                    &(rating / 100.0).clamp(0.0, 1.0).to_string(),
                )?;
            }
        }
        for role in ARTWORK_ROLES {
            if let Some(path) = artwork.get(&(rom.id, role)) {
                anyhow::ensure!(
                    path.starts_with(".rommfs-artwork/")
                        && !path.contains('\\')
                        && path.split('/').all(|part| {
                            crate::save_sync::validate_windows_path_component(part).is_ok()
                        }),
                    "unsafe artwork reference"
                );
                field(&mut writer, role, &format!("./{ROMM_FOLDER}/{path}"))?;
            } else if let Some((_, node)) = captured
                .get(&key)
                .and_then(|c| c.artwork.iter().find(|(name, _)| name == role))
            {
                // Artwork still downloading: keep what the gamelist already shows.
                writer.get_mut().extend_from_slice(node.as_bytes());
            }
        }
        for raw in &state.frontend_fields {
            let nodes = xml_children(&format!("<game>{raw}</game>"), "game")?;
            anyhow::ensure!(
                nodes.len() == 1 && !GENERATED.contains(&nodes[0].0.as_str()),
                "invalid saved frontend field"
            );
            writer.get_mut().extend_from_slice(raw.as_bytes());
        }
        writer.write_event(Event::End(BytesEnd::new("game")))?;
        games
            .entry(entry.platform_dir.clone())
            .or_default()
            .push(writer.into_inner());
    }

    let systems: BTreeSet<&String> = games.keys().chain(had_ours.iter()).collect();
    let mut gamelists = BTreeMap::new();
    for system in systems {
        if skipped_systems.contains(system) {
            continue;
        }
        let mut xml = b"<?xml version=\"1.0\"?>\n<gameList>\n".to_vec();
        for node in kept.get(system).into_iter().flatten() {
            xml.extend_from_slice(b"\t");
            xml.extend_from_slice(node.as_bytes());
            xml.extend_from_slice(b"\n");
        }
        for game in games.get(system).into_iter().flatten() {
            xml.extend_from_slice(b"\t");
            xml.extend_from_slice(game);
            xml.extend_from_slice(b"\n");
        }
        xml.extend_from_slice(b"</gameList>\n");
        if frontend.files.get(system).map(String::as_bytes) != Some(&xml[..]) {
            gamelists.insert(format!("{system}/gamelist.xml"), xml);
        }
    }
    Ok(MetadataPublication {
        gamelists,
        index,
        skipped,
    })
}

/// Read RetroBat's gamelists for the linked systems. A missing file is fine;
/// one that can't be read is reported and will not be rewritten.
pub fn read_gamelists(installation: &Path, systems: &[String]) -> FrontendGamelists {
    let mut gamelists = FrontendGamelists::default();
    for system in systems {
        let path = installation.join("roms").join(system).join("gamelist.xml");
        let result = (|| -> anyhow::Result<Option<String>> {
            crate::save_sync::validate_windows_path_component(system)
                .map_err(anyhow::Error::msg)?;
            ensure_no_reparse_components(path.parent().context("missing system folder")?)?;
            match fs::symlink_metadata(&path) {
                Ok(m) if path_is_reparse_point(&m) || !m.is_file() => {
                    anyhow::bail!("gamelist.xml is not an ordinary file")
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            }
            let text = String::from_utf8(fs::read(&path)?).context("gamelist.xml is not UTF-8")?;
            Ok(Some(text.trim_start_matches('\u{feff}').to_owned()))
        })();
        match result {
            Ok(Some(text)) => {
                gamelists.files.insert(system.clone(), text);
            }
            Ok(None) => {}
            Err(error) => gamelists.unreadable.push((
                system.clone(),
                format!("couldn't read roms\\{system}\\gamelist.xml: {error:#}"),
            )),
        }
    }
    gamelists
}

/// Write changed gamelists into RetroBat's linked system folders, each one
/// replaced atomically. The caller holds the stopped-frontend guard. Returns
/// the systems that could not be written; the rest are still published.
pub fn publish_gamelists(
    installation: &Path,
    systems: &[String],
    publication: &MetadataPublication,
) -> Vec<(String, String)> {
    let mut failures = Vec::new();
    for (relative, bytes) in &publication.gamelists {
        let Some(system) = relative.strip_suffix("/gamelist.xml") else {
            continue;
        };
        if !systems.iter().any(|s| s == system) {
            continue;
        }
        let folder = installation.join("roms").join(system);
        let result = (|| -> anyhow::Result<()> {
            ensure_no_reparse_components(&folder)?;
            let target = folder.join("gamelist.xml");
            if let Ok(m) = fs::symlink_metadata(&target) {
                anyhow::ensure!(
                    m.is_file() && !path_is_reparse_point(&m),
                    "gamelist.xml is not an ordinary file"
                );
            }
            let mut file = tempfile::NamedTempFile::new_in(&folder)?;
            file.write_all(bytes)?;
            file.as_file().sync_all()?;
            file.persist(&target).map_err(|error| error.error)?;
            Ok(())
        })();
        if let Err(error) = result {
            failures.push((
                system.to_owned(),
                format!("couldn't write roms\\{system}\\gamelist.xml: {error:#}"),
            ));
        }
    }
    failures
}

fn field(writer: &mut Writer<Vec<u8>>, name: &str, text: &str) -> anyhow::Result<()> {
    // XML 1.0 cannot encode most control characters; replace them rather than
    // writing a gamelist the frontend cannot read.
    let text: String = text
        .chars()
        .filter(|c| {
            matches!(*c, '\t' | '\n' | '\r')
                || (*c >= ' ' && !matches!(*c, '\u{fffe}' | '\u{ffff}'))
        })
        .collect();
    writer.write_event(Event::Start(BytesStart::new(name)))?;
    writer.write_event(Event::Text(BytesText::new(&text)))?;
    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

/// Use quick-xml for structural validation and retain each original direct
/// child's bytes. This preserves unknown frontend extensions without a second
/// XML model or reconstructing their attributes/entities.
fn xml_children(xml: &str, root: &str) -> anyhow::Result<Vec<(String, String)>> {
    super::xml::validate_document(xml, root)?;
    anyhow::ensure!(
        !xml.contains("<!DOCTYPE"),
        "gamelist document types are unsupported"
    );
    let mut reader = Reader::from_str(xml);
    let mut opened = false;
    let mut closed = false;
    let mut children = Vec::new();
    loop {
        let start = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Start(element) if !opened => {
                anyhow::ensure!(
                    element.name().as_ref() == root.as_bytes() && !closed,
                    "unexpected gamelist root"
                );
                opened = true;
            }
            Event::Empty(element) if !opened => {
                anyhow::ensure!(
                    element.name().as_ref() == root.as_bytes() && !closed,
                    "unexpected gamelist root"
                );
                opened = true;
                closed = true;
            }
            Event::Start(element) | Event::Empty(element) => {
                anyhow::ensure!(!closed, "multiple gamelist roots");
                let name = String::from_utf8(element.name().as_ref().to_vec())?;
                let name_bytes = element.name();
                // Empty elements are already complete; nonempty ones include
                // their matching close tag, checked by the library reader.
                let end = reader.buffer_position() as usize;
                if !xml[start..end].trim_end().ends_with("/>") {
                    reader.read_to_end(name_bytes)?;
                }
                children.push((
                    name,
                    xml[start..reader.buffer_position() as usize].to_owned(),
                ));
            }
            Event::End(element) => {
                anyhow::ensure!(
                    opened && !closed && element.name().as_ref() == root.as_bytes(),
                    "unexpected gamelist end"
                );
                closed = true;
            }
            Event::Text(text) => {
                anyhow::ensure!(text.decode()?.trim().is_empty(), "unexpected gamelist text")
            }
            Event::Eof => break,
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
            _ => anyhow::bail!("unsupported gamelist XML event"),
        }
    }
    anyhow::ensure!(opened && closed, "incomplete gamelist XML");
    Ok(children)
}
