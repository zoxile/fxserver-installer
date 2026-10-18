use super::{
    preflight, safe_text,
    settings::{self, Settings},
};
use crate::{
    commands::{
        artifact,
        fxserver::{self, FxserverManager},
    },
    models::{
        artifact::ArtifactBuild,
        fxserver::{
            FxserverCommandRequest, FxserverRconConfig, FxserverResourceInfo, FxserverStatus,
            FxserverTerminalEntry, ResourceScanRequest,
        },
        mariadb::MariaDBCredentials,
    },
    services::mariadb,
};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame,
};
use std::{
    collections::VecDeque,
    io::{self, IsTerminal},
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};
use tokio::runtime::Runtime;

mod render;
use render::{ConfirmationPreview, Input};

const TABS: [&str; 7] = [
    "Server",
    "Console",
    "Resources",
    "Database",
    "Artifacts",
    "Checks",
    "Settings",
];
const FIELDS: [&str; 11] = [
    "Artifact folder",
    "txData folder",
    "Profile",
    "RCON host",
    "RCON port",
    "Database host",
    "Database port",
    "Database user",
    "Database",
    "RCON password",
    "Database password",
];
const MAX_LINES: usize = 2000;

enum Job {
    Start(Settings, bool),
    Stop,
    Restart(Settings),
    Rcon(FxserverCommandRequest),
    Resources(Settings),
    Catalog,
    Install(ArtifactBuild, String),
    Query(MariaDBCredentials, String),
    Databases(MariaDBCredentials),
    Check(Settings),
    ReadSql(PathBuf),
    Quit,
}

enum Update {
    Complete(Result<String, String>),
    Rcon(Result<String, String>),
    Resources(Result<Vec<FxserverResourceInfo>, String>),
    Catalog(Result<Vec<ArtifactBuild>, String>),
    Sample(Option<FxserverStatus>, Vec<FxserverTerminalEntry>),
    SqlFile(Result<String, String>),
    Stopped(Result<(), String>),
}

enum Edit {
    Console,
    Query,
    Setting(usize),
    SqlFile,
}

struct App {
    settings: Settings,
    config_path: PathBuf,
    rcon_password: String,
    database_password: String,
    tab: usize,
    selected: usize,
    input: Input,
    edit: Option<Edit>,
    confirm: Option<(ConfirmationPreview, Job)>,
    busy: bool,
    quitting: bool,
    notice: String,
    error: bool,
    logs: VecDeque<String>,
    output: Vec<String>,
    resources: Vec<FxserverResourceInfo>,
    builds: Vec<ArtifactBuild>,
    status: Option<FxserverStatus>,
    scroll: usize,
    started: Instant,
}

impl App {
    fn new(settings: Settings, config_path: PathBuf) -> Self {
        Self {
            settings,
            config_path,
            rcon_password: String::new(),
            database_password: String::new(),
            tab: 0,
            selected: 0,
            input: Input::default(),
            edit: None,
            confirm: None,
            busy: false,
            quitting: false,
            notice: "Ready".into(),
            error: false,
            logs: VecDeque::new(),
            output: Vec::new(),
            resources: Vec::new(),
            builds: Vec::new(),
            status: None,
            scroll: 0,
            started: Instant::now(),
        }
    }
    fn paste(&mut self, value: &str) {
        if self.edit.is_none() || self.confirm.is_some() || self.quitting {
            return;
        }
        if let Err(error) = self.input.paste(value) {
            self.notice = error.into();
            self.error = true;
        }
    }

