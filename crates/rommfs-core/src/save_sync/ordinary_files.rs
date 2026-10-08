use super::{
    file_slot, path_from_file_slot, portable_save_path, resolve_save_target, save_file_key,
    MappingReport, SaveMapping, SaveProfile,
};
use crate::catalog::Catalogue;
use crate::romm::{RemoteSave, RomDto};
use crate::{Error, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct SaveFileRules {
    server: String,
    games: BTreeMap<i64, GameRule>,
    explicit: BTreeMap<String, i64>,
}
#[derive(Clone)]
struct GameRule {
    name: String,
    platform: String,
    stems: HashSet<String>,
    target: Option<(String, String)>,
}
pub struct SaveFilePlan {
    pub report: MappingReport,
    pub unmatched: Vec<String>,
}
impl GameRule {
    fn legacy_path(&self) -> Option<PathBuf> {
        let (stem, extension) = self.name.rsplit_once('.')?;
        (self.platform == "gb"
            && self.stems.len() == 1
            && extension.eq_ignore_ascii_case("gb")
            && !stem.contains('/'))
        .then(|| PathBuf::from(format!("gb/{stem}.srm")))
    }
}
impl SaveFileRules {
    pub fn from_catalogue(
        catalogue: &Catalogue,
        roms: &[RomDto],
        explicit: BTreeMap<String, i64>,
    ) -> Result<Self> {
        let source: HashMap<_, _> = roms.iter().map(|r| (r.id, r)).collect();
        let mut games: BTreeMap<i64, GameRule> = BTreeMap::new();
        for entry in catalogue
            .entries
            .iter()
            .filter(|e| catalogue.is_launchable(&e.key))
        {
            let game = games.entry(entry.key.rom_id).or_insert_with(|| GameRule {
                name: entry.file_name.clone(),
                platform: entry.platform_dir.to_lowercase(),
                stems: HashSet::new(),
                target: source.get(&entry.key.rom_id).and_then(|rom| {
                    Some((
                        rom.metadata.save_target.clone()?,
                        rom.metadata.save_target_layout.clone()?,
                    ))
                }),
            });
            if let Some(stem) = Path::new(&entry.file_name)
                .file_stem()
                .and_then(|s| s.to_str())
            {
                game.stems.insert(stem.to_lowercase());
            }
        }
        let server = catalogue
            .entries
            .first()
            .map(|e| e.key.server_id.clone())
            .unwrap_or_default();
        for path in explicit.keys() {
            file_slot(Path::new(path))?;
        }
        Ok(Self {
            server,
            games,
            explicit,
        })
    }
    pub fn game_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.games.keys().copied()
    }
    pub fn game_names(&self) -> Vec<(i64, String)> {
        self.games
            .iter()
            .map(|(id, g)| (*id, g.name.clone()))
            .collect()
    }
    pub fn mapping(&self, rom_id: i64, path: &Path, root: &Path) -> Result<SaveMapping> {
        let game = self.games.get(&rom_id).ok_or_else(|| {
            Error::Unsupported("Remote save game is not in this catalogue".into())
        })?;
        file_slot(path)?;
        if shared_path(&portable_save_path(path)?) {
            return Err(Error::Unsupported(
                "Shared saves/cards and RomMFS backup files cannot be associated for sync".into(),
            ));
        }
        Ok(SaveMapping {
            rom_key: save_file_key(&self.server, rom_id, path)?,
            profile: SaveProfile::ordinary_files(),
            relative_path: path.into(),
            target_path: resolve_save_target(root, path)?,
            visible_rom_name: format!("{} · {}", game.name, path.display()),
            legacy_compatible: game.legacy_path().as_deref() == Some(path),
        })
    }
    /// Restore only a current owner; keep unavailable or reassigned owners in
    /// the journal for review without scheduling their artifacts for transfer.
    pub fn tracked_mapping(
        &self,
        rom_id: i64,
        path: &Path,
        root: &Path,
    ) -> Result<Option<SaveMapping>> {
        let relative = portable_save_path(path)?;
        if !self.games.contains_key(&rom_id)
            || self.explicit.get(&relative).is_some_and(|id| *id != rom_id)
        {
            return Ok(None);
        }
        self.mapping(rom_id, path, root).map(Some)
    }
    pub fn associate(&self, path: &Path, root: &Path) -> Result<Option<SaveMapping>> {
        let relative = portable_save_path(path)?;
        if shared_path(&relative) {
            return Ok(None);
        }
        if let Some(id) = self.explicit.get(&relative) {
            return self.mapping(*id, path, root).map(Some);
        }
        let lower = relative.to_lowercase();
        let components: Vec<_> = lower.split('/').collect();
        let mut owners = HashSet::new();
        for (id, game) in &self.games {
            // A system-rooted filename convention, or a source-provided save
            // target. Never search arbitrary emulator portable directories.
            if components.first().copied() != Some(game.platform.as_str()) {
                continue;
            }
            let suffix = components[1..].join("/");
            let conventional = components.get(1).is_some_and(|part| {
                let stem = Path::new(part)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(part);
                game.stems.contains(*part) || game.stems.contains(stem)
            });
            let targeted = game.target.as_ref().is_some_and(|(target, layout)| {
                let target = target.replace('\\', "/").to_lowercase();
                match layout.as_str() {
                    "folder-exact" => suffix.starts_with(&format!("{target}/")),
                    "file-exact" => suffix == target,
                    "file-prefix" => {
                        suffix.starts_with(&target) && !suffix[target.len()..].contains('/')
                    }
                    "folder-prefix" => {
                        suffix
                            .split('/')
                            .next()
                            .is_some_and(|part| part.starts_with(&target))
                            && suffix.contains('/')
                    }
                    // Split layouts need an explicit per-game rule; a prefix
                    // guess could capture shared saves from another title.
                    _ => false,
                }
            });
            if conventional || targeted {
                owners.insert(*id);
            }
        }
        if owners.len() != 1 {
            return Ok(None);
        }
        self.mapping(*owners.iter().next().expect("one owner"), path, root)
            .map(Some)
    }
    pub fn scan(&self, root: &Path) -> Result<SaveFilePlan> {
        super::ensure_no_reparse_components(root)?;
        let mut plan = SaveFilePlan {
            report: MappingReport::default(),
            unmatched: self
                .explicit
                .iter()
                .filter(|(_, id)| !self.games.contains_key(id))
                .map(|(path, id)| {
                    format!("{path}: associated game {id} is unavailable; select another game")
                })
                .collect(),
        };
        let mut pending = vec![root.to_path_buf()];
        let mut count = 0usize;
        while let Some(directory) = pending.pop() {
            super::ensure_no_reparse_components(&directory)?;
            if !directory.exists() {
                continue;
            }
            let _guard = super::hold_save_directory_chain(&directory)?;
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                let metadata = std::fs::symlink_metadata(&path)?;
                count += 1;
                if count > 100_000 {
                    return Err(Error::Unsupported(
                        "Save scan exceeds 100,000 entries; choose a narrower save folder".into(),
                    ));
                }
                let relative = path.strip_prefix(root).map_err(|_| {
                    Error::Unsupported("Save scan escaped its selected folder".into())
                })?;
                let display = relative.to_string_lossy().into_owned();
                if super::path_is_reparse_point(&metadata) || portable_save_path(relative).is_err()
                {
                    plan.unmatched
                        .push(format!("{display}: redirected or unsafe path"));
                    continue;
                }
                if shared_path(&display) {
                    plan.unmatched
                        .push(format!("{display}: shared saves/cards are not supported"));
                    continue;
                }
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() {
                    if self
                        .explicit
                        .get(&portable_save_path(relative)?)
                        .is_some_and(|id| !self.games.contains_key(id))
                    {
                        continue;
                    }
                    match self.associate(relative, root) {
                        Ok(Some(mapping)) => plan.report.mappings.push(mapping),
                        Ok(None) => plan
                            .unmatched
                            .push(format!("{display}: select the owning game")),
                        Err(error) => plan.unmatched.push(format!("{display}: {error}")),
                    }
                }
            }
        }
        plan.report
            .mappings
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        ensure_distinct(&plan.report.mappings)?;
        Ok(plan)
    }
    pub fn remote_mappings(
        &self,
        inventory: &[RemoteSave],
        account: i64,
        root: &Path,
    ) -> Result<Vec<SaveMapping>> {
        let mut mappings = BTreeMap::new();
        for save in inventory {
            if save.user_id != account {
                return Err(Error::Unsupported(
                    "Save inventory belongs to another account".into(),
                ));
            }
            if !self.games.contains_key(&save.rom_id) {
                continue;
            }
            if save.slot.as_deref() == Some(super::RETROBAT_GB_SRM_PROFILE) {
                let game = &self.games[&save.rom_id];
                if let Some(relative) = game.legacy_path() {
                    // Legacy slots were game-scoped, so the single visible stem
                    // is the only recoverable original filename contract.
                    if let Some(mapping) = self.tracked_mapping(save.rom_id, &relative, root)? {
                        mappings.insert((save.rom_id, relative), mapping);
                    }
                }
                continue;
            }
            let Some(slot) = save
                .slot
                .as_deref()
                .filter(|s| s.starts_with(super::FILE_SLOT_PREFIX))
            else {
                continue;
            };
            let relative = path_from_file_slot(slot)?;
            if shared_path(&portable_save_path(&relative)?) {
                continue;
            }
            if let Some(mapping) = self.tracked_mapping(save.rom_id, &relative, root)? {
                mappings.insert((save.rom_id, relative), mapping);
            }
        }
        let mappings: Vec<_> = mappings.into_values().collect();
        ensure_distinct(&mappings)?;
        Ok(mappings)
    }
}
fn shared_path(path: &str) -> bool {
    path.replace('\\', "/").split('/').any(|part| {
        part.contains(".rommfs-")
            || part.starts_with(".rommfs")
            || matches!(
                part.to_ascii_lowercase().as_str(),
                "shared" | "memcards" | "memorycards" | "memory_cards"
            )
    })
}
pub fn ensure_distinct(mappings: &[SaveMapping]) -> Result<()> {
    let mut paths: HashMap<String, (i64, String)> = HashMap::new();
    let mut keys = HashMap::new();
    for mapping in mappings {
        let original = portable_save_path(&mapping.relative_path)?;
        let path = original.to_lowercase();
        if let Some((owner, previous)) =
            paths.insert(path.clone(), (mapping.rom_key.rom_id, original.clone()))
        {
            if owner != mapping.rom_key.rom_id || previous != original {
                return Err(Error::Unsupported(
                    "Save path has multiple owners or a Windows case collision".into(),
                ));
            }
        }
        if let Some(previous) = keys.insert(mapping.rom_key.clone(), path.clone()) {
            if previous != path {
                return Err(Error::Unsupported(
                    "Save file identity collision; no files were merged".into(),
                ));
            }
        }
    }
    Ok(())
}
