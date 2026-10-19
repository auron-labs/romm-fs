use super::super::*;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::switch::Switch;

/// Activity page — UI-PLAN task 5. Operations-first: a "Needs attention"
/// summary when anything needs action, the live operations with their real
/// progress, then the technical log in its own scroll region with a severity
/// filter and a follow toggle.
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let log_scroll = view.log_scroll.clone();

    v_flex()
        .w_full()
        .gap_4()
        .child(page_heading("Activity"))
        .when_some(attention_section(view, window, cx), |this, section| {
            this.child(section)
        })
        .child(operations_section(view, cx))
        .child(
            h_flex().child(
                Button::new("toggle-technical-log")
                    .label(if view.activity_log_open {
                        "Hide technical log"
                    } else {
                        "Technical log"
                    })
                    .outline()
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.activity_log_open = !view.activity_log_open;
                        cx.notify();
                    })),
            ),
        )
        .when(view.activity_log_open, |this| {
            this.child(log_section(view, window, log_scroll, cx))
        })
}

/// One consistent treatment for anything needing action (phase 2a rule 5):
/// a headline naming what failed, a primary action, and the raw text behind
/// "Details".
fn attention_row(
    id: impl Into<SharedString>,
    headline: impl Into<SharedString>,
    detail: Option<String>,
    action: Option<Button>,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let id = id.into();
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().danger)
                        .child(headline.into()),
                )
                .when_some(action, |this, button| this.child(button)),
        )
        .when_some(detail, |this, detail| {
            this.child(details_disclosure(
                SharedString::from(format!("{id}-details")),
                detail,
                window,
                cx,
            ))
        })
}

fn retry_button(
    id: &'static str,
    cx: &mut Context<RommfsWindow>,
    action: impl Fn(&mut RommfsWindow, &mut Context<RommfsWindow>) + 'static,
) -> Button {
    Button::new(id)
        .label("Retry")
        .outline()
        .small()
        .on_click(cx.listener(move |view, _, _, cx| {
            action(view, cx);
        }))
}

/// "Go fix this" for an attention row whose fix lives on a Settings tab.
fn settings_action_button(
    id: &'static str,
    label: &'static str,
    tab: usize,
    cx: &mut Context<RommfsWindow>,
) -> Button {
    Button::new(id)
        .label(label)
        .outline()
        .small()
        .on_click(cx.listener(move |view, _, _, cx| {
            view.active_view = 3;
            view.settings_tab = tab;
            cx.notify();
        }))
}

