use crate::plugin::LiveMarkdownPlugin;
use crate::server::ServerConfig;
use crate::session::BufferSnapshot;
use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, CreateCommandOpts, OptionOpts};
use nvim_oxi::api::types::{AutocmdCallbackArgs, CommandArgs, CommandNArgs};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Dictionary, Function, Object, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::runtime::{Builder, Runtime};

static APP_STATE: OnceLock<Mutex<Option<Arc<AppState>>>> = OnceLock::new();
static CALLBACKS_REGISTERED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct AppState {
    plugin: LiveMarkdownPlugin,
    runtime: Runtime,
}

impl AppState {
    fn new(config: ServerConfig) -> std::result::Result<Self, String> {
        let runtime = Builder::new_multi_thread()
            .thread_name("live-markdown.nvim")
            .enable_all()
            .build()
            .map_err(|err| format!("failed to start runtime: {err}"))?;

        Ok(Self {
            plugin: LiveMarkdownPlugin::new(config),
            runtime,
        })
    }

    fn shutdown(&self) {
        self.runtime.block_on(self.plugin.shutdown());
    }

    fn has_session(&self, bufnr: i64) -> bool {
        self.runtime.block_on(self.plugin.has_session(bufnr))
    }

    fn has_active_previews(&self) -> bool {
        self.runtime
            .block_on(self.plugin.sessions().session_count())
            > 0
    }

    fn start_current(&self) -> std::result::Result<String, String> {
        let buffer = api::get_current_buf();
        if !is_markdown_buffer(&buffer) {
            return Err(String::from(
                "current buffer is not markdown (filetype or extension mismatch)",
            ));
        }

        let snapshot = snapshot_from_buffer(&buffer)?;
        let url = self
            .runtime
            .block_on(self.plugin.start_preview(snapshot))
            .map_err(|err| err.to_string())?;

        Ok(url)
    }

    fn stop_active(&self) -> std::result::Result<bool, String> {
        self.runtime
            .block_on(async {
                let sessions = self.plugin.sessions();
                let Some(bufnr) = sessions.active_bufnr().await else {
                    return Ok(false);
                };

                self.plugin.stop_preview(bufnr).await
            })
            .map_err(|err| err.to_string())
    }

    fn show_url_current(&self) -> std::result::Result<Option<String>, String> {
        let bufnr = i64::from(api::get_current_buf().handle());
        self.runtime
            .block_on(self.plugin.open_preview(bufnr))
            .map_err(|err| err.to_string())
    }

    fn on_text_changed(&self, buffer: api::Buffer) {
        let bufnr = i64::from(buffer.handle());
        if !self.has_session(bufnr) {
            return;
        }

        let snapshot = match snapshot_from_buffer(&buffer) {
            Ok(snapshot) => snapshot,
            Err(_) => return,
        };

        let plugin = self.plugin.clone();
        self.runtime.spawn(async move {
            plugin.on_text_changed(snapshot).await;
        });
    }

    fn on_buf_write(&self, buffer: api::Buffer) {
        let bufnr = i64::from(buffer.handle());
        if !self.has_session(bufnr) {
            return;
        }

        let snapshot = match snapshot_from_buffer(&buffer) {
            Ok(snapshot) => snapshot,
            Err(_) => return,
        };

        let plugin = self.plugin.clone();
        self.runtime.spawn(async move {
            plugin.on_buf_write(snapshot).await;
        });
    }

    fn on_cursor_moved(&self, buffer: api::Buffer) {
        let bufnr = i64::from(buffer.handle());
        if !self.has_session(bufnr) {
            return;
        }

        let (line, col) = cursor_for_buffer(&buffer);
        let plugin = self.plugin.clone();

        self.runtime.spawn(async move {
            plugin.on_cursor_moved(bufnr, line, col).await;
        });
    }