    fn credentials(&self) -> MariaDBCredentials {
        MariaDBCredentials {
            host: self.settings.database_host.clone(),
            port: self.settings.database_port,
            username: self.settings.database_user.clone(),
            password: self.database_password.clone(),
            database: (!self.settings.database.is_empty()).then(|| self.settings.database.clone()),
        }
    }
    fn rcon(&self, command: String) -> FxserverCommandRequest {
        FxserverCommandRequest {
            command,
            rcon: FxserverRconConfig {
                host: self.settings.rcon_host.clone(),
                port: self.settings.rcon_port,
                password: self.rcon_password.clone(),
            },
        }
    }
    fn submit(&mut self, sender: &SyncSender<Job>, job: Job) {
        if self.busy {
            self.notice = "An operation is already running. You can continue browsing.".into();
            return;
        }
        match sender.try_send(job) {
            Ok(()) => {
                self.busy = true;
                self.error = false;
                self.notice = "Working".into();
                self.started = Instant::now();
            }
            Err(_) => {
                self.error = true;
                self.notice = "The worker is busy or unavailable.".into();
            }
        }
    }
    fn require_confirmation(&mut self, title: String, job: Job) {
        if self.busy {
            self.notice = "Wait for the current operation before starting another.".into();
            return;
        }
        self.confirm = Some((ConfirmationPreview::new(title), job));
    }
    fn set_result(&mut self, result: Result<String, String>) {
        self.busy = false;
        self.error = result.is_err();
        match result {
            Ok(text) => {
                self.notice = "Complete".into();
                self.output = safe_text(&text)
                    .lines()
                    .take(MAX_LINES)
                    .map(str::to_owned)
                    .collect();
                self.scroll = 0;
            }
            Err(error) => {
                self.notice = safe_text(&error);
            }
        }
    }
    fn update(&mut self, update: Update) -> Option<Result<(), String>> {
        match update {
            Update::Complete(result) => self.set_result(result),
            Update::Rcon(result) => {
                self.busy = false;
                self.error = result.is_err();
                let (label, text) = match result {
                    Ok(text) => ("RCON", text),
                    Err(error) => ("RCON error", error),
                };
                let text = safe_text(&text);
                self.notice = if self.error {
                    "RCON failed. See Console."
                } else {
                    "RCON complete. See Console."
                }
                .into();
                if text.trim().is_empty() {
                    self.logs.push_back(format!("[{label}] No response text."));
                } else {
                    for line in text.lines() {
                        self.logs.push_back(format!("[{label}] {line}"));
                        while self.logs.len() > MAX_LINES {
                            self.logs.pop_front();
                        }
                    }
                }
                while self.logs.len() > MAX_LINES {
                    self.logs.pop_front();
                }
                if self.tab == 1 {
                    self.scroll = 0;
                }
            }
            Update::Resources(result) => match result {
                Ok(resources) => {
                    self.resources = resources;
                    self.selected = 0;
                    self.set_result(Ok(format!("{} resources", self.resources.len())));
                }
                Err(error) => self.set_result(Err(error)),
            },
            Update::Catalog(result) => match result {
                Ok(builds) => {
                    self.builds = builds;
                    self.selected = 0;
                    self.set_result(Ok(format!("{} available builds", self.builds.len())));
                }
                Err(error) => self.set_result(Err(error)),
            },
            Update::Sample(status, entries) => {
                if status.is_some() {
                    self.status = status;
                }
                for entry in entries {
                    self.logs.push_back(safe_text(&entry.plain_line));
                }
                while self.logs.len() > MAX_LINES {
                    self.logs.pop_front();
                }
            }
            Update::SqlFile(result) => {
                self.busy = false;
                match result {
                    Ok(sql) => self.require_confirmation(
                        format!(
                            "Execute this SQL file against {} / {}? Back up before changes.\n\n{}",
                            self.settings.database_host,
                            if self.settings.database.is_empty() {
                                "global scope"
                            } else {
                                &self.settings.database
                            },
                            sql
                        ),
                        Job::Query(self.credentials(), sql),
                    ),
                    Err(error) => self.set_result(Err(error)),
                }
            }
            Update::Stopped(result) => return Some(result),
        }
        None
    }
    fn field(&self, index: usize) -> String {
        match index {
            0 => self.settings.artifact_path.clone(),
            1 => self.settings.tx_data_path.clone(),
            2 => self.settings.profile.clone(),
            3 => self.settings.rcon_host.clone(),
            4 => self.settings.rcon_port.to_string(),
            5 => self.settings.database_host.clone(),
            6 => self.settings.database_port.to_string(),
            7 => self.settings.database_user.clone(),
            8 => self.settings.database.clone(),
            9 => self.rcon_password.clone(),
            10 => self.database_password.clone(),
            _ => String::new(),
        }
    }
    fn set_field(&mut self, index: usize, value: String) -> Result<(), String> {
        match index {
            0 => self.settings.artifact_path = value,
            1 => self.settings.tx_data_path = value,
            2 => self.settings.profile = value,
            3 => self.settings.rcon_host = value,
            4 => self.settings.rcon_port = parse_port(&value)?,
            5 => self.settings.database_host = value,
            6 => self.settings.database_port = parse_port(&value)?,
            7 => self.settings.database_user = value,
            8 => self.settings.database = value,
            9 => self.rcon_password = value,
            10 => self.database_password = value,
            _ => {}
        }
        self.notice = if index >= 9 {
            "Credential updated for this session only."
        } else {
            "Setting updated. Press S in Settings to save paths and connection details."
        }
        .into();
        Ok(())
    }
    fn key(&mut self, key: KeyEvent, sender: &SyncSender<Job>) {
        if self.quitting {
            return;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.request_quit(sender);
            return;
        }
        if self.confirm.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some((_, job)) = self.confirm.take() {
                        self.submit(sender, job);
                    }
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => self.confirm = None,
                _ => {
                    if let Some((preview, _)) = &mut self.confirm {
                        preview.key(key.code);
                    }
                }
            }
            return;
        }
        if self.edit.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.edit = None;
                    self.input = Input::default();
                }
                KeyCode::Enter => {
                    let value = std::mem::take(&mut self.input.value);
                    self.input.cursor = 0;
                    match self.edit.take() {
                        Some(Edit::Console) if !value.trim().is_empty() => {
                            self.submit(sender, Job::Rcon(self.rcon(value)))
                        }
                        Some(Edit::Query) if !value.trim().is_empty() => self.require_confirmation(
                            format!(
                                "Execute this SQL against {} / {}?\n\n{}",
                                self.settings.database_host,
                                if self.settings.database.is_empty() {
                                    "global scope"
                                } else {
                                    &self.settings.database
                                },
                                value
                            ),
                            Job::Query(self.credentials(), value),
                        ),
                        Some(Edit::Setting(index)) => {
                            if let Err(error) = self.set_field(index, value) {
                                self.error = true;
                                self.notice = error;
                            }
                        }
                        Some(Edit::SqlFile) => {
                            self.submit(sender, Job::ReadSql(PathBuf::from(value)))
                        }
                        _ => {}
                    }
                }
                _ => self.input.key(key),
            }
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.request_quit(sender),
            KeyCode::Tab => {
                self.tab = (self.tab + 1) % TABS.len();
                self.selected = 0;
                self.scroll = 0;
            }
            KeyCode::BackTab => {
                self.tab = (self.tab + TABS.len() - 1) % TABS.len();
                self.selected = 0;
                self.scroll = 0;
            }
            KeyCode::F(n) if (1..=7).contains(&n) => {
                self.tab = n as usize - 1;
                self.selected = 0;
                self.scroll = 0;
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.row_count().saturating_sub(1))
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::PageUp => self.scroll = (self.scroll + 10).min(MAX_LINES),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::End => self.scroll = 0,
            KeyCode::Char('s') if self.tab == 0 => {
                self.submit(sender, Job::Start(self.settings.clone(), false))
            }
            KeyCode::Char('f') if self.tab == 0 => self.require_confirmation(
                "Force start despite preflight errors? Review Checks first.".into(),
                Job::Start(self.settings.clone(), true),
            ),
            KeyCode::Char('x') if self.tab == 0 => self.require_confirmation(
                "Stop the managed server? Connected players will disconnect.".into(),
                Job::Stop,
            ),
            KeyCode::Char('r') if self.tab == 0 => self.require_confirmation(
                "Restart the managed server? Connected players will disconnect.".into(),
                Job::Restart(self.settings.clone()),
            ),
            KeyCode::Char('r') if self.tab == 2 => {
                self.submit(sender, Job::Resources(self.settings.clone()))
            }
            KeyCode::Char('r') if self.tab == 4 => self.submit(sender, Job::Catalog),
            KeyCode::Char('r') if self.tab == 5 => {
                self.submit(sender, Job::Check(self.settings.clone()))
            }
            KeyCode::Char('d') if self.tab == 3 => {
                self.submit(sender, Job::Databases(self.credentials()))
            }
            KeyCode::Char('f') if self.tab == 3 => {
                self.edit = Some(Edit::SqlFile);
                self.input = Input::default();
            }
            KeyCode::Char('s') | KeyCode::Char('S') if self.tab == 6 => {
                match settings::save(&self.config_path, &self.settings) {
                    Ok(()) => self.notice = "Settings saved. Passwords remain session-only.".into(),
                    Err(error) => {
                        self.error = true;
                        self.notice = error;
                    }
                }
            }
            KeyCode::Enter if self.tab == 1 => {
                self.edit = Some(Edit::Console);
                self.input = Input::default();
            }
            KeyCode::Enter if self.tab == 3 => {
                self.edit = Some(Edit::Query);
                self.input = Input::default();
            }
            KeyCode::Enter if self.tab == 6 => {
                self.input = Input::new(self.field(self.selected));
                self.edit = Some(Edit::Setting(self.selected));
            }
            KeyCode::Enter if self.tab == 4 => {
                if let Some(build) = self.builds.get(self.selected).cloned() {
                    self.require_confirmation(format!("Install artifact {} ({}) into {}? Back up first. This acknowledges any health warnings:\n{}", build.version, build.health, self.settings.artifact_path, build.issues.iter().map(|i| i.reason.as_str()).collect::<Vec<_>>().join("\n")), Job::Install(build, self.settings.artifact_path.clone()));
                }
            }
            KeyCode::Char(key @ ('s' | 'x' | 'e' | 't')) if self.tab == 2 => {
                if let Some(resource) = self.resources.get(self.selected) {
                    let action = match key {
                        's' => "start",
                        'x' => "stop",
                        'e' => "ensure",
                        _ => "restart",
                    };
                    if !resource
                        .name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                    {
                        self.notice = "Resource name contains unsupported RCON characters.".into();
                        return;
                    }
                    self.require_confirmation(
                        format!("{action} resource {}?", resource.name),
                        Job::Rcon(self.rcon(format!("{action} {}", resource.name))),
                    );
                }
            }
            _ => {}
        }
    }
    fn row_count(&self) -> usize {
        match self.tab {
            2 => self.resources.len(),
            4 => self.builds.len(),
            6 => FIELDS.len(),
            _ => 1,
        }
    }
    fn request_quit(&mut self, sender: &SyncSender<Job>) {
        if self.busy {
            self.notice = "Wait for the current operation to finish before quitting safely.".into();
            return;
        }
        self.submit(sender, Job::Quit);
        self.quitting = true;
        self.notice = "Stopping the managed server and closing".into();
    }
}

