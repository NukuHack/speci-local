//! The modal dialogs: unsaved changes, and pasting a project by hand.

use super::App;
use crate::convert::{self, Op};
use crate::platform;
use crate::ui::{self, I_COPY, I_PASTE, I_RUN, Unsaved, modal_width};
use eframe::egui::{self, Id, TextEdit, TextStyle};

/// The state of the “Convert links” dialog: what to convert, the input and the last result.
pub(super) struct ConvertBox {
    pub op: Op,
    pub input: String,
    /// The result for `(op, input)` it was made from.
    done: Option<(Op, String, Result<convert::Converted, String>)>,
}

impl ConvertBox {
    pub fn new(op: Op) -> Self {
        ConvertBox {
            op,
            input: String::new(),
            done: None,
        }
    }

    pub(super) fn result(&mut self) -> Option<&Result<convert::Converted, String>> {
        if self.input.trim().is_empty() {
            self.done = None;
            return None;
        }
        let stale = !matches!(&self.done, Some((o, i, _)) if *o == self.op && *i == self.input);
        if stale {
            let r = convert::run(self.op, &self.input);
            self.done = Some((self.op, self.input.clone(), r));
        }
        self.done.as_ref().map(|(_, _, r)| r)
    }
}

impl App {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        self.confirm_dialog(ctx);
        #[cfg(target_os = "android")]
        self.files_dialog(ctx);
        self.paste_dialog(ctx);
        self.convert_dialog(ctx);
        self.update_dialog(ctx);
    }

    fn after_save(&mut self, action: super::Replace) {
        // Android: the save dialog is open, continue once it's done
        #[cfg(target_os = "android")]
        if let Some(f) = &mut self.files {
            f.then = Some(action);
        }
        #[cfg(not(target_os = "android"))]
        let _ = action;
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm.clone() else {
            return;
        };
        let Some(choice) = ui::unsaved_dialog(ctx, "discard-dialog", "project") else {
            return;
        };
        self.confirm = None;
        match choice {
            Unsaved::Save => {
                self.save(false);
                if self.dirty() {
                    self.after_save(action);
                } else {
                    self.run(action);
                }
            }
            Unsaved::Discard => self.run(action),
            Unsaved::Cancel => {}
        }
    }

    fn paste_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut text) = self.paste_box.take() else {
            return;
        };
        let mut done = false;
        let modal = egui::Modal::new(Id::unique("paste-dialog")).show(ctx, |ui| {
            ui.set_width(modal_width(ctx, 520.0));
            ui.heading("Paste a project");
            ui.label(platform::shortcut("The clipboard couldn't be read directly. Paste the copied project (SPECI1:…) or a specification here with Ctrl+V:"));
            ui.add(TextEdit::multiline(&mut text).desired_rows(6).desired_width(f32::INFINITY).font(TextStyle::Monospace));
            ui.horizontal(|ui| {
                if ui.button(format!("{I_PASTE}  Import")).clicked() {
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    text.clear();
                    done = true;
                }
            });
        });
        if modal.should_close() && !done {
            return; // closed with Escape or a click outside
        }
        if done {
            if !text.trim().is_empty() {
                self.import_text(&text);
            }
        } else {
            self.paste_box = Some(text);
        }
    }

    /// Opens the dialog (the settings menu's “Convert links…”). On the Code tab the program is
    /// the input of the C# conversions.
    pub(super) fn open_convert(&mut self, op: Op) {
        let mut b = ConvertBox::new(op);
        if op.wants_csharp() {
            b.input = self.code.text.clone();
        }
        self.convert_box = Some(b);
    }

    fn convert_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut b) = self.convert_box.take() else {
            return;
        };
        let code_text = self.code.text.clone();
        let mut close = false;
        let mut import: Option<String> = None;
        let mut open_url: Option<String> = None;
        let mut copy: Option<String> = None;
        let modal = egui::Modal::new(Id::unique("convert-dialog")).show(ctx, |ui| {
            ui.set_width(modal_width(ctx, 640.0));
            ui.heading("Convert links");
            ui.label(
                "progalap.elte.hu links, SPECI1: / STUKI1: lines, their content and C# programs.",
            );
            // a short window scrolls the whole dialog; the Close button stays in view
            egui::ScrollArea::vertical()
                .id_salt("convert-scroll")
                .max_height((ctx.content_rect().height() * 0.7).max(200.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Convert:");
                        let before = b.op;
                        egui::ComboBox::from_id_salt("convert-op")
                            .selected_text(b.op.label())
                            .show_ui(ui, |ui| {
                                for op in Op::ALL {
                                    ui.selectable_value(&mut b.op, op, op.label());
                                }
                            });
                        if b.op != before && b.op.wants_csharp() && b.input.trim().is_empty() {
                            b.input = code_text.clone();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(if b.op.wants_csharp() {
                            "C# program"
                        } else {
                            "Input"
                        });
                        if ui.small_button(format!("{I_PASTE} Paste")).clicked()
                            && let Some(t) = platform::paste()
                        {
                            b.input = t;
                        }
                        if ui.small_button("Use the Code tab").clicked() {
                            b.input = code_text.clone();
                            if !b.op.wants_csharp() {
                                b.op = Op::CsToUrl;
                            }
                        }
                        if ui.small_button("Clear").clicked() {
                            b.input.clear();
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("convert-in")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            ui.add(
                                TextEdit::multiline(&mut b.input)
                                    .desired_rows(6)
                                    .desired_width(f32::INFINITY)
                                    .font(TextStyle::Monospace),
                            );
                        });
                    match b.result() {
                        None => {
                            ui.weak("Paste or type something above; the result appears here.");
                        }
                        Some(Err(e)) => {
                            ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                        }
                        Some(Ok(c)) => {
                            ui.label(format!("Result ({})", c.note));
                            let mut shown = c.text.clone();
                            egui::ScrollArea::vertical()
                                .id_salt("convert-out")
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        TextEdit::multiline(&mut shown)
                                            .desired_rows(6)
                                            .desired_width(f32::INFINITY)
                                            .font(TextStyle::Monospace),
                                    );
                                });
                            ui.horizontal_wrapped(|ui| {
                                if ui.button(format!("{I_COPY}  Copy result")).clicked() {
                                    copy = Some(c.text.clone());
                                }
                                if c.text.starts_with("https://")
                                    && ui.button(format!("{I_RUN}  Open link")).clicked()
                                {
                                    open_url = Some(c.text.clone());
                                }
                                let importable =
                                    convert::is_line(&c.text) || convert::is_link(&c.text);
                                if importable
                                    && ui
                                        .button("Import as project")
                                        .on_hover_text(
                                            "Read the result into the editors (like Paste project)",
                                        )
                                        .clicked()
                                {
                                    import = Some(c.text.clone());
                                }
                            });
                        }
                    }
                });
            ui.separator();
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        if let Some(t) = copy {
            ctx.copy_text(t);
            self.status = "Copied the conversion result".into();
        }
        if let Some(u) = open_url {
            ctx.open_url(egui::OpenUrl::new_tab(u));
        }
        if let Some(t) = import {
            self.import_text(&t);
            close = true;
        }
        if modal.should_close() || close {
            return;
        }
        self.convert_box = Some(b);
    }
}
