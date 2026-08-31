//! Find where `setVideoId` lives in a real `GetPlaylistTracksQuery` response.
//!
//! Option A of PROGRESS.md open question 1: `ytmapi-rs` 0.3.3 drops the field
//! during parsing, so we read it out of the wire JSON ourselves. The path has to
//! come from a real response rather than a guess (CLAUDE.md).
//!
//! `println!` is allowed here — examples are exempt from the no-stdout rule.
//!
//!   cargo run -p ytm-core --example dump_playlist_tracks -- <playlistId>
//!   cargo run -p ytm-core --example dump_playlist_tracks -- <playlistId> --raw

mod common;

use ytmapi_rs::common::YoutubeID;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let raw_mode = args.iter().any(|a| a == "--raw");
    let id = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .ok_or("usage: dump_playlist_tracks <playlistId> [--raw]")?;

    let common::AuthChoice::Cookie(path) = common::auth_choice()?;
    eprintln!("auth: browser cookie ({})", path.display());

    let api = ytmapi_rs::YtMusic::from_cookie_file(&path).await?;
    let json: String = api
        .raw_json_query(ytmapi_rs::query::GetPlaylistTracksQuery::new(
            ytmapi_rs::common::PlaylistID::from_raw(id),
        ))
        .await?;

    if raw_mode {
        println!("{json}");
        return Ok(());
    }

    // Walk the tree and print every path that ends in setVideoId, so the shape
    // is established rather than assumed.
    let v: serde_json::Value = serde_json::from_str(&json)?;
    let mut hits = Vec::new();
    find(&v, String::new(), &mut hits);
    eprintln!("{} setVideoId occurrence(s)", hits.len());
    for (path, val) in hits.iter().take(5) {
        println!("{path} = {val}");
    }
    Ok(())
}

fn find(v: &serde_json::Value, path: String, out: &mut Vec<(String, String)>) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, child) in m {
                let p = format!("{path}/{k}");
                if k == "setVideoId" {
                    out.push((p.clone(), child.to_string()));
                }
                find(child, p, out);
            }
        }
        serde_json::Value::Array(a) => {
            for (i, child) in a.iter().enumerate() {
                find(child, format!("{path}/{i}"), out);
            }
        }
        _ => {}
    }
}