fn parse_port(value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| "Port must be between 1 and 65535.".into())
}

fn worker(runtime: Runtime, jobs: Receiver<Job>, updates: SyncSender<Update>) {
    let manager = FxserverManager::default();
    let mut cursor = None;
    let mut last_status = Instant::now() - Duration::from_secs(2);
    loop {
        match jobs.recv_timeout(Duration::from_millis(100)) {
            Ok(Job::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                manager.begin_shutdown();
                let _ = updates.send(Update::Stopped(manager.stop_running_process()));
                return;
            }
            Ok(job) => {
                let event = match job {
                    Job::Start(settings, force) => Update::Complete(runtime.block_on(async {
                        let report = preflight(&settings, true).await?;
                        if report.blocking && !force { return Err(format!("Preflight blocked start: {} errors. Run Checks to review, or explicitly force start.", report.error_count)); }
                        manager.start(settings.launch()?).map(|result| format!("Started process {}", result.pid))
                    })),
                    Job::Stop => Update::Complete(manager.stop_running_process().map(|_| "Server stopped.".into())),
                    Job::Restart(settings) => Update::Complete(settings.launch().and_then(|request| manager.restart(request)).map(|result| format!("Restarted process {}", result.pid))),
                    Job::Rcon(request) => {
                        let password = request.rcon.password.clone();
                        let result = runtime.block_on(fxserver::send_fxserver_rcon_command(request));
                        Update::Rcon(redact_rcon_password(result, &password))
                    },
                    Job::Resources(settings) => Update::Resources(runtime.block_on(fxserver::scan_fxserver_resources(ResourceScanRequest { tx_data_path: settings.tx_data_path, profile: settings.profile })).map(|r| r.resources)),
                    Job::Catalog => Update::Catalog(runtime.block_on(artifact::get_windows_artifact_catalog(true)).map(|r| r.builds)),
                    Job::Install(build, destination) => Update::Complete(artifact::install_legacy_for_manager(&manager, crate::models::artifact::ArtifactInstallRequest { version: build.version, url: build.download_url, destination, acknowledge_risk: true }).map(|r| format!("Installed {} in {}", r.version, r.destination))),
                    Job::Query(credentials, sql) => Update::Complete((|| {
                        let _lease = mariadb::access::database_access()?;
                        let result = mariadb::query::execute_query(credentials, sql)?;
                        if result.success {
                            Ok(format!("{}\n{}", result.stdout, result.stderr))
                        } else {
                            Err(result.stderr)
                        }
                    })()),
                    Job::Databases(credentials) => Update::Complete((|| {
                        let _lease = mariadb::access::database_access()?;
                        mariadb::query::list_databases(credentials).map(|names| names.join("\n"))
                    })()),
                    Job::Check(settings) => Update::Complete(runtime.block_on(preflight(&settings, true)).map(|r| r.checks.iter().map(|c| format!("{:?} | {}\n{}", c.severity, c.title, c.detail)).collect::<Vec<_>>().join("\n\n"))),
                    Job::ReadSql(path) => Update::SqlFile(super::read_sql(&path)),
                    Job::Quit => unreachable!(),
                };
                if updates.send(event).is_err() {
                    manager.begin_shutdown();
                    let _ = manager.stop_running_process();
                    return;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let entries = manager
            .terminal(Some(500), cursor)
            .map(|r| r.entries)
            .unwrap_or_default();
        let status = if last_status.elapsed() >= Duration::from_secs(1) {
            last_status = Instant::now();
            manager.status().ok()
        } else {
            None
        };
        let next = entries.last().map(|e| e.id).or(cursor);
        if (status.is_some() || !entries.is_empty())
            && updates.try_send(Update::Sample(status, entries)).is_ok()
        {
            cursor = next;
        }
    }
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

fn redact_rcon_password(result: Result<String, String>, password: &str) -> Result<String, String> {
    let redact = |text: String| {
        if password.is_empty() {
            text
        } else {
            text.replace(password, "[redacted]")
        }
    };
    result.map(redact).map_err(redact)
}

fn finished_worker_result(app: &mut App, receive: &Receiver<Update>) -> Result<(), String> {
    // Called only after is_finished(): all final sends have completed. A missing
    // Stopped event remains a failure, including during an explicit quit.
    for update in receive.try_iter() {
        if let Some(result) = app.update(update) {
            return result;
        }
    }
    Err("The terminal worker stopped unexpectedly.".into())
}

pub(super) fn run(config: Settings, path: PathBuf, runtime: Runtime) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "The TUI requires an interactive terminal. Use --help for non-interactive commands."
                .into(),
        );
    }
    if std::env::var("TERM").as_deref() == Ok("dumb") {
        return Err("Use a terminal with cursor support, or choose a CLI subcommand.".into());
    }
    let mut terminal = ratatui::try_init().map_err(|e| e.to_string())?;
    let guard = TerminalGuard;
    crossterm::execute!(io::stdout(), EnableBracketedPaste).map_err(|e| e.to_string())?;
    let (send, jobs) = mpsc::sync_channel(1);
    let (updates, receive) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || worker(runtime, jobs, updates));
    let mut app = App::new(config, path);
    let mut dirty = true;
    let mut last_draw = Instant::now();
    let result = (|| loop {
        for update in receive.try_iter().take(32) {
            dirty = true;
            if let Some(result) = app.update(update) {
                return result;
            }
        }
        if dirty || (app.busy && last_draw.elapsed() >= Duration::from_millis(200)) {
            terminal
                .draw(|frame| draw(frame, &mut app))
                .map_err(|e| e.to_string())?;
            dirty = false;
            last_draw = Instant::now();
        }
        if event::poll(Duration::from_millis(50)).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.key(key, &send);
                    dirty = true;
                }
                Event::Paste(value) => {
                    app.paste(&value);
                    dirty = true;
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
        if worker.is_finished() {
            return finished_worker_result(&mut app, &receive);
        }
    })();
    drop(send);
    drop(receive);
    drop(guard);
    worker
        .join()
        .map_err(|_| "The terminal worker failed during shutdown.")?;
    result
}

