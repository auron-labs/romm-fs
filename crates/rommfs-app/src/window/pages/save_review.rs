use super::super::*;
use gpui_kit::component::pagination::Pagination;

/// Bounded review-table page (UI-PLAN §5): every result stays reachable
/// through search, filtering, and paging — never behind a truncated dump.
const REVIEW_PAGE_SIZE: usize = 50;

/// Current work leads each row; retained versions stay inside save details.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowStatus {
    NeedsAttention,
    ConflictingChanges,
    NotLinked,
    NotChecked,
    Checking,
    WaitingToUpload,
    Uploading,
    WaitingForRomm,
    UpToDate,
    Excluded,
}

impl RowStatus {
    fn label(self) -> &'static str {
        match self {
            Self::NeedsAttention => "Needs review",
            Self::ConflictingChanges => "Conflicting changes",
            Self::NotLinked => "Not linked",
            Self::NotChecked => "Not checked",
            Self::Checking => "Checking for changes…",
            Self::WaitingToUpload => "Waiting to upload",
            Self::Uploading => "Uploading to RomM…",
            Self::WaitingForRomm => "Waiting for RomM",
            Self::UpToDate => "Up to date",
            Self::Excluded => "Skipped",
        }
    }
    fn kind(self) -> StatusKind {
        match self {
            Self::NeedsAttention
            | Self::ConflictingChanges
            | Self::NotLinked
            | Self::WaitingForRomm => StatusKind::Warning,
            Self::UpToDate => StatusKind::Success,
            _ => StatusKind::Muted,
        }
    }
    fn rank(self) -> usize {
        match self {
            Self::NeedsAttention | Self::ConflictingChanges => 0,
            Self::NotLinked | Self::WaitingForRomm => 1,
            Self::WaitingToUpload | Self::Uploading | Self::Checking => 2,
            Self::NotChecked => 3,
            Self::UpToDate => 4,
            Self::Excluded => 5,
        }
    }
}
/// The design's filter pills: All, Needs review, Tracked, Not linked,
/// Skipped — each with a live count beside its label.
const SAVE_FILTERS: [&str; 5] = ["All", "Needs review", "Tracked", "Not linked", "Skipped"];

fn filter_matches(filter_ix: usize, row: &ReviewRow) -> bool {
    match filter_ix {
        1 => matches!(
            row.status,
            RowStatus::NeedsAttention | RowStatus::ConflictingChanges
        ),
        // "Tracked" = rows the sync queue actually watches: linked games and
        // memory-card conflicts, never unmatched or skipped files.
        2 => !matches!(row.status, RowStatus::NotLinked | RowStatus::Excluded),
        3 => row.status == RowStatus::NotLinked,
        4 => row.status == RowStatus::Excluded,
        _ => true,
    }
}

/// One pill in the filter row; the active pill keeps the selected surface.
fn filter_pill(
    index: usize,
    label: &str,
    count: usize,
    active: bool,
    cx: &mut Context<RommfsWindow>,
) -> Button {
    Button::new(("save-filter", index))
        .label(format!("{label} {}", fmt_count(count)))
        .small()
        .rounded(gpui_kit::component::button::ButtonRounded::Large)
        .ghost()
        .toggled(active)
        .on_click(cx.listener(move |view, _, _, cx| {
            view.save_review_filter = index;
            view.save_review_page = 0;
            cx.notify();
        }))
}

/// Everything a row's expanded detail needs — keyed by domain identity
/// (file path, rom/file id, incoming id), never by row position.
enum RowPayload {
    /// A file the save-folder scan could not associate with a game.
    Unmatched {
        path: PathBuf,
        reason: String,
        mappable: bool,
        /// Save-root-relative containing directory ("" at the root).
        dir: String,
        /// Mappable files in the same directory — the AssociateSaveFolder scope.
        dir_mappable: usize,
    },
    /// A linked game in the live reconciliation queue.
    Game {
        local_path: String,
        rom_id: i64,
        file_id: i64,
        issue: Option<String>,
        local_hash: Option<String>,
        remote_id: Option<String>,
        remote_hash: Option<String>,
        installed_incoming: bool,
    },

