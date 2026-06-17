use crate::settings::Settings;
use rocket::http::ContentType;
use rocket::http::Status;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket::{delete, get, post, put};
use rocket::State;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::atomic::AtomicU64;
use std::time::{SystemTime, UNIX_EPOCH};
use local_ip_address::local_ip;
use rocket::data::{Data, ToByteUnit};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("popasound")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SavedSound {
    pub id: String,
    pub title: String,
    pub url: String,
    pub mp3_url: String,
    pub added_at: String,
    #[serde(default = "default_volume")]
    pub volume: f64,
}

fn default_volume() -> f64 { 1.0 }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HotkeyBinding {
    pub id: String,
    pub sound_id: String,
    pub sound_title: String,
    pub modifiers: String,
    pub key: String,
    pub display: String,
}

#[derive(Debug)]
pub enum MainCommand {
    RegisterHotkey { id: String, modifiers: String, key: String },
    UnregisterHotkey(String),
    PlaySound(String),
    PlayAudio { data: Vec<u8>, title: String, volume: f64 },
    Stop,
    SetDevice(Option<String>),
    SetGlobalVolume(f64),
}

pub struct AppData {
    pub sounds: Vec<SavedSound>,
    pub bindings: Vec<HotkeyBinding>,
}

pub struct AppState {
    pub data: Mutex<AppData>,
    pub command_tx: mpsc::Sender<MainCommand>,
    pub settings: Mutex<Settings>,
    pub now_playing: Mutex<Vec<String>>,
    pub play_gen: AtomicU64,
    pub local_ip: String,
}

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    status: String,
    author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveRequest {
    pub id: String,
    pub title: String,
    pub url: String,
    pub mp3_url: String,
}

#[derive(Deserialize)]
pub struct VolumeRequest {
    pub volume: f64,
}

#[derive(Serialize)]
pub struct NowPlayingInfo {
    pub seq: u64,
    pub id: String,
}

#[derive(Deserialize)]
pub struct PlayUrlRequest {
    pub url: String,
    pub title: Option<String>,
}

#[derive(Deserialize)]
pub struct BindRequest {
    pub sound_id: String,
    pub sound_title: String,
    pub modifiers: String,
    pub key: String,
    pub display: String,
}

#[derive(Serialize)]
pub struct LibrarySound {
    pub id: String,
    pub title: String,
    pub url: String,
    pub mp3: String,
    pub added_at: String,
    pub volume: f64,
    pub hotkey_display: Option<String>,
}

#[derive(Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub is_current: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SettingsResponse {
    pub global_volume: f64,
    pub device_name: Option<String>,
}

#[derive(Serialize)]
pub struct HotkeyResponse {
    pub id: String,
    pub sound_id: String,
    pub sound_title: String,
    pub display: String,
}

fn init_dirs() {
    fs::create_dir_all(data_dir()).ok();
    fs::create_dir_all(data_dir().join("mp3")).ok();
}

fn load_json<T: serde::de::DeserializeOwned>(path: impl Into<std::path::PathBuf>) -> Vec<T> {
    let path = path.into();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_json<T: serde::Serialize>(path: impl Into<std::path::PathBuf>, data: &T) {
    let path = path.into();
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = fs::write(&path, json);
    }
}

fn timestamp() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    dur.as_secs().to_string()
}

fn ok_response<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        status: "200".to_string(),
        author: "abdipr".to_string(),
        data: Some(data),
        message: None,
    })
}

fn err_response(status: u16, msg: &str) -> (Status, Json<ApiResponse<()>>) {
    (
        Status::from_code(status).unwrap_or(Status::NotFound),
        Json(ApiResponse {
            status: status.to_string(),
            author: "abdipr".to_string(),
            data: None,
            message: Some(msg.to_string()),
        }),
    )
}

impl AppState {
    pub fn load(command_tx: mpsc::Sender<MainCommand>) -> Self {
        init_dirs();
        let sounds: Vec<SavedSound> = load_json(data_dir().join("sounds.json"));
        let bindings: Vec<HotkeyBinding> = load_json(data_dir().join("hotkeys.json"));
        let mut settings = Settings::load();
        settings.apply_deserialize_overrides();
        let local_ip = local_ip().map(|ip| ip.to_string()).unwrap_or_else(|_| "127.0.0.1".to_string());
        AppState {
            data: Mutex::new(AppData { sounds, bindings }),
            command_tx,
            settings: Mutex::new(settings),
            now_playing: Mutex::new(Vec::new()),
            play_gen: AtomicU64::new(0),
            local_ip,
        }
    }

