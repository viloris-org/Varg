//! Copilot panel for the editor shell — AI assistant with plan preview, approval, and trace.

use egui::{Color32, CornerRadius, RichText, Vec2};

use super::super::copilot_engine;
use super::super::types::{
    CopilotChatMessage, CopilotStatus, InfernuxPalette, PlanPreviewItem, ShellUiState,
};
use super::super::widgets::buttons::small_chip;
use super::super::widgets::layout::{panel_title, toolbar_row};
use crate::EditorShell;
use engine_i18n::Translations;

/// Renders the Copilot panel.
pub fn draw_copilot(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    panel_title(ui, tr.tr("panel_copilot"), pal);

    // ── Auto-accept & trace toggle toolbar ──────────────────────
    toolbar_row(ui, pal, |ui| {
        // Auto-accept toggle
        let auto_label = if ui_state.copilot.auto_accept {
            tr.tr("copilot_auto_accept_on")
        } else {
            tr.tr("copilot_auto_accept_off")
        };
        let toggle_fill = if ui_state.copilot.auto_accept {
            pal.accent
        } else {
            pal.frame_bg
        };
        if ui
            .add(
                egui::Button::new(RichText::new(auto_label).size(11.0).color(pal.text))
                    .fill(toggle_fill)
                    .min_size(Vec2::new(80.0, 20.0)),
            )
            .clicked()
        {
            ui_state.copilot.auto_accept = !ui_state.copilot.auto_accept;
        }

        ui.add_space(4.0);

        // Trace toggle
        let trace_label = if ui_state.copilot.trace_expanded {
            tr.tr("copilot_trace_hide")
        } else {
            tr.tr("copilot_trace_show")
        };
        if small_chip(ui, trace_label, 72.0, pal).clicked() {
            ui_state.copilot.trace_expanded = !ui_state.copilot.trace_expanded;
        }

        // Clear button
        if small_chip(ui, tr.tr("copilot_clear"), 50.0, pal).clicked() {
            ui_state.copilot.messages.clear();
            ui_state.copilot.plan_preview.clear();
            ui_state.copilot.trace_entries.clear();
            ui_state.copilot.console_entry_count = 0;
            ui_state.copilot.console_error_count = 0;
            ui_state.copilot.status_message = None;
            ui_state.copilot.error_message = None;
            ui_state.copilot.status = CopilotStatus::Idle;
        }
    });

    // ── Chat / status area (scrollable) ────────────────────────
    let available = ui.available_size();
    let chat_height_ratio =
        if !ui_state.copilot.trace_entries.is_empty() && ui_state.copilot.trace_expanded {
            0.45
        } else {
            0.60
        };

    egui::ScrollArea::vertical()
        .id_salt("copilot_chat_scroll")
        .stick_to_bottom(true)
        .max_height((available.y * chat_height_ratio).max(80.0))
        .show(ui, |ui| {
            // Status indicator
            match &ui_state.copilot.status {
                CopilotStatus::Planning => {
                    ui.label(
                        RichText::new(tr.tr("copilot_status_planning"))
                            .size(12.0)
                            .color(pal.text_dim),
                    );
                    ui.add_space(4.0);
                }
                CopilotStatus::Executing => {
                    ui.label(
                        RichText::new(tr.tr("copilot_status_executing"))
                            .size(12.0)
                            .color(pal.text_dim),
                    );
                    ui.add_space(4.0);
                }
                CopilotStatus::Error(msg) => {
                    ui.label(
                        RichText::new(format!("{}: {}", tr.tr("copilot_error"), msg))
                            .size(12.0)
                            .color(pal.error),
                    );
                    ui.add_space(4.0);
                }
                _ => {}
            }

            // Status message
            if let Some(msg) = &ui_state.copilot.status_message {
                ui.label(RichText::new(msg).size(13.0).color(pal.success));
                ui.add_space(4.0);
            }

            // Error message
            if let Some(msg) = &ui_state.copilot.error_message {
                let error_rect = ui.available_rect_before_wrap();
                ui.painter().rect_filled(
                    error_rect,
                    CornerRadius::same(4),
                    pal.error.gamma_multiply(0.15),
                );
                ui.label(RichText::new(msg).size(12.0).color(pal.error));
                ui.add_space(4.0);
            }

            // Chat history
            for msg in &ui_state.copilot.messages {
                let role_color = if msg.role == "user" {
                    pal.accent
                } else {
                    pal.text
                };
                let role_label = if msg.role == "user" {
                    tr.tr("copilot_you")
                } else {
                    tr.tr("copilot_assistant")
                };

                ui.label(
                    RichText::new(format!("{}:", role_label))
                        .size(11.0)
                        .color(role_color),
                );
                ui.label(RichText::new(&msg.content).size(12.0).color(pal.text));
                ui.add_space(4.0);
            }

            // Plan preview
            if !ui_state.copilot.plan_preview.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(tr.tr("copilot_plan_title"))
                        .size(13.0)
                        .color(pal.text),
                );
                ui.add_space(2.0);

                for item in &mut ui_state.copilot.plan_preview {
                    let _icon = if item.requires_write {
                        "\u{270F}" // ✏
                    } else {
                        "\u{1F50D}" // 🔍
                    };
                    ui.horizontal(|ui| {
                        // Approve checkbox (for write ops)
                        if item.requires_write {
                            let label = if item.approved {
                                RichText::new("\u{2611}").size(14.0).color(pal.success)
                            // ☑
                            } else {
                                RichText::new("\u{2610}").size(14.0).color(pal.text_dim)
                                // ☐
                            };
                            if ui
                                .add(
                                    egui::Button::new(label)
                                        .fill(Color32::TRANSPARENT)
                                        .min_size(Vec2::new(18.0, 18.0)),
                                )
                                .clicked()
                            {
                                item.approved = !item.approved;
                            }
                        } else {
                            // Read-only ops show an info icon
                            ui.label(
                                RichText::new("\u{2139}").size(12.0).color(pal.text_dim), // ℹ
                            );
                        }

                        ui.label(
                            RichText::new(format!("  {}", &item.preview))
                                .size(12.0)
                                .color(pal.text),
                        );
                    });
                    ui.add_space(2.0);
                }

                ui.add_space(6.0);

                // Apply/Reject buttons
                let approved_count = ui_state
                    .copilot
                    .plan_preview
                    .iter()
                    .filter(|p| p.approved)
                    .count();
                let enabled = approved_count > 0;

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(
                                RichText::new(format!(
                                    "{} ({})",
                                    tr.tr("copilot_apply_selected"),
                                    approved_count
                                ))
                                .size(12.0)
                                .color(Color32::WHITE),
                            )
                            .fill(pal.accent)
                            .min_size(Vec2::new(100.0, 24.0)),
                        )
                        .clicked()
                    {
                        apply_approved(shell, ui_state, tr);
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(tr.tr("copilot_reject_all"))
                                    .size(12.0)
                                    .color(pal.text),
                            )
                            .fill(pal.frame_bg)
                            .min_size(Vec2::new(90.0, 24.0)),
                        )
                        .clicked()
                    {
                        ui_state.copilot.plan_preview.clear();
                        ui_state.copilot.status = CopilotStatus::Idle;
                    }
                });
            }
        });

    // ── Trace entries (collapsible) ─────────────────────────────
    if ui_state.copilot.trace_expanded && !ui_state.copilot.trace_entries.is_empty() {
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("copilot_trace_scroll")
            .stick_to_bottom(true)
            .max_height((available.y * 0.25).max(60.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tr.tr("copilot_trace_title"))
                        .size(12.0)
                        .color(pal.text_dim),
                );
                ui.add_space(2.0);

                for entry in &ui_state.copilot.trace_entries {
                    ui.label(RichText::new(entry).size(11.0).color(pal.text_dim));
                    ui.add_space(1.0);
                }
            });
    }

    // ── Console entries summary ─────────────────────────────────
    if ui_state.copilot.console_entry_count > 0 {
        ui.separator();
        let text = if ui_state.copilot.console_error_count > 0 {
            format!(
                "{} {} {} ({} {} {})",
                tr.tr("copilot_console_label"),
                ui_state.copilot.console_entry_count,
                tr.tr("copilot_entries"),
                ui_state.copilot.console_error_count,
                tr.tr("copilot_error"),
                tr.tr("copilot_entries"),
            )
        } else {
            format!(
                "{} {} {}",
                tr.tr("copilot_console_label"),
                ui_state.copilot.console_entry_count,
                tr.tr("copilot_entries"),
            )
        };
        ui.label(RichText::new(text).size(11.0).color(pal.text_dim));
    }

    // ── Input area ─────────────────────────────────────────────
    ui.separator();
    ui.add_space(4.0);

    let is_working = matches!(
        ui_state.copilot.status,
        CopilotStatus::Planning | CopilotStatus::Executing
    );

    ui.horizontal(|ui| {
        // Text input
        let input_response = ui.add_sized(
            Vec2::new(ui.available_width() - 60.0, 36.0),
            egui::TextEdit::singleline(&mut ui_state.copilot.input)
                .hint_text(if is_working {
                    tr.tr("copilot_input_working")
                } else {
                    tr.tr("copilot_input_hint")
                })
                .font(egui::FontId::proportional(12.0))
                .text_color(pal.text)
                .desired_width(f32::INFINITY),
        );

        // Send button
        if ui
            .add_enabled(
                !is_working && !ui_state.copilot.input.trim().is_empty(),
                egui::Button::new(
                    RichText::new(tr.tr("copilot_send"))
                        .size(14.0)
                        .color(Color32::WHITE),
                )
                .fill(pal.accent)
                .min_size(Vec2::new(50.0, 36.0)),
            )
            .clicked()
        {
            submit_copilot_prompt(shell, ui_state, tr);
        }

        // Send on Enter key
        if input_response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !is_working
            && !ui_state.copilot.input.trim().is_empty()
        {
            submit_copilot_prompt(shell, ui_state, tr);
            input_response.request_focus();
        }
    });
}

