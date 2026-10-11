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
    // Page-private scroll state: window.rs's event pump still writes the
    // shared `log_scroll` field on every new log line — that write is what
    // pinned the old log to the top (ISSUES.md). This handle is private to
    // the log region below, so the log stays scrollable and Follow works.
    let log_scroll = window
        .use_keyed_state("activity-log-scroll", cx, |_, _| ScrollHandle::new())
        .read(cx)
        .clone();
    if view.activity_follow_log {
        log_scroll.scroll_to_bottom();
    }

    // The superseded pre-task-5 renderers stay referenced so they keep
    // type-checking: window.rs is owned by a parallel slice and cannot gain
    // `#[allow(dead_code)]` from here.
    let _ = (
        RommfsWindow::render_activity,
        RommfsWindow::render_downloads,
        RommfsWindow::render_log,
    );

    v_flex()
        .w_full()
        .gap_4()
        .child(page_heading("Activity"))
        .when_some(attention_section(view, window, cx), |this, section| {
            this.child(section)
        })
        .child(operations_section(view, cx))
        .child(log_section(view, log_scroll, cx))
}

/// One consistent treatment for anything needing action: a danger line plus
/// an optional retry where a real command exists.
fn attention_row(message: impl Into<String>, retry: Option<Button>, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_3()
        .items_center()
        .child(div().flex_1().min_w_0().child(error_line(message, cx)))
        .when_some(retry, |this, button| this.child(button))
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
                format!("Connection problem: {error}"),
                Some(retry_button("retry-connection", cx, |view, _| {
                    view.controller.send(Command::RestoreConnection);
                })),
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.startup_error {
        rows.push(
            attention_row(
                format!("Startup problem: {error}"),
                Some(retry_button("retry-startup", cx, |view, _| {
                    view.controller.send(Command::RestoreConnection);
                })),
                cx,
            )
            .into_any_element(),
        );
    }
    if state.mount == MountState::Failed {
        let message = match &state.mount_error {
            Some(error) => format!("Mount failed: {error}"),
            None => "Mount failed".to_string(),
        };
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
        // §6: the failure keeps expandable details — the target path the
        // one-line summary doesn't carry.
        rows.push(
            v_flex()
                .gap_1()
                .child(attention_row(message, retry, cx))
                .when_some(target, |this, path| {
                    this.child(details_disclosure(
                        "attention-mount-details",
                        format!("Mount target: {path}"),
                        window,
                        cx,
                    ))
                })
                .into_any_element(),
        );
    }
    let failed_downloads = state
        .downloads
        .iter()
        .filter(|d| matches!(d.finished, Some(Err(_))))
        .count();
    if failed_downloads > 0 {
        // No retry command exists for downloads — the attention row only
        // surfaces the failure; rows below keep the per-file error.
        let message = if failed_downloads == 1 {
            "1 download failed — see the file rows below".to_string()
        } else {
            format!(
                "{} downloads failed — see the file rows below",
                fmt_count(failed_downloads)
            )
        };
        rows.push(attention_row(message, None, cx).into_any_element());
    }
    let save_sync_error = state
        .save_sync_failure
        .as_ref()
        .or(state.save_sync_problem.as_ref())
        .or_else(|| {
            state
                .save_sync_queue
                .as_ref()
                .and_then(|queue| queue.failure.as_ref())
        })
        .cloned();
    if let Some(error) = save_sync_error {
        rows.push(
            attention_row(
                format!("Save sync needs attention: {error}"),
                Some(retry_button("retry-save-sync", cx, |view, _| {
                    view.controller.send(Command::RefreshSaveSync);
                })),
                cx,
            )
            .into_any_element(),
        );
    } else if state
        .save_sync_queue
        .as_ref()
        .is_some_and(|queue| queue.actor_failed)
    {
        rows.push(
            attention_row(
                "Save sync stopped working — the background task exited".to_string(),
                Some(retry_button("retry-save-sync", cx, |view, _| {
                    view.controller.send(Command::RefreshSaveSync);
                })),
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.auxiliary_error {
        rows.push(
            attention_row(
                format!("Support-file download failed: {error}"),
                Some(retry_button("retry-auxiliary", cx, |view, _| {
                    view.controller.send(Command::MaterializeAuxiliary);
                })),
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.retrobat_setup_error {
        // SetupRetroBat needs the install + managed roots, which only the
        // settings page collects — no retry here.
        rows.push(
            attention_row(format!("RetroBat setup failed: {error}"), None, cx).into_any_element(),
        );
    }
    if let Some(error) = &state.installer_error {
        rows.push(
            attention_row(
                format!("RetroBat installer download failed: {error}"),
                Some(retry_button("retry-installer", cx, |view, _| {
                    view.controller.send(Command::DownloadRetroBatInstaller);
                })),
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.frontend_metadata_error {
        rows.push(
            attention_row(
                format!("Frontend metadata refresh failed: {error}"),
                None,
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(error) = &state.integration_error {
        rows.push(
            attention_row(format!("Library layout change failed: {error}"), None, cx)
                .into_any_element(),
        );
    }
    if let Some(Err(error)) = &state.foreign_removal {
        rows.push(
            attention_row(format!("Leftover-link cleanup failed: {error}"), None, cx)
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
                Some((stage, completed, total, elapsed)) => {
                    operation_progress("mount-progress", stage, *completed, *total, *elapsed, cx)
                        .into_any_element()
                }
                None => Progress::new("mount-progress")
                    .loading(true)
                    .small()
                    .into_any_element(),
            };
            running.push(
                op_row(cx.theme().accent, "Mounting games folder", Some(detail)).into_any_element(),
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
                    "Games folder mounted",
                    detail,
                )
                .into_any_element(),
            );
        }
        MountState::Failed | MountState::NotMounted => {}
    }

    // Catalogue refresh
    if let Some((stage, completed, total, elapsed)) = &state.catalogue_progress {
        running.push(
            op_row(
                cx.theme().accent,
                "Refreshing library catalogue",
                Some(
                    operation_progress(
                        "catalogue-progress",
                        stage,
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
                "Library catalogue refreshed",
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

    // Save sync — derived summary plus live transfers.
    if state.save_sync_enabled || !state.save_sync_transfers.is_empty() {
        let (label, kind) = view.save_sync_status();
        let queue_detail = state.save_sync_queue.as_ref().map(|queue| {
            let mut parts = Vec::new();
            if queue.pending_outbound > 0 {
                parts.push(format!("{} outgoing", fmt_count(queue.pending_outbound)));
            }
            if queue.pending_incoming > 0 {
                parts.push(format!("{} incoming", queue.pending_incoming));
            }
            if queue.attention_games > 0 {
                parts.push(format!("{} need review", queue.attention_games));
            }
            if queue.network_paused {
                parts.push("paused (offline)".to_string());
            }
            parts.join(" · ")
        });
        let detail = queue_detail.and_then(|text| {
            if text.is_empty() {
                None
            } else {
                Some(muted_xs(text, cx).into_any_element())
            }
        });
        recent.push(
            op_row(kind.color(cx), format!("Save sync — {label}"), detail).into_any_element(),
        );
        for transfer in &state.save_sync_transfers {
            let detail = transfer
                .detail
                .as_ref()
                .map(|detail| muted_xs(detail.clone(), cx).into_any_element());
            running.push(
                op_row(
                    cx.theme().accent,
                    format!(
                        "Save sync · game {} · rev {} · {}",
                        transfer.rom_id, transfer.revision, transfer.phase
                    ),
                    detail,
                )
                .into_any_element(),
            );
        }
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
        let detail = state
            .frontend_metadata_detail
            .as_ref()
            .map(|detail| muted_xs(detail.clone(), cx).into_any_element());
        running.push(
            op_row(cx.theme().accent, "Fetching frontend metadata", detail).into_any_element(),
        );
    }

    let idle = running.is_empty() && failed.is_empty() && recent.is_empty() && completed.is_empty();
    v_flex()
        .gap_2()
        .child(section_heading("Operations"))
        .when(idle, |this| {
            this.child(muted("Nothing happening right now", cx))
        })
        .children(running)
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
                            .label("Clear")
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
    log_scroll: ScrollHandle,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let min_level = view.activity_min_level;
    let needle = view.activity_search.read(cx).value().to_lowercase();
    let log_empty = view.state.log.lines().next().is_none();
    let lines: Vec<AnyElement> = view
        .state
        .log
        .lines()
        .filter(|line| level_visible(line.level, min_level))
        .filter(|line| {
            needle.is_empty()
                || line.message.to_lowercase().contains(&needle)
                || line.op.to_lowercase().contains(&needle)
        })
        .map(|line| render_log_line(line, cx).into_any_element())
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
                            div().w_40()
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
