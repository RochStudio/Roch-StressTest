//! The window: a toolbar, Quick Start, a tab per tool with settings worth
//! offering, the log, and the status strip.
//!
//! Everything that touches the window happens on the UI thread. The runner
//! works on its own thread and reports through a channel that is drained once
//! per frame; that boundary is the only threading rule in the program, and it
//! is why a six-hour run does not deadlock at hour five.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use eframe::egui::{self, Margin, RichText, ScrollArea, Theme};
use serde_json::Value;

use crate::app::panel::{PanelState, TestsState, TEST_COLUMNS};
use crate::app::platform;
use crate::app::theme::{self, Pair};
use crate::app::widgets::{self, ButtonKind};
use crate::core::hardware;
use crate::core::memory::{self, CleanResult};
use crate::core::pyutil::{format_duration, local_timestamp, thousands};
use crate::core::runner::{Event, RunState, Runner};
use crate::core::settings;
use crate::core::toolbase::{Config, Tool};
use crate::core::version::{APP_NAME, VERSION};
use crate::tools;

/// How often the UI drains the runner's events. Fast enough that output
/// feels live, slow enough to be free.
pub const PUMP_MS: u64 = 120;

/// The log keeps this many lines. A twelve-hour Linpack run produces far more
/// than is worth redrawing, and only the tail is ever read.
pub const LOG_LIMIT: usize = 4000;

/// How often the live memory readout is refreshed.
pub const RAM_MS: u64 = 1000;

pub const SOCIAL_LINKS: [(&str, &str); 3] = [
    ("YouTube", "https://www.youtube.com/@MateoPcTech"),
    ("X", "https://x.com/MateoPCTech"),
    ("Discord", "https://discord.gg/KfzExpKQHB"),
];