    fn on_buf_enter(&self, buffer: api::Buffer) {
        if !is_markdown_buffer(&buffer) || !self.has_active_previews() {
            return;
        }

        let bufnr = i64::from(buffer.handle());
        if self.has_session(bufnr) {
            return;
        }

        let snapshot = match snapshot_from_buffer(&buffer) {
            Ok(snapshot) => snapshot,
            Err(_) => return,
        };

        let plugin = self.plugin.clone();
        self.runtime.spawn(async move {
            plugin.on_buf_enter(snapshot).await;
        });
    }

    fn on_buf_wipeout(&self, bufnr: i64) {
        if !self.has_session(bufnr) {
            return;
        }

        let plugin = self.plugin.clone();
        self.runtime.spawn(async move {
            let _ = plugin.on_buf_wipeout(bufnr).await;
        });
    }
}

pub fn module() -> Result<Dictionary> {
    Ok(Dictionary::from_iter([
        ("setup", Object::from(Function::from_fn(setup))),
        ("stop", Object::from(Function::from_fn(stop))),
        ("show_url", Object::from(Function::from_fn(show_url))),
        ("start", Object::from(Function::from_fn(start))),
        ("shutdown", Object::from(Function::from_fn(shutdown))),
    ]))
}

fn setup(opts: Option<Dictionary>) {
    if let Err(err) = setup_impl(opts) {
        notify_err(&format!("[live-markdown.nvim] setup failed: {err}"));
    }
}

fn setup_impl(opts: Option<Dictionary>) -> Result<()> {
    ensure_callbacks_registered()?;

    let config = parse_server_config(opts);
    let state = match AppState::new(config) {
        Ok(state) => Arc::new(state),
        Err(err) => {
            notify_err(&format!("[live-markdown.nvim] {err}"));
            return Ok(());
        }
    };

    let old = replace_state(state);
    if let Some(old) = old {
        old.shutdown();
    }

    Ok(())
}

fn stop(_: Option<bool>) {
    let Some(state) = state() else {
        notify_err("[live-markdown.nvim] plugin is not configured");
        return;
    };

    match state.stop_active() {
        Ok(true) => notify_info("[live-markdown.nvim] stopped preview server"),
        Ok(false) => notify_info("[live-markdown.nvim] no active preview session"),
        Err(err) => notify_err(&format!("[live-markdown.nvim] {err}")),
    }
}

fn start(_: ()) {
    let Some(state) = state() else {
        notify_err("[live-markdown.nvim] plugin is not configured");
        return;
    };

    match state.start_current() {
        Ok(url) => notify_info(&format!("[live-markdown.nvim] preview started: {url}")),
        Err(err) => notify_err(&format!("[live-markdown.nvim] {err}")),
    }
}

fn show_url(_: ()) {
    let Some(state) = state() else {
        notify_err("[live-markdown.nvim] plugin is not configured");
        return;
    };

    match state.show_url_current() {
        Ok(Some(url)) => notify_info(&format!("[live-markdown.nvim] preview URL: {url}")),
        Ok(None) => notify_info("[live-markdown.nvim] no active preview for current buffer"),
        Err(err) => notify_err(&format!("[live-markdown.nvim] {err}")),
    }
}

fn shutdown(_: ()) {
    if let Some(state) = take_state() {
        state.shutdown();
        notify_info("[live-markdown.nvim] plugin shut down");
    }
}

fn ensure_callbacks_registered() -> Result<()> {
    if CALLBACKS_REGISTERED.load(Ordering::Acquire) {
        return Ok(());
    }

    register_commands()?;
    register_autocmds()?;
    CALLBACKS_REGISTERED.store(true, Ordering::Release);
    Ok(())
}

