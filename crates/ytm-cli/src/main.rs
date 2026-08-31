mod app_loop;
mod config;
mod logging;

use color_eyre::eyre::{Context, eyre};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout};
use std::sync::Arc;
use ytm_core::MusicSource;
use ytm_tui::{app::AppState, keymap::KeyMap, theme::Theme};

/// Restores the terminal on drop, including during a panic unwind (NFR-5).
pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, crossterm::cursor::Hide)?;
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(out))?,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
}

/// Put the terminal back *before* the panic message is printed, or the report
/// lands in the alternate screen and vanishes with it (NFR-5). `Drop` alone is
/// not enough: it runs after the hook.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

/// Minimal `~` expansion so `cookie_file = "~/.config/..."` works. Not worth a
/// dependency.
fn expand_tilde(p: &std::path::Path) -> std::path::PathBuf {
    let Some(rest) = p.to_str().and_then(|s| s.strip_prefix("~/")) else {
        return p.to_path_buf();
    };
    match std::env::var("HOME") {
        Ok(home) => std::path::PathBuf::from(home).join(rest),
        Err(_) => p.to_path_buf(),
    }
}

/// Build whichever `MusicSource` config selects.
///
/// Cookie auth is the live path; OAuth is kept because it is correct and will
/// work again if Google restores device-flow tokens on InnerTube (see
/// PROGRESS.md, Open question 3).
async fn build_source(cfg: &config::Config) -> color_eyre::Result<Arc<dyn MusicSource>> {
    use ytm_core::ytmusic::YtMusicSource;

    match cfg.auth.kind {
        config::AuthKind::Cookie => {
            let path = cfg
                .auth
                .cookie_file
                .as_deref()
                .map(expand_tilde)
                .ok_or_else(|| {
                    eyre!(
                        "auth.kind = \"cookie\" but auth.cookie_file is not set in {}",
                        config::Config::default_path().display()
                    )
                })?;
            let source = YtMusicSource::from_cookie_file(&path)
                .await
                .with_context(|| format!("could not use the cookie file at {}", path.display()))?;
            Ok(Arc::new(source))
        }
        config::AuthKind::OAuth => {
            use ytm_core::auth::{KeyringStore, TokenStore};
            let (id, secret) = match (&cfg.auth.client_id, &cfg.auth.client_secret) {
                (Some(i), Some(s)) => (i.clone(), s.clone()),
                _ => {
                    return Err(eyre!(
                        "auth.kind = \"oauth\" but auth.client_id / auth.client_secret are not set in {}",
                        config::Config::default_path().display()
                    ));
                }
            };
            let stored = KeyringStore::default_store()
                .load()
                .context("could not read the stored token from the OS keyring")?
                .ok_or_else(|| {
                    eyre!("not signed in — run `cargo run -p ytm-core --example login_spike` first")
                })?;
            let token = ytm_core::oauth::oauth_token_from_stored(&stored, &id, &secret)
                .context("the stored token could not be rebuilt — sign in again")?;
            Ok(Arc::new(YtMusicSource::from_oauth(token)))
        }
    }
}

/// Theme from config: an explicit file wins, otherwise just the accent override.
fn build_theme(cfg: &config::Config) -> color_eyre::Result<Theme> {
    if let Some(path) = cfg.ui.theme_file.as_deref().map(expand_tilde) {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read the theme file at {}", path.display()))?;
        return Ok(Theme::from_toml_str(&text)?);
    }
    let mut theme = Theme::default();
    if let Some(hex) = &cfg.ui.accent {
        theme.accent = ytm_tui::theme::parse_hex(hex)
            .ok_or_else(|| eyre!("ui.accent is not a hex color like \"#7aa2f7\": {hex:?}"))?;
    }
    Ok(theme)
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    // Held for the process lifetime; dropping it loses buffered log lines.
    let _log_guard = logging::init(&config::paths::log_dir())?;
    let cfg = config::Config::load(None)?;
    tracing::info!(
        auth = ?cfg.auth.kind,
        volume = cfg.playback.volume,
        vim_keys = cfg.ui.vim_keys,
        "config loaded"
    );

    // Opened before the source so the first frame can be drawn from it. A
    // corrupt file rebuilds itself; an unopenable one degrades to no cache
    // rather than blocking startup, because none of this is authoritative data.
    let cache_path = config::paths::cache_dir().join("cache.db");
    let cache = match ytm_core::cache::Cache::open(&cache_path) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(error = %e, path = %cache_path.display(), "running without a cache");
            None
        }
    };

    let volume = cfg.playback.volume.min(100) as u8;
    let theme = build_theme(&cfg)?;

    // Fails cleanly here rather than mid-frame if libmpv is missing.
    let (player, player_events) = ytm_player::actor::spawn_player(volume)?;

    let mut state = AppState {
        volume,
        shuffle: cfg.playback.shuffle,
        ..Default::default()
    };
    if let Some(c) = cache.as_ref() {
        app_loop::preload_from_cache(c, &mut state);
    }

    install_panic_hook();
    let mut guard = TerminalGuard::new()?;

    // The cached frame goes up before anything touches the network (NFR-1).
    // `build_source` is an `.await` on a cookie-validation round trip — running
    // it first put a blank terminal on screen for ~2.4s. Keys typed during the
    // wait are buffered by the terminal and handled once the loop starts.
    let keymap = KeyMap::default();
    guard
        .terminal
        .draw(|f| ytm_tui::render::render(f, &state, &theme, &keymap))?;
    tracing::info!(
        cached_playlists = state.playlists.len(),
        cached_tracks = state.tracks.len(),
        "first frame drawn"
    );

    let source = match build_source(&cfg).await {
        Ok(s) => s,
        Err(e) => {
            // The terminal is already in the alternate screen, so restore it
            // before the report goes out or the error lands where it cannot be read.
            drop(guard);
            return Err(e);
        }
    };
    let result = app_loop::run(
        &mut guard.terminal,
        state,
        source,
        player,
        player_events,
        keymap,
        theme,
        cfg.ui.tick_ms,
        cfg.auth.kind == config::AuthKind::Cookie,
        cache,
    )
    .await;

    // Drop the guard before returning, so an error report prints to a restored
    // terminal rather than into the alternate screen.
    drop(guard);
    result
}