pub fn state_colour(state: RunState) -> Pair {
    match state {
        RunState::Idle => theme::IDLE_COLOR,
        RunState::Running => theme::RUNNING_COLOR,
        RunState::Passed => theme::PASS_COLOR,
        RunState::Failed => theme::FAIL_COLOR,
        RunState::Stopped => theme::WARN_COLOR,
        RunState::Broken => theme::FAIL_COLOR,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tab {
    Quick,
    Tool(&'static str),
    Log,
}

/// One Quick Start card, worked out once rather than every frame: finding
/// the tools means globbing folders, and Cinebench's search is recursive.
struct QuickCard {
    tool: &'static dyn Tool,
    headline: String,
    headline_colour: Pair,
    note: String,
    /// (label, config); no config means "run the default".
    actions: Vec<(String, Option<Config>)>,
    enabled: bool,
}

/// Something a tab asked for while it was being drawn, done afterwards.
enum PanelAction {
    Start(&'static dyn Tool, Config, String),
    Log(String),
    Save(usize),
    Delete(usize),
}

pub struct StressApp {
    root: PathBuf,
    runner: Runner,
    panels: Vec<PanelState>,
    tab: Tab,
    log: VecDeque<String>,
    current_label: String,
    stat_text: String,
    state: RunState,
    detail: String,
    clock: String,
    dark: bool,
    appearance: String,
    cpu_text: String,
    ram_text: String,
    ram_colour: Pair,
    last_ram: Option<Instant>,
    cleaning: Option<Receiver<CleanResult>>,
    quick_cards: [Vec<QuickCard>; 2],
    hwnd: Option<isize>,
}

impl StressApp {
    pub fn new(cc: &eframe::CreationContext<'_>, admin: bool) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::install_style(&cc.egui_ctx);
        let mut app = Self::headless(tools::tools_root());
        app.apply_appearance(&cc.egui_ctx);
        if !admin {
            app.log(
                "Running without administrator rights: TestMem5 and RAM Test Pro will not be able \
                 to lock memory.",
            );
        }
        app
    }

    /// Everything but the window: panels, cards and the opening log lines.
    pub fn headless(root: PathBuf) -> Self {
        let panels = tools::tools()
            .iter()
            .map(|t| t.as_ref())
            .filter(|t| t.has_tab() && t.available(&root))
            .map(|t| PanelState::new(t, &root))
            .collect();
        let mut app = StressApp {
            root,
            runner: Runner::new(),
            panels,
            tab: Tab::Quick,
            log: VecDeque::new(),
            current_label: String::new(),
            stat_text: String::new(),
            state: RunState::Idle,
            detail: "Nothing running.".into(),
            clock: String::new(),
            dark: true,
            appearance: settings::load_appearance_mode(),
            cpu_text: hardware::describe_cpu(),
            ram_text: String::new(),
            ram_colour: theme::TEXT_COLOR,
            last_ram: None,
            cleaning: None,
            quick_cards: [Vec::new(), Vec::new()],
            hwnd: None,
        };
        app.dark = app.appearance != "Light";
        app.refresh_quick_tab();
        app.log(&format!("{APP_NAME} {VERSION}"));
        app.log(&hardware::describe());
        app.report_missing();
        app
    }

    pub fn panels(&self) -> &[PanelState] {
        &self.panels
    }

    pub fn log_lines(&self) -> impl Iterator<Item = &String> {
        self.log.iter()
    }

    // -- chrome --------------------------------------------------------------

    fn apply_appearance(&mut self, ctx: &egui::Context) {
        self.dark = self.appearance != "Light";
        ctx.set_theme(if self.dark { Theme::Dark } else { Theme::Light });
    }

    fn change_appearance_mode(&mut self, ctx: &egui::Context, mode: &str) {
        self.appearance = mode.to_string();
        self.apply_appearance(ctx);
        settings::save_setting("appearance_mode", Value::from(mode));
    }

    /// Redraw Quick Start, after a profile is saved or deleted.
    pub fn refresh_quick_tab(&mut self) {
        let root = self.root.clone();
        self.quick_cards = tools::quick_columns().map(|column| {
            column
                .into_iter()
                .map(|tool| Self::quick_card(tool, &root))
                .collect()
        });
    }

    fn quick_card(tool: &'static dyn Tool, root: &Path) -> QuickCard {
        let available = tool.available(root);
        let blocked = if available {
            tool.unsupported_reason(root)
        } else {
            String::new()
        };
        let headline = if !available && !tool.has_tab() {
            // No tab to send anybody to, so the card has to say where it
            // should go instead.
            "Not found -- unpack it into the program folder.".to_string()
        } else if !available {
            format!("Not found -- see the {} tab.", tool.name())
        } else if !blocked.is_empty() {
            "Not for this processor".to_string()
        } else {
            tool.quick_summary(root)
        };
        let headline_colour = if available && blocked.is_empty() {
            theme::TEXT_COLOR
        } else if !blocked.is_empty() {
            theme::WARN_COLOR
        } else {
            theme::FAIL_COLOR
        };
        // Only the two that say something the card cannot: why this tool will
        // not run on this processor, and where to unpack one that was not
        // found.
        let note = if !blocked.is_empty() {
            blocked.clone()
        } else if !available && !tool.has_tab() {
            root.display().to_string()
        } else {
            String::new()
        };
        let enabled = available && blocked.is_empty();
        let actions = if enabled {
            tool.quick_actions(root)
                .into_iter()
                .map(|(label, config)| (label, Some(config)))
                .collect()
        } else {
            vec![("Start".to_string(), None)]
        };
        QuickCard {
            tool,
            headline,
            headline_colour,
            note,
            actions,
            enabled,
        }
    }

    // -- memory --------------------------------------------------------------

    fn update_ram(&mut self) {
        let (available, total, used) = memory::reading();
        self.ram_text = format!(
            "RAM {} / {} MB free",
            thousands(available),
            thousands(total)
        );
        // Red once memory is nearly gone, which during a 28 GB run is the
        // normal state and worth being able to see at a glance.
        self.ram_colour = if used >= 90 {
            theme::FAIL_COLOR
        } else if used >= 75 {
            theme::WARN_COLOR
        } else {
            theme::TEXT_COLOR
        };
    }

    /// Empty working sets and purge the standby lists, off the UI thread.
    fn clean_memory(&mut self) {
        if self.cleaning.is_some() {
            return;
        }
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let _ = sender.send(memory::clean());
        });
        self.cleaning = Some(receiver);
    }

    fn poll_cleaning(&mut self) {
        let Some(receiver) = &self.cleaning else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            if matches!(
                receiver.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Disconnected)
            ) {
                self.log("Could not clean memory: the cleaner stopped unexpectedly.");
                self.cleaning = None;
            }
            return;
        };
        self.cleaning = None;
        self.log(&result.describe());
        if !result.failed.is_empty() && result.done.is_empty() {
            self.log(
                "  Memory cleaning needs administrator rights, which this program normally asks \
                 for at launch.",
            );
        }
        let (available, total, _) = memory::reading();
        self.ram_text = format!(
            "RAM {} / {} MB free",
            thousands(available),
            thousands(total)
        );
    }

    // -- log -----------------------------------------------------------------

    pub fn log(&mut self, text: &str) {
        let lines: Vec<&str> = if text.is_empty() {
            vec![""]
        } else {
            text.split('\n').collect()
        };
        for line in lines {
            self.log.push_back(line.trim_end_matches('\r').to_string());
        }
        while self.log.len() > LOG_LIMIT {
            self.log.pop_front();
        }
    }

    fn log_text(&self) -> String {
        let mut text = String::new();
        for line in &self.log {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    fn clear_log(&mut self) {
        self.log.clear();
    }

    /// Written as Python wrote a text file on Windows: CRLF line endings.
    fn write_text(path: &Path, text: &str) -> std::io::Result<()> {
        fs::write(path, text.replace('\n', "\r\n"))
    }

    fn save_log(&mut self) {
        let path = settings::log_dir().join(format!("stress-{}.txt", local_timestamp()));
        match Self::write_text(&path, &(self.log_text() + "\n")) {
            Ok(()) => self.log(&format!("Transcript saved to {}", path.display())),
            Err(error) => self.log(&format!("Could not save the transcript: {error}")),
        }
    }

    fn report_missing(&mut self) {
        let names: Vec<&str> = tools::missing(&self.root)
            .iter()
            .map(|t| t.name())
            .collect();
        for name in names {
            self.log(&format!(
                "Not found: {name} (its tab explains where to unpack it)"
            ));
        }
    }

    // -- running -------------------------------------------------------------

    pub fn busy(&self) -> bool {
        self.runner.running()
    }

    pub fn start_single(&mut self, tool: &'static dyn Tool, config: &Config, label: &str) {
        if self.busy() {
            self.log("Already running. Stop the current test first.");
            return;
        }
        let spec = match tool.build(config, &self.root) {
            Ok(spec) => spec,
            Err(error) => {
                self.log(&format!("Could not start {}: {}", tool.name(), error));
                self.set_state(RunState::Broken, &error.to_string());
                return;
            }
        };
        self.current_label = label.to_string();
        self.log("");
        self.log(&format!("=== {label} ==="));
        self.log(&spec.summary);
        // The raw command line when there is one, because for the tool that
        // uses it the argument list is not what actually gets run.
        let command = spec.cmdline.clone().unwrap_or_else(|| spec.argv.join(" "));
        self.log(&format!("Command: {command}"));
        if spec.duration_seconds > 0 {
            self.log(&format!(
                "Time limit: {}",
                format_duration(spec.duration_seconds as f64)
            ));
        } else {
            self.log("No time limit -- runs until you press Stop.");
        }
        if let Err(error) = self.runner.start(spec, label) {
            self.log(&format!("Could not start the process: {error}"));
            self.set_state(RunState::Broken, &error);
        }
    }

    fn quick_label(&self, tool: &dyn Tool) -> String {
        let name = tool.quick_preset_name(&self.root);
        if name.is_empty() {
            tool.name().to_string()
        } else {
            format!("{} -- {}", tool.name(), name)
        }
    }

    /// Run a tool's default configuration straight from Quick Start.
    pub fn start_quick(&mut self, tool: &'static dyn Tool) {
        let config = tool.quick_config(&self.root);
        let label = self.quick_label(tool);
        self.start_single(tool, &config, &label);
    }

    fn pump_events(&mut self) {
        for event in self.runner.drain() {
            self.handle(event);
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Output(line) => self.log(&format!("  {line}")),
            Event::Stat(text) => self.stat_text = text,
            Event::Tick { elapsed, remaining } => self.update_clock(elapsed, remaining),
            Event::State { state, note } => self.on_state(state, &note),
        }
    }

    fn update_clock(&mut self, elapsed: f64, remaining: Option<f64>) {
        let mut text = format_duration(elapsed);
        if let Some(remaining) = remaining {
            text += &format!("  /  {} left", format_duration(remaining));
        }
        self.clock = text;
        if !self.stat_text.is_empty() {
            self.detail = format!("{}   {}", self.current_label, self.stat_text);
        }
    }

    fn set_state(&mut self, state: RunState, note: &str) {
        self.state = state;
        self.detail = if !note.is_empty() {
            note.to_string()
        } else if !self.current_label.is_empty() {
            self.current_label.clone()
        } else {
            "Nothing running.".to_string()
        };
    }

    fn on_state(&mut self, state: RunState, note: &str) {
        self.set_state(state, note);
        if state == RunState::Running {
            self.stat_text.clear();
            return;
        }
        self.log(&format!("{}: {}", state.text().to_uppercase(), note));
        if !self.stat_text.is_empty() {
            let stat = format!("  {}", self.stat_text);
            self.log(&stat);
        }
        if state == RunState::Failed {
            self.announce_failure(note);
        }
        self.clock.clear();
    }

    /// Make a failure impossible to miss, and keep a copy of it.
    ///
    /// The transcript is written without being asked because the run that
    /// matters is the one that failed at 4am, and the window may well be gone
    /// by the time anybody looks.
    fn announce_failure(&mut self, note: &str) {
        platform::bell();
        let path = settings::log_dir().join(format!("FAILED-{}.txt", local_timestamp()));
        let text = format!(
            "{APP_NAME} {VERSION}\n{}\n{}\n{}\n\n{}\n",
            hardware::describe(),
            self.current_label,
            note,
            self.log_text()
        );
        if Self::write_text(&path, &text).is_ok() {
            self.log(&format!(
                "Transcript of the failure saved to {}",
                path.display()
            ));
        }
        self.tab = Tab::Log;
    }

    /// Never leave a stress test running with no window to stop it.
    pub fn on_close(&mut self) {
        if self.busy() {
            self.runner.stop();
        }
    }

    // -- profiles ------------------------------------------------------------

    fn save_profile(&mut self, index: usize) {
        let hwnd = self.hwnd;
        let panel = &mut self.panels[index];
        panel.rescan_profiles();
        let (name, existing) = panel.save_target();
        if name.is_empty() {
            platform::warn(hwnd, APP_NAME, "Give the profile a name first.");
            return;
        }
        let picked = panel
            .profile
            .as_ref()
            .map(|p| p.value())
            .unwrap_or_default();
        if let Some(existing) = existing {
            if name != picked {
                let file = existing
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !platform::ask_yes_no(
                    hwnd,
                    APP_NAME,
                    &format!("{file} already exists. Replace it?"),
                ) {
                    return;
                }
            }
        }
        match panel.save_profile(&name) {
            Ok(path) => {
                let line = format!("Saved {} profile {}", panel.tool.name(), path.display());
                self.refresh_quick_tab();
                self.log(&line);
            }
            Err(error) => {
                platform::error(
                    hwnd,
                    APP_NAME,
                    &format!("Could not save the profile:\n{error}"),
                );
            }
        }
    }

    fn delete_profile(&mut self, index: usize) {
        let hwnd = self.hwnd;
        let panel = &mut self.panels[index];
        panel.rescan_profiles();
        let Some(state) = panel.profile.as_ref() else {
            return;
        };
        let Some(path) = state.path_of(&state.value()) else {
            return;
        };
        let file = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !platform::ask_yes_no(hwnd, APP_NAME, &format!("Delete {file}?")) {
            return;
        }
        match panel.delete_profile(&path) {
            Ok(()) => {
                let line = format!("Deleted {} profile {}", panel.tool.name(), path.display());
                self.refresh_quick_tab();
                self.log(&line);
            }
            Err(error) => {
                platform::error(hwnd, APP_NAME, &format!("Could not delete it:\n{error}"))
            }
        }
    }

    // -- drawing -------------------------------------------------------------

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(theme::HEADER_COLOR.get(dark))
                    .inner_margin(Margin::symmetric(6, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    widgets::text(ui, &self.cpu_text, theme::SUBTITLE_COLOR, dark, false);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut mode = self.appearance.clone();
                        let modes = ["Dark".to_string(), "Light".to_string()];
                        if widgets::choice(ui, "appearance", &mut mode, &modes, 76.0) {
                            self.change_appearance_mode(ui.ctx(), &mode);
                        }
                        let cleaning = self.cleaning.is_some();
                        let label = if cleaning {
                            "Cleaning..."
                        } else {
                            "Clean memory"
                        };
                        if widgets::action_button(
                            ui,
                            label,
                            ButtonKind::Normal,
                            120.0,
                            !cleaning,
                            dark,
                        )
                        .clicked()
                        {
                            self.clean_memory();
                        }
                        ui.label(
                            RichText::new(&self.ram_text)
                                .font(theme::bold(theme::FONT_SIZE))
                                .color(self.ram_colour.get(dark)),
                        );
                    });
                });
            });
    }

    fn tab_names(&self) -> Vec<(Tab, String)> {
        let mut tabs = vec![(Tab::Quick, "Quick Start".to_string())];
        for tool in tools::tools() {
            // A tool configured in its own window has nothing to put on a tab.
            if tool.has_tab() {
                tabs.push((Tab::Tool(tool.key()), tool.name().to_string()));
            }
        }
        tabs.push((Tab::Log, "Log".to_string()));
        tabs
    }

    fn draw_tab_bar(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_COLOR.get(dark))
                    .inner_margin(Margin::symmetric(6, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (tab, name) in self.tab_names() {
                        if widgets::tab_button(ui, &name, self.tab == tab, dark).clicked() {
                            self.tab = tab;
                        }
                    }
                });
            });
    }

    fn draw_status(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        // The same three links as Roch CPU, GPU and Viewer: a line of their
        // own in the bottom-left corner, in the accent red, with the
        // separators in the text colour.
        egui::TopBottomPanel::bottom("links")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_COLOR.get(dark))
                    .inner_margin(Margin {
                        left: 12,
                        right: 12,
                        top: 2,
                        bottom: 6,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (index, (text, url)) in SOCIAL_LINKS.iter().enumerate() {
                        if index > 0 {
                            widgets::text(ui, "|", theme::TEXT_COLOR, dark, true);
                        }
                        if widgets::link(ui, text, dark).clicked() {
                            platform::open_url(url);
                        }
                    }
                });
            });
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::HEADER_COLOR.get(dark))
                    .inner_margin(Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let state = RichText::new(self.state.text())
                        .font(theme::bold(theme::FONT_SIZE))
                        .color(state_colour(self.state).get(dark));
                    ui.allocate_ui_with_layout(
                        egui::vec2(110.0, 18.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(110.0);
                            ui.label(state);
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(&self.clock)
                                .font(theme::bold(theme::FONT_SIZE))
                                .color(theme::TEXT_COLOR.get(dark)),
                        );
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.detail)
                                        .font(theme::font(theme::FONT_SIZE))
                                        .color(theme::TEXT_COLOR.get(dark)),
                                )
                                .truncate(),
                            );
                        });
                    });
                });
            });
    }

    fn draw_quick(&mut self, ui: &mut egui::Ui) -> Vec<PanelAction> {
        let dark = self.dark;
        let mut actions = Vec::new();
        // Two columns of cards rather than one tall list, so every tool is on
        // screen at once and Quick Start needs no scrolling to use.
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.columns(2, |columns| {
                    for (column, cards) in columns.iter_mut().zip(self.quick_cards.iter()) {
                        for card in cards {
                            Self::draw_quick_card(column, card, dark, &mut actions);
                        }
                    }
                });
            });
        actions
    }

    fn draw_quick_card(
        ui: &mut egui::Ui,
        card: &QuickCard,
        dark: bool,
        actions: &mut Vec<PanelAction>,
    ) {
        widgets::section(ui, dark, card.tool.name(), |ui| {
            if !card.headline.is_empty() {
                ui.scope(|ui| {
                    ui.set_max_width(400.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&card.headline)
                                .font(theme::bold(theme::FONT_SIZE))
                                .color(card.headline_colour.get(dark)),
                        )
                        .wrap(),
                    );
                });
            }
            widgets::hint(ui, &card.note, dark, 380.0);
            ui.add_space(3.0);
            // Four to a row. Cinebench has a button per version installed,
            // which is seven on a machine with the lot.
            let width = if card.actions.len() == 1 { 110.0 } else { 88.0 };
            for chunk in card.actions.chunks(4) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (label, config) in chunk {
                        let response = widgets::action_button(
                            ui,
                            label,
                            ButtonKind::Start,
                            width,
                            card.enabled,
                            dark,
                        );
                        if response.clicked() {
                            match config {
                                Some(config) => actions.push(PanelAction::Start(
                                    card.tool,
                                    config.clone(),
                                    format!("{} -- {}", card.tool.name(), label),
                                )),
                                None => actions.push(PanelAction::Start(
                                    card.tool,
                                    Config::new(),
                                    String::new(),
                                )),
                            }
                        }
                    }
                });
            }
        });
    }

    fn draw_missing(ui: &mut egui::Ui, tool: &dyn Tool, root: &Path, dark: bool) {
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.label(
                RichText::new(format!("{} was not found.", tool.name()))
                    .font(theme::bold(theme::FONT_SIZE))
                    .color(theme::FAIL_COLOR.get(dark)),
            );
        });
        ui.add_space(4.0);
        let mut text = format!("Unpack it into\n{}\n\nExpected one of:\n  ", root.display());
        text += &tool.exe_globs().join("\n  ");
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            widgets::text(ui, &text, theme::SUBTITLE_COLOR, dark, false);
        });
    }

    fn draw_panel(
        ui: &mut egui::Ui,
        index: usize,
        panel: &mut PanelState,
        dark: bool,
    ) -> Vec<PanelAction> {
        let mut actions = Vec::new();
        let key = panel.tool.key();

        // The action bar first, against the bottom, so Start keeps its place
        // no matter how long the settings list grows.
        egui::TopBottomPanel::bottom(format!("{key}-actions"))
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_COLOR.get(dark))
                    .inner_margin(Margin::same(6)),
            )
            .show_inside(ui, |ui| {
                egui::Frame::new()
                    .fill(theme::SECTION_COLOR.get(dark))
                    .corner_radius(6)
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let enabled = panel.unsupported.is_empty();
                        if widgets::action_button(
                            ui,
                            "Start",
                            ButtonKind::Start,
                            120.0,
                            enabled,
                            dark,
                        )
                        .clicked()
                        {
                            actions.push(PanelAction::Start(
                                panel.tool,
                                panel.config(),
                                panel.label(),
                            ));
                        }
                    });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_COLOR.get(dark)))
            .show_inside(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt(format!("{key}-scroll"))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.scope(|ui| {
                                ui.set_max_width(640.0);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(panel.tool.blurb())
                                            .font(theme::font(theme::FONT_SIZE))
                                            .color(theme::SUBTITLE_COLOR.get(dark)),
                                    )
                                    .wrap(),
                                );
                            });
                        });
                        ui.add_space(8.0);
                        Self::draw_unsupported(ui, panel, dark);
                        Self::draw_profile_section(ui, index, panel, dark, &mut actions);
                        Self::draw_tests_section(ui, panel, dark);
                        Self::draw_preset_section(ui, panel, dark);
                        Self::draw_settings_section(ui, panel, dark);
                        Self::draw_preview_section(ui, panel, dark);
                    });
            });
        actions
    }

    fn draw_unsupported(ui: &mut egui::Ui, panel: &PanelState, dark: bool) {
        if panel.unsupported.is_empty() {
            return;
        }
        widgets::section(ui, dark, "Not for this processor", |ui| {
            ui.scope(|ui| {
                ui.set_max_width(880.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(&panel.unsupported)
                            .font(theme::font(theme::FONT_SIZE))
                            .color(theme::WARN_COLOR.get(dark)),
                    )
                    .wrap(),
                );
            });
        });
    }

    fn draw_profile_section(
        ui: &mut egui::Ui,
        index: usize,
        panel: &mut PanelState,
        dark: bool,
        actions: &mut Vec<PanelAction>,
    ) {
        let Some(profiles) = panel.tool.as_profiles() else {
            return;
        };
        let key = panel.tool.key();
        let preview_inline = profiles.profile_preview_label().is_empty();
        let preview = if preview_inline {
            panel.preview()
        } else {
            String::new()
        };
        let mut picked = false;
        widgets::section(ui, dark, "Profile", |ui| {
            egui::Grid::new(format!("{key}-profile"))
                .num_columns(2)
                .spacing([8.0, 3.0])
                .min_col_width(theme::LABEL_MINSIZE - 8.0)
                .show(ui, |ui| {
                    let Some(state) = panel.profile.as_mut() else {
                        return;
                    };
                    widgets::text(ui, "Profile", theme::TEXT_COLOR, dark, false);
                    let names = state.names();
                    let mut selected = state.value();
                    if widgets::choice(
                        ui,
                        format!("{key}-profile-pick"),
                        &mut selected,
                        &names,
                        theme::FIELD_WIDTH + 60.0,
                    ) {
                        state.selected = selected;
                        picked = true;
                    }
                    ui.end_row();

                    widgets::text(ui, "Save as", theme::TEXT_COLOR, dark, false);
                    ui.horizontal(|ui| {
                        widgets::entry(ui, &mut state.name, theme::FIELD_WIDTH + 60.0, "name");
                        if widgets::action_button(ui, "Save", ButtonKind::Normal, 70.0, true, dark)
                            .clicked()
                        {
                            actions.push(PanelAction::Save(index));
                        }
                        let can_delete = state.delete_enabled();
                        if widgets::action_button(
                            ui,
                            "Delete",
                            ButtonKind::Normal,
                            70.0,
                            can_delete,
                            dark,
                        )
                        .clicked()
                        {
                            actions.push(PanelAction::Delete(index));
                        }
                    });
                    ui.end_row();

                    ui.label("");
                    widgets::hint(ui, profiles.profile_hint(), dark, 520.0);
                    ui.end_row();

                    // A one-line command fits under the name; a whole file
                    // gets a section of its own at the bottom.
                    if preview_inline {
                        widgets::text(ui, "Command line", theme::TEXT_COLOR, dark, false);
                        ui.scope(|ui| {
                            ui.set_max_width(560.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&preview)
                                        .font(theme::font(theme::LOG_SIZE))
                                        .color(theme::TEXT_COLOR.get(dark)),
                                )
                                .wrap(),
                            );
                        });
                        ui.end_row();
                    }
                });
        });
        if picked {
            if let Err(line) = panel.profile_picked() {
                actions.push(PanelAction::Log(line));
            }
        }
    }

    fn draw_tests_section(ui: &mut egui::Ui, panel: &mut PanelState, dark: bool) {
        let key = panel.tool.key();
        let Some(tests) = panel.tests.as_mut() else {
            return;
        };
        widgets::section(ui, dark, "Tests", |ui| {
            let mut remove = None;
            let mut move_up = None;
            egui::Grid::new(format!("{key}-tests"))
                .num_columns(8)
                .spacing([8.0, 3.0])
                .show(ui, |ui| {
                    ui.label("");
                    for (_, title) in TEST_COLUMNS {
                        widgets::text(ui, title, theme::SUBTITLE_COLOR, dark, false);
                    }
                    widgets::text(ui, "Memory", theme::SUBTITLE_COLOR, dark, false);
                    ui.end_row();
                    let count = tests.rows.len();
                    for (row_index, row) in tests.rows.iter_mut().enumerate() {
                        widgets::text(
                            ui,
                            &(row_index + 1).to_string(),
                            theme::TEXT_COLOR,
                            dark,
                            true,
                        );
                        for cell in row.iter_mut() {
                            widgets::entry(ui, cell, 104.0, "");
                        }
                        widgets::text(
                            ui,
                            &TestsState::memory_text(row),
                            theme::SUBTITLE_COLOR,
                            dark,
                            false,
                        );
                        if row_index > 0 {
                            if widgets::action_button(
                                ui,
                                "Up",
                                ButtonKind::Normal,
                                40.0,
                                true,
                                dark,
                            )
                            .clicked()
                            {
                                move_up = Some(row_index);
                            }
                        } else {
                            ui.label("");
                        }
                        if widgets::action_button(
                            ui,
                            "Remove",
                            ButtonKind::Normal,
                            60.0,
                            count > 1,
                            dark,
                        )
                        .clicked()
                        {
                            remove = Some(row_index);
                        }
                        ui.end_row();
                    }
                });
            if let Some(index) = move_up {
                tests.move_up(index);
            }
            if let Some(index) = remove {
                tests.remove(index);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                widgets::text(ui, "Add a test sized for", theme::TEXT_COLOR, dark, false);
                widgets::entry(ui, &mut tests.add_gb, 50.0, "");
                widgets::text(ui, "GB", theme::SUBTITLE_COLOR, dark, false);
                if widgets::action_button(ui, "Add test", ButtonKind::Normal, 80.0, true, dark)
                    .clicked()
                {
                    tests.add();
                }
            });
            widgets::hint(
                ui,
                "They run top to bottom. A leading dimension smaller than the problem size is \
                 replaced with Intel's recommended one.",
                dark,
                640.0,
            );
        });
    }

    fn draw_preset_section(ui: &mut egui::Ui, panel: &mut PanelState, dark: bool) {
        // Some tools have nothing to preset, and a picker with one entry
        // would be a control that does nothing.
        if panel.preset.is_none() {
            return;
        }
        let key = panel.tool.key();
        let names: Vec<String> = panel.presets().into_iter().map(|p| p.name).collect();
        let mut changed = None;
        widgets::section(ui, dark, "What to run", |ui| {
            let Some(state) = panel.preset.as_mut() else {
                return;
            };
            egui::Grid::new(format!("{key}-preset"))
                .num_columns(2)
                .spacing([8.0, 3.0])
                .min_col_width(theme::LABEL_MINSIZE - 8.0)
                .show(ui, |ui| {
                    widgets::text(ui, "Preset", theme::TEXT_COLOR, dark, false);
                    let mut selected = state.selected.clone();
                    if widgets::choice(
                        ui,
                        format!("{key}-preset-pick"),
                        &mut selected,
                        &names,
                        theme::FIELD_WIDTH + 60.0,
                    ) {
                        state.selected = selected.clone();
                        changed = Some(selected);
                    }
                    ui.end_row();
                    ui.label("");
                    widgets::hint(ui, &state.note, dark, 520.0);
                    ui.end_row();
                });
        });
        if let Some(name) = changed {
            panel.apply_preset(&name, false);
        }
    }

    fn draw_settings_section(ui: &mut egui::Ui, panel: &mut PanelState, dark: bool) {
        let key = panel.tool.key();
        let mut edited = Vec::new();
        widgets::section(ui, dark, "Settings", |ui| {
            egui::Grid::new(format!("{key}-settings"))
                .num_columns(3)
                .spacing([8.0, 3.0])
                .min_col_width(theme::LABEL_MINSIZE - 8.0)
                .show(ui, |ui| {
                    for row in panel.rows.iter_mut() {
                        let id = format!("{key}-{}", row.field.key);
                        if widgets::field_row(ui, &id, row, dark) {
                            edited.push(row.field.key);
                        }
                        ui.end_row();
                        if !row.field.hint.is_empty() {
                            ui.label("");
                            widgets::hint(ui, row.field.hint, dark, 520.0);
                            ui.end_row();
                        }
                    }
                });
        });
        for key in edited {
            panel.field_changed(key);
        }
    }

    fn draw_preview_section(ui: &mut egui::Ui, panel: &PanelState, dark: bool) {
        let Some(profiles) = panel.tool.as_profiles() else {
            return;
        };
        let title = profiles.profile_preview_label();
        if title.is_empty() {
            return;
        }
        let preview = panel.preview();
        widgets::section(ui, dark, title, |ui| {
            ui.label(
                RichText::new(preview)
                    .font(theme::font(theme::LOG_SIZE))
                    .color(theme::TEXT_COLOR.get(dark)),
            );
        });
    }

    fn draw_log(&mut self, ui: &mut egui::Ui) {
        let dark = self.dark;
        let mut save = false;
        let mut clear = false;
        egui::TopBottomPanel::bottom("log-actions")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_COLOR.get(dark))
                    .inner_margin(Margin {
                        left: 12,
                        right: 12,
                        top: 0,
                        bottom: 12,
                    }),
            )
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    save = widgets::action_button(
                        ui,
                        "Save transcript",
                        ButtonKind::Normal,
                        130.0,
                        true,
                        dark,
                    )
                    .clicked();
                    ui.add_space(4.0);
                    clear =
                        widgets::action_button(ui, "Clear", ButtonKind::Normal, 80.0, true, dark)
                            .clicked();
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_COLOR.get(dark))
                    .inner_margin(Margin {
                        left: 12,
                        right: 12,
                        top: 10,
                        bottom: 8,
                    }),
            )
            .show_inside(ui, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_COLOR2.get(dark))
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_COLOR.get(dark)))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        let row_height =
                            ui.fonts_mut(|f| f.row_height(&theme::font(theme::LOG_SIZE)));
                        let total = self.log.len();
                        ScrollArea::both()
                            .id_salt("log-scroll")
                            .auto_shrink([false, false])
                            .stick_to_bottom(true)
                            .show_rows(ui, row_height, total, |ui, range| {
                                for line in self.log.range(range) {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(line.as_str())
                                                .font(theme::font(theme::LOG_SIZE))
                                                .color(theme::TEXT_COLOR.get(dark)),
                                        )
                                        .extend(),
                                    );
                                }
                            });
                    });
            });
        if save {
            self.save_log();
        }
        if clear {
            self.clear_log();
        }
    }

    fn draw_central(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        let mut actions = Vec::new();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_COLOR.get(dark))
                    .inner_margin(Margin::symmetric(2, 2)),
            )
            .show(ctx, |ui| match self.tab.clone() {
                Tab::Quick => actions = self.draw_quick(ui),
                Tab::Log => self.draw_log(ui),
                Tab::Tool(key) => {
                    if let Some(index) = self.panels.iter().position(|p| p.tool.key() == key) {
                        let panel = &mut self.panels[index];
                        actions = Self::draw_panel(ui, index, panel, dark);
                    } else if let Some(tool) = tools::by_key(key) {
                        Self::draw_missing(ui, tool, &self.root, dark);
                    }
                }
            });
        for action in actions {
            match action {
                PanelAction::Start(tool, config, label) => {
                    if label.is_empty() {
                        self.start_quick(tool);
                    } else {
                        self.start_single(tool, &config, &label);
                    }
                }
                PanelAction::Log(line) => self.log(&line),
                PanelAction::Save(index) => self.save_profile(index),
                PanelAction::Delete(index) => self.delete_profile(index),
            }
        }
    }
}