fn register_commands() -> Result<()> {
    let stop_opts = CreateCommandOpts::builder()
        .desc("Stop markdown preview server")
        .force(true)
        .nargs(CommandNArgs::Zero)
        .build();
    api::create_user_command("LiveMarkdownStop", command_stop, &stop_opts)?;

    let show_url_opts = CreateCommandOpts::builder()
        .desc("Show markdown preview URL")
        .force(true)
        .nargs(CommandNArgs::Zero)
        .build();
    api::create_user_command("LiveMarkdownShowUrl", command_show_url, &show_url_opts)?;

    let start_opts = CreateCommandOpts::builder()
        .desc("Start markdown preview and follow buffer")
        .force(true)
        .nargs(CommandNArgs::Zero)
        .build();
    api::create_user_command("LiveMarkdownStart", command_start, &start_opts)?;

    Ok(())
}

fn register_autocmds() -> Result<()> {
    let augroup = CreateAugroupOpts::builder().clear(true).build();
    let group_id = api::create_augroup("LiveMarkdown", &augroup)?;

    let text_opts = CreateAutocmdOpts::builder()
        .group(group_id)
        .callback(autocmd_text_changed)
        .build();
    api::create_autocmd(["TextChanged", "TextChangedI"], &text_opts)?;

    let write_opts = CreateAutocmdOpts::builder()
        .group(group_id)
        .callback(autocmd_buf_write_post)
        .build();
    api::create_autocmd(["BufWritePost"], &write_opts)?;

    let cursor_opts = CreateAutocmdOpts::builder()
        .group(group_id)
        .callback(autocmd_cursor_moved)
        .build();
    api::create_autocmd(["CursorMoved", "CursorMovedI"], &cursor_opts)?;

    let enter_opts = CreateAutocmdOpts::builder()
        .group(group_id)
        .callback(autocmd_buf_enter)
        .build();
    api::create_autocmd(["BufEnter"], &enter_opts)?;

    let wipeout_opts = CreateAutocmdOpts::builder()
        .group(group_id)
        .callback(autocmd_buf_wipeout)
        .build();
    api::create_autocmd(["BufWipeout"], &wipeout_opts)?;

    let vimleave_opts = CreateAutocmdOpts::builder()
        .group(group_id)
        .callback(autocmd_vim_leave)
        .build();
    api::create_autocmd(["VimLeavePre"], &vimleave_opts)?;

    Ok(())
}

fn command_stop(_: CommandArgs) {
    stop(None);
}

fn command_show_url(_: CommandArgs) {
    show_url(());
}

fn command_start(_: CommandArgs) {
    start(());
}

fn autocmd_text_changed(args: AutocmdCallbackArgs) -> bool {
    if !is_markdown_buffer(&args.buffer) {
        return false;
    }

    if let Some(state) = state() {
        state.on_text_changed(args.buffer);
    }

    false
}

fn autocmd_cursor_moved(args: AutocmdCallbackArgs) -> bool {
    if !is_markdown_buffer(&args.buffer) {
        return false;
    }

    if let Some(state) = state() {
        state.on_cursor_moved(args.buffer);
    }

    false
}

fn autocmd_buf_write_post(args: AutocmdCallbackArgs) -> bool {
    if !is_markdown_buffer(&args.buffer) {
        return false;
    }

    if let Some(state) = state() {
        state.on_buf_write(args.buffer);
    }

    false
}

fn autocmd_buf_enter(args: AutocmdCallbackArgs) -> bool {
    if let Some(state) = state() {
        state.on_buf_enter(args.buffer);
    }

    false
}

fn autocmd_buf_wipeout(args: AutocmdCallbackArgs) -> bool {
    if let Some(state) = state() {
        state.on_buf_wipeout(i64::from(args.buffer.handle()));
    }

    false
}

fn autocmd_vim_leave(_: AutocmdCallbackArgs) -> bool {
    if let Some(state) = take_state() {
        state.shutdown();
    }

    false
}

fn state() -> Option<Arc<AppState>> {
    let lock = APP_STATE.get_or_init(|| Mutex::new(None));
    let guard = match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clone()
}

fn replace_state(state: Arc<AppState>) -> Option<Arc<AppState>> {
    let lock = APP_STATE.get_or_init(|| Mutex::new(None));
    let mut guard = match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.replace(state)
}