fn tab_titles(width: u16, selected: usize) -> Vec<&'static str> {
    const SHORT: [&str; 7] = ["Srv", "Log", "Res", "DB", "Build", "Check", "Set"];
    let full_width = TABS.iter().map(|title| title.len()).sum::<usize>() + 3 * (TABS.len() - 1);
    TABS.iter()
        .enumerate()
        .map(|(index, title)| {
            if width as usize >= full_width || index == selected {
                *title
            } else {
                SHORT[index]
            }
        })
        .collect()
}

fn footer_lines(tab: usize, width: u16) -> Vec<Line<'static>> {
    let actions = match tab {
        1 => "Enter: command  PgUp/PgDn: scroll",
        2 => "R: refresh S: start X: stop T: restart E: ensure",
        3 => "Enter: SQL  D: databases  F: SQL file",
        4 => "R: refresh  Enter: install",
        5 => "R: run preflight",
        6 => "Enter: edit  S: save",
        _ => "S: start  X: stop  R: restart",
    };
    const GLOBAL: &str = "Tab/F1-F7: view  Q: quit";
    let combined = format!("{GLOBAL}  {actions}");
    if combined.len() <= width as usize {
        vec![Line::raw(combined)]
    } else {
        vec![Line::raw(GLOBAL), Line::raw(actions)]
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 50 || area.height < 14 {
        frame.render_widget(
            Paragraph::new(
                "FXServer Installer\nIncrease the terminal to at least 50 columns and 14 rows.",
            ),
            area,
        );
        return;
    }
    let footer = footer_lines(app.tab, area.width);
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(3),
        Constraint::Length(footer.len() as u16),
    ])
    .split(area);
    let running = app.status.as_ref().is_some_and(|s| s.running);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" FXServer Installer ", Style::new().bold()),
            Span::styled(
                if running { " RUNNING " } else { " STOPPED " },
                if running {
                    Style::new().fg(Color::Green)
                } else {
                    Style::new().fg(Color::Red)
                },
            ),
            Span::raw(format!("  {}", safe_text(&app.settings.profile))),
        ])),
        layout[0],
    );
    frame.render_widget(
        Tabs::new(tab_titles(area.width, app.tab))
            .select(app.tab)
            .highlight_style(Style::new().fg(Color::Cyan).bold())
            .padding("", "")
            .divider(" | "),
        layout[1],
    );
    let block = Block::default().borders(Borders::ALL).title(TABS[app.tab]);
    let body = block.inner(layout[2]);
    frame.render_widget(block, layout[2]);
    match app.tab {
        0 => {
            let mut lines = vec![
                Line::from(format!(
                    "Artifact: {}",
                    safe_text(&app.settings.artifact_path)
                )),
                Line::from(format!(
                    "txData:   {}",
                    safe_text(&app.settings.tx_data_path)
                )),
                Line::from(""),
            ];
            if let Some(status) = &app.status {
                if let Some(pid) = status.pid {
                    lines.push(Line::from(format!("PID {pid}")));
                }
                if let Some(resources) = &status.resources {
                    lines.push(Line::from(format!(
                        "CPU {:.1}%    RAM {:.0} MiB    Threads {}    Handles/FDs {}",
                        resources.cpu_percent,
                        resources.memory_bytes as f64 / 1048576.,
                        resources.thread_count,
                        resources.handle_count
                    )));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Start [S]    Stop [X]    Restart [R]    Force start [F]",
            ));
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
        }
        1 => render_output(frame, body, app.logs.iter(), app.scroll),
        2 => {
            let items: Vec<_> = app
                .resources
                .iter()
                .map(|r| {
                    ListItem::new(format!(
                        "{}  {}",
                        safe_text(&r.name),
                        r.version.as_deref().unwrap_or("no version")
                    ))
                })
                .collect();
            render_list(
                frame,
                body,
                items,
                app.selected,
                "No resources loaded. Refresh [R]",
            );
        }
        4 => {
            let items: Vec<_> = app
                .builds
                .iter()
                .map(|b| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<8}", b.version), Style::new().bold()),
                        Span::styled(
                            safe_text(&b.health),
                            if !b.issues.is_empty() {
                                Style::new().fg(Color::Red)
                            } else if b.recommended {
                                Style::new().fg(Color::Green)
                            } else {
                                Style::new().fg(Color::Yellow)
                            },
                        ),
                    ]))
                })
                .collect();
            render_list(
                frame,
                body,
                items,
                app.selected,
                "No artifacts loaded. Refresh [R]",
            );
        }
        6 => {
            let items = FIELDS
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    let value = if index >= 9 {
                        if app.field(index).is_empty() {
                            "not set".into()
                        } else {
                            "********".into()
                        }
                    } else {
                        safe_text(&app.field(index))
                    };
                    ListItem::new(format!("{name:<20} {value}"))
                })
                .collect();
            render_list(frame, body, items, app.selected, "");
        }
        _ => render_output(frame, body, app.output.iter(), app.scroll),
    }
    if let Some(edit) = &app.edit {
        let label = match edit {
            Edit::Console => "RCON command",
            Edit::Query => "SQL (review before execution)",
            Edit::Setting(i) => FIELDS[*i],
            Edit::SqlFile => "SQL file path",
        };
        let mut input = Block::default()
            .borders(Borders::ALL)
            .title(label)
            .border_style(Style::new().fg(Color::Cyan));
        if app.error {
            input = input
                .border_style(Style::new().fg(Color::Red))
                .title_bottom(app.notice.as_str());
        }
        let inner = input.inner(layout[3]);
        frame.render_widget(input, layout[3]);
        let (value, cursor) = app
            .input
            .visible(inner.width as usize, matches!(edit, Edit::Setting(9 | 10)));
        frame.render_widget(Paragraph::new(value), inner);
        frame.set_cursor_position((inner.x + cursor, inner.y));
    } else {
        let spinner = if app.busy {
            ["|", "/", "-", "\\"][(app.started.elapsed().as_millis() / 200) as usize % 4]
        } else {
            ""
        };
        frame.render_widget(
            Paragraph::new(format!("{spinner} {}", app.notice))
                .style(if app.error {
                    Style::new().fg(Color::Red)
                } else {
                    Style::new().fg(Color::Gray)
                })
                .wrap(Wrap { trim: false }),
            layout[3],
        );
    }
    frame.render_widget(Paragraph::new(footer).fg(Color::DarkGray), layout[4]);
    if let Some((preview, _)) = &mut app.confirm {
        let width = area.width.saturating_sub(6);
        let height = area.height.saturating_sub(6);
        let modal = Rect::new(3, 3, width, height);
        preview.draw(frame, modal);
    }
}

