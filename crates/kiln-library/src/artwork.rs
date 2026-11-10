//! Artwork pipeline: Steam CDN public resolver, Steam store name search, SteamGridDB API v2
//! client (optional, needs a key), local disk cache, and fallbacks.
//!
//! Order for a game without art:
//! 1. Its Steam App ID (Steam games) -> official Steam library art. No key needed.
//! 2. Epic games: the official cover and key art listed in the Epic launcher's own catalog
//!    on this PC (`catcache.bin`). No key needed.
//! 3. Otherwise search the Steam store **by name** (many Epic / standalone games are on
//!    Steam too) and use that App ID, but only if the names really match.
//! 4. Otherwise SteamGridDB by name, if the user has set an API key.
//!
//! Every name lookup is remembered in `lookups.json` in the artwork folder, so each game is
//! searched once; failed lookups are retried after a week.

use kiln_core::models::DiscoveredGame;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct ArtworkManager {
    cache_dir: PathBuf,
    api_key: Option<String>,
    /// Plain client for Steam: never carries the SteamGridDB key.
    client: reqwest::Client,
    /// Authorised client for SteamGridDB API calls (only when a key is set).
    sgdb: Option<reqwest::Client>,
}

#[derive(Debug, Deserialize)]
struct StoreSearch {
    #[serde(default)]
    items: Vec<StoreItem>,
}

#[derive(Debug, Deserialize)]
struct StoreItem {
    id: u64,
    name: String,
}

/// Remembered name lookups (game id -> result), stored as `lookups.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LookupCache {
    #[serde(default)]
    entries: HashMap<String, Lookup>,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Lookup {
    /// Steam App ID found by name, or None if nothing matched.
    steam_appid: Option<String>,
    /// Unix time of the lookup.
    checked: u64,
    /// SteamGridDB had nothing either (only tried when a key is set).
    #[serde(default)]
    sgdb_miss: bool,
}

/// Failed lookups are retried after this long (new releases, renamed games).
const RETRY_MISSES_AFTER_SECS: u64 = 7 * 24 * 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl LookupCache {
    pub fn load(cache_dir: &Path) -> Self {
        let path = cache_dir.join("lookups.json");
        let mut c: LookupCache = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        c.path = path;
        c
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let tmp = self.path.with_extension("json.tmp");
            if fs::write(&tmp, json).is_ok() && fs::rename(&tmp, &self.path).is_ok() {
                self.dirty = false;
            }
        }
    }

    fn fresh(&self, game_id: &str) -> Option<&Lookup> {
        self.entries.get(game_id).filter(|l| {
            l.steam_appid.is_some()
                || now_secs().saturating_sub(l.checked) < RETRY_MISSES_AFTER_SECS
        })
    }

    fn put(&mut self, game_id: &str, lookup: Lookup) {
        self.entries.insert(game_id.to_string(), lookup);
        self.dirty = true;
    }
}

