use crate::catalog::Catalogue;
use crate::romm::RomDto;
use anyhow::Context;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    path: String,
    frontend_fields: Vec<String>,
    #[serde(default = "initially_active")]
    active: bool,
}

fn initially_active() -> bool {
    true
}

/// Complete publication built without reading or downloading any ROM body.
pub struct MetadataPublication {
    pub(crate) gamelists: BTreeMap<String, Vec<u8>>,
    pub(crate) index: MetadataIndex,
}
impl MetadataPublication {
    pub fn gamelists(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.gamelists
    }
    pub fn index(&self) -> &MetadataIndex {
        &self.index
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

/// Merge RomM descriptive fields into actual projected paths. Unknown frontend
/// fields (including favourites, play history, hidden flags and collections) are
/// copied intact. Previous paths, not display titles, locate frontend state.
pub fn generate_gamelists(
    catalogue: &Catalogue,
    roms: &[RomDto],
    server_id: &str,
    account_id: i64,
    previous: &MetadataIndex,
    existing: &BTreeMap<String, String>,
    artwork: &HashMap<(i64, &'static str), String>,
) -> anyhow::Result<MetadataPublication> {
    anyhow::ensure!(account_id > 0, "metadata requires a verified account");
    anyhow::ensure!(previous.schema == 0 || (previous.schema == 1 && previous.server_id == server_id && previous.account_id == account_id),
        "frontend metadata belongs to another server/account or an unsupported schema; existing state was preserved");
    let mut index = previous.clone();
    index.schema = 1;
    index.server_id = server_id.into();
    index.account_id = account_id;
    let mut previous_fields = HashMap::new();
    let mut folders = BTreeMap::new();
    for (platform, xml) in existing {
        crate::save_sync::validate_windows_path_component(platform).map_err(anyhow::Error::msg)?;
        let mut folder_nodes = Vec::new();
        for (kind, node) in xml_children(xml, "gameList")? {
            if kind != "game" {
                folder_nodes.push(node);
                continue;
            }
            let mut path = None;
            let mut fields = Vec::new();
            for (name, field) in xml_children(&node, "game")? {
                if name == "path" {
                    anyhow::ensure!(path.is_none(), "duplicate gamelist path");
                    #[derive(Deserialize)]
                    struct Value {
                        #[serde(rename = "$text")]
                        text: String,
                    }
                    let value: Value = quick_xml::de::from_str(&field)?;
                    let relative = value.text.strip_prefix("./").unwrap_or(&value.text);
                    anyhow::ensure!(
                        !relative.contains('\\')
                            && relative
                                .split('/')
                                .all(|p| crate::save_sync::validate_windows_path_component(p)
                                    .is_ok()),
                        "unsafe frontend gamelist path"
                    );
                    path = Some(format!("{platform}/{relative}"));
                } else if !GENERATED.contains(&name.as_str()) {
                    fields.push(field);
                }
            }
            if let Some(path) = path {
                anyhow::ensure!(
                    previous_fields
                        .insert(path.to_lowercase(), fields)
                        .is_none(),
                    "duplicate frontend game path"
                );
            }
        }
        folders.insert(platform.clone(), folder_nodes);
    }
    // Capture all old identities before updating any paths, including games
    // that disappeared in the new catalogue. This also survives name swapping.
    if !previous.pending_publication {
        for game in index.games.values_mut() {
            if let Some(fields) = previous_fields
                .get(&game.path.to_lowercase())
                .filter(|_| game.active)
            {
                game.frontend_fields = fields.clone();
            }
        }
    }
    index.pending_publication = false;
    for game in index.games.values_mut() {
        game.active = false;
    }
    let roms: HashMap<_, _> = roms.iter().map(|r| (r.id, r)).collect();
    let mut writers = BTreeMap::new();
    // Empty old-platform gamelists too: retaining their bytes/history must not
    // leave removed games advertised as launchable files after a remount.
    let platforms: BTreeSet<_> = catalogue.platforms.iter().chain(existing.keys()).collect();
    for platform in platforms {
        if platform == "switchupdates" {
            continue;
        }
        let mut writer = Writer::new(Vec::new());
        writer.write_event(Event::Start(BytesStart::new("gameList")))?;
        if let Some(nodes) = folders.remove(platform) {
            for node in nodes {
                writer.get_mut().extend_from_slice(node.as_bytes());
            }
        }
        writers.insert(platform.clone(), writer);
    }
    for entry in catalogue
        .entries
        .iter()
        .filter(|e| catalogue.is_launchable(&e.key))
    {
        anyhow::ensure!(
            entry.key.server_id == server_id,
            "metadata catalogue belongs to another server"
        );
        let rom = roms
            .get(&entry.key.rom_id)
            .context("launchable file has no metadata source")?;
        let path = format!("{}/{}", entry.platform_dir, entry.file_name);
        let identity = format!("{}:{}", entry.key.rom_id, entry.key.file_id);
        let state = index.games.entry(identity).or_insert_with(|| IndexedGame {
            path: path.clone(),
            frontend_fields: Vec::new(),
            active: true,
        });
        state.path = path;
        state.active = true;
        let writer = writers
            .get_mut(&entry.platform_dir)
            .context("missing gamelist platform")?;
        writer.write_event(Event::Start(BytesStart::new("game")))?;
        field(writer, "path", &format!("./{}", entry.file_name))?;
        field(
            writer,
            "name",
            rom.metadata
                .name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&rom.fs_name),
        )?;
        if let Some(summary) = &rom.metadata.summary {
            field(writer, "desc", summary)?;
        }
        if let Some(metadata) = &rom.metadata.metadatum {
            for (name, values) in [
                ("genre", &metadata.genres),
                ("developer", &metadata.developers),
                ("publisher", &metadata.publishers),
            ] {
                if !values.is_empty() {
                    field(writer, name, &values.join(", "))?;
                }
            }
            if let Some(players) = &metadata.player_count {
                field(writer, "players", players)?;
            }
            if let Some(date) = metadata
                .first_release_date
                .and_then(|d| chrono::DateTime::from_timestamp(d, 0))
            {
                field(
                    writer,
                    "releasedate",
                    &date.format("%Y%m%dT000000").to_string(),
                )?;
            }
            if let Some(rating) = metadata.average_rating.filter(|r| r.is_finite()) {
                field(
                    writer,
                    "rating",
                    &(rating / 100.0).clamp(0.0, 1.0).to_string(),
                )?;
            }
        }
        for role in ["image", "thumbnail"] {
            if let Some(path) = artwork.get(&(rom.id, role)) {
                anyhow::ensure!(
                    path.starts_with(".rommfs-artwork/")
                        && !path.contains('\\')
                        && path.split('/').all(|part| {
                            crate::save_sync::validate_windows_path_component(part).is_ok()
                        }),
                    "unsafe artwork reference"
                );
                field(writer, role, &format!("./{path}"))?;
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
    }
    let mut gamelists = BTreeMap::new();
    for (platform, mut writer) in writers {
        writer.write_event(Event::End(BytesEnd::new("gameList")))?;
        gamelists.insert(format!("{platform}/gamelist.xml"), writer.into_inner());
    }
    Ok(MetadataPublication { gamelists, index })
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
