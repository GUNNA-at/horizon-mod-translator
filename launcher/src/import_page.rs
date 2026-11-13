//! Files dropped anywhere on the window (`horizon_launcher::import`): a shared instance becomes
//! a new instance, a Java modpack goes to the Modpacks import, a resource or shader pack into
//! the selected instance, a game build (a `horizon-client` program, or an archive or an Actions
//! artifact holding one) installed as its own version. The Packs section and the world rows take
//! their own drops first.
use super::*;
use gpui::ExternalPaths;
use horizon_launcher::import::{self, Dropped};
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct Importing {
    /// Shared instances being made.
    busy: usize,
    /// Java mods being translated.
    translating: usize,
    /// Java modpacks being translated into new instances.
    translating_packs: usize,
    /// What the last drop did, or why not.
    note: Option<Result<String, String>>,
}

impl Importing {
    pub(super) fn location_busy(&self) -> bool {
        self.busy > 0
    }
}

impl Launcher {
    pub(super) fn import_dropped(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        for file in files {
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match import::what(&file) {
                Err(e) => self.importing.note = Some(Err(e)),
                Ok(Dropped::Instance) => self.import_instance(file, cx),
                // The same import the Versions page's Import button starts. It
                // reports through `Msg::Imported`, which the Versions page
                // already shows, so the note here only says it began -- and
                // says so only if it did, since one import runs at a time.
                Ok(Dropped::Build) => {
                    if !self.can_import_build() {
                        self.importing.note = Some(Err(tr(
                            "Wait for the current operation to finish.",
                        )
                        .to_owned()));
                        continue;
                    }
                    self.start_build_import(file, cx);
                    self.importing.note = Some(Ok(tr("Importing…").to_owned()));
                }
                Ok(Dropped::Modpack) => self.translate_modpack(file, cx),
                Ok(Dropped::Pack(_)) => {
                    let Some(i) = self.selected_instance().cloned() else {
                        continue;
                    };
                    self.importing.note =
                        Some(horizon_launcher::packs::add(&i.home, &file).map(|p| {
                            trf(
                                "Added {title} to {instance}'s packs: switch it on under Packs.",
                                &[("title", &p.title), ("instance", &i.name)],
                            )
                        }));
                    self.game_packs.stale();
                }
                Ok(Dropped::JavaMod) => {
                    if let Some(i) = self.selected_instance().cloned() {
                        self.translate_java(file, i, cx);
                    }
                }
                Ok(Dropped::DataPack) => {
                    self.importing.note = Some(Err(trf(
                        "{name} is a data pack: drop it on a world on Home, or install it into \
                         one from there.",
                        &[("name", &name)],
                    )))
                }
            }
        }
    }

    fn import_instance(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        self.importing.busy += 1;
        self.importing.note = None;
        let task = cx
            .background_executor()
            .spawn(async move { import::import_instance(&file, &|_, _| {}) });
        cx.spawn(async move |this, cx| {
            let made = task.await;
            let _ = this.update(cx, |this, cx| {
                this.importing.busy -= 1;
                this.importing.note = Some(made.map(|i| {
                    this.reload_instances();
                    this.select_instance(&i);
                    this.go(Page::Play);
                    trf(
                        "Made the instance {name} from the shared file.",
                        &[("name", &i.name)],
                    )
                }));
                cx.notify();
            });
        })
        .detach();
    }

    /// Translates the Java mod `file` into `instance` on a background thread: its content mods
    /// and its code, translated into Rust, turned on; the Mods page's Build then builds the game
    /// with them.
    fn translate_java(&mut self, file: PathBuf, instance: instances::Instance, cx: &mut Context<Self>) {
        self.importing.translating += 1;
        self.importing.note = None;
        let task = cx.background_executor().spawn(async move {
            horizon_launcher::native_mods::add_java(&file, &instance.home, &|_| {})
                .map(|added| (added, instance))
        });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                this.importing.translating -= 1;
                this.importing.note = Some(done.and_then(|(added, i)| {
                    let mods: Vec<&str> = added
                        .outcome
                        .mods
                        .iter()
                        .filter(|m| m.native.is_some())
                        .map(|m| m.mod_id.as_str())
                        .collect();
                    match mods.is_empty() {
                        true => Err(tr("No mod in it has code that could be translated.").into()),
                        false => Ok(trf(
                            "Translated {mods} into {instance} and turned it on. Build the game with it on the Mods page.",
                            &[("mods", &mods.join(", ")), ("instance", &i.name)],
                        )),
                    }
                }));
                cx.notify();
            });
        })
        .detach();
    }

    /// Makes a new instance of the Java modpack `file` on a background thread, with its mods
    /// translated into it (`import::translate_new`), and selects it on the Mods page, where Build
    /// builds the game with them.
    fn translate_modpack(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        self.importing.translating_packs += 1;
        self.importing.note = None;
        let task = cx
            .background_executor()
            .spawn(async move { import::translate_new(&file, &|_| {}) });
        cx.spawn(async move |this, cx| {
            let made = task.await;
            let _ = this.update(cx, |this, cx| {
                this.importing.translating_packs -= 1;
                this.importing.note = Some(made.map(|(i, added)| {
                    this.reload_instances();
                    this.select_instance(&i);
                    this.go(Page::Mods);
                    let mods: Vec<&str> = added
                        .outcome
                        .mods
                        .iter()
                        .filter(|m| m.native.is_some())
                        .map(|m| m.mod_id.as_str())
                        .collect();
                    match mods.is_empty() {
                        true => trf(
                            "Made the instance {instance}: its mods' blocks and items are turned on; none of them has code to translate.",
                            &[("instance", &i.name)],
                        ),
                        false => trf(
                            "Made the instance {instance} and translated {mods} into it. Build the game with them here.",
                            &[("instance", &i.name), ("mods", &mods.join(", "))],
                        ),
                    }
                }));
                cx.notify();
            });
        })
        .detach();
    }

    /// The root's drop target: every drop the sections don't take.
    pub(super) fn import_drop_target(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
            this.import_dropped(paths.paths().to_vec(), cx);
            cx.notify();
        }))
    }

    /// What the last drop did, in a corner over the page until closed.
    pub(super) fn import_note(&self, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme;
        let line = match (&self.importing.note, self.importing.busy) {
            (_, n) if n > 0 => text(theme::LABEL)
                .text_color(t.ink_soft)
                .child(tr("Making the instance from the shared file…")),
            _ if self.importing.translating_packs > 0 => text(theme::LABEL)
                .text_color(t.ink_soft)
                .child(tr("Translating the Java modpack into a new instance…")),
            _ if self.importing.translating > 0 => text(theme::LABEL)
                .text_color(t.ink_soft)
                .child(tr("Translating the Java mod into Rust…")),
            (Some(Ok(said)), _) => text(theme::LABEL)
                .text_color(t.ink_soft)
                .child(said.clone()),
            (Some(Err(e)), _) => text(theme::LABEL).text_color(t.danger).child(e.clone()),
            (None, _) => return None,
        };
        Some(
            div()
                .absolute()
                .bottom(px(24.))
                .right(px(24.))
                .w(px(440.))
                .child(
                    plate(&t)
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(16.))
                        .py(px(12.))
                        .border_1()
                        .border_color(t.rule)
                        .bg(t.page)
                        .child(blocks::icon("download", 18., t.accent))
                        .child(div().flex_1().min_w_0().child(line))
                        .child(button("import-close", "Close", Kind::Ghost, &t).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.importing.note = None;
                                cx.notify();
                            }),
                        )),
                ),
        )
    }
}