/// Lowercase, '&' -> "and", trademark signs dropped, punctuation -> spaces, collapsed.
pub fn normalize_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.to_lowercase().chars() {
        match c {
            '™' | '®' | '©' | '\'' | '’' => {}
            '&' => out.push_str(" and "),
            c if c.is_alphanumeric() => out.push(c),
            _ => out.push(' '),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Edition suffixes ignored when comparing names ("Batman GOTY Edition" = "Batman").
const EDITION_SUFFIXES: &[&str] = &[
    " game of the year edition",
    " goty edition",
    " goty",
    " definitive edition",
    " complete edition",
    " deluxe edition",
    " enhanced edition",
    " anniversary edition",
    " standard edition",
    " ultimate edition",
    " gold edition",
    " directors cut",
    " director s cut",
    " remastered",
    " edition",
];

fn strip_editions(mut s: &str) -> &str {
    loop {
        match EDITION_SUFFIXES
            .iter()
            .find(|suf| s.ends_with(*suf) && s.len() > suf.len())
        {
            Some(suf) => s = &s[..s.len() - suf.len()],
            None => return s,
        }
    }
}

/// True when two game names refer to the same game. Deliberately strict: a wrong poster is
/// worse than none.
pub fn titles_match(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_title(a), normalize_title(b));
    !a.is_empty() && (a == b || strip_editions(&a) == strip_editions(&b))
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Kiln game launcher")
            .build()
            .unwrap_or_default();
        let _ = fs::create_dir_all(&cache_dir);
        let mut mgr = Self {
            cache_dir,
            api_key: None,
            client,
            sgdb: None,
        };
        if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
            mgr.set_api_key(key);
        }
        mgr
    }

    pub fn set_api_key(&mut self, api_key: String) {
        let api_key = api_key.trim().to_string();
        let mut headers = HeaderMap::new();
        if let Ok(mut val) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
            val.set_sensitive(true);
            headers.insert(AUTHORIZATION, val);
        }
        self.sgdb = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .build()
            .ok();
        self.api_key = Some(api_key);
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Downloads the first URL that returns an image, saving it as `<stem>.jpg` / `.png`
    /// according to its actual format.
    async fn download_image(&self, urls: &[String], stem: &Path) -> Option<PathBuf> {
        for url in urls {
            let Ok(resp) = self.client.get(url).send().await else {
                continue;
            };
            if !resp.status().is_success() {
                continue;
            }
            let Ok(bytes) = resp.bytes().await else {
                continue;
            };
            let ext = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                "png"
            } else if bytes.starts_with(&[0xFF, 0xD8]) {
                "jpg"
            } else {
                continue; // unknown / unsupported format
            };
            let dest = stem.with_extension(ext);
            if File::create(&dest)
                .and_then(|mut f| f.write_all(&bytes))
                .is_ok()
            {
                return Some(dest);
            }
        }
        None
    }

    /// Official Epic art: portrait cover as the poster, landscape key art as the hero.
    async fn fetch_epic_art(
        &self,
        game_id: &str,
        tall: &str,
        wide: &str,
    ) -> (Option<PathBuf>, Option<PathBuf>) {
        let safe = sanitize_filename(game_id);
        // Ask Epic's CDN for a TV-sized copy first; fall back to the original file.
        let sized = |u: &str, w: u32, h: u32| format!("{u}?w={w}&h={h}&resize=1");
        let grid = self
            .download_image(
                &[sized(tall, 600, 900), tall.to_string()],
                &self.cache_dir.join(format!("grid_{safe}")),
            )
            .await;
        let hero = self
            .download_image(
                &[sized(wide, 1920, 1080), wide.to_string()],
                &self.cache_dir.join(format!("hero_{safe}")),
            )
            .await;
        (grid, hero)
    }

    /// Searches the Steam store by name and returns the App ID of an exact (edition-
    /// insensitive) name match. No key needed; only the game's name is sent.
    pub async fn find_steam_appid_by_name(&self, title: &str) -> Option<String> {
        let resp = self
            .client
            .get("https://store.steampowered.com/api/storesearch/")
            .query(&[("term", title), ("l", "english"), ("cc", "US")])
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let found: StoreSearch = resp.json().await.ok()?;
        found
            .items
            .iter()
            .find(|i| titles_match(title, &i.name))
            .map(|i| i.id.to_string())
    }

    /// Finds and downloads art for a game that has none, following the order in the module
    /// docs. Returns (poster, hero) paths of whatever was found.
    pub async fn resolve_missing(
        &self,
        game: &DiscoveredGame,
        lookups: &mut LookupCache,
    ) -> (Option<PathBuf>, Option<PathBuf>) {
        // 1. Real Steam games carry their App ID.
        if let Some(appid) = extract_steam_appid(&game.launch_target, &game.id, &game.title) {
            return self.fetch_steam_cdn_artwork(&game.id, &appid).await;
        }
        // 1b. Epic games: official art from the Epic launcher's catalog.
        if let Some(app_name) = game.id.strip_prefix("epic_") {
            if let Some((tall, wide)) = epic_catalog_art(app_name) {
                let art = self.fetch_epic_art(&game.id, &tall, &wide).await;
                if art.0.is_some() {
                    return art;
                }
            }
        }
        // 2. Name search on Steam (remembered).
        let remembered = lookups.fresh(&game.id).cloned();
        let appid = match &remembered {
            Some(l) => l.steam_appid.clone(),
            None => {
                let found = self.find_steam_appid_by_name(&game.title).await;
                lookups.put(
                    &game.id,
                    Lookup {
                        steam_appid: found.clone(),
                        checked: now_secs(),
                        sgdb_miss: false,
                    },
                );
                found
            }
        };
        if let Some(appid) = appid {
            let art = self.fetch_steam_cdn_artwork(&game.id, &appid).await;
            if art.0.is_some() {
                return art;
            }
        }
        // 3. SteamGridDB, if the user set a key.
        if self.sgdb.is_none() || remembered.as_ref().is_some_and(|l| l.sgdb_miss) {
            return (None, None);
        }
        let art = self
            .fetch_and_cache_artwork(&game.id, &game.title)
            .await
            .unwrap_or((None, None));
        if art.0.is_none() {
            if let Some(l) = lookups.entries.get_mut(&game.id) {
                l.sgdb_miss = true;
                lookups.dirty = true;
            }
        }
        art
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
        let Some(sgdb) = self.sgdb.as_ref() else {
            return Ok((None, None));
        };

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

        let resp = sgdb
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
        // Only an exact name match: a wrong poster is worse than none.
        let target_sgdb_game = match search_res
            .data
            .iter()
            .find(|g| titles_match(title, &g.name))
        {
            Some(g) => g,
            None => return Ok((None, None)),
        };

        // Fetch Grid (600x900)
        let grid_url = format!(
            "https://www.steamgriddb.com/api/v2/grids/game/{}?dimensions=600x900",
            target_sgdb_game.id
        );
        let mut grid_path = None;
        if let Ok(resp) = sgdb.get(&grid_url).send().await {
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
        if let Ok(resp) = sgdb.get(&hero_url).send().await {
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

/// Official art URLs (portrait cover, landscape key art) for an Epic game, from the Epic
/// launcher's local catalog cache. `app_name` is the manifest's AppName.
pub fn epic_catalog_art(app_name: &str) -> Option<(String, String)> {
    let program_data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    let path = Path::new(&program_data).join(r"Epic\EpicGamesLauncher\Data\Catalog\catcache.bin");
    epic_catalog_art_in(&fs::read(path).ok()?, app_name)
}

/// Parses a catcache.bin (base64-encoded JSON list of catalog items).
pub fn epic_catalog_art_in(raw: &[u8], app_name: &str) -> Option<(String, String)> {
    use base64::Engine;
    let text: Vec<u8> = raw
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let json = base64::engine::general_purpose::STANDARD
        .decode(text)
        .ok()?;
    let items: Vec<serde_json::Value> = serde_json::from_slice(&json).ok()?;
    let item = items.iter().find(|item| {
        item["releaseInfo"]
            .as_array()
            .is_some_and(|r| r.iter().any(|r| r["appId"].as_str() == Some(app_name)))
    })?;
    let image = |kind: &str| {
        item["keyImages"].as_array()?.iter().find_map(|k| {
            (k["type"].as_str() == Some(kind)).then(|| k["url"].as_str().map(str::to_string))?
        })
    };
    let tall = image("DieselGameBoxTall")?;
    let wide = image("DieselGameBox").unwrap_or_else(|| tall.clone());
    Some((tall, wide))
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

    // Anything else is found by name (see `ArtworkManager::resolve_missing`). There used to
    // be a hard-coded title table here; it gave Alan Wake 2 Cyberpunk's art.
    let _ = title;
    None
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
            None,
            "non-Steam games are looked up by name, never guessed"
        );
    }

    #[test]
    fn title_matching_is_strict_but_ignores_editions() {
        assert!(titles_match("Brotato", "Brotato"));
        assert!(titles_match(
            "Batman: Arkham Asylum GOTY Edition",
            "Batman: Arkham Asylum Game of the Year Edition"
        ));
        assert!(titles_match(
            "Batman Arkham Asylum",
            "Batman™: Arkham Asylum GOTY Edition"
        ));
        assert!(titles_match("Fall Guys", "Fall Guys"));
        assert!(titles_match(
            "Tom Clancy's The Division",
            "Tom Clancys The Division"
        ));
        assert!(titles_match("Ratchet & Clank", "Ratchet and Clank"));
        assert!(!titles_match("Hades", "Hades II"));
        assert!(!titles_match("Alan Wake 2", "Cyberpunk 2077"));
        assert!(!titles_match("Portal", "Portal 2"));
        assert!(!titles_match("", ""));
    }

    #[test]
    fn reads_epic_catalog_cache() {
        use base64::Engine;
        let json = r#"[{"title":"Brotato","releaseInfo":[{"appId":"e8bb"}],
            "keyImages":[{"type":"DieselGameBox","url":"https://x/wide.png"},
                         {"type":"DieselGameBoxTall","url":"https://x/tall.png"}]},
            {"title":"Other","releaseInfo":[{"appId":"zzz"}],"keyImages":[]}]"#;
        let raw = base64::engine::general_purpose::STANDARD.encode(json);
        assert_eq!(
            epic_catalog_art_in(raw.as_bytes(), "e8bb"),
            Some(("https://x/tall.png".into(), "https://x/wide.png".into()))
        );
        assert_eq!(epic_catalog_art_in(raw.as_bytes(), "zzz"), None);
        assert_eq!(epic_catalog_art_in(raw.as_bytes(), "nope"), None);
    }

    #[test]
    fn lookups_round_trip_and_misses_expire() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = LookupCache::load(dir.path());
        c.put(
            "epic_a",
            Lookup {
                steam_appid: Some("1942280".into()),
                checked: 0,
                sgdb_miss: false,
            },
        );
        c.put(
            "epic_b",
            Lookup {
                steam_appid: None,
                checked: now_secs(),
                sgdb_miss: false,
            },
        );
        c.put(
            "epic_c",
            Lookup {
                steam_appid: None,
                checked: 0,
                sgdb_miss: false,
            },
        );
        c.save();
        let c = LookupCache::load(dir.path());
        assert_eq!(
            c.fresh("epic_a").and_then(|l| l.steam_appid.clone()),
            Some("1942280".into())
        );
        assert!(c.fresh("epic_b").is_some(), "recent miss is remembered");
        assert!(c.fresh("epic_c").is_none(), "old miss is retried");
    }

    /// Live check against the Steam store (network). Run manually:
    ///   cargo test -p kiln-library steam_name_search_finds_brotato -- --ignored
    #[test]
    #[ignore = "needs internet"]
    fn steam_name_search_finds_brotato() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ArtworkManager::new(dir.path().to_path_buf(), None);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(mgr.find_steam_appid_by_name("Brotato")),
            Some("1942280".into())
        );
        // Strict matching: "Hades" must not pick "Hades II", which Steam lists first.
        assert_eq!(
            rt.block_on(mgr.find_steam_appid_by_name("Hades")),
            Some("1145360".into())
        );
        // Delisted from Steam: no match rather than a wrong one.
        assert_eq!(rt.block_on(mgr.find_steam_appid_by_name("Fall Guys")), None);
        assert_eq!(
            rt.block_on(mgr.find_steam_appid_by_name("zzqq not a real game 9876")),
            None
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