impl eframe::App for StressApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if self.hwnd.is_none() {
                if let Ok(handle) = frame.window_handle() {
                    if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                        self.hwnd = Some(win32.hwnd.get());
                    }
                }
            }
        }
        #[cfg(not(windows))]
        let _ = frame;

        self.pump_events();
        self.poll_cleaning();
        let due = self
            .last_ram
            .map(|t| t.elapsed() >= Duration::from_millis(RAM_MS))
            .unwrap_or(true);
        if due {
            self.last_ram = Some(Instant::now());
            self.update_ram();
        }

        self.draw_toolbar(ctx);
        self.draw_tab_bar(ctx);
        self.draw_status(ctx);
        self.draw_central(ctx);

        ctx.request_repaint_after(Duration::from_millis(PUMP_MS));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.on_close();
    }
}

/// The window's icon, out of the same .ico the executable carries.
pub fn window_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../icon.ico");
    let dir = ico::IconDir::read(std::io::Cursor::new(&bytes[..])).ok()?;
    let entry = dir.entries().iter().max_by_key(|e| e.width())?;
    let image = entry.decode().ok()?;
    Some(egui::IconData {
        rgba: image.rgba_data().to_vec(),
        width: image.width(),
        height: image.height(),
    })
}

/// Open the window and run until it is closed.
pub fn run(admin: bool) -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("{APP_NAME} {VERSION}"))
        .with_inner_size([850.0, 775.0])
        .with_min_inner_size([700.0, 560.0]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        &format!("{APP_NAME} {VERSION}"),
        options,
        Box::new(move |cc| Ok(Box::new(StressApp::new(cc, admin)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_decodes() {
        let icon = window_icon().expect("icon.ico decodes");
        assert_eq!(icon.rgba.len() as u32, icon.width * icon.height * 4);
    }

    #[test]
    fn profile_names_start_with_new() {
        use crate::app::panel::NEW_PROFILE;
        assert_eq!(NEW_PROFILE, "New profile");
    }
}