fn render_list(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem<'_>>,
    selected: usize,
    empty: &str,
) {
    if items.is_empty() {
        frame.render_widget(Paragraph::new(empty), area);
        return;
    }
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(Style::new().fg(Color::Cyan).bold()),
        area,
        &mut state,
    );
}

fn render_output<'a>(
    frame: &mut Frame,
    area: Rect,
    lines: impl ExactSizeIterator<Item = &'a String>,
    scroll: usize,
) {
    let end = lines
        .len()
        .saturating_sub(scroll.min(lines.len().saturating_sub(1)));
    let start = end.saturating_sub(area.height as usize);
    let text = lines
        .skip(start)
        .take(end - start)
        .map(|line| line.chars().take(area.width as usize).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(
        Paragraph::new(if text.is_empty() {
            "No output yet.".into()
        } else {
            text
        }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn paste_never_submits_commands_or_accepts_a_confirmation() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.paste("sxq");
        assert!(app.input.value.is_empty());
        assert!(!app.quitting);
        app.edit = Some(Edit::Query);
        app.paste("SELECT 'hello';");
        assert_eq!(app.input.value, "SELECT 'hello';");
        app.paste("\ny\n");
        assert_eq!(app.input.value, "SELECT 'hello';");
        assert!(app.confirm.is_none());
        assert!(!app.busy);
        app.edit = None;
        app.require_confirmation("Stop?".into(), Job::Stop);
        app.paste("y");
        assert!(app.confirm.is_some());
        assert!(!app.busy);
    }
    #[test]
    fn text_editing_uses_characters_and_visible_cell_width() {
        let mut input = Input::new("a界b".into());
        input.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        input.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(input.value, "ab");
        assert_eq!(input.cursor, 1);
        input.key(KeyEvent::new(KeyCode::Char('界'), KeyModifiers::NONE));
        assert_eq!(input.value, "a界b");
        assert!(input.visible(3, false).1 < 3);
        assert!(!input.visible(20, true).0.contains('界'));
    }
    #[test]
    fn all_views_render_at_supported_sizes_without_exposing_passwords() {
        for (width, height) in [(50, 14), (80, 24), (140, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(Settings::default(), PathBuf::from("unused.json"));
            app.rcon_password = "rcon-secret-fixture".into();
            app.database_password = "db-secret-fixture".into();
            for (tab, title) in TABS.iter().enumerate() {
                app.tab = tab;
                terminal.draw(|f| draw(f, &mut app)).unwrap();
                let content: String = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                assert!(!content.contains("secret-fixture"));
                assert!(content.contains(title));
            }
        }
    }
    #[test]
    fn busy_operations_do_not_block_navigation_or_queue_duplicates() {
        let (send, jobs) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.submit(&send, Job::Catalog);
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &send);
        assert_eq!(app.tab, 1);
        assert!(app.busy);
        app.submit(&send, Job::Catalog);
        assert!(jobs.try_recv().is_ok());
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn selected_tabs_and_footer_actions_fit_supported_terminal_sizes() {
        for (width, height) in [(50, 14), (80, 24), (140, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(Settings::default(), PathBuf::new());
            for tab in 0..TABS.len() {
                app.tab = tab;
                let titles = tab_titles(width, tab);
                assert_eq!(titles[tab], TABS[tab]);
                assert!(
                    titles.iter().map(|title| title.len()).sum::<usize>() + 3 * (titles.len() - 1)
                        <= width as usize
                );
                let footer = footer_lines(tab, width);
                assert!(footer.iter().all(|line| line.width() <= width as usize));
                terminal.draw(|frame| draw(frame, &mut app)).unwrap();
                let buffer = terminal.backend().buffer();
                let tab_row: String = (0..width).map(|x| buffer[(x, 1)].symbol()).collect();
                assert!(tab_row.contains(TABS[tab]));
                for (index, line) in footer.iter().enumerate() {
                    let row = height - footer.len() as u16 + index as u16;
                    let actual: String = (0..width).map(|x| buffer[(x, row)].symbol()).collect();
                    assert!(
                        actual.contains(&line.to_string()),
                        "Missing footer at {width}x{height}: {line}"
                    );
                }
            }
        }
    }

    #[test]
    fn finished_worker_drains_final_stopped_after_the_initial_empty_poll() {
        let (send, receive) = mpsc::sync_channel(16);
        let (release, ready) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            ready.recv().unwrap();
            send.send(Update::Complete(Ok("final update".into())))
                .unwrap();
            send.send(Update::Stopped(Ok(()))).unwrap();
        });
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.quitting = true;
        assert!(receive.try_iter().next().is_none());
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(finished_worker_result(&mut app, &receive), Ok(()));
        assert_eq!(app.output, ["final update"]);
    }

    #[test]
    fn missing_stopped_and_failed_shutdown_remain_errors_while_quitting() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.quitting = true;
        let (send, receive) = mpsc::sync_channel(1);
        drop(send);
        assert!(finished_worker_result(&mut app, &receive)
            .unwrap_err()
            .contains("unexpectedly"));
        let (send, receive) = mpsc::sync_channel(1);
        send.send(Update::Stopped(Err("stop failed".into())))
            .unwrap();
        drop(send);
        assert_eq!(
            finished_worker_result(&mut app, &receive),
            Err("stop failed".into())
        );
    }

    #[test]
    fn sql_confirmation_navigation_never_changes_or_sends_the_job_before_approval() {
        let (send, receive) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        let sql = format!("{}SELECT '\t\\literal';\r\n", "SELECT 1;\n".repeat(100_000));
        app.update(Update::SqlFile(Ok(sql.clone())));
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        for key in [
            KeyCode::End,
            KeyCode::Home,
            KeyCode::PageDown,
            KeyCode::Down,
            KeyCode::PageUp,
        ] {
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            app.key(KeyEvent::new(key, KeyModifiers::NONE), &send);
            assert!(receive.try_recv().is_err());
        }
        app.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), &send);
        let Job::Query(_, submitted) = receive.try_recv().unwrap() else {
            panic!("Expected SQL job")
        };
        assert_eq!(submitted, sql);
        assert!(app.confirm.is_none());
    }

    #[test]
    fn rcon_responses_are_visible_bounded_and_do_not_echo_sent_commands() {
        let (send, receive) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 1;
        app.edit = Some(Edit::Console);
        app.rcon_password = "session-secret".into();
        app.input = Input::new("setr private_password command-secret".into());
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        let Job::Rcon(request) = receive.try_recv().unwrap() else {
            panic!("Expected RCON job")
        };
        assert!(request.command.contains("command-secret"));
        assert!(
            app.logs.is_empty(),
            "Do not echo submitted commands into console history"
        );
        app.output = vec!["previous SQL result".into()];
        app.scroll = 10;
        app.update(Update::Rcon(redact_rcon_password(
            Ok("online: 3\nreply session-secret\x1b".into()),
            &request.rcon.password,
        )));
        assert!(!app.busy && !app.error);
        assert_eq!(app.scroll, 0);
        assert_eq!(app.output, ["previous SQL result"]);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("[RCON] online: 3"));
        assert!(!content.contains("session-secret") && !content.contains("command-secret"));
        assert!(!app.logs.iter().any(|line| line.contains('\x1b')));
        app.update(Update::Rcon(Ok("line\n".repeat(MAX_LINES + 10))));
        assert_eq!(app.logs.len(), MAX_LINES);
        app.update(Update::Rcon(redact_rcon_password(
            Err("denied session-secret".into()),
            &request.rcon.password,
        )));
        assert!(app.error);
        assert_eq!(app.logs.back().unwrap(), "[RCON error] denied [redacted]");
        assert_eq!(app.logs.len(), MAX_LINES);
    }
}
