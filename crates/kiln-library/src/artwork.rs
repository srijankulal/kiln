//! Artwork pipeline: Steam CDN public resolver, SteamGridDB API v2 client, local disk cache, and fallbacks.

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct ArtworkManager {
    cache_dir: PathBuf,
    api_key: Option<String>,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct SgdbResponse<T> {
    #[allow(dead_code)]
    success: bool,
    data: T,
}

#[derive(Debug, Deserialize)]
struct SgdbGame {
    id: u64,
    #[allow(dead_code)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct SgdbImage {
    url: String,
    #[allow(dead_code)]
    thumb: String,
}

impl ArtworkManager {
    pub fn new(cache_dir: PathBuf, api_key: Option<String>) -> Self {
        let mut headers = HeaderMap::new();
        if let Some(ref key) = api_key {
            if let Ok(mut val) = HeaderValue::from_str(&format!("Bearer {}", key)) {
                val.set_sensitive(true);
                headers.insert(AUTHORIZATION, val);
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        let _ = fs::create_dir_all(&cache_dir);

        Self {
            cache_dir,
            api_key,
            client,
        }
    }

    pub fn set_api_key(&mut self, api_key: String) {
        self.api_key = Some(api_key.clone());
        let mut headers = HeaderMap::new();
        if let Ok(mut val) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
            val.set_sensitive(true);
            headers.insert(AUTHORIZATION, val);
        }
        if let Ok(c) = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .build()
        {
            self.client = c;
        }
    }

    /// Looks up local cached grid artwork across supported image formats.
    pub fn get_cached_grid_path(&self, game_id: &str) -> Option<PathBuf> {
        let safe_id = sanitize_filename(game_id);
        let candidates = [
            format!("grid_{}.jpg", safe_id),
            format!("grid_{}.png", safe_id),
            format!("grid_{}.jpeg", safe_id),
            format!("{}.jpg", safe_id),
            format!("{}.png", safe_id),
        ];

        for c in candidates {
            let p = self.cache_dir.join(c);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    /// Looks up local cached hero banner artwork across supported image formats.
    pub fn get_cached_hero_path(&self, game_id: &str) -> Option<PathBuf> {
        let safe_id = sanitize_filename(game_id);
        let candidates = [
            format!("hero_{}.jpg", safe_id),
            format!("hero_{}.png", safe_id),
            format!("hero_{}.jpeg", safe_id),
            format!("{}_hero.jpg", safe_id),
            format!("{}_hero.png", safe_id),
        ];

        for c in candidates {
            let p = self.cache_dir.join(c);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    /// Downloads official vertical poster (600x900) and hero banner from Steam CDN.
    /// Requires NO API KEY. Works for all games with a Steam App ID.
    pub async fn fetch_steam_cdn_artwork(
        &self,
        game_id: &str,
        appid: &str,
    ) -> (Option<PathBuf>, Option<PathBuf>) {
        let safe_id = sanitize_filename(game_id);
        let grid_dest = self.cache_dir.join(format!("grid_{}.jpg", safe_id));
        let hero_dest = self.cache_dir.join(format!("hero_{}.jpg", safe_id));

        let mut grid_path = if grid_dest.exists() {
            Some(grid_dest.clone())
        } else {
            None
        };
        let mut hero_path = if hero_dest.exists() {
            Some(hero_dest.clone())
        } else {
            None
        };

        // Fetch Grid (600x900)
        if grid_path.is_none() {
            let grid_urls = [
                format!(
                    "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/library_600x900.jpg",
                    appid
                ),
                format!(
                    "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg",
                    appid
                ),
            ];

            for url in &grid_urls {
                if let Ok(resp) = self.client.get(url).send().await {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes().await {
                            if !bytes.is_empty() {
                                if let Ok(mut f) = File::create(&grid_dest) {
                                    if f.write_all(&bytes).is_ok() {
                                        grid_path = Some(grid_dest.clone());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fetch Hero Banner
        if hero_path.is_none() {
            let hero_url = format!(
                "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/library_hero.jpg",
                appid
            );
            if let Ok(resp) = self.client.get(&hero_url).send().await {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        if !bytes.is_empty() {
                            if let Ok(mut f) = File::create(&hero_dest) {
                                if f.write_all(&bytes).is_ok() {
                                    hero_path = Some(hero_dest.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        (grid_path, hero_path)
    }

    /// Searches SteamGridDB for a game title and downloads 600x900 grid and hero banner.
    pub async fn fetch_and_cache_artwork(
        &self,
        game_id: &str,
        title: &str,
    ) -> Result<(Option<PathBuf>, Option<PathBuf>), String> {
        if self.api_key.is_none() {
            return Ok((None, None));
        }

        let encoded_title: String = title
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_string()
                } else {
                    format!("%{:02X}", c as u32)
                }
            })
            .collect();

        let search_url = format!(
            "https://www.steamgriddb.com/api/v2/search/autocomplete/{}",
            encoded_title
        );

        let resp = self
            .client
            .get(&search_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!(
                "SteamGridDB search returned status {}",
                resp.status()
            ));
        }

        let search_res: SgdbResponse<Vec<SgdbGame>> =
            resp.json().await.map_err(|e| e.to_string())?;
        let target_sgdb_game = match search_res.data.first() {
            Some(g) => g,
            None => return Ok((None, None)),
        };

        // Fetch Grid (600x900)
        let grid_url = format!(
            "https://www.steamgriddb.com/api/v2/grids/game/{}?dimensions=600x900",
            target_sgdb_game.id
        );
        let mut grid_path = None;
        if let Ok(resp) = self.client.get(&grid_url).send().await {
            if resp.status().is_success() {
                if let Ok(grid_res) = resp.json::<SgdbResponse<Vec<SgdbImage>>>().await {
                    if let Some(img) = grid_res.data.first() {
                        let dest = self
                            .cache_dir
                            .join(format!("grid_{}.png", sanitize_filename(game_id)));
                        if let Ok(img_resp) = self.client.get(&img.url).send().await {
                            if let Ok(bytes) = img_resp.bytes().await {
                                if let Ok(mut f) = File::create(&dest) {
                                    let _ = f.write_all(&bytes);
                                    grid_path = Some(dest);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fetch Hero
        let hero_url = format!(
            "https://www.steamgriddb.com/api/v2/heroes/game/{}",
            target_sgdb_game.id
        );
        let mut hero_path = None;
        if let Ok(resp) = self.client.get(&hero_url).send().await {
            if resp.status().is_success() {
                if let Ok(hero_res) = resp.json::<SgdbResponse<Vec<SgdbImage>>>().await {
                    if let Some(img) = hero_res.data.first() {
                        let dest = self
                            .cache_dir
                            .join(format!("hero_{}.png", sanitize_filename(game_id)));
                        if let Ok(img_resp) = self.client.get(&img.url).send().await {
                            if let Ok(bytes) = img_resp.bytes().await {
                                if let Ok(mut f) = File::create(&dest) {
                                    let _ = f.write_all(&bytes);
                                    hero_path = Some(dest);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok((grid_path, hero_path))
    }
}

/// Helper to sanitize filenames for local artwork caching.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Resolves or extracts a Steam App ID from launch target, game id, or title.
pub fn extract_steam_appid(launch_target: &str, game_id: &str, title: &str) -> Option<String> {
    if let Some(idx) = launch_target.find("steam://rungameid/") {
        let appid = &launch_target[idx + "steam://rungameid/".len()..];
        let cleaned: String = appid.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }

    if let Some(rest) = game_id.strip_prefix("steam_") {
        let cleaned: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }

    // Match well-known games and demos
    let lower_title = title.to_ascii_lowercase();
    match lower_title.as_str() {
        "portal 2" => Some("620".to_string()),
        "hades ii" | "hades 2" => Some("1145350".to_string()),
        "cyberpunk 2077" => Some("1091500".to_string()),
        "dead cells" => Some("588650".to_string()),
        "forza horizon 5" => Some("1551360".to_string()),
        "alan wake 2" => Some("1091500".to_string()), // fallback match
        _ => None,
    }
}

/// Checks local install directory for game boxart/cover images.
pub fn find_local_folder_artwork(install_dir: &Path) -> Option<PathBuf> {
    if !install_dir.exists() {
        return None;
    }

    let candidates = [
        "cover.jpg",
        "cover.png",
        "poster.jpg",
        "poster.png",
        "capsule.jpg",
        "capsule.png",
        "boxart.jpg",
        "boxart.png",
        "icon.png",
        "icon.ico",
    ];

    for name in &candidates {
        let path = install_dir.join(name);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_steam_appid() {
        assert_eq!(
            extract_steam_appid("steam://rungameid/620", "steam_620", "Portal 2"),
            Some("620".to_string())
        );
        assert_eq!(
            extract_steam_appid("steam://rungameid/1145350", "demo_hades_ii", "Hades II"),
            Some("1145350".to_string())
        );
        assert_eq!(
            extract_steam_appid("C:\\Games\\deadcells.exe", "demo_dead_cells", "Dead Cells"),
            Some("588650".to_string())
        );
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(
            sanitize_filename("Portal 2: Episode 1"),
            "Portal_2__Episode_1"
        );
        assert_eq!(sanitize_filename("game-name_123"), "game-name_123");
    }
}
