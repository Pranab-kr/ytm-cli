mod app_loop;
mod config;
mod logging;
mod mpris;

use color_eyre::eyre::{Context, eyre};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
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
        // Mouse capture is for the wheel only (FR-U8). Clicks are deliberately
        // not bound: capturing them would take the terminal's own click-drag
        // text selection away from the user, which costs more than it adds.
        execute!(
            out,
            EnterAlternateScreen,
            EnableMouseCapture,
            crossterm::cursor::Hide
        )?;
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
    // Mouse capture off before leaving: a terminal left in capture mode ignores
    // the user's own selection and scrollback afterwards.
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
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
/// The theme to start with, plus the preset name so `t` knows where the cycle
/// is. A `theme_file` is the most specific answer and wins over `ui.theme`.
fn build_theme(cfg: &config::Config) -> color_eyre::Result<(Theme, String)> {
    if let Some(path) = cfg.ui.theme_file.as_deref().map(expand_tilde) {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read the theme file at {}", path.display()))?;
        return Ok((Theme::from_toml_str(&text)?, "custom".to_owned()));
    }
    let name = match &cfg.ui.theme {
        config::ThemeChoice::Auto => config::auto_theme_name(),
        config::ThemeChoice::Named(n) => n.clone(),
    };
    let mut theme = Theme::preset(&name).ok_or_else(|| {
        eyre!(
            "ui.theme names no built-in theme: {name:?} (try one of: {})",
            Theme::preset_names().join(", ")
        )
    })?;
    if let Some(hex) = &cfg.ui.accent {
        theme.accent = ytm_tui::theme::parse_hex(hex)
            .ok_or_else(|| eyre!("ui.accent is not a hex color like \"#7aa2f7\": {hex:?}"))?;
    }
    Ok((theme, name))
}

/// How long `ytm login` waits for the browser authorization before giving up.
const LOGIN_DEADLINE_SECS: u64 = 300;

/// `ytm` with no subcommand launches the TUI; the subcommands are the
/// non-interactive paths, which is what makes auth debuggable without a
/// terminal UI in the way.
#[derive(Debug, clap::Parser)]
#[command(name = "ytm", about = "YouTube Music in your terminal", version)]
pub struct Cli {
    /// Path to config.toml (defaults to the platform config dir)
    #[arg(long, global = true)]
    pub config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Sign in to YouTube Music
    Login,
    /// Forget stored credentials
    Logout,
    /// Print your playlists and exit
    Playlists,
    /// Write config.toml with every default and binding, then open it
    Config {
        /// Write the file and print its path without opening an editor
        #[arg(long)]
        no_edit: bool,
    },
    /// Cache maintenance
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Debug, clap::Subcommand, PartialEq, Eq)]
pub enum CacheAction {
    /// Delete all cached metadata
    Clear,
}

/// Print the device code and wait for the browser authorization (FR-A1).
///
/// `println!` is allowed here and in the other subcommands: the TUI is not
/// running, so there is no frame to corrupt. That is the whole reason these
/// exist as subcommands rather than as panes.
async fn run_login(cfg: &config::Config) -> color_eyre::Result<()> {
    use ytm_core::auth::{KeyringStore, TokenStore};
    use ytm_core::oauth::{begin_device_login, complete_device_login};

    let (Some(client_id), Some(client_secret)) = (&cfg.auth.client_id, &cfg.auth.client_secret)
    else {
        return Err(eyre!(
            "auth.client_id / auth.client_secret are not set in {}",
            config::Config::default_path().display()
        ));
    };

    // ytmapi_rs's own client, not reqwest's — that is what `begin_device_login`
    // takes.
    let client = ytmapi_rs::Client::new()?;
    let (info, code) = begin_device_login(&client, client_id).await?;
    println!("1. Open: {}", info.verification_url);
    println!("2. Enter code: {}", info.user_code);
    println!("3. Approve the request, then wait here.\n");
    println!(
        "polling every {}s, giving up after {LOGIN_DEADLINE_SECS}s...",
        info.interval_secs
    );

    let store = KeyringStore::default_store();
    let token = complete_device_login(
        &client,
        code,
        client_id,
        client_secret,
        &store,
        info.interval_secs,
        LOGIN_DEADLINE_SECS,
    )
    .await?;
    // StoredToken's Debug is redacted by hand, so this cannot leak the token.
    tracing::info!(?token, "signed in");
    println!("\nSigned in. Tokens are in the OS keyring, not on disk.");
    println!("keyring round-trip: {}", store.load()?.is_some());
    Ok(())
}

