//! Manual check that every library pane returns data (or a clean empty) against
//! the live account — the albums pane used to fail to parse when the account had
//! no saved albums (FR-B3).
//!
//! `println!` is allowed here — examples are exempt from the no-stdout rule.
//!
//!   cargo run -p ytm-core --example dump_library

mod common;

use ytm_core::MusicSource;
use ytm_core::ytmusic::YtMusicSource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let common::AuthChoice::Cookie(path) = common::auth_choice()? else {
        return Err("this example is cookie-auth only".into());
    };
    let source = YtMusicSource::from_cookie_file(&path).await?;

    match source.library_albums().await {
        Ok(a) => println!("albums:   OK, {} entries", a.len()),
        Err(e) => println!("albums:   FAILED — {e}"),
    }
    match source.library_artists().await {
        Ok(a) => {
            println!("artists:  OK, {} entries", a.len());
            for x in a.iter().take(3) {
                println!("            {}", x.name);
            }
        }
        Err(e) => println!("artists:  FAILED — {e}"),
    }
    match source.library_songs().await {
        Ok(s) => println!("songs:    OK, {} entries", s.len()),
        Err(e) => println!("songs:    FAILED — {e}"),
    }
    match source.library_playlists().await {
        Ok(p) => println!("playlists: OK, {} entries", p.len()),
        Err(e) => println!("playlists: FAILED — {e}"),
    }
    Ok(())
}
