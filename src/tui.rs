use std::{
    collections::BTreeMap,
    io::{self, IsTerminal},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    AGENT_AUTH_BUNDLE, Config, identity_text, is_reserved_agent_auth_key, read_cache,
    remote_delete_bundle, remote_list_bundles, remote_read_bundle, remote_read_bundle_for_display,
    remote_write_bundle, sync_bundle, validate_bundle_key,
};

const MASK_GLYPHS: &str = "••••••••";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Bundles,
    Detail,
    Editor(EditorState),
    Confirm(ConfirmState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    AddKey,
    EditValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorStage {
    Key,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorState {
    pub mode: EditorMode,
    pub stage: EditorStage,
    pub key_buffer: String,
    pub value_buffer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmState {
    pub prompt: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    DeleteKey { bundle: String, key: String },
    DeleteBundle { bundle: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    RefreshBundles,
    OpenBundle(String),
    SyncBundle(String),
    DeleteBundle(String),
    SetKey {
        bundle: String,
        key: String,
        value: String,
    },
    UnsetKey {
        bundle: String,
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub bundles: Vec<String>,
    pub bundle_index: usize,
    pub current_bundle: Option<String>,
    pub detail: BTreeMap<String, String>,
    pub detail_keys: Vec<String>,
    pub detail_index: usize,
    pub masked: bool,
    pub view: View,
    pub status: Option<StatusLine>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            bundles: Vec::new(),
            bundle_index: 0,
            current_bundle: None,
            detail: BTreeMap::new(),
            detail_keys: Vec::new(),
            detail_index: 0,
            masked: true,
            view: View::Bundles,
            status: None,
        }
    }

    pub fn set_bundles(&mut self, bundles: Vec<String>) {
        self.bundles = bundles;
        if self.bundles.is_empty() {
            self.bundle_index = 0;
        } else if self.bundle_index >= self.bundles.len() {
            self.bundle_index = self.bundles.len() - 1;
        }
    }

    pub fn set_detail(&mut self, bundle: String, env: BTreeMap<String, String>) {
        self.current_bundle = Some(bundle);
        self.detail_keys = env.keys().cloned().collect();
        self.detail = env;
        if self.detail_keys.is_empty() {
            self.detail_index = 0;
        } else if self.detail_index >= self.detail_keys.len() {
            self.detail_index = self.detail_keys.len() - 1;
        }
    }

    pub fn selected_bundle(&self) -> Option<&str> {
        self.bundles.get(self.bundle_index).map(String::as_str)
    }

    pub fn selected_key(&self) -> Option<&str> {
        self.detail_keys.get(self.detail_index).map(String::as_str)
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.status = Some(StatusLine {
            text: msg.into(),
            is_error: false,
        });
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.status = Some(StatusLine {
            text: msg.into(),
            is_error: true,
        });
    }
}

pub fn run(cfg: &Config, allow_ssh_keychain: bool) -> Result<()> {
    let _ = identity_text(cfg, allow_ssh_keychain)
        .context("validate age identity before opening the TUI")?;

    if !io::stdout().is_terminal() {
        bail!("`rage tui` requires a terminal; stdout is not a TTY");
    }

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("init terminal")?;

    let result = event_loop(&mut terminal, cfg, allow_ssh_keychain);

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    cfg: &Config,
    allow_ssh_keychain: bool,
) -> Result<()> {
    let mut state = AppState::new();
    match remote_list_bundles(cfg, allow_ssh_keychain) {
        Ok(bundles) => state.set_bundles(bundles),
        Err(err) => state.error(format!("list bundles: {err}")),
    }

    loop {
        terminal.draw(|f| draw(&state, f))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let action = handle_key(&mut state, key);
        if matches!(action, Action::Quit) {
            return Ok(());
        }
        dispatch(&mut state, action, cfg, allow_ssh_keychain);
    }
}

fn dispatch(state: &mut AppState, action: Action, cfg: &Config, allow_ssh_keychain: bool) {
    match action {
        Action::None | Action::Quit => {}
        Action::RefreshBundles => match remote_list_bundles(cfg, allow_ssh_keychain) {
            Ok(bundles) => {
                state.set_bundles(bundles);
                state.info("refreshed bundle list");
            }
            Err(err) => state.error(format!("list bundles: {err}")),
        },
        Action::OpenBundle(bundle) if bundle == AGENT_AUTH_BUNDLE => {
            match remote_read_bundle_for_display(cfg, &bundle, allow_ssh_keychain) {
                Ok(env) => {
                    state.set_detail(bundle.clone(), env.unwrap_or_default());
                    state.view = View::Detail;
                    state.info(format!("opened {bundle}"));
                }
                Err(err) => state.error(format!("open {bundle}: {err}")),
            }
        }
        Action::OpenBundle(bundle) => match read_cache(cfg, &bundle, allow_ssh_keychain) {
            Ok(env) => {
                state.set_detail(bundle.clone(), env);
                state.view = View::Detail;
                state.info(format!("opened {bundle}"));
            }
            Err(_) => match sync_bundle(cfg, &bundle, allow_ssh_keychain)
                .and_then(|()| read_cache(cfg, &bundle, allow_ssh_keychain))
            {
                Ok(env) => {
                    state.set_detail(bundle.clone(), env);
                    state.view = View::Detail;
                    state.info(format!("synced and opened {bundle}"));
                }
                Err(err) => state.error(format!("open {bundle}: {err}")),
            },
        },
        Action::SyncBundle(bundle) => match sync_bundle(cfg, &bundle, allow_ssh_keychain) {
            Ok(()) => match read_bundle_detail(cfg, &bundle, allow_ssh_keychain) {
                Ok(env) => {
                    if state.current_bundle.as_deref() == Some(bundle.as_str()) {
                        state.set_detail(bundle.clone(), env);
                    }
                    state.info(format!("synced {bundle}"));
                }
                Err(err) => state.error(format!("read cache for {bundle}: {err}")),
            },
            Err(err) => state.error(format!("sync {bundle}: {err}")),
        },
        Action::DeleteBundle(bundle) => {
            match remote_delete_bundle(cfg, &bundle, allow_ssh_keychain) {
                Ok(()) => {
                    state.bundles.retain(|b| b != &bundle);
                    if state.bundle_index >= state.bundles.len() && !state.bundles.is_empty() {
                        state.bundle_index = state.bundles.len() - 1;
                    }
                    if state.current_bundle.as_deref() == Some(bundle.as_str()) {
                        state.current_bundle = None;
                        state.detail.clear();
                        state.detail_keys.clear();
                        state.detail_index = 0;
                        state.view = View::Bundles;
                    }
                    state.info(format!("deleted {bundle}"));
                }
                Err(err) => state.error(format!("delete {bundle}: {err}")),
            }
        }
        Action::SetKey { bundle, key, value } => {
            if let Err(err) = validate_bundle_key(&key) {
                state.error(format!("{err}"));
                return;
            }
            let result = (|| -> Result<BTreeMap<String, String>> {
                let mut env =
                    remote_read_bundle(cfg, &bundle, allow_ssh_keychain)?.unwrap_or_default();
                env.insert(key.clone(), value);
                remote_write_bundle(cfg, &bundle, &env, allow_ssh_keychain)?;
                crate::write_cache(cfg, &bundle, &env)?;
                Ok(env)
            })();
            match result {
                Ok(env) => {
                    if state.current_bundle.as_deref() == Some(bundle.as_str()) {
                        state.set_detail(bundle.clone(), env);
                    }
                    state.info(format!("set {key} in {bundle}"));
                }
                Err(err) => state.error(format!("set {key}: {err}")),
            }
        }
        Action::UnsetKey { bundle, key } => {
            let result = (|| -> Result<BTreeMap<String, String>> {
                let mut env = remote_read_bundle(cfg, &bundle, allow_ssh_keychain)?
                    .with_context(|| format!("remote bundle '{bundle}' does not exist"))?;
                env.remove(&key);
                remote_write_bundle(cfg, &bundle, &env, allow_ssh_keychain)?;
                crate::write_cache(cfg, &bundle, &env)?;
                Ok(env)
            })();
            match result {
                Ok(env) => {
                    if state.current_bundle.as_deref() == Some(bundle.as_str()) {
                        state.set_detail(bundle.clone(), env);
                    }
                    state.info(format!("unset {key} in {bundle}"));
                }
                Err(err) => state.error(format!("unset {key}: {err}")),
            }
        }
    }
}

fn read_bundle_detail(
    cfg: &Config,
    bundle: &str,
    allow_ssh_keychain: bool,
) -> Result<BTreeMap<String, String>> {
    if bundle == AGENT_AUTH_BUNDLE {
        return Ok(
            remote_read_bundle_for_display(cfg, bundle, allow_ssh_keychain)?.unwrap_or_default(),
        );
    }
    read_cache(cfg, bundle, allow_ssh_keychain)
}

pub fn handle_key(state: &mut AppState, key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
    {
        return Action::Quit;
    }

    match &state.view {
        View::Bundles => handle_bundles_key(state, key),
        View::Detail => handle_detail_key(state, key),
        View::Editor(_) => handle_editor_key(state, key),
        View::Confirm(_) => handle_confirm_key(state, key),
    }
}

fn handle_bundles_key(state: &mut AppState, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('j') | KeyCode::Down => {
            if !state.bundles.is_empty() && state.bundle_index + 1 < state.bundles.len() {
                state.bundle_index += 1;
            }
            Action::None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if state.bundle_index > 0 {
                state.bundle_index -= 1;
            }
            Action::None
        }
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            if let Some(bundle) = state.selected_bundle() {
                Action::OpenBundle(bundle.to_string())
            } else {
                Action::None
            }
        }
        KeyCode::Char('s') => {
            if let Some(bundle) = state.selected_bundle() {
                Action::SyncBundle(bundle.to_string())
            } else {
                Action::None
            }
        }
        KeyCode::Char('r') => Action::RefreshBundles,
        KeyCode::Char('D') => {
            if let Some(bundle) = state.selected_bundle().map(str::to_owned) {
                state.view = View::Confirm(ConfirmState {
                    prompt: format!("Delete bundle '{bundle}' from Infisical? (y/n)"),
                    action: ConfirmAction::DeleteBundle { bundle },
                });
            }
            Action::None
        }
        KeyCode::Char('m') => {
            state.masked = !state.masked;
            Action::None
        }
        _ => Action::None,
    }
}

fn handle_detail_key(state: &mut AppState, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
            state.view = View::Bundles;
            Action::None
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if !state.detail_keys.is_empty() && state.detail_index + 1 < state.detail_keys.len() {
                state.detail_index += 1;
            }
            Action::None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if state.detail_index > 0 {
                state.detail_index -= 1;
            }
            Action::None
        }
        KeyCode::Char('m') => {
            state.masked = !state.masked;
            Action::None
        }
        KeyCode::Char('a') => {
            state.view = View::Editor(EditorState {
                mode: EditorMode::AddKey,
                stage: EditorStage::Key,
                key_buffer: String::new(),
                value_buffer: String::new(),
            });
            Action::None
        }
        KeyCode::Char('e') => {
            if let (Some(_bundle), Some(key)) =
                (state.current_bundle.as_deref(), state.selected_key())
            {
                if is_reserved_agent_auth_key(key) {
                    state.error("agent auth records are managed by rage import");
                    return Action::None;
                }
                let value = state.detail.get(key).cloned().unwrap_or_default();
                state.view = View::Editor(EditorState {
                    mode: EditorMode::EditValue,
                    stage: EditorStage::Value,
                    key_buffer: key.to_string(),
                    value_buffer: value,
                });
            }
            Action::None
        }
        KeyCode::Char('d') => {
            if let (Some(bundle), Some(key)) = (
                state.current_bundle.clone(),
                state.selected_key().map(str::to_owned),
            ) {
                if is_reserved_agent_auth_key(&key) {
                    state.error("agent auth records are managed by rage import");
                    return Action::None;
                }
                state.view = View::Confirm(ConfirmState {
                    prompt: format!("Delete key '{key}' from {bundle}? (y/n)"),
                    action: ConfirmAction::DeleteKey { bundle, key },
                });
            }
            Action::None
        }
        KeyCode::Char('s') => {
            if let Some(bundle) = state.current_bundle.clone() {
                Action::SyncBundle(bundle)
            } else {
                Action::None
            }
        }
        _ => Action::None,
    }
}