    pub fn persist_sounds(&self) {
        let data = self.data.lock().unwrap();
        save_json(data_dir().join("sounds.json"), &data.sounds);
    }

    pub fn persist_bindings(&self) {
        let data = self.data.lock().unwrap();
        save_json(data_dir().join("hotkeys.json"), &data.bindings);
    }
}

async fn download_mp3(url: &str, path: &PathBuf) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    fs::write(path, &bytes).map_err(|e| e.to_string())?;
    Ok(())
}

#[post("/library/save", data = "<req>")]
pub async fn save_sound(
    req: Json<SaveRequest>,
    state: &State<Arc<AppState>>,
) -> Result<Json<ApiResponse<LibrarySound>>, (Status, Json<ApiResponse<()>>)> {
    let sound_id = req.id.clone();
    {
        let data = state.data.lock().unwrap();
        if data.sounds.iter().any(|s| s.id == sound_id) {
            return Err(err_response(409, "Sound already in library"));
        }
    }

    let mp3_path = data_dir().join("mp3").join(format!("{}.mp3", sound_id));
    download_mp3(&req.mp3_url, &mp3_path)
        .await
        .map_err(|e| err_response(500, &format!("Failed to download MP3: {}", e)))?;

    let saved = SavedSound {
        id: sound_id,
        title: req.title.clone(),
        url: req.url.clone(),
        mp3_url: req.mp3_url.clone(),
        added_at: timestamp(),
        volume: 1.0,
    };

    {
        let mut data = state.data.lock().unwrap();
        data.sounds.push(saved.clone());
    }
    state.persist_sounds();

    let lib_sound = LibrarySound {
        id: saved.id.clone(),
        title: saved.title.clone(),
        url: saved.url.clone(),
        mp3: format!("/library/mp3/{}.mp3", saved.id),
        added_at: saved.added_at.clone(),
        volume: 1.0,
        hotkey_display: None,
    };

    Ok(ok_response(lib_sound))
}

#[post("/library/upload?<title>", data = "<data>")]
pub async fn upload_mp3(
    title: String,
    data: Data<'_>,
    state: &State<Arc<AppState>>,
) -> Result<Json<ApiResponse<LibrarySound>>, (Status, Json<ApiResponse<()>>)> {
    let bytes = data
        .open(20.megabytes())
        .into_bytes()
        .await
        .map_err(|e| err_response(400, &format!("Failed to read upload: {}", e)))?
        .into_inner();

    if bytes.is_empty() {
        return Err(err_response(400, "Uploaded file is empty"));
    }

    let id = format!("custom_{}", timestamp());
    let mp3_path = data_dir().join("mp3").join(format!("{}.mp3", id));

    fs::write(&mp3_path, &bytes)
        .map_err(|e| err_response(500, &format!("Failed to save file: {}", e)))?;

    let saved = SavedSound {
        id: id.clone(),
        title,
        url: String::new(),
        mp3_url: String::new(),
        added_at: timestamp(),
        volume: 1.0,
    };

    {
        let mut data = state.data.lock().unwrap();
        data.sounds.push(saved.clone());
    }
    state.persist_sounds();

    let lib_sound = LibrarySound {
        id: saved.id.clone(),
        title: saved.title.clone(),
        url: saved.url.clone(),
        mp3: format!("/library/mp3/{}.mp3", saved.id),
        added_at: saved.added_at.clone(),
        volume: 1.0,
        hotkey_display: None,
    };

    Ok(ok_response(lib_sound))
}

#[get("/library")]
pub fn list_sounds(state: &State<Arc<AppState>>) -> Json<ApiResponse<Vec<LibrarySound>>> {
    let data = state.data.lock().unwrap();
    let sounds: Vec<LibrarySound> = data
        .sounds
        .iter()
        .map(|s| {
            let hotkey_display = data
                .bindings
                .iter()
                .find(|b| b.sound_id == s.id)
                .map(|b| b.display.clone());
            LibrarySound {
                id: s.id.clone(),
                title: s.title.clone(),
                url: s.url.clone(),
                mp3: format!("/library/mp3/{}.mp3", s.id),
                added_at: s.added_at.clone(),
                volume: s.volume,
                hotkey_display,
            }
        })
        .collect();
    ok_response(sounds)
}

