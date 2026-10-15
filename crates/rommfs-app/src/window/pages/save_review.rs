use super::super::*;
use gpui_kit::component::pagination::Pagination;
use std::cell::Cell;

/// Bounded review-table page (UI-PLAN §5): every result stays reachable
/// through search, filtering, and paging — never behind a truncated dump.
const REVIEW_PAGE_SIZE: usize = 50;

/// Per-row status words (UI-PLAN §5). `Incoming` covers remote saves the
/// journal keeps for explicit review; "Synced" is never claimed — every row
/// status derives from the live queue or the last scan.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowStatus {
    NeedsAttention,
    Incoming,
    NotLinked,
    Linked,
    Excluded,
}

impl RowStatus {
    fn label(self) -> &'static str {
        match self {
            Self::NeedsAttention => "Needs attention",
            Self::Incoming => "Incoming",
            Self::NotLinked => "Not linked",
            Self::Linked => "Linked",
            Self::Excluded => "Excluded",
        }
    }

    fn kind(self) -> StatusKind {
        match self {
            Self::NeedsAttention => StatusKind::Danger,
            Self::Incoming | Self::NotLinked => StatusKind::Warning,
            Self::Linked => StatusKind::Success,
            Self::Excluded => StatusKind::Muted,
        }
    }

    /// Rows needing a decision sort ahead of healthy and excluded ones.
    fn rank(self) -> usize {
        match self {
            Self::NeedsAttention => 0,
            Self::Incoming => 1,
            Self::NotLinked => 2,
            Self::Linked => 3,
            Self::Excluded => 4,
        }
    }
}

/// Status filter dropdown options; index 0 means "no filter".
const STATUS_FILTERS: &[Option<RowStatus>] = &[
    None,
    Some(RowStatus::NeedsAttention),
    Some(RowStatus::Incoming),
    Some(RowStatus::NotLinked),
    Some(RowStatus::Linked),
    Some(RowStatus::Excluded),
];

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
        rom_id: i64,
        file_id: i64,
        issue: Option<String>,
        local_hash: Option<String>,
        remote_id: Option<String>,
        remote_hash: Option<String>,
        installed_incoming: bool,
    },
    /// A verified remote save kept in the journal for review/export.
    Incoming(SaveSyncIncomingStatus),
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