    /// A card whose local and remote copies both changed; the remote copy is
    /// kept beside the local card for review.
    MemoryCard(MemoryCardConflictStatus),
}

struct ReviewRow {
    key: SharedString,
    /// File path or save/game title shown in the File column.
    item: String,
    status: RowStatus,
    /// Reason, issue, or hash summary shown in the Detail column.
    detail: String,
    payload: RowPayload,
}

/// One row per supported identity, plus eligible unlinked files. Structural
/// exclusions are collected separately for the skipped-files disclosure.
fn collect_review_rows(state: &UiState, show_ignored: bool) -> Vec<ReviewRow> {
    let mut rows = Vec::new();

    // Folder scope for AssociateSaveFolder: mappable files per save-root dir.
    let mut dir_mappable: std::collections::BTreeMap<String, usize> = Default::default();
    for file in &state.save_file_unmatched {
        if file.mappable {
            let dir = file
                .path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            *dir_mappable.entry(dir).or_default() += 1;
        }
    }

    for file in &state.save_file_unmatched {
        if !file.mappable && !show_ignored {
            continue;
        }
        let path = file.path.display().to_string();
        let dir = file
            .path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        rows.push(ReviewRow {
            key: SharedString::from(format!("unmatched:{path}")),
            item: path,
            status: if file.mappable {
                RowStatus::NotLinked
            } else {
                RowStatus::Excluded
            },
            detail: file.reason.clone(),
            payload: RowPayload::Unmatched {
                path: file.path.clone(),
                reason: file.reason.clone(),
                mappable: file.mappable,
                dir_mappable: dir_mappable.get(&dir).copied().unwrap_or(0),
                dir,
            },
        });
    }

    if let Some(queue) = &state.save_sync_queue {
        for game in &queue.games {
            use rommfs_core::events::SaveSyncWork;
            let confirmed_conflict = state
                .save_sync_review_versions(game.rom_id, game.file_id)
                .any(|version| version.reason == "two_sided_conflict");
            let status = if confirmed_conflict {
                RowStatus::ConflictingChanges
            } else if game.work == SaveSyncWork::NeedsReview || !game.incoming_ids.is_empty() {
                RowStatus::NeedsAttention
            } else if state.save_sync_checking && state.save_sync_enabled {
                RowStatus::Checking
            } else {
                match game.work {
                    SaveSyncWork::NotChecked => RowStatus::NotChecked,
                    SaveSyncWork::Checking => RowStatus::Checking,
                    SaveSyncWork::WaitingToUpload => RowStatus::WaitingToUpload,
                    SaveSyncWork::Uploading => RowStatus::Uploading,
                    SaveSyncWork::WaitingForRomm => RowStatus::WaitingForRomm,
                    SaveSyncWork::NeedsReview => RowStatus::NeedsAttention,
                    SaveSyncWork::UpToDate => RowStatus::UpToDate,
                }
            };
            let detail = if matches!(
                status,
                RowStatus::NeedsAttention | RowStatus::ConflictingChanges
            ) {
                "Review details".to_string()
            } else if !state.save_sync_enabled {
                "Last known result · automatic sync is off".to_string()
            } else if status == RowStatus::UpToDate {
                "Local and RomM copies match".to_string()
            } else {
                "Save details".to_string()
            };
            rows.push(ReviewRow {
                key: SharedString::from(format!("game:{}:{}", game.rom_id, game.file_id)),
                item: format!(
                    "{} · {}",
                    game.rom_name,
                    std::path::Path::new(&game.local_path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
                status,
                detail,
                payload: RowPayload::Game {
                    local_path: game.local_path.clone(),
                    rom_id: game.rom_id,
                    file_id: game.file_id,
                    issue: game.issue.clone(),
                    local_hash: game.local_hash.clone(),
                    remote_id: game.remote_id.clone(),
                    remote_hash: game.remote_hash.clone(),
                    installed_incoming: game.installed_incoming,
                },
            });
        }
    }

    for conflict in &state.memory_card_conflicts {
        rows.push(ReviewRow {
            key: SharedString::from(format!(
                "memory-card:{}:{}",
                conflict.emulator, conflict.card_name
            )),
            item: format!("{} · {}", conflict.card_name, conflict.emulator),
            status: RowStatus::ConflictingChanges,
            detail: format!(
                "local and remote both changed · remote version {}",
                conflict.remote_version_id
            ),
            payload: RowPayload::MemoryCard(conflict.clone()),
        });
    }

    rows.sort_by(|a, b| {
        (a.status.rank(), a.item.to_lowercase()).cmp(&(b.status.rank(), b.item.to_lowercase()))
    });
    rows
}

fn row_matches_query(row: &ReviewRow, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    row.item.to_lowercase().contains(query)
        || row.detail.to_lowercase().contains(query)
        || row.status.label().to_lowercase().contains(query)
}

/// The game chosen in the shared picker, parsed from its "id · name" value.
fn selected_rom_id(view: &RommfsWindow, cx: &App) -> Option<i64> {
    view.save_game_select
        .read(cx)
        .selected_value()
        .and_then(|value| value.split_once(" · "))
        .and_then(|(id, _)| id.parse::<i64>().ok())
}

fn toggle_expanded(view: &mut RommfsWindow, key: &SharedString) {
    view.save_review_expanded = if view.save_review_expanded.as_ref() == Some(key) {
        None
    } else {
        Some(key.clone())
    };
}

impl RommfsWindow {
    /// A slim status card that only appears when sync state needs words:
    /// the derived status pill, the off-state hint or paused problem, the
    /// last failure, and the existing-file scan inventory (UI-PLAN §5).
    fn save_review_status_card(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let enabled = self.state.save_sync_enabled;
        let (status, status_kind) = self.save_sync_status();
        let failure = self
            .state
            .save_sync_queue
            .as_ref()
            .and_then(|queue| queue.failure.clone())
            .or_else(|| self.state.save_sync_failure.clone())
            .filter(|_| enabled);
        let hint = self.state.save_sync_off_hint().map(str::to_owned);
        let needs_folder = !enabled && !self.state.save_sync_available && hint.is_none();
        let paused = enabled
            .then(|| self.state.save_sync_problem.clone())
            .flatten();
        let (inventory_chips, inventory_details) =
            existing_save_inventory(self.state.save_sync_existing_saves.as_ref());
        if failure.is_none()
            && hint.is_none()
            && !needs_folder
            && paused.is_none()
            && inventory_chips.is_empty()
        {
            return None;
        }
        let mut body = v_flex()
            .gap_2()
            .child(status_pill(status_kind, status, cx))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(muted_xs(
                        format!(
                            "{} tracked · {} eligible unlinked saves",
                            plural(self.state.save_sync_mapped_targets, "save"),
                            fmt_count(self.state.save_sync_catalogue_unmapped)
                        ),
                        cx,
                    ))
                    .when(self.state.save_files_ignored() > 0, |this| {
                        this.child(muted_xs(
                            format!(
                                "· {} skipped files",
                                fmt_count(self.state.save_files_ignored())
                            ),
                            cx,
                        ))
                    })
                    .children(inventory_chips.into_iter().map(chip)),
            );
        if let Some(details) = inventory_details {
            body = body.child(details_disclosure(
                "save-inventory-details",
                details,
                window,
                cx,
            ));
        }
        if let Some(hint) = hint {
            body = body
                .child(muted_xs(hint, cx))
                .child(self.settings_nav_button(
                    "save-sync-open-settings",
                    "Open Settings",
                    3,
                    false,
                    cx,
                ));
        } else if needs_folder {
            body = body.child(muted_xs("Save sync needs a save folder.", cx));
        }
        if let Some(problem) = paused {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().warning)
                    .child(format!("Paused: {problem}")),
            );
        }
        if let Some(failure) = failure {
            body = body.child(error_line(format!("Last failure: {failure}"), cx));
        }
        Some(card(cx).p_4().child(body).into_any_element())
    }

    /// Detail block under an expanded row: the row's full identity plus the
    /// commands that apply to it.
    fn save_review_row_detail(
        &mut self,
        row: &ReviewRow,
        has_games: bool,
        can_export: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut detail = v_flex().gap_2().px_3().py_2();
        match &row.payload {
            RowPayload::Unmatched {
                path,
                reason,
                mappable,
                dir,
                dir_mappable,
            } => {
                detail = detail
                    .child(
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(path.display().to_string()),
                    )
                    .child(muted_xs(reason.clone(), cx));
                if *mappable && has_games {
                    detail = detail.child(
                        Select::new(&self.save_game_select)
                            .w_full()
                            .accessibility_label("Save file game")
                            .placeholder("Select the owning game…"),
                    );
                    let file_path = path.clone();
                    let mut actions = h_flex().gap_2().child(
                        Button::new(format!("save-review-map-file-{}", row.key))
                            .label("Map file")
                            .outline()
                            .small()
                            .on_click(cx.listener(move |view, _, _, cx| {
                                if let Some(rom_id) = selected_rom_id(view, cx) {
                                    view.controller.send(Command::AssociateSaveFile {
                                        relative_path: file_path.clone(),
                                        rom_id,
                                    });
                                }
                                cx.notify();
                            })),
                    );
                    if *dir_mappable > 1 {
                        let prefix = dir.clone();
                        let dir_display = if dir.is_empty() {
                            "the save folder root".to_string()
                        } else {
                            dir.to_string()
                        };
                        // §5: the dir-level action lists the actual files
                        // its selection produces (the same-dir, mappable
                        // scope of AssociateSaveFolder).
                        let affected: Vec<String> = self
                            .state
                            .save_file_unmatched
                            .iter()
                            .filter(|file| {
                                file.mappable
                                    && file
                                        .path
                                        .parent()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default()
                                        == *dir
                            })
                            .map(|file| file.path.display().to_string())
                            .collect();
                        detail = detail.child(
                            v_flex()
                                .gap_1()
                                .child(muted_xs(format!("Files this covers in {dir_display}:"), cx))
                                .children(affected.iter().take(8).map(|path| {
                                    div()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .child(path.clone())
                                }))
                                .when(affected.len() > 8, |this| {
                                    this.child(muted_xs(
                                        format!("…and {} more", affected.len() - 8),
                                        cx,
                                    ))
                                }),
                        );
                        actions = actions.child(
                            Button::new(format!("save-review-map-dir-{}", row.key))
                                .label(format!("Map {} files in {}", dir_mappable, dir_display))
                                .outline()
                                .small()
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    if let Some(rom_id) = selected_rom_id(view, cx) {
                                        view.controller.send(Command::AssociateSaveFolder {
                                            prefix: PathBuf::from(prefix.clone()),
                                            rom_id,
                                        });
                                    }
                                    cx.notify();
                                })),
                        );
                    }
                    detail = detail.child(actions);
                } else if *mappable {
                    detail = detail.child(muted_xs(
                        "Mapping unlocks once RomM returns the game list.",
                        cx,
                    ));
                } else {
                    detail = detail.child(muted_xs(
                        "This file is skipped. It is preserved locally and is not waiting to be linked.",
                        cx,
                    ));
                }
            }
            RowPayload::Game {
                local_path,
                rom_id,
                file_id,
                issue,
                local_hash,
                remote_id,
                remote_hash,
                installed_incoming,
            } => {
                let path_state = window.use_keyed_state(
                    SharedString::from(format!("save-local-path-{}", row.key)),
                    cx,
                    |window, cx| InputState::new(window, cx).default_value(local_path.clone()),
                );
                detail = detail.child(muted(format!("Linked to {}", row.item), cx))
                    .child(Input::new(&path_state).readonly(true).w_full())
                    .child(details_disclosure(SharedString::from(format!("save-diagnostics-{}", row.key)), format!("ROM {rom_id} · file {file_id}\nLocal hash: {}\nRemote ID: {}\nRemote hash: {}\n{}", local_hash.as_deref().unwrap_or("unknown"), remote_id.as_deref().unwrap_or("none"), remote_hash.as_deref().unwrap_or("unknown"), issue.as_deref().unwrap_or("")), window, cx));
                if *installed_incoming {
                    detail = detail.child(muted_xs(
                        "A verified remote save was installed for this game.",
                        cx,
                    ));
                }
                if let Some(issue) = issue {
                    detail = detail.child(section_heading("This save needs review"));
                    detail = detail.child(muted(if row.status == RowStatus::ConflictingChanges { "Local and RomM copies have diverged. Automatic sync is paused for this save; review both copies before recovering your progress." } else if issue.contains("outside the verified owned revision format; no remote file was adopted") {
                        "RomM returned a save version that RomM-FS could not verify. This unverified version was not applied. Automatic sync is paused for this save."
                    } else {
                        "Automatic sync needs attention for this save. Open Technical details for the reported issue."
                    }, cx));
                }
                let records = self
                    .state
                    .save_sync_review_versions(*rom_id, *file_id)
                    .cloned()
                    .collect::<Vec<_>>();
                if !records.is_empty() {
                    detail = detail.child(section_heading("Remote versions to review"));
                    for (index, record) in records.into_iter().enumerate() {
                        detail = detail
                            .child(muted(format!("Remote version {}", index + 1), cx))
                            .child(self.render_incoming_row(&record, can_export, window, cx));
                    }
                    detail = detail.child(muted("No automatic recovery is available for these review records. Export a copy for inspection; automatic sync cannot adopt an unverified version.", cx));
                }
            }
            RowPayload::MemoryCard(conflict) => {
                detail = detail
                    .child(muted_xs(
                        format!("{} · {}", conflict.emulator, conflict.relative_path),
                        cx,
                    ))
                    .child(muted_xs(
                        format!(
                            "Your local card was kept. The remote copy (version {}) is beside it as a .rommfs-conflict file or folder; nothing was overwritten.",
                            conflict.remote_version_id,
                        ),
                        cx,
                    ));
            }
        }
        detail
            .bg(cx.theme().muted)
            .border_t_1()
            .border_color(cx.theme().border)
    }

    /// One clickable, keyboard-operable table row plus its optional detail.
    /// Reuses the Disclosure key context so Enter/Space expand it.
    fn save_review_row(
        &mut self,
        row: &ReviewRow,
        has_games: bool,
        can_export: bool,
        is_last: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focus_handle = window
            .use_keyed_state(
                SharedString::from(format!("save-review-focus-{}", row.key)),
                cx,
                |_, cx| cx.focus_handle().tab_stop(true),
            )
            .read(cx)
            .clone();
        let focused = focus_handle.is_focused(window);
        let expanded = view_expanded(self, &row.key);
        let click_key = row.key.clone();
        let action_key = row.key.clone();
        let click_focus = focus_handle.clone();

        let row_el = h_flex()
            .id(SharedString::from(format!("save-review-row-{}", row.key)))
            .role(gpui_kit::Role::Button)
            .aria_label(SharedString::from(format!(
                "{} — {} — Save details",
                row.item,
                row.status.label()
            )))
            .aria_expanded(expanded)
            .key_context("Disclosure")
            .track_focus(&focus_handle)
            .on_action(cx.listener(move |view, _: &ActivateControl, _window, cx| {
                toggle_expanded(view, &action_key);
                cx.notify();
            }))
            .on_click(cx.listener(move |view, _, window, cx| {
                window.focus(&click_focus, cx);
                toggle_expanded(view, &click_key);
                cx.notify();
            }))
            .w_full()
            .gap_3()
            .min_h_9()
            .py_1()
            .px_3()
            .items_center()
            .border_1()
            .border_color(if focused {
                cx.theme().primary
            } else {
                gpui_kit::transparent_black()
            })
            .when(expanded, |this| this.bg(cx.theme().accent))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(row.item.clone())),
            )
            .child(div().w_40().flex_shrink_0().child(status_marker(
                row.status.kind(),
                row.status.label(),
                cx,
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().truncate().child(row.detail.clone())),
            )
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .small()
                .text_color(cx.theme().muted_foreground),
            );

        let mut entry = v_flex().child(row_el);
        if expanded {
            entry =
                entry.child(self.save_review_row_detail(row, has_games, can_export, window, cx));
        }
        if !is_last {
            entry = entry.border_b_1().border_color(cx.theme().border);
        }
        entry
    }

    /// Secondary linking form outside the current-saves table.
    fn save_review_link_action(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = SharedString::from("manual-map");
        let expanded = view_expanded(self, &key);
        let row_el = Button::new("link-save-file")
            .label("Link a save file…")
            .outline()
            .on_click(cx.listener(move |view, _, _, cx| {
                toggle_expanded(view, &key);
                cx.notify();
            }));
        let mut entry = v_flex()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(row_el);
        if expanded {
            entry = entry.child(
                v_flex()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .bg(cx.theme().muted)
                    .child(Input::new(&self.save_file_input).w_full())
                    .child(
                        Select::new(&self.save_game_select)
                            .w_full()
                            .accessibility_label("Save file game")
                            .placeholder("Select the owning game…"),
                    )
                    .child(
                        Button::new("save-review-map-manual")
                            .label("Map file")
                            .outline()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| {
                                let path = view.save_file_input.read(cx).value().to_string();
                                let rom_id = selected_rom_id(view, cx);
                                if let (Some(rom_id), false) = (rom_id, path.trim().is_empty()) {
                                    view.controller.send(Command::AssociateSaveFile {
                                        relative_path: PathBuf::from(path.trim()),
                                        rom_id,
                                    });
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        entry
    }
}

fn view_expanded(view: &RommfsWindow, key: &SharedString) -> bool {
    view.save_review_expanded.as_ref() == Some(key)
}

/// Save sync page per the redesign: header with the live toggle + check
/// action, an attention banner, count filter pills + search, the table card,
/// and pagination (UI-PLAN task 4 / §5).
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let has_games = !view.state.save_file_games.is_empty();
    let can_export = view.state.save_sync_effective_saves_root.is_some()
        && view.state.save_sync_account_id.is_some();
    let enabled = view.state.save_sync_enabled;

    let filter_ix = view.save_review_filter.min(SAVE_FILTERS.len() - 1);
    let query = view
        .save_review_search
        .read(cx)
        .value()
        .to_string()
        .to_lowercase();
    // Skipped files are real table rows under their own pill, so the unfiltered
    // set always includes them and the pill counts stay true.
    let rows = collect_review_rows(&view.state, true);
    let counts = [
        rows.len(),
        rows.iter().filter(|r| filter_matches(1, r)).count(),
        rows.iter().filter(|r| filter_matches(2, r)).count(),
        rows.iter().filter(|r| filter_matches(3, r)).count(),
        rows.iter().filter(|r| filter_matches(4, r)).count(),
    ];
    let filtered: Vec<&ReviewRow> = rows
        .iter()
        .filter(|row| filter_matches(filter_ix, row))
        .filter(|row| row_matches_query(row, &query))
        .collect();

    let total_pages = filtered.len().div_ceil(REVIEW_PAGE_SIZE).max(1);
    let page = view.save_review_page.min(total_pages - 1);
    let start = page * REVIEW_PAGE_SIZE;
    let end = (start + REVIEW_PAGE_SIZE).min(filtered.len());
    let page_rows = &filtered[start..end];

    let mut table = v_flex();
    table = table.child(
        h_flex()
            .w_full()
            .gap_3()
            .px_3()
            .h_8()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().flex_1().min_w_0().child(muted_xs("FILE", cx)))
            .child(div().w_40().flex_shrink_0().child(muted_xs("STATUS", cx)))
            .child(div().flex_1().min_w_0().child(muted_xs("DETAIL", cx)))
            .child(div().w_4().flex_shrink_0()),
    );
    if page_rows.is_empty() {
        let empty = if !query.is_empty() || filter_ix != 0 {
            "No results match this filter.".to_string()
        } else {
            match view.state.save_sync_summary() {
                SaveSyncSummary::Off => "No supported saves found. Automatic save sync is off.".into(),
                SaveSyncSummary::NeedsSetup => {
                    "Save sync needs a save folder. Configure one in Settings.".into()
                }
                SaveSyncSummary::Checking | SaveSyncSummary::Reconciling => {
                    "Checking saves…".into()
                }
                SaveSyncSummary::NoLinkedFiles => "Nothing to sync yet.".into(),
                _ => "No supported saves found. Check your save folder in Settings, then check for changes.".into(),
            }
        };
        table = table.child(div().px_3().py_4().child(muted(empty, cx)));
    } else {
        let count = page_rows.len();
        for (index, row) in page_rows.iter().enumerate() {
            table = table.child(view.save_review_row(
                row,
                has_games,
                can_export,
                index + 1 == count,
                window,
                cx,
            ));
        }
    }
    // The manual linking form lives inside the table card's footer (design).
    if has_games {
        table = table.child(view.save_review_link_action(window, cx));
    }

    let mut section = v_flex()
        .gap_3()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .children(SAVE_FILTERS.iter().enumerate().map(|(index, label)| {
                    filter_pill(index, label, counts[index], index == filter_ix, cx)
                }))
                .child(div().flex_1())
                .child(
                    div().w(px(220.)).flex_shrink_0().child(
                        Input::new(&view.save_review_search)
                            .w_full()
                            .prefix(Icon::new(IconName::Search).small()),
                    ),
                ),
        )
        .child(card(cx).child(table));

    if total_pages > 1 {
        let weak = cx.weak_entity();
        section = section.child(
            h_flex()
                .justify_between()
                .items_center()
                .child(muted_xs(
                    format!("Page {} of {}", page + 1, total_pages),
                    cx,
                ))
                .child(
                    Pagination::new("save-review-pagination")
                        .current_page(page + 1)
                        .total_pages(total_pages)
                        .small()
                        .on_click(move |target, _window, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.save_review_page = target.saturating_sub(1);
                                cx.notify();
                            });
                        }),
                ),
        );
    }

    let last_scan = view
        .state
        .save_scan_at
        .map(|at| format!("Last scan: {}", relative_time(at)))
        .unwrap_or_else(|| "No scan yet — automatic sync and manual checks appear here.".into());
    let sync_switch = Switch::new("save-sync-toggle")
        .label("Automatic sync")
        .checked(enabled)
        .disabled(!enabled && !view.state.save_sync_available)
        .on_change(cx.listener(|view, checked, _, cx| {
            view.state.request_save_sync_enabled(*checked);
            view.controller
                .send(Command::SetSaveSyncEnabled { enabled: *checked });
            cx.notify();
        }));
    let check_button = Button::new("save-sync-refresh")
        .icon(Icon::new(IconName::RefreshCw))
        .label("Check for changes")
        .outline()
        .disabled(view.state.save_sync_checking)
        .on_click(cx.listener(|view, _, _, cx| {
            view.state.save_sync_checking = true;
            view.controller.send(Command::RefreshSaveSync);
            cx.notify();
        }));
    let review_count = counts[1];

    v_flex()
        .gap_4()
        .child(page_header(
            "Saves",
            last_scan,
            Some(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(sync_switch)
                    .child(check_button)
                    .into_any_element(),
            ),
            cx,
        ))
        .when_some(view.save_review_status_card(window, cx), |this, card| {
            this.child(card)
        })
        .when(review_count > 0 && filter_ix != 1, |this| {
            this.child(banner(
                StatusKind::Warning,
                IconName::TriangleAlert,
                if review_count == 1 {
                    "1 save needs review".to_string()
                } else {
                    format!("{} saves need review", fmt_count(review_count))
                },
                Some(
                    Button::new("save-review-attention")
                        .label("Review details")
                        .warning()
                        .small()
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.save_review_filter = 1;
                            view.save_review_page = 0;
                            cx.notify();
                        })),
                ),
                cx,
            ))
        })
        .child(section)
}