fn attention_section(
    view: &RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> Option<impl IntoElement> {
    let state = &view.state;
    let mut rows: Vec<AnyElement> = Vec::new();

    if let Some(error) = &state.conn_error {
        rows.push(
            attention_row(
                "attention-connection",
                "Couldn't connect to RomM",
                Some(error.clone()),
                Some(retry_button("retry-connection", cx, |view, _| {
                    view.controller.send(Command::RestoreConnection);
                })),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.startup_error {
        rows.push(
            attention_row(
                "attention-startup",
                "Couldn't restore your last session",
                Some(error.clone()),
                Some(retry_button("retry-startup", cx, |view, _| {
                    view.controller.send(Command::RestoreConnection);
                })),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if state.mount == MountState::Failed {
        let target = state
            .mount_path
            .clone()
            .or_else(|| state.configured_mount_root.clone());
        let retry = target.clone().map(|path| {
            retry_button("retry-mount", cx, move |view, _| {
                view.controller
                    .send(Command::StartMount { path: path.clone() });
            })
        });
        let detail = match (&state.mount_error, target) {
            (Some(error), _) => Some(error.clone()),
            (None, Some(path)) => Some(path),
            (None, None) => None,
        };
        rows.push(
            attention_row(
                "attention-mount",
                "Couldn't start the games folder",
                detail,
                retry,
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    let failed_downloads = state
        .downloads
        .iter()
        .filter(|d| matches!(d.finished, Some(Err(_))))
        .count();
    if failed_downloads > 0 {
        // No retry command exists for downloads — the raw per-file error
        // stays on the file rows below.
        rows.push(
            attention_row(
                "attention-downloads",
                format!("{} failed", plural(failed_downloads, "download")),
                None,
                None,
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    for game in state.save_sync_games.iter().filter(|game| {
        game.work == rommfs_core::events::SaveSyncWork::NeedsReview || !game.incoming_ids.is_empty()
    }) {
        let key = SharedString::from(format!("game:{}:{}", game.rom_id, game.file_id));
        rows.push(
            attention_row(
                SharedString::from(format!("attention-save-{}-{}", game.rom_id, game.file_id)),
                format!("{} — save needs review", game.rom_name),
                game.issue.clone(),
                Some(
                    Button::new(SharedString::from(format!("review-{}", key)))
                        .label("Review details")
                        .outline()
                        .small()
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.active_view = 1;
                            view.save_review_attention_only = true;
                            view.save_review_expanded = Some(key.clone());
                            view.save_review_page = 0;
                            view.save_review_search
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            cx.notify();
                        })),
                ),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if !state.memory_card_conflicts.is_empty() {
        rows.push(
            attention_row(
                "attention-memory-cards",
                format!(
                    "{} have conflicting changes",
                    plural(state.memory_card_conflicts.len(), "memory card")
                ),
                None,
                Some(
                    Button::new("review-memory-card-conflicts")
                        .label("Review details")
                        .outline()
                        .small()
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.active_view = 1;
                            view.save_review_attention_only = true;
                            view.save_review_page = 0;
                            view.save_review_search
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            cx.notify();
                        })),
                ),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    let save_sync_error = state.save_sync_attention();
    if let Some(error) = save_sync_error.filter(|_| {
        !state.save_sync_games.iter().any(|game| {
            game.work == rommfs_core::events::SaveSyncWork::NeedsReview
                || !game.incoming_ids.is_empty()
        })
    }) {
        rows.push(
            attention_row(
                "attention-save-sync",
                "Save sync needs attention",
                Some(error),
                Some(retry_button("retry-save-sync", cx, |view, _| {
                    view.controller.send(Command::RefreshSaveSync);
                })),
                window,
                cx,
            )
            .into_any_element(),
        );
    } else if state.save_sync_enabled
        && state
            .save_sync_queue
            .as_ref()
            .is_some_and(|queue| queue.actor_failed)
    {
        rows.push(
            attention_row(
                "attention-save-sync-actor",
                "Save sync stopped working",
                Some("The background task exited.".into()),
                Some(retry_button("retry-save-sync", cx, |view, _| {
                    view.controller.send(Command::RefreshSaveSync);
                })),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.auxiliary_error {
        let problem = UserProblem::from_text("Couldn't install support files", error);
        rows.push(
            problem_block(
                "attention-auxiliary",
                &problem,
                Some(retry_button("retry-auxiliary", cx, |view, _| {
                    view.controller.send(Command::MaterializeAuxiliary);
                })),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.retrobat_setup_error {
        // SetupRetroBat needs the install + managed roots, which only the
        // Settings page collects — the action opens the RetroBat tab.
        let problem = UserProblem::from_text("Couldn't finish RetroBat setup", error);
        rows.push(
            problem_block(
                "attention-retrobat-setup",
                &problem,
                Some(settings_action_button(
                    "attention-retrobat-settings",
                    "RetroBat settings",
                    2,
                    cx,
                )),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.installer_error {
        let problem = UserProblem::from_text("Couldn't get the RetroBat installer", error);
        rows.push(
            problem_block(
                "attention-installer",
                &problem,
                Some(retry_button("retry-installer", cx, |view, _| {
                    view.controller.send(Command::DownloadRetroBatInstaller);
                })),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = state
        .frontend_metadata_error
        .as_ref()
        .filter(|_| !state.frontend_metadata_pending)
    {
        rows.push(
            attention_row(
                "attention-metadata",
                "Couldn't update artwork and game information",
                Some(error.clone()),
                Some(retry_button("retry-metadata", cx, |view, _| {
                    view.controller.send(Command::RefreshMetadata);
                })),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.integration_error {
        rows.push(
            attention_row(
                "attention-integration",
                "Library layout change failed",
                Some(error.clone()),
                Some(settings_action_button(
                    "attention-integration-settings",
                    "Open Settings",
                    1,
                    cx,
                )),
                window,
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(Err(error)) = &state.foreign_removal {
        rows.push(
            attention_row(
                "attention-foreign",
                "Couldn't remove leftover links",
                Some(error.clone()),
                None,
                window,
                cx,
            )
            .into_any_element(),
        );
    }

    if rows.is_empty() {
        None
    } else {
        Some(
            v_flex()
                .gap_2()
                .child(section_heading("Needs attention"))
                .children(rows),
        )
    }
}

/// One operation row: a leading status dot, the operation name, and an
/// optional detail lane (progress bar, counts, timestamps).
fn op_row(
    color: Hsla,
    title: impl Into<SharedString>,
    detail: Option<AnyElement>,
) -> impl IntoElement {
    h_flex()
        .gap_2()
        .items_start()
        .child(div().pt(px(5.)).flex_shrink_0().child(status_dot(color)))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().text_sm().child(title.into()))
                .when_some(detail, |this, detail| this.child(detail)),
        )
}

fn operations_section(view: &RommfsWindow, cx: &mut Context<RommfsWindow>) -> impl IntoElement {
    let state = &view.state;
    let mut running: Vec<AnyElement> = Vec::new();
    let mut failed: Vec<AnyElement> = Vec::new();
    let mut recent: Vec<AnyElement> = Vec::new();

    // Mount
    match state.mount {
        MountState::Mounting => {
            let detail = match &state.mount_progress {
                Some((stage, completed, total, elapsed)) => operation_progress(
                    "mount-progress",
                    stage_label(stage),
                    *completed,
                    *total,
                    *elapsed,
                    cx,
                )
                .into_any_element(),
                None => Progress::new("mount-progress")
                    .loading(true)
                    .small()
                    .into_any_element(),
            };
            running.push(
                op_row(cx.theme().accent, "Preparing games…", Some(detail)).into_any_element(),
            );
        }
        MountState::Mounted => {
            let detail = state
                .mount_path
                .as_ref()
                .map(|path| muted_xs(path.clone(), cx).into_any_element());
            recent.push(
                op_row(
                    StatusKind::Success.color(cx),
                    "Made games available",
                    detail,
                )
                .into_any_element(),
            );
        }
        MountState::Failed | MountState::NotMounted => {}
    }

    if state.mount_stopping {
        running.push(
            op_row(cx.theme().muted_foreground, "Stopping games folder…", None).into_any_element(),
        );
    }

    // Catalogue refresh
    if let Some((stage, completed, total, elapsed)) = &state.catalogue_progress {
        running.push(
            op_row(
                cx.theme().accent,
                "Checking for library changes…",
                Some(
                    operation_progress(
                        "catalogue-progress",
                        stage_label(stage),
                        *completed,
                        *total,
                        *elapsed,
                        cx,
                    )
                    .into_any_element(),
                ),
            )
            .into_any_element(),
        );
    } else if let Some(at) = state.catalogue_refreshed_at {
        recent.push(
            op_row(
                StatusKind::Muted.color(cx),
                "Checked for library changes",
                Some(muted_xs(relative_time(at), cx).into_any_element()),
            )
            .into_any_element(),
        );
    }

    // Downloads — in-flight first, then failures, completed behind a toggle.
    for download in state.downloads.iter().filter(|d| d.finished.is_none()) {
        running.push(
            view.render_download_row(download.clone(), cx)
                .into_any_element(),
        );
    }
    for download in state
        .downloads
        .iter()
        .filter(|d| matches!(d.finished, Some(Err(_))))
    {
        failed.push(
            view.render_download_row(download.clone(), cx)
                .into_any_element(),
        );
    }
    let completed: Vec<_> = state
        .downloads
        .iter()
        .filter(|d| matches!(d.finished, Some(Ok(()))))
        .cloned()
        .collect();

    // Stages share the revision identity; terminal stages live in recent history.
    for transfer in state.save_sync_transfers.iter().rev() {
        let name = state
            .save_sync_games
            .iter()
            .find(|game| game.rom_id == transfer.rom_id)
            .map(|game| game.rom_name.clone())
            .unwrap_or_else(|| format!("Save for game {}", transfer.rom_id));
        let label = match transfer.phase.as_str() {
            "captured" => "Waiting to upload",
            "uploading" => "Uploading to RomM…",
            "verified" => "Uploaded to RomM",
            "retry_wait" | "paused" => "Waiting for RomM",
            _ => "Needs review",
        };
        let detail = Some(muted_xs(relative_time(transfer.updated_at), cx).into_any_element());
        let row = op_row(
            if transfer.phase == "verified" {
                StatusKind::Success.color(cx)
            } else {
                cx.theme().muted_foreground
            },
            format!("{name} — {label}"),
            detail,
        )
        .into_any_element();
        if transfer.phase == "verified" {
            recent.push(row);
        } else if matches!(
            transfer.phase.as_str(),
            "captured" | "uploading" | "retry_wait" | "paused"
        ) {
            running.push(row);
        }
    }
    if state.save_sync_checking {
        running.push(
            op_row(
                cx.theme().muted_foreground,
                "Checking saves for changes…",
                None,
            )
            .into_any_element(),
        );
    }

    // RetroBat setup
    if state.retrobat_setup_running {
        let detail = state
            .retrobat_setup_detail
            .as_ref()
            .map(|detail| muted_xs(detail.clone(), cx).into_any_element());
        running
            .push(op_row(cx.theme().accent, "RetroBat setup running", detail).into_any_element());
    }

    // Auxiliary/support-file download
    if state.auxiliary_running {
        let detail = state
            .auxiliary_detail
            .as_ref()
            .map(|detail| muted_xs(detail.clone(), cx).into_any_element());
        running.push(
            op_row(cx.theme().accent, "Downloading support files", detail).into_any_element(),
        );
    }

    // RetroBat installer download
    if state.installer_running {
        let (received, total) = state.installer_progress;
        let percent = total
            .filter(|t| *t > 0)
            .map_or(0.0, |t| (100.0 * received as f32 / t as f32).min(100.0));
        let progress = v_flex()
            .gap_1()
            .child(muted_xs(
                match total {
                    Some(total) => format!("{} / {}", fmt_bytes(received), fmt_bytes(total)),
                    None => format!("{} —", fmt_bytes(received)),
                },
                cx,
            ))
            .child(
                Progress::new("installer-progress")
                    .value(percent)
                    .loading(total.is_none())
                    .small(),
            );
        running.push(
            op_row(
                cx.theme().accent,
                "Downloading RetroBat installer",
                Some(progress.into_any_element()),
            )
            .into_any_element(),
        );
    } else if state.installer_ready {
        recent.push(
            op_row(
                StatusKind::Success.color(cx),
                "RetroBat installer ready",
                None,
            )
            .into_any_element(),
        );
    }

    // Frontend metadata fetch
    if state.frontend_metadata_pending {
        let detail = match state.frontend_metadata_progress {
            Some((completed, total)) if completed < total => v_flex()
                .gap_1()
                .when_some(state.frontend_metadata_detail.clone(), |this, stage| {
                    this.child(muted_xs(stage, cx))
                })
                .child(muted_xs(
                    format!("{} / {} artwork files checked", completed, total),
                    cx,
                ))
                .child(
                    Progress::new("metadata-progress")
                        .value(if total > 0 {
                            (100.0 * completed as f32 / total as f32).min(100.0)
                        } else {
                            0.0
                        })
                        .small(),
                )
                .into_any_element(),
            progress => {
                let mut block = v_flex()
                    .gap_1()
                    .child(Progress::new("metadata-progress").loading(true).small());
                if progress.is_some() {
                    block = block.child(muted_xs("Artwork checks complete", cx));
                }
                if let Some(detail) = state.frontend_metadata_detail.clone() {
                    block = block.child(muted_xs(detail, cx));
                }
                block.into_any_element()
            }
        };
        running.push(
            op_row(
                cx.theme().accent,
                "Artwork and game information",
                Some(detail),
            )
            .into_any_element(),
        );
    }

    let idle = running.is_empty();
    v_flex()
        .gap_2()
        .child(section_heading("In progress"))
        .when(idle, |this| {
            this.child(muted("No operations in progress", cx))
        })
        .children(running)
        .child(section_heading("Recent activity"))
        .children(failed)
        .children(recent)
        .when(!completed.is_empty(), |this| {
            this.child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("toggle-completed")
                            .label(if view.completed_open {
                                "Hide completed".to_string()
                            } else {
                                format!("Show {} completed", fmt_count(completed.len()))
                            })
                            .outline()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.completed_open = !view.completed_open;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("clear-completed")
                            .label("Clear completed downloads")
                            .outline()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.state.clear_completed_downloads();
                                cx.notify();
                            })),
                    ),
            )
            .when(view.completed_open, |this| {
                this.child(
                    div()
                        .id("completed-downloads")
                        .v_flex()
                        .gap_2()
                        .max_h_40()
                        .overflow_y_scrollbar()
                        .children(
                            completed
                                .into_iter()
                                .map(|d| view.render_download_row(d, cx).into_any_element()),
                        ),
                )
            })
        })
}

fn level_visible(level: Level, min_level: usize) -> bool {
    match min_level {
        0 => true,
        1 => !matches!(level, Level::Info),
        _ => matches!(level, Level::Error),
    }
}

fn log_section(
    view: &RommfsWindow,
    window: &mut Window,
    log_scroll: ScrollHandle,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let min_level = view.activity_min_level;
    let needle = view.activity_search.read(cx).value().to_lowercase();
    let log_empty = view.state.log.lines().next().is_none();
    // §6: repeated diagnostics collapse into one entry keyed by the
    // stable reason (level + op + message) with an occurrence count,
    // instead of flooding the log.
    let mut entries: Vec<(&rommfs_core::events::LogLine, usize)> = Vec::new();
    let mut reasons = std::collections::HashMap::new();
    for line in view.state.log.lines() {
        if !level_visible(line.level, min_level) {
            continue;
        }
        if !needle.is_empty()
            && !line.message.to_lowercase().contains(&needle)
            && !line.op.to_lowercase().contains(&needle)
        {
            continue;
        }
        let reason = (level_str(line.level), line.op, line.message.as_str());
        if let Some(&index) = reasons.get(&reason) {
            let (_, count) = &mut entries[index];
            *count += 1;
        } else {
            reasons.insert(reason, entries.len());
            entries.push((line, 1));
        }
    }
    let lines: Vec<AnyElement> = entries
        .into_iter()
        .map(|(line, count)| render_log_line(line, count, window, cx).into_any_element())
        .collect();
    let empty_text = if log_empty {
        "No activity yet"
    } else if needle.is_empty() {
        "No log entries at this level"
    } else {
        "No log entries match"
    };

    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_3()
                .justify_between()
                .items_center()
                .child(section_heading("Technical log"))
                .child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .w_40()
                                .child(Input::new(&view.activity_search).small()),
                        )
                        .child(
                            RadioGroup::horizontal("activity-log-level")
                                .children(["All", "Warnings+", "Errors"])
                                .selected_index(Some(min_level))
                                .on_change(cx.listener(|view, index, _, cx| {
                                    view.activity_min_level = *index;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Switch::new("activity-follow-log")
                                .checked(view.activity_follow_log)
                                .label("Follow")
                                .on_change(cx.listener(|view, checked, _, cx| {
                                    view.activity_follow_log = *checked;
                                    if *checked {
                                        view.log_scroll.scroll_to_bottom();
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("copy-log")
                                .label("Copy")
                                .outline()
                                .small()
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.on_copy_log(cx);
                                })),
                        )
                        .child(
                            Button::new("open-log-folder")
                                .label("Open log folder")
                                .outline()
                                .small()
                                .on_click(cx.listener(|view, _, _, cx| {
                                    if let Some(path) =
                                        crate::controller::diagnostic_log_directory()
                                    {
                                        cx.reveal_path(&path);
                                    } else {
                                        view.controller.sink().emit(AppEvent::log(
                                            Level::Error,
                                            "diagnostics",
                                            "No application log directory is available",
                                        ));
                                    }
                                })),
                        ),
                ),
        )
        .child(
            div()
                .relative()
                .h_96()
                .w_full()
                .bg(cx.theme().muted)
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .child(
                    div()
                        .id("activity-log")
                        .track_scroll(&log_scroll)
                        .size_full()
                        .overflow_y_scroll()
                        .p_2()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .children(if lines.is_empty() {
                            vec![div()
                                .text_color(cx.theme().muted_foreground)
                                .child(empty_text)
                                .into_any_element()]
                        } else {
                            lines
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(Scrollbar::vertical(&log_scroll).viewport_from_layout()),
                ),
        )
}

impl RommfsWindow {
    fn render_download_row(
        &self,
        download: crate::controller::DownloadView,
        cx: &App,
    ) -> impl IntoElement {
        let (status_text, status_kind) = match &download.finished {
            None => (
                match download.total {
                    Some(total) => {
                        format!("{} / {}", fmt_bytes(download.received), fmt_bytes(total))
                    }
                    None => format!("{} —", fmt_bytes(download.received)),
                },
                StatusKind::Muted,
            ),
            Some(Ok(())) => ("Complete".to_string(), StatusKind::Success),
            Some(Err(error)) => (format!("Failed: {error}"), StatusKind::Danger),
        };
        let percent = match download.total {
            Some(total) if total > 0 => {
                (100.0 * download.received as f64 / total as f64).min(100.0)
            }
            _ if matches!(download.finished, Some(Ok(()))) => 100.0,
            _ => 0.0,
        } as f32;
        h_flex()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(download.file_name.clone()),
            )
            .child(
                div().w_64().child(
                    h_flex()
                        .gap_2()
                        .child(
                            div().flex_1().child(
                                Progress::new(("download", download.file_id))
                                    .value(percent)
                                    .loading(
                                        download.finished.is_none() && download.total.is_none(),
                                    )
                                    .xsmall(),
                            ),
                        )
                        .child(
                            div()
                                .w_10()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    if download.finished.is_none() && download.total.is_none() {
                                        "—".into()
                                    } else {
                                        format!("{percent:.0}%")
                                    },
                                ),
                        ),
                ),
            )
            .child(
                div()
                    .w_40()
                    .text_sm()
                    .whitespace_nowrap()
                    .text_color(status_kind.color(cx))
                    .child(status_text),
            )
    }
}

fn level_str(level: Level) -> &'static str {
    match level {
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    }
}
fn level_kind(level: Level) -> StatusKind {
    match level {
        Level::Info => StatusKind::Muted,
        Level::Warn => StatusKind::Warning,
        Level::Error => StatusKind::Danger,
    }
}
fn fmt_time(unix_secs: u64) -> String {
    let secs = unix_secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}
pub(in crate::window) fn format_log_line(line: &rommfs_core::events::LogLine) -> String {
    if line.op.is_empty() {
        format!(
            "{} {:<5} {}",
            fmt_time(line.unix_secs),
            level_str(line.level),
            line.message
        )
    } else {
        format!(
            "{} {:<5} [{}] {}",
            fmt_time(line.unix_secs),
            level_str(line.level),
            line.op,
            line.message
        )
    }
}
fn render_log_line(
    line: &rommfs_core::events::LogLine,
    count: usize,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let body = if line.op.is_empty() {
        line.message.clone()
    } else {
        format!("[{}] {}", line.op, line.message)
    };
    let body = if count > 1 {
        format!("{body} (×{count})")
    } else {
        body
    };
    // One line by default; the toggle reveals the whole entry. A short entry
    // that still spans lines (embedded newlines) gets the toggle too (WS6.13).
    let long = body.chars().count() > 72 || body.contains('\n') || body.contains('\r');
    // A reason has one row even when its occurrences are interleaved with
    // other diagnostics. Keep its identity independent of the display count.
    let key = SharedString::from(format!(
        "log-line-{:?}-{:?}-{:?}",
        line.level, line.op, line.message
    ));
    let open = window.use_keyed_state(key.clone(), cx, |_, _| std::cell::Cell::new(false));
    let is_open = open.read(cx).get();
    let toggle = open.clone();
    let time = div()
        .flex_shrink_0()
        .text_color(cx.theme().muted_foreground)
        .child(fmt_time(line.unix_secs));
    let level = div()
        .w_12()
        .flex_shrink_0()
        .text_color(level_kind(line.level).color(cx))
        .child(level_str(line.level));
    h_flex()
        .gap_2()
        .items_start()
        .child(time)
        .child(level)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_color(cx.theme().foreground)
                        .when(!is_open, |this| this.truncate().whitespace_nowrap())
                        .child(body),
                )
                .when(long || is_open, |this| {
                    this.child(
                        Button::new(key)
                            .ghost()
                            .small()
                            .label(if is_open {
                                "Hide full message"
                            } else {
                                "Show full message"
                            })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                toggle.update(cx, |open, _| open.set(!open.get()));
                                cx.notify();
                            })),
                    )
                }),
        )
}
fn fmt_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}