/// Submits the current input and processes it through the Copilot engine.
fn submit_copilot_prompt(shell: &mut EditorShell, ui_state: &mut ShellUiState, tr: &Translations) {
    let prompt = ui_state.copilot.input.trim().to_owned();
    if prompt.is_empty() {
        return;
    }

    ui_state.copilot.input.clear();

    // Check if a project is open
    if shell.project().is_none() {
        ui_state.copilot.messages.push(CopilotChatMessage {
            role: "user".to_owned(),
            content: prompt.clone(),
        });
        ui_state.copilot.error_message = Some(tr.tr("copilot_error").to_owned());
        ui_state.copilot.status = CopilotStatus::Error("No project is open".to_owned());
        return;
    }

    // Add user message to chat
    ui_state.copilot.messages.push(CopilotChatMessage {
        role: "user".to_owned(),
        content: prompt.clone(),
    });

    // Process through engine
    ui_state.copilot.status = CopilotStatus::Planning;
    ui_state.copilot.status_message = None;
    ui_state.copilot.error_message = None;

    match copilot_engine::process_copilot_prompt(shell, ui_state, &prompt) {
        Ok(operations) => {
            let previews: Vec<String> = operations.iter().map(|op| op.preview.clone()).collect();

            // Add assistant message showing what was planned
            let plan_text = if previews.is_empty() {
                tr.tr("copilot_status_complete").to_owned()
            } else {
                format!(
                    "{}\n{}",
                    tr.tr("copilot_plan_intro"),
                    previews
                        .iter()
                        .map(|p| format!("  • {}", p))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            ui_state.copilot.messages.push(CopilotChatMessage {
                role: "assistant".to_owned(),
                content: plan_text,
            });
        }
        Err(err) => {
            ui_state.copilot.error_message = Some(err.to_string());
            ui_state.copilot.status = CopilotStatus::Error(err.to_string());
        }
    }
}

/// Applies the currently approved operations.
fn apply_approved(shell: &mut EditorShell, ui_state: &mut ShellUiState, tr: &Translations) {
    ui_state.copilot.status = CopilotStatus::Executing;
    ui_state.copilot.status_message = Some(tr.tr("copilot_applying").to_owned());

    match copilot_engine::apply_approved_operations(shell, ui_state) {
        Ok(summary) => {
            ui_state.copilot.status_message = Some(summary);
            ui_state.copilot.status = CopilotStatus::Complete;
            ui_state.copilot.plan_preview.clear();
        }
        Err(err) => {
            ui_state.copilot.error_message = Some(err.to_string());
            ui_state.copilot.status = CopilotStatus::Error(err.to_string());
        }
    }
}

/// Updates the plan preview in the UI state from parsed operations.
pub fn set_plan_preview(ui_state: &mut ShellUiState, operations: &[(String, bool)]) {
    ui_state.copilot.plan_preview = operations
        .iter()
        .enumerate()
        .map(|(i, (preview, requires_write))| PlanPreviewItem {
            index: i,
            preview: preview.clone(),
            requires_write: *requires_write,
            approved: !requires_write,
        })
        .collect();
    ui_state.copilot.status = if operations.is_empty() {
        CopilotStatus::Error("No operations to preview".to_owned())
    } else {
        CopilotStatus::ReadyForReview
    };
}

/// Updates trace and console results after execution.
pub fn set_execution_results(
    ui_state: &mut ShellUiState,
    trace_entries: Vec<String>,
    console_entry_count: usize,
    console_error_count: usize,
    summary: Option<String>,
) {
    ui_state.copilot.trace_entries = trace_entries;
    ui_state.copilot.console_entry_count = console_entry_count;
    ui_state.copilot.console_error_count = console_error_count;
    ui_state.copilot.status_message = summary;
    ui_state.copilot.status = CopilotStatus::Complete;
    ui_state.copilot.plan_preview.clear();
}

/// Sets the Copilot into error state.
pub fn set_copilot_error(ui_state: &mut ShellUiState, message: &str) {
    ui_state.copilot.error_message = Some(message.to_owned());
    ui_state.copilot.status = CopilotStatus::Error(message.to_owned());
    ui_state.copilot.status_message = None;
}