impl RommfsWindow {
    fn render_incoming_row(
        &mut self,
        incoming: &SaveSyncIncomingStatus,
        can_export: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let incoming = incoming.clone();
        let error = self
            .state
            .save_sync_export_errors
            .get(&incoming.incoming_id)
            .cloned();
        let feedback = self
            .state
            .save_sync_export_feedback
            .get(&incoming.incoming_id)
            .cloned();
        let mut row = v_flex()
            .gap_1()
            .child(details_disclosure(
                SharedString::from(format!("incoming-details-{}", incoming.incoming_id)),
                format!(
                    "{} · RomM save {} · revision {} · {} · {} · {}",
                    incoming.rom_name,
                    incoming.remote_id,
                    incoming.incoming_id,
                    incoming.state,
                    incoming.reason,
                    incoming.content_hash
                ),
                window,
                cx,
            ))
            .child(
                h_flex().child(
                    Button::new(format!("export-{}", incoming.incoming_id))
                        .label("Export…")
                        .outline()
                        .small()
                        .disabled(!can_export)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.prompt_export(incoming.clone(), cx);
                        })),
                ),
            );
        if let Some(error) = error {
            row = row.child(error_line(format!("Export failed: {error}"), cx));
        }
        if let Some(path) = feedback {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().success)
                    .child(format!("Exported to {path}; review remains pending.")),
            );
        }
        row
    }
}