fn handle_editor_key(state: &mut AppState, key: KeyEvent) -> Action {
    let View::Editor(editor) = &mut state.view else {
        return Action::None;
    };
    match key.code {
        KeyCode::Esc => {
            state.view = View::Detail;
            Action::None
        }
        KeyCode::Enter => match (editor.mode, editor.stage) {
            (EditorMode::AddKey, EditorStage::Key) => {
                if editor.key_buffer.is_empty() {
                    return Action::None;
                }
                editor.stage = EditorStage::Value;
                Action::None
            }
            (EditorMode::AddKey, EditorStage::Value) | (EditorMode::EditValue, _) => {
                let key = editor.key_buffer.clone();
                let value = editor.value_buffer.clone();
                let bundle = state.current_bundle.clone();
                state.view = View::Detail;
                if let Some(bundle) = bundle {
                    Action::SetKey { bundle, key, value }
                } else {
                    Action::None
                }
            }
        },
        KeyCode::Backspace => {
            match editor.stage {
                EditorStage::Key => {
                    editor.key_buffer.pop();
                }
                EditorStage::Value => {
                    editor.value_buffer.pop();
                }
            }
            Action::None
        }
        KeyCode::Char(ch) => {
            match editor.stage {
                EditorStage::Key => editor.key_buffer.push(ch),
                EditorStage::Value => editor.value_buffer.push(ch),
            }
            Action::None
        }
        _ => Action::None,
    }
}