#[delete("/library/<id>")]
pub fn delete_sound(
    id: &str,
    state: &State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (Status, Json<ApiResponse<()>>)> {
    {
        let mut data = state.data.lock().unwrap();
        let len_before = data.sounds.len();
        data.sounds.retain(|s| s.id != id);
        if data.sounds.len() == len_before {
            return Err(err_response(404, "Sound not found in library"));
        }
        let removed_bindings: Vec<String> =
            data.bindings.iter().filter(|b| b.sound_id == id).map(|b| b.id.clone()).collect();
        for bid in &removed_bindings {
            let _ = state.command_tx.send(MainCommand::UnregisterHotkey(bid.clone()));
        }
        data.bindings.retain(|b| b.sound_id != id);
    }
    state.persist_sounds();
    state.persist_bindings();

    let mp3_path = data_dir().join("mp3").join(format!("{}.mp3", id));
    let _ = fs::remove_file(mp3_path);

    Ok(ok_response(()))
}

#[post("/library/<id>/play")]
pub fn play_sound(
    id: &str,
    state: &State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (Status, Json<ApiResponse<()>>)> {
    let data = state.data.lock().unwrap();
    if !data.sounds.iter().any(|s| s.id == id) {
        return Err(err_response(404, "Sound not found in library"));
    }
    let _ = state.command_tx.send(MainCommand::PlaySound(id.to_string()));
    Ok(ok_response(()))
}

#[post("/play-url", data = "<req>")]
pub async fn play_url(
    req: Json<PlayUrlRequest>,
    state: &State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (Status, Json<ApiResponse<()>>)> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| err_response(500, &e.to_string()))?;
    let resp = client.get(&req.url).send().await
        .map_err(|e| err_response(500, &format!("Download failed: {}", e)))?;
    let bytes = resp.bytes().await
        .map_err(|e| err_response(500, &format!("Read failed: {}", e)))?;
    let title = req.title.clone().unwrap_or_default();
    let _ = state.command_tx.send(MainCommand::PlayAudio {
        data: bytes.to_vec(),
        title,
        volume: 1.0,
    });
    Ok(ok_response(()))
}

#[post("/stop")]
pub fn stop_playback(state: &State<Arc<AppState>>) -> Json<ApiResponse<()>> {
    let _ = state.command_tx.send(MainCommand::Stop);
    ok_response(())
}

#[get("/now-playing")]
pub fn now_playing(state: &State<Arc<AppState>>) -> Json<ApiResponse<Vec<NowPlayingInfo>>> {
    let playing = state.now_playing.lock().unwrap().clone();
    let info: Vec<NowPlayingInfo> = playing.iter().map(|s| {
        let (seq_str, id) = s.split_once(':').unwrap_or(("0", s));
        let seq: u64 = seq_str.parse().unwrap_or(0);
        NowPlayingInfo { seq, id: id.to_string() }
    }).collect();
    ok_response(info)
}

#[get("/local-ip")]
pub fn local_ip_endpoint(state: &State<Arc<AppState>>) -> Json<ApiResponse<String>> {
    ok_response(state.local_ip.clone())
}

#[post("/hotkey/bind", data = "<req>")]
pub fn bind_hotkey(
    req: Json<BindRequest>,
    state: &State<Arc<AppState>>,
) -> Result<Json<ApiResponse<HotkeyResponse>>, (Status, Json<ApiResponse<()>>)> {
    {
        let data = state.data.lock().unwrap();
        if data
            .bindings
            .iter()
            .any(|b| b.modifiers == req.modifiers && b.key == req.key)
        {
            return Err(err_response(409, "Hotkey already bound to another sound"));
        }
        if !data.sounds.iter().any(|s| s.id == req.sound_id) {
            return Err(err_response(404, "Sound not found in library"));
        }
    }

    let binding_id = format!("hk_{}", timestamp());

    let binding = HotkeyBinding {
        id: binding_id.clone(),
        sound_id: req.sound_id.clone(),
        sound_title: req.sound_title.clone(),
        modifiers: req.modifiers.clone(),
        key: req.key.clone(),
        display: req.display.clone(),
    };

    let _ = state.command_tx.send(MainCommand::RegisterHotkey {
        id: binding_id.clone(),
        modifiers: req.modifiers.clone(),
        key: req.key.clone(),
    });

    {
        let mut data = state.data.lock().unwrap();
        data.bindings.push(binding);
    }
    state.persist_bindings();

    Ok(ok_response(HotkeyResponse {
        id: binding_id,
        sound_id: req.sound_id.clone(),
        sound_title: req.sound_title.clone(),
        display: req.display.clone(),
    }))
}

#[get("/hotkey/list")]
pub fn list_hotkeys(state: &State<Arc<AppState>>) -> Json<ApiResponse<Vec<HotkeyResponse>>> {
    let data = state.data.lock().unwrap();
    let bindings: Vec<HotkeyResponse> = data
        .bindings
        .iter()
        .map(|b| HotkeyResponse {
            id: b.id.clone(),
            sound_id: b.sound_id.clone(),
            sound_title: b.sound_title.clone(),
            display: b.display.clone(),
        })
        .collect();
    ok_response(bindings)
}

#[delete("/hotkey/<id>")]
pub fn unbind_hotkey(
    id: &str,
    state: &State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (Status, Json<ApiResponse<()>>)> {
    {
        let mut data = state.data.lock().unwrap();
        let len_before = data.bindings.len();
        data.bindings.retain(|b| b.id != id);
        if data.bindings.len() == len_before {
            return Err(err_response(404, "Hotkey binding not found"));
        }
    }
    state.persist_bindings();
    let _ = state.command_tx.send(MainCommand::UnregisterHotkey(id.to_string()));
    Ok(ok_response(()))
}

#[put("/library/<id>/volume", data = "<req>")]
pub fn set_volume(
    id: &str,
    req: Json<VolumeRequest>,
    state: &State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (Status, Json<ApiResponse<()>>)> {
    let vol = req.volume.clamp(0.0, 1.0);
    {
        let mut data = state.data.lock().unwrap();
        let sound = data.sounds.iter_mut().find(|s| s.id == id);
        match sound {
            Some(s) => s.volume = vol,
            None => return Err(err_response(404, "Sound not found in library")),
        }
    }
    state.persist_sounds();
    Ok(ok_response(()))
}

#[get("/settings")]
pub fn get_settings(state: &State<Arc<AppState>>) -> Json<ApiResponse<SettingsResponse>> {
    let settings = state.settings.lock().unwrap();
    ok_response(SettingsResponse {
        global_volume: settings.global_volume,
        device_name: settings.device_name.clone(),
    })
}

#[put("/settings", data = "<req>")]
pub fn update_settings(
    req: Json<SettingsResponse>,
    state: &State<Arc<AppState>>,
) -> Json<ApiResponse<()>> {
    let vol = req.global_volume.clamp(0.0, 1.0);
    let old_device: Option<String>;
    let old_vol: f64;
    {
        let mut settings = state.settings.lock().unwrap();
        old_device = settings.device_name.clone();
        old_vol = settings.global_volume;
        settings.global_volume = vol;
        settings.device_name = req.device_name.clone();
        settings.save();
    }
    let new_device = req.device_name.clone();
    if new_device != old_device {
        let _ = state.command_tx.send(MainCommand::SetDevice(new_device));
    }
    if vol != old_vol {
        let _ = state.command_tx.send(MainCommand::SetGlobalVolume(vol));
    }
    ok_response(())
}

#[get("/devices")]
pub fn list_devices(state: &State<Arc<AppState>>) -> Json<ApiResponse<Vec<DeviceInfo>>> {
    let current_device = {
        let settings = state.settings.lock().unwrap();
        settings.device_name.clone()
    };
    let mut devices: Vec<DeviceInfo> = Vec::new();
    let host = rodio::cpal::default_host();
    if let Ok(output_devices) = host.output_devices() {
        for device in output_devices {
            if let Ok(desc) = device.description() {
                let name = desc.to_string();
                devices.push(DeviceInfo {
                    is_current: Some(name.clone()) == current_device
                        || (current_device.is_none()),
                    name,
                });
            }
        }
    }
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    ok_response(devices)
}

#[get("/library/mp3/<file..>", rank = 2)]
pub async fn serve_mp3(file: PathBuf) -> Option<(ContentType, Vec<u8>)> {
    let base = data_dir().join("mp3");
    let path = base.join(file);
    let bytes = std::fs::read(&path).ok()?;
    Some((ContentType::MP3, bytes))
}

#[derive(rust_embed::RustEmbed)]
#[folder = "static/"]
pub struct StaticAssets;

#[get("/<_path..>")]
pub fn serve_static(_path: std::path::PathBuf) -> Option<(ContentType, Vec<u8>)> {
    let filename = if _path.to_str().unwrap_or("").is_empty() {
        "index.html"
    } else {
        _path.to_str().unwrap_or("index.html")
    };
    let asset = StaticAssets::get(filename)?;
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let content_type = match ext {
        "html" => ContentType::HTML,
        "css" => ContentType::CSS,
        "js" => ContentType::JavaScript,
        "ico" => ContentType::Icon,
        "png" => ContentType::PNG,
        "svg" => ContentType::SVG,
        "ttf" => ContentType::TTF,
        _ => ContentType::Binary,
    };
    Some((content_type, asset.data.to_vec()))
}