/// Existing-file scan as chips plus diagnostics behind "Details" (phase 2b:
/// counts read as numbers, the prose moves behind a disclosure).
fn existing_save_inventory(preview: Option<&ExistingSavePreview>) -> (Vec<String>, Option<String>) {
    let Some(preview) = preview else {
        return (vec![], None);
    };
    match preview.status {
        ExistingSaveScanStatus::Unavailable => (
            vec!["Save scan unavailable".into()],
            Some("Existing-file scan unavailable; counts are not reported.".into()),
        ),
        ExistingSaveScanStatus::Complete | ExistingSaveScanStatus::Partial => {
            let mut chips = vec![format!("{} recognised", preview.supported_files)];
            if preview.skipped_files > 0 {
                chips.push(format!("{} skipped", preview.skipped_files));
            }
            let mut details = Vec::new();
            if preview.status == ExistingSaveScanStatus::Partial {
                details.push("Scan was partial; counts may be incomplete.".to_string());
            }
            let reasons = preview
                .skipped_reasons
                .iter()
                .map(|count| format!("{} {}", count.files, count.reason.label()))
                .collect::<Vec<_>>()
                .join(", ");
            if !reasons.is_empty() {
                details.push(reasons);
            }
            details.extend(preview.diagnostics.iter().cloned());
            (chips, (!details.is_empty()).then(|| details.join("\n")))
        }
    }
}