/// Forget the stored token. Deliberately does not touch the cookie file: that
/// is the user's own export, and deleting someone's file is not this command's
/// business.
fn run_logout() -> color_eyre::Result<()> {
    use ytm_core::auth::{KeyringStore, TokenStore};
    // `clear` is documented idempotent: clearing when nothing is stored
    // succeeds, so "already signed out" is not an error to report.
    KeyringStore::default_store().clear()?;
    println!("Signed out — the stored token has been cleared.");
    println!("A cookie file, if you use one, is left alone.");
    Ok(())
}

/// One playlist title per line — the fastest auth check there is, and scriptable.
async fn run_playlists(cfg: &config::Config) -> color_eyre::Result<()> {
    let source = build_source(cfg).await?;
    let playlists = source.library_playlists().await?;
    if playlists.is_empty() {
        // Same trap as the TUI's empty-library hint: an expired cookie answers
        // HTTP 200 with zero rows, so silence here would read as "no playlists".
        return Err(eyre!(
            "the library came back empty — if that is wrong, the cookie or token has expired"
        ));
    }
    for p in &playlists {
        match p.track_count {
            Some(n) => println!("{}\t{} tracks", p.title, n),
            None => println!("{}", p.title),
        }
    }
    Ok(())
}

/// Write the documented config and open it in `$EDITOR` (FR-U9).
///
/// The point is that nothing has to be written by hand: the file that lands
/// carries every setting and every keybinding at its default value, commented
/// out, so changing one is uncommenting a line. Without this the user had to
/// know the file's location, its schema, and the action names before they could
/// rebind anything.
///
/// An existing file is never overwritten — that would discard the user's own
/// settings, which is the opposite of helpful. It is opened as it is.
fn run_config(no_edit: bool) -> color_eyre::Result<()> {
    let path = config::Config::default_path();
    let existed = path.exists();
    if !existed {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, config::EXAMPLE_TOML)?;
    }

    println!(
        "{} {}",
        if existed {
            "config:"
        } else {
            "config written:"
        },
        path.display()
    );

    if no_edit {
        println!("\nEvery setting and keybinding is in that file, commented out at its");
        println!("default. Uncomment a line to change it.");
        return Ok(());
    }

    let Some(editor) = app_loop::editor_command() else {
        // Not an error: the file is written, which is the useful half. Telling
        // the user to set $EDITOR and exiting 1 would bury that.
        println!("\n$EDITOR is not set, so the file was not opened.");
        println!("Edit it directly, or set $EDITOR and run this again.");
        return Ok(());
    };

    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let status = std::process::Command::new(bin)
        .args(parts)
        .arg(&path)
        .status()?;
    if !status.success() {
        return Err(eyre!("{bin} exited with {status}"));
    }
    // Validated after editing so a typo is caught here rather than surfacing as
    // a silently ignored binding later.
    match config::Config::load(Some(&path)) {
        Ok(_) => println!("config is valid"),
        Err(e) => return Err(eyre!("config.toml is not valid: {e}")),
    }
    Ok(())
}