fn handle_confirm_key(state: &mut AppState, key: KeyEvent) -> Action {
    let View::Confirm(confirm) = state.view.clone() else {
        return Action::None;
    };
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => {
            let action = match confirm.action {
                ConfirmAction::DeleteKey { bundle, key } => Action::UnsetKey { bundle, key },
                ConfirmAction::DeleteBundle { bundle } => Action::DeleteBundle(bundle),
            };
            state.view = if state.current_bundle.is_some() {
                View::Detail
            } else {
                View::Bundles
            };
            action
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            state.view = if state.current_bundle.is_some() {
                View::Detail
            } else {
                View::Bundles
            };
            Action::None
        }
        _ => Action::None,
    }
}

pub fn draw(state: &AppState, frame: &mut Frame) {
    let root = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(root);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(vertical[0]);

    draw_bundles_pane(state, frame, panes[0]);
    draw_detail_pane(state, frame, panes[1]);
    draw_status(state, frame, vertical[1]);

    match &state.view {
        View::Editor(editor) => draw_editor(editor, frame, root),
        View::Confirm(confirm) => draw_confirm(confirm, frame, root),
        _ => {}
    }
}

fn draw_bundles_pane(state: &AppState, frame: &mut Frame, area: Rect) {
    let highlighted = matches!(state.view, View::Bundles);
    let items: Vec<ListItem> = state
        .bundles
        .iter()
        .map(|b| ListItem::new(b.clone()))
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Bundles ")
        .border_style(border_style(highlighted));
    let list = List::new(items)
        .block(block)
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
    let mut list_state = ListState::default();
    if !state.bundles.is_empty() {
        list_state.select(Some(state.bundle_index));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_detail_pane(state: &AppState, frame: &mut Frame, area: Rect) {
    let highlighted = matches!(state.view, View::Detail);
    let title = match &state.current_bundle {
        Some(b) => format!(" Bundle: {b} "),
        None => " Bundle ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style(highlighted));

    if state.detail_keys.is_empty() {
        let hint = if state.current_bundle.is_some() {
            "(empty bundle — press 'a' to add a key)"
        } else {
            "(select a bundle and press Enter)"
        };
        let paragraph = Paragraph::new(hint).block(block).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
        return;
    }

    let items: Vec<ListItem> = state
        .detail_keys
        .iter()
        .map(|k| {
            let raw = state.detail.get(k).map(String::as_str).unwrap_or("");
            let shown = if state.masked { MASK_GLYPHS } else { raw };
            let line = Line::from(vec![
                Span::raw(format!("{k} = ")),
                Span::raw(shown.to_string()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
    let mut list_state = ListState::default();
    list_state.select(Some(state.detail_index));
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_status(state: &AppState, frame: &mut Frame, area: Rect) {
    let hint = match state.view {
        View::Bundles => "j/k move  Enter open  s sync  r refresh  D delete bundle  m mask  q quit",
        View::Detail => "j/k move  a add  e edit  d delete  s sync  m mask  Esc back  q quit",
        View::Editor(_) => "type value  Enter confirm  Esc cancel",
        View::Confirm(_) => "y/Enter confirm  n/Esc cancel",
    };

    let status_text = state
        .status
        .as_ref()
        .map(|s| {
            let prefix = if s.is_error { "error: " } else { "" };
            format!("{prefix}{}", s.text)
        })
        .unwrap_or_else(|| String::from("ready"));

    let lines = vec![Line::from(status_text), Line::from(hint)];
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

fn draw_editor(editor: &EditorState, frame: &mut Frame, root: Rect) {
    let area = centered_rect(60, 7, root);
    frame.render_widget(Clear, area);
    let title = match editor.mode {
        EditorMode::AddKey => " Add key ",
        EditorMode::EditValue => " Edit value ",
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    let key_line = match (editor.mode, editor.stage) {
        (EditorMode::AddKey, EditorStage::Key) => {
            format!("KEY> {}_", editor.key_buffer)
        }
        (EditorMode::AddKey, EditorStage::Value) => {
            format!("KEY: {}", editor.key_buffer)
        }
        (EditorMode::EditValue, _) => format!("KEY: {}", editor.key_buffer),
    };
    let value_line = match (editor.mode, editor.stage) {
        (EditorMode::AddKey, EditorStage::Key) => "VAL: ".to_string(),
        (EditorMode::AddKey, EditorStage::Value) | (EditorMode::EditValue, _) => {
            format!("VAL> {}_", editor.value_buffer)
        }
    };
    let lines = vec![
        Line::from(key_line),
        Line::from(value_line),
        Line::from(""),
        Line::from("Enter confirm  Esc cancel"),
    ];
    frame.render_widget(Paragraph::new(lines), inner_area);
}

fn draw_confirm(confirm: &ConfirmState, frame: &mut Frame, root: Rect) {
    let area = centered_rect(60, 5, root);
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL).title(" Confirm ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(confirm.prompt.clone()),
        Line::from(""),
        Line::from("y/Enter confirm   n/Esc cancel"),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn border_style(active: bool) -> Style {
    if active {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn buffer_text(buffer: &Buffer) -> String {
        let mut out = String::new();
        let area = buffer.area;
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn render(state: &AppState, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(state, f)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn seeded_state() -> AppState {
        let mut state = AppState::new();
        state.set_bundles(vec!["global".to_string(), "project/foo/dev".to_string()]);
        let mut env = BTreeMap::new();
        env.insert("OPENAI_API_KEY".to_string(), "sk-abcdef".to_string());
        env.insert("DATABASE_URL".to_string(), "postgres://secret".to_string());
        state.set_detail("global".to_string(), env);
        state
    }

    fn seeded_agents_state() -> AppState {
        let mut state = AppState::new();
        state.set_bundles(vec![AGENT_AUTH_BUNDLE.to_string()]);
        let mut env = BTreeMap::new();
        env.insert(
            "AUTHLESS_CODEX_JSON".to_string(),
            crate::AGENT_AUTH_DISPLAY_VALUE.to_string(),
        );
        env.insert(
            "AUTHLESS_GROK_JSON".to_string(),
            crate::AGENT_AUTH_DISPLAY_VALUE.to_string(),
        );
        env.insert("DOPPLER_TOKEN".to_string(), "dp.st.secret".to_string());
        state.set_detail(AGENT_AUTH_BUNDLE.to_string(), env);
        state.view = View::Detail;
        state
    }

    #[test]
    fn bundle_list_renders_names_and_selection() {
        let state = seeded_state();
        let buffer = render(&state, 80, 12);
        let text = buffer_text(&buffer);
        assert!(text.contains("global"), "missing 'global' in:\n{text}");
        assert!(
            text.contains("project/foo/dev"),
            "missing 'project/foo/dev' in:\n{text}"
        );
        assert!(
            text.contains("> global"),
            "selection indicator missing in:\n{text}"
        );
    }

    #[test]
    fn detail_view_masks_values_by_default() {
        let mut state = seeded_state();
        state.view = View::Detail;
        assert!(state.masked, "mask should default to on");
        let buffer = render(&state, 80, 12);
        let text = buffer_text(&buffer);
        assert!(
            text.contains("OPENAI_API_KEY"),
            "key name missing in:\n{text}"
        );
        assert!(
            text.contains(MASK_GLYPHS),
            "mask glyphs missing in:\n{text}"
        );
        assert!(
            !text.contains("sk-abcdef"),
            "secret leaked into masked view:\n{text}"
        );
        assert!(
            !text.contains("postgres://secret"),
            "secret leaked into masked view:\n{text}"
        );
    }

    #[test]
    fn mask_toggle_reveals_values() {
        let mut state = seeded_state();
        state.view = View::Detail;
        let action = handle_key(&mut state, key(KeyCode::Char('m')));
        assert_eq!(action, Action::None);
        assert!(!state.masked, "pressing 'm' should toggle mask off");

        let buffer = render(&state, 80, 12);
        let text = buffer_text(&buffer);
        assert!(
            text.contains("sk-abcdef"),
            "value missing after reveal:\n{text}"
        );
    }

    #[test]
    fn agents_detail_renders_imported_auth_records_as_managed_entries() {
        let mut state = seeded_agents_state();
        state.masked = false;
        let buffer = render(&state, 100, 12);
        let text = buffer_text(&buffer);
        assert!(
            text.contains("AUTHLESS_CODEX_JSON"),
            "Codex auth import missing in:\n{text}"
        );
        assert!(
            text.contains("AUTHLESS_GROK_JSON"),
            "Grok auth import missing in:\n{text}"
        );
        assert!(
            text.contains("DOPPLER_TOKEN"),
            "ordinary agents bundle key missing in:\n{text}"
        );
        assert!(
            text.contains(crate::AGENT_AUTH_DISPLAY_VALUE),
            "managed auth placeholder missing in:\n{text}"
        );
        assert!(
            !text.contains("access_token"),
            "auth JSON should not be rendered:\n{text}"
        );
    }

    #[test]
    fn agents_auth_records_cannot_be_edited_or_deleted_in_tui() {
        let mut state = seeded_agents_state();
        state.detail_index = 0;

        let action = handle_key(&mut state, key(KeyCode::Char('e')));
        assert_eq!(action, Action::None);
        assert!(matches!(state.view, View::Detail));
        assert!(state.status.as_ref().is_some_and(|s| s.is_error));

        let action = handle_key(&mut state, key(KeyCode::Char('d')));
        assert_eq!(action, Action::None);
        assert!(matches!(state.view, View::Detail));
        assert!(state.status.as_ref().is_some_and(|s| s.is_error));
    }

    #[test]
    fn open_bundle_action_uses_selected_bundle() {
        let mut state = seeded_state();
        state.view = View::Bundles;
        state.bundle_index = 1;
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(action, Action::OpenBundle("project/foo/dev".to_string()));
    }

    #[test]
    fn add_key_editor_two_stage_emits_set_action() {
        let mut state = seeded_state();
        state.view = View::Detail;
        // open add-key editor
        let action = handle_key(&mut state, key(KeyCode::Char('a')));
        assert_eq!(action, Action::None);
        assert!(matches!(state.view, View::Editor(_)));

        for ch in "NEW_KEY".chars() {
            handle_key(&mut state, key(KeyCode::Char(ch)));
        }
        // advance to value stage
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(action, Action::None);
        for ch in "v1".chars() {
            handle_key(&mut state, key(KeyCode::Char(ch)));
        }
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(
            action,
            Action::SetKey {
                bundle: "global".to_string(),
                key: "NEW_KEY".to_string(),
                value: "v1".to_string(),
            }
        );
        assert!(matches!(state.view, View::Detail));
    }

    #[test]
    fn delete_key_confirm_yields_unset_action() {
        let mut state = seeded_state();
        state.view = View::Detail;
        state.detail_index = 0; // DATABASE_URL after BTreeMap sort
        let action = handle_key(&mut state, key(KeyCode::Char('d')));
        assert_eq!(action, Action::None);
        assert!(matches!(state.view, View::Confirm(_)));

        let action = handle_key(&mut state, key(KeyCode::Char('y')));
        assert_eq!(
            action,
            Action::UnsetKey {
                bundle: "global".to_string(),
                key: "DATABASE_URL".to_string(),
            }
        );
        assert!(matches!(state.view, View::Detail));
    }

    #[test]
    fn quit_key_returns_quit_action() {
        let mut state = seeded_state();
        let action = handle_key(&mut state, key(KeyCode::Char('q')));
        assert_eq!(action, Action::Quit);
    }
}