fn take_state() -> Option<Arc<AppState>> {
    let lock = APP_STATE.get_or_init(|| Mutex::new(None));
    let mut guard = match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.take()
}

fn parse_server_config(opts: Option<Dictionary>) -> ServerConfig {
    let mut config = ServerConfig::default();
    let Some(opts) = opts else {
        return config;
    };

    if let Some(port) = get_dict_i64(&opts, &["port"])
        && (1..=u16::MAX as i64).contains(&port)
    {
        config.port = port as u16;
    }

    if let Some(debounce_ms_content) =
        get_dict_i64(&opts, &["debounce_ms_content", "debounceMsContent"])
        && debounce_ms_content >= 0
    {
        config.debounce_ms_content = debounce_ms_content as u64;
    }

    if let Some(throttle_ms_cursor) =
        get_dict_i64(&opts, &["throttle_ms_cursor", "throttleMsCursor"])
        && throttle_ms_cursor >= 0
    {
        config.throttle_ms_cursor = throttle_ms_cursor as u64;
    }

    if let Some(bind_address) = get_dict_string(&opts, &["bind_address", "bindAddress"])
        && (bind_address == "127.0.0.1" || bind_address == "localhost")
    {
        config.bind_address = String::from("127.0.0.1");
    }

    if let Some(auto_scroll) = get_dict_bool(&opts, &["auto_scroll", "autoScroll"]) {
        config.auto_scroll = auto_scroll;
    }

    if let Some(scroll_comfort_top) =
        get_dict_f64(&opts, &["scroll_comfort_top", "scrollComfortTop"])
        && (0.0..1.0).contains(&scroll_comfort_top)
    {
        config.scroll_comfort_top = scroll_comfort_top;
    }

    if let Some(scroll_comfort_bottom) =
        get_dict_f64(&opts, &["scroll_comfort_bottom", "scrollComfortBottom"])
        && (0.0..=1.0).contains(&scroll_comfort_bottom)
    {
        config.scroll_comfort_bottom = scroll_comfort_bottom;
    }

    if config.scroll_comfort_top >= config.scroll_comfort_bottom {
        config.scroll_comfort_top = ServerConfig::default().scroll_comfort_top;
        config.scroll_comfort_bottom = ServerConfig::default().scroll_comfort_bottom;
    }

    config
}

fn get_dict_i64(opts: &Dictionary, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(obj) = opts.get(key)
            && let Ok(value) = i64::from_object(obj.clone())
        {
            return Some(value);
        }
    }

    None
}

fn get_dict_f64(opts: &Dictionary, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(obj) = opts.get(key)
            && let Ok(value) = f64::from_object(obj.clone())
        {
            return Some(value);
        }
    }

    None
}

fn get_dict_bool(opts: &Dictionary, keys: &[&str]) -> Option<bool> {
    for key in keys {
        if let Some(obj) = opts.get(key)
            && let Ok(value) = bool::from_object(obj.clone())
        {
            return Some(value);
        }
    }

    None
}

fn get_dict_string(opts: &Dictionary, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(obj) = opts.get(key)
            && let Ok(value) = String::from_object(obj.clone())
        {
            return Some(value);
        }
    }

    None
}