fn run_cache_clear() -> color_eyre::Result<()> {
    let path = config::paths::cache_dir().join("cache.db");
    let cache = ytm_core::cache::Cache::open(&path)?;
    cache.clear()?;
    println!("Cache cleared ({}).", path.display());
    Ok(())
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    use clap::Parser;
    color_eyre::install()?;
    // Held for the process lifetime; dropping it loses buffered log lines.
    let _log_guard = logging::init(&config::paths::log_dir())?;
    let cli = Cli::parse();
    let cfg = config::Config::load(cli.config.as_deref())?;

    // Every subcommand returns an Err on failure, which `main` turns into a
    // non-zero exit — that is what makes these usable from a script.
    match &cli.command {
        Some(Command::Login) => return run_login(&cfg).await,
        Some(Command::Logout) => return run_logout(),
        Some(Command::Playlists) => return run_playlists(&cfg).await,
        Some(Command::Config { no_edit }) => return run_config(*no_edit),
        Some(Command::Cache { action }) => {
            return match action {
                CacheAction::Clear => run_cache_clear(),
            };
        }
        None => {}
    }
    run_tui(cfg).await
}

/// The default path: the full terminal UI.
async fn run_tui(cfg: config::Config) -> color_eyre::Result<()> {
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
    let (theme, theme_name) = build_theme(&cfg)?;

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
    // Built from `[keys]` so a rebind applies on the first frame, not after a
    // reload. An unparseable table is reported rather than silently ignored.
    let keymap = if cfg.keys.is_empty() {
        KeyMap::default()
    } else {
        KeyMap::from_toml_str(&toml::to_string(&cfg.keys)?)?
    };
    // The pre-probe frame cannot draw art: the picker does not exist yet.
    let mut art_probe = ytm_tui::widgets::art::ArtCache::disabled();
    guard
        .terminal
        .draw(|f| ytm_tui::render::render(f, &state, &theme, &keymap, &mut art_probe))?;
    tracing::info!(
        cached_playlists = state.playlists.len(),
        cached_tracks = state.tracks.len(),
        "first frame drawn"
    );

    // Probed after the alternate screen is entered (what `from_query_stdio`'s own
    // docs require) and after the first draw (it blocks up to 2s on a terminal
    // that never answers, which would eat the whole NFR-1 budget). Still before
    // the event stream exists, or the reply would be read as a key press.
    let art = if cfg.ui.album_art {
        ytm_tui::widgets::art::ArtCache::detect()
    } else {
        ytm_tui::widgets::art::ArtCache::disabled()
    };

    // Optional by design: no bus means no media keys and nothing else changes.
    let (media_tx, media_keys) = tokio::sync::mpsc::unbounded_channel();
    let media = mpris::attach(media_tx);

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
        art,
        media,
        media_keys,
        cfg.config_path.clone(),
        theme_name,
    )
    .await;

    // Drop the guard before returning, so an error report prints to a restored
    // terminal rather than into the alternate screen.
    drop(guard);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn no_arguments_launches_the_tui() {
        let c = Cli::parse_from(["ytm"]);
        assert!(c.command.is_none());
    }

    #[test]
    fn login_and_logout_parse() {
        assert!(matches!(
            Cli::parse_from(["ytm", "login"]).command,
            Some(Command::Login)
        ));
        assert!(matches!(
            Cli::parse_from(["ytm", "logout"]).command,
            Some(Command::Logout)
        ));
    }

    #[test]
    fn playlists_is_a_non_interactive_listing() {
        // Useful for scripting and for verifying auth without the TUI.
        assert!(matches!(
            Cli::parse_from(["ytm", "playlists"]).command,
            Some(Command::Playlists)
        ));
    }

    #[test]
    fn a_config_path_can_be_overridden() {
        let c = Cli::parse_from(["ytm", "--config", "/tmp/x.toml"]);
        assert_eq!(
            c.config.as_deref(),
            Some(std::path::Path::new("/tmp/x.toml"))
        );
    }

    #[test]
    fn an_unknown_subcommand_is_rejected() {
        assert!(Cli::try_parse_from(["ytm", "frobnicate"]).is_err());
    }

    #[test]
    fn cache_clear_parses_as_a_nested_subcommand() {
        assert!(matches!(
            Cli::parse_from(["ytm", "cache", "clear"]).command,
            Some(Command::Cache {
                action: CacheAction::Clear
            })
        ));
    }

    #[test]
    fn cache_without_an_action_is_rejected() {
        // Better an error than silently doing nothing to someone's cache.
        assert!(Cli::try_parse_from(["ytm", "cache"]).is_err());
    }
}
