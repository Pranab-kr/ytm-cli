//! Prove the playlist-id form fix against the live account, reversibly.
//!
//! Renames a playlist, reads the library back as evidence, then restores the
//! original title. Nothing is created or deleted.
//!
//!   cargo run -p ytm-core --example verify_edit -- <title-to-rename>

mod common;

use ytm_core::MusicSource;
use ytm_core::ytmusic::YtMusicSource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::args()
        .nth(1)
        .ok_or("usage: verify_edit <existing playlist title>")?;

    let common::AuthChoice::Cookie(path) = common::auth_choice()?;
    let src = YtMusicSource::from_cookie_file(&path).await?;

    let before = src.library_playlists().await?;
    let p = before
        .iter()
        .find(|p| p.title == target)
        .ok_or_else(|| format!("no playlist titled {target:?}"))?;
    println!("target: {:?} id={}", p.title, p.id.as_str());
    println!(
        "  browse_form={}  mutation_form={}",
        p.id.browse_form(),
        p.id.mutation_form()
    );

    let probe = format!("{target}-verify");
    println!("renaming -> {probe:?}");
    src.edit_playlist(p.id.clone(), Some(probe.clone()), None, None)
        .await?;

    let mid = src.library_playlists().await?;
    let renamed = mid.iter().any(|q| q.title == probe);
    println!("library now shows {probe:?}: {renamed}");

    println!("restoring -> {target:?}");
    src.edit_playlist(p.id.clone(), Some(target.clone()), None, None)
        .await?;
    let after = src.library_playlists().await?;
    let restored = after.iter().any(|q| q.title == target);
    println!("restored: {restored}");

    if renamed && restored {
        println!("FR-C2 (rename): OK");
    } else {
        println!("FR-C2 (rename): FAILED");
    }
    Ok(())
}