/// One flat review list: queue failures and incoming saves first, then
/// unlinked files, linked games, and structural exclusions.
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
        if file.no_candidate && !show_ignored {
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
            let detail = match &game.issue {
                Some(issue) => issue.clone(),
                None => format!(
                    "local {} · remote ID {} · remote {}",
                    game.local_hash.as_deref().unwrap_or("unknown"),
                    game.remote_id.as_deref().unwrap_or("none"),
                    game.remote_hash.as_deref().unwrap_or("unknown"),
                ),
            };
            rows.push(ReviewRow {
                key: SharedString::from(format!("game:{}:{}", game.rom_id, game.file_id)),
                item: game.rom_name.clone(),
                status: if game.issue.is_some() {
                    RowStatus::NeedsAttention
                } else {
                    RowStatus::Linked
                },
                detail,
                payload: RowPayload::Game {
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
        for incoming in &queue.incoming {
            rows.push(ReviewRow {
                key: SharedString::from(format!("incoming:{}", incoming.incoming_id)),
                item: incoming.rom_name.clone(),
                status: RowStatus::Incoming,
                detail: format!("{} · remote save {}", incoming.reason, incoming.remote_id),
                payload: RowPayload::Incoming(incoming.clone()),
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
            status: RowStatus::NeedsAttention,
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
    /// Compact live status: the shared derived marker, problem/failure
    /// detail, and the page's only setup actions (enable + refresh).
    /// Folder/install selection lives in Settings (UI-PLAN task 3).
    fn save_review_status_strip(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let enabled = self.state.save_sync_enabled;
        let (status, status_kind) = self.save_sync_status();
        // One compact identity line replaces the old server/account/folder/
        // last-action block (phase 2b). Last action now lives in Activity.
        let identity = match (
            &self.state.save_sync_server_id,
            self.state.save_sync_account_id,
        ) {
            (Some(server), Some(_)) => {
                let user = self.user_input.read(cx).value().to_string();
                if user.is_empty() {
                    server.clone()
                } else {
                    format!("{server} · {user}")
                }
            }
            _ => "Server and account not verified".into(),
        };
        let checked = self
            .state
            .save_sync_profile_version
            .as_deref()
            .map(|version| format!("checked {version}"))
            .unwrap_or_else(|| "save folder not checked".into());
        let failure = self
            .state
            .save_sync_queue
            .as_ref()
            .and_then(|queue| queue.failure.clone())
            .or_else(|| self.state.save_sync_failure.clone());
        let (inventory_chips, inventory_details) =
            existing_save_inventory(self.state.save_sync_existing_saves.as_ref());

        let mut card = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(status_marker(status_kind, status, cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Button::new("save-sync-refresh")
                                    .label("Refresh")
                                    .outline()
                                    .small()
                                    .on_click(cx.listener(|view, _, _, _cx| {
                                        view.controller.send(Command::RefreshSaveSync);
                                    })),
                            )
                            .child(if enabled {
                                Button::new("save-sync-toggle")
                                    .label("Disable save sync")
                                    .outline()
                                    .small()
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.state.save_sync_enabled = false;
                                        view.controller
                                            .send(Command::SetSaveSyncEnabled { enabled: false });
                                        cx.notify();
                                    }))
                            } else {
                                Button::new("save-sync-toggle")
                                    .label("Enable save sync")
                                    .primary()
                                    .small()
                                    .disabled(!self.state.save_sync_available)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.controller
                                            .send(Command::SetSaveSyncEnabled { enabled: true });
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .child(muted_xs(format!("{identity} · {checked}"), cx))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(chip(format!(
                        "{} mapped",
                        fmt_count(self.state.save_sync_mapped_targets)
                    )))
                    .child(chip(format!(
                        "{} unmatched",
                        fmt_count(self.state.save_sync_catalogue_unmapped)
                    )))
                    .when(self.state.save_files_ignored() > 0, |this| {
                        this.child(chip(format!(
                            "{} ignored",
                            fmt_count(self.state.save_files_ignored())
                        )))
                    })
                    .children(inventory_chips.into_iter().map(chip)),
            );
        if let Some(details) = inventory_details {
            card = card.child(details_disclosure(
                "save-inventory-details",
                details,
                window,
                cx,
            ));
        }
        if let Some(hint) = self.state.save_sync_off_hint() {
            card = card
                .child(muted_xs(hint, cx))
                .child(self.settings_nav_button(
                    "save-sync-open-settings",
                    "Open Settings",
                    3,
                    false,
                    cx,
                ));
        } else if !enabled && !self.state.save_sync_available {
            card = card.child(muted_xs("Save sync needs a save folder.", cx));
        }
        if enabled {
            if let Some(problem) = self.state.save_sync_problem.clone() {
                card = card.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(format!("Paused: {problem}")),
                );
            }
        }
        let failure = failure.filter(|_| enabled);
        if let Some(failure) = failure {
            card = card.child(error_line(format!("Last failure: {failure}"), cx));
        }
        for path in &self.state.save_sync_preview {
            card = card.child(
                div()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground)
                    .child(path.clone()),
            );
        }
        div()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .p_3()
            .child(card)
    }

    /// Detail block under an expanded row: the row's full identity plus the
    /// commands that apply to it.
    fn save_review_row_detail(
        &mut self,
        row: &ReviewRow,
        has_games: bool,
        can_export: bool,
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
                        "Excluded from save sync; it stays unmatched for every game.",
                        cx,
                    ));
                }
            }
            RowPayload::Game {
                rom_id,
                file_id,
                issue,
                local_hash,
                remote_id,
                remote_hash,
                installed_incoming,
            } => {
                detail = detail
                    .child(muted_xs(format!("ROM {rom_id} · file {file_id}"), cx))
                    .child(muted_xs(
                        format!(
                            "local {} · remote ID {} · remote {}",
                            local_hash.as_deref().unwrap_or("unknown"),
                            remote_id.as_deref().unwrap_or("none"),
                            remote_hash.as_deref().unwrap_or("unknown"),
                        ),
                        cx,
                    ));
                if *installed_incoming {
                    detail = detail.child(muted_xs(
                        "A reviewed remote save was installed for this game.",
                        cx,
                    ));
                }
                if let Some(issue) = issue {
                    detail = detail.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().warning)
                            .child(format!("Attention: {issue}")),
                    );
                }
            }
            RowPayload::Incoming(incoming) => {
                detail = detail
                    .child(muted_xs(
                        format!("ROM {} · {}", incoming.rom_id, incoming.rom_name),
                        cx,
                    ))
                    .child(muted_xs(
                        format!(
                            "remote save {} · pending revision {} · {}",
                            incoming.remote_id, incoming.incoming_id, incoming.state,
                        ),
                        cx,
                    ))
                    .child(muted_xs(
                        format!("{} · {}", incoming.reason, incoming.content_hash),
                        cx,
                    ))
                    .child(self.render_incoming_row(incoming, can_export, cx));
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

        let row_el = h_flex()
            .id(SharedString::from(format!("save-review-row-{}", row.key)))
            .key_context("Disclosure")
            .track_focus(&focus_handle)
            .on_action(cx.listener(move |view, _: &ActivateControl, _window, cx| {
                toggle_expanded(view, &action_key);
                cx.notify();
            }))
            .on_click(cx.listener(move |view, _, _, cx| {
                toggle_expanded(view, &click_key);
                cx.notify();
            }))
            .w_full()
            .gap_3()
            .h_9()
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
            .child(div().w(px(112.)).flex_shrink_0().child(status_marker(
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
            entry = entry.child(self.save_review_row_detail(row, has_games, can_export, cx));
        }
        if !is_last {
            entry = entry.border_b_1().border_color(cx.theme().border);
        }
        entry
    }

    /// Footer row for files the scan never listed: type a save-root-relative
    /// path, pick the owning game, and Map file — the same AssociateSaveFile
    /// path the old typed input used.
    fn save_review_manual_row(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        const MANUAL_KEY: &str = "manual-map";
        let focus_handle = window
            .use_keyed_state(
                SharedString::from("save-review-focus-manual"),
                cx,
                |_, cx| cx.focus_handle().tab_stop(true),
            )
            .read(cx)
            .clone();
        let focused = focus_handle.is_focused(window);
        let key = SharedString::from(MANUAL_KEY);
        let expanded = view_expanded(self, &key);
        let click_key = key.clone();
        let action_key = key.clone();

        let row_el = h_flex()
            .id(SharedString::from("save-review-row-manual"))
            .key_context("Disclosure")
            .track_focus(&focus_handle)
            .on_action(cx.listener(move |view, _: &ActivateControl, _window, cx| {
                toggle_expanded(view, &action_key);
                cx.notify();
            }))
            .on_click(cx.listener(move |view, _, _, cx| {
                toggle_expanded(view, &click_key);
                cx.notify();
            }))
            .w_full()
            .gap_3()
            .h_9()
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
                    .child(div().truncate().child("Link a file by path")),
            )
            .child(div().w(px(112.)).flex_shrink_0().child(status_marker(
                RowStatus::NotLinked.kind(),
                "Manual",
                cx,
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .truncate()
                            .child("For saves the folder scan didn't list"),
                    ),
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

/// Save sync page: truthful status strip plus the searchable, paginated
/// review table (UI-PLAN task 4 / §5).
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let has_games = !view.state.save_file_games.is_empty();
    let can_export = view.state.save_sync_effective_saves_root.is_some()
        && view.state.save_sync_account_id.is_some();

    // Status filter dropdown — page-local keyed state, index 0 = all rows.
    let filter_state = window.use_keyed_state(
        SharedString::from("save-review-status-filter"),
        cx,
        |window, cx| {
            SelectState::new(
                STATUS_FILTERS
                    .iter()
                    .map(|filter| {
                        SharedString::from(filter.map(RowStatus::label).unwrap_or("All statuses"))
                    })
                    .collect::<Vec<_>>(),
                None,
                window,
                cx,
            )
        },
    );
    let filter_prev = window.use_keyed_state(
        SharedString::from("save-review-status-filter-prev"),
        cx,
        |_, _| Cell::new(0usize),
    );
    let filter_ix = filter_state
        .read(cx)
        .selected_index(cx)
        .map(|index| index.row)
        .unwrap_or(0);
    if filter_ix != filter_prev.read(cx).get() {
        filter_prev.update(cx, |prev, _| prev.set(filter_ix));
        view.save_review_page = 0;
    }
    let status_filter = STATUS_FILTERS.get(filter_ix).copied().flatten();

    let query = view
        .save_review_search
        .read(cx)
        .value()
        .to_string()
        .to_lowercase();
    let rows = collect_review_rows(&view.state, view.save_review_show_ignored);
    let total_rows = rows.len();
    let filtered: Vec<&ReviewRow> = rows
        .iter()
        .filter(|row| status_filter.is_none_or(|status| row.status == status))
        .filter(|row| row_matches_query(row, &query))
        .collect();

    let total_pages = filtered.len().div_ceil(REVIEW_PAGE_SIZE).max(1);
    let page = view.save_review_page.min(total_pages - 1);
    let start = page * REVIEW_PAGE_SIZE;
    let end = (start + REVIEW_PAGE_SIZE).min(filtered.len());
    let page_rows = &filtered[start..end];

    let count_text = if filtered.len() == total_rows {
        format!("{} items", fmt_count(total_rows))
    } else {
        format!(
            "{} of {} items",
            fmt_count(filtered.len()),
            fmt_count(total_rows)
        )
    };

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
            .child(div().flex_1().min_w_0().child(muted_xs("File", cx)))
            .child(
                div()
                    .w(px(112.))
                    .flex_shrink_0()
                    .child(muted_xs("Status", cx)),
            )
            .child(div().flex_1().min_w_0().child(muted_xs("Detail", cx)))
            .child(div().w_4().flex_shrink_0()),
    );
    if page_rows.is_empty() {
        let empty = if !query.is_empty() || status_filter.is_some() {
            "Nothing matches the current search or filter.".to_string()
        } else {
            match view.state.save_sync_summary() {
                SaveSyncSummary::Off => "Save sync is off. Enable it to review files.".into(),
                SaveSyncSummary::NeedsSetup => {
                    "Save sync needs a save folder. Configure one in Settings.".into()
                }
                SaveSyncSummary::Checking | SaveSyncSummary::Reconciling => {
                    "Checking saves…".into()
                }
                SaveSyncSummary::NoLinkedFiles => "Nothing to sync yet.".into(),
                _ => "Nothing to review.".into(),
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
    if has_games {
        table = table.child(view.save_review_manual_row(window, cx));
    }

    let ignored = view.state.save_files_ignored();
    let ignored_row = (ignored > 0).then(|| {
        let shown = view.save_review_show_ignored;
        h_flex()
            .gap_2()
            .items_center()
            .child(muted_xs(
                format!(
                    "{} don't belong to a game in your library",
                    plural(ignored, "file")
                ),
                cx,
            ))
            .child(
                Button::new("save-review-toggle-ignored")
                    .label(if shown { "Hide" } else { "Show" })
                    .outline()
                    .small()
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.save_review_show_ignored = !view.save_review_show_ignored;
                        view.save_review_page = 0;
                        cx.notify();
                    })),
            )
    });
    let mut section = v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .items_end()
                .child(section_heading("Review files"))
                .child(muted_xs(count_text, cx)),
        )
        .child(
            h_flex()
                .gap_3()
                .items_end()
                .child(div().flex_1().min_w_0().child(field_column(
                    "Search",
                    Input::new(&view.save_review_search).w_full(),
                    cx,
                )))
                .child(
                    div().w(px(176.)).flex_shrink_0().child(field_column(
                        "Status",
                        Select::new(&filter_state)
                            .w_full()
                            .accessibility_label("Filter by status")
                            .placeholder("All statuses"),
                        cx,
                    )),
                ),
        )
        .children(ignored_row)
        .child(
            div()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .child(table),
        );

    if !page_rows.is_empty() {
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

    v_flex()
        .gap_4()
        .child(page_heading("Save sync"))
        .child(view.save_review_status_strip(window, cx))
        .child(section)
}

impl RommfsWindow {
    fn render_incoming_row(
        &mut self,
        incoming: &SaveSyncIncomingStatus,
        can_export: bool,
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
            .child(div().text_xs().child(format!(
                "{} · RomM save {} · pending revision {} · {} · {} · {}",
                incoming.rom_name,
                incoming.remote_id,
                incoming.incoming_id,
                incoming.state,
                incoming.reason,
                incoming.content_hash,
            )))
            .child(
                Button::new(format!("export-{}", incoming.incoming_id))
                    .label("Export…")
                    .outline()
                    .small()
                    .disabled(!can_export)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.prompt_export(incoming.clone(), cx);
                    })),
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
        return (vec!["No save scan yet".into()], None);
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