fn snapshot_from_buffer(buffer: &api::Buffer) -> std::result::Result<BufferSnapshot, String> {
    let changedtick = u64::from(
        buffer
            .get_changedtick()
            .map_err(|err| format!("failed to get changedtick: {err}"))?,
    );

    let mut markdown = String::new();
    let lines = buffer
        .get_lines(.., false)
        .map_err(|err| format!("failed to read buffer lines: {err}"))?;

    for (idx, line) in lines.enumerate() {
        if idx > 0 {
            markdown.push('\n');
        }
        markdown.push_str(line.to_string_lossy().as_ref());
    }

    let (cursor_line, cursor_col) = cursor_for_buffer(buffer);
    let source_path = {
        let name = buffer
            .get_name()
            .map_err(|err| format!("failed to read buffer path: {err}"))?;
        let path = name.to_string_lossy();
        let trimmed = path.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    Ok(BufferSnapshot {
        bufnr: i64::from(buffer.handle()),
        changedtick,
        markdown,
        cursor_line,
        cursor_col,
        source_path,
    })
}

fn cursor_for_buffer(buffer: &api::Buffer) -> (usize, usize) {
    let win = api::get_current_win();
    let Ok(win_buf) = win.get_buf() else {
        return (1, 0);
    };

    if win_buf.handle() != buffer.handle() {
        return (1, 0);
    }

    win.get_cursor().unwrap_or((1, 0))
}

fn is_markdown_buffer(buffer: &api::Buffer) -> bool {
    let option_opts = OptionOpts::builder().buffer(buffer.clone()).build();

    if let Ok(filetype) = api::get_option_value::<String>("filetype", &option_opts) {
        let filetype = filetype.to_ascii_lowercase();
        if matches!(
            filetype.as_str(),
            "markdown" | "mdx" | "rmd" | "quarto" | "pandoc"
        ) {
            return true;
        }
    }

    let Ok(name) = buffer.get_name() else {
        return false;
    };

    let path = name.to_string_lossy();
    let path = Path::new(path.as_ref());
    let Some(ext) = path.extension() else {
        return false;
    };

    matches!(
        ext.to_string_lossy().to_ascii_lowercase().as_str(),
        "md" | "markdown" | "mdown" | "mkd" | "mdx" | "qmd"
    )
}

fn notify_info(message: &str) {
    nvim_oxi::print!("{message}");
}

fn notify_err(message: &str) {
    api::err_writeln(message);
}

#[cfg(test)]
mod tests {
    use super::parse_server_config;
    use crate::server::ServerConfig;
    use nvim_oxi::{Dictionary, Object};

    #[test]
    fn uses_default_config_without_opts() {
        let parsed = parse_server_config(None);
        let defaults = ServerConfig::default();

        assert_eq!(parsed.port, defaults.port);
        assert_eq!(parsed.bind_address, defaults.bind_address);
        assert_eq!(parsed.debounce_ms_content, defaults.debounce_ms_content);
        assert_eq!(parsed.throttle_ms_cursor, defaults.throttle_ms_cursor);
        assert_eq!(parsed.auto_scroll, defaults.auto_scroll);
    }

    #[test]
    fn accepts_setup_overrides_from_dictionary() {
        let opts = Dictionary::from_iter([
            ("port", Object::from(6520)),
            ("debounceMsContent", Object::from(140)),
            ("throttle_ms_cursor", Object::from(35)),
            ("bindAddress", Object::from("localhost")),
            ("auto_scroll", Object::from(false)),
            ("scroll_comfort_top", Object::from(0.2)),
            ("scrollComfortBottom", Object::from(0.7)),
        ]);

        let parsed = parse_server_config(Some(opts));

        assert_eq!(parsed.port, 6520);
        assert_eq!(parsed.debounce_ms_content, 140);
        assert_eq!(parsed.throttle_ms_cursor, 35);
        assert_eq!(parsed.bind_address, "127.0.0.1");
        assert!(!parsed.auto_scroll);
        assert!((parsed.scroll_comfort_top - 0.2).abs() < f64::EPSILON);
        assert!((parsed.scroll_comfort_bottom - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn rejects_invalid_scroll_band() {
        let opts = Dictionary::from_iter([
            ("scroll_comfort_top", Object::from(0.9)),
            ("scroll_comfort_bottom", Object::from(0.1)),
        ]);

        let parsed = parse_server_config(Some(opts));
        let defaults = ServerConfig::default();

        assert_eq!(parsed.scroll_comfort_top, defaults.scroll_comfort_top);
        assert_eq!(parsed.scroll_comfort_bottom, defaults.scroll_comfort_bottom);
    }
}
