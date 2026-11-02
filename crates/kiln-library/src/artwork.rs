//! Artwork pipeline: SteamGridDB API v2 client, local disk cache, and fallbacks.

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
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

    pub fn get_cached_grid_path(&self, game_id: &str) -> Option<PathBuf> {
        let path = self
            .cache_dir
            .join(format!("grid_{}.png", sanitize_filename(game_id)));
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    pub fn get_cached_hero_path(&self, game_id: &str) -> Option<PathBuf> {
        let path = self
            .cache_dir
            .join(format!("hero_{}.png", sanitize_filename(game_id)));
        if path.exists() {
            Some(path)
        } else {
            None
        }
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

fn sanitize_filename(name: &str) -> String {
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
