#![windows_subsystem = "windows"]

use rdev::{Event, EventType, Key, listen};
use rocket::http::Method;
use rocket::routes;
use rocket_cors::{AllowedHeaders, AllowedOrigins, CorsOptions};
use rodio::DeviceSinkBuilder;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use winit::event_loop::{ControlFlow, EventLoop};

mod library;
mod myinstants;
mod settings;

use library::{AppState, MainCommand};

fn create_icon() -> Icon {
    if let Some(asset) = library::StaticAssets::get("icons/logo.ico") {
        let data = asset.data.as_ref();
        if let Ok(img) = image::load_from_memory(data) {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            return Icon::from_rgba(rgba.into_raw(), w, h).unwrap_or_else(|_| fallback_icon());
        }
    }
    fallback_icon()
}

fn fallback_icon() -> Icon {
    let size = 32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let cx = size as i32 / 2;
            let cy = size as i32 / 2;
            let dx = x as i32 - cx;
            let dy = y as i32 - cy;
            let dist = ((dx * dx + dy * dy) as f64).sqrt();
            if dist < 12.0 {
                rgba.extend_from_slice(&[0xE9, 0x45, 0x60, 0xFF]);
            } else if dist < 15.0 {
                rgba.extend_from_slice(&[0x0F, 0x34, 0x60, 0xFF]);
            } else {
                rgba.extend_from_slice(&[0x1A, 0x1A, 0x2E, 0x00]);
            }
        }
    }
    Icon::from_rgba(rgba, size, size).expect("valid icon")
}

fn create_sink(device_name: &Option<String>) -> Option<rodio::MixerDeviceSink> {
    if let Some(name) = device_name {
        let host = rodio::cpal::default_host();
        if let Ok(output_devices) = host.output_devices() {
            for device in output_devices {
                if let Ok(dname) = device.description() {
                    if dname.to_string() == *name {
                        if let Ok(builder) = DeviceSinkBuilder::from_device(device) {
                            return builder.open_stream().ok();
                        }
                    }
                }
            }
        }
    }
    DeviceSinkBuilder::open_default_sink().ok()
}

struct CurrentPlayback {
    player: Arc<rodio::Player>,
    sound_volume: f64,
    sound_id: String,
    play_id: String,
}

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let (command_tx, command_rx) = mpsc::channel::<MainCommand>();

    let app_state = Arc::new(AppState::load(command_tx));
    let rocket_state = app_state.clone();

    let initial_device = {
        let settings = app_state.settings.lock().unwrap();
        settings.device_name.clone()
    };
    let mut sink = create_sink(&initial_device);
    let mut active_playbacks: Vec<CurrentPlayback> = Vec::new();

    let hk_state = app_state.clone();
    thread::spawn(move || {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut meta = false;
        let _ = listen(move |event: Event| match event.event_type {
            EventType::KeyPress(key) => match key {
                Key::ControlLeft | Key::ControlRight => ctrl = true,
                Key::ShiftLeft | Key::ShiftRight => shift = true,
                Key::Alt | Key::AltGr => alt = true,
                Key::MetaLeft | Key::MetaRight => meta = true,
                _ => {
                    let data = hk_state.data.lock().unwrap();
                    for binding in &data.bindings {
                        let mods_match = match binding.modifiers.as_str() {
                            "" => !ctrl && !shift && !alt && !meta,
                            "Ctrl" => ctrl && !shift && !alt && !meta,
                            "Shift" => !ctrl && shift && !alt && !meta,
                            "Alt" => !ctrl && !shift && alt && !meta,
                            "Meta" => !ctrl && !shift && !alt && meta,
                            "Ctrl+Shift" => ctrl && shift && !alt && !meta,
                            "Ctrl+Alt" => ctrl && !shift && alt && !meta,
                            "Shift+Alt" => !ctrl && shift && alt && !meta,
                            "Ctrl+Shift+Alt" => ctrl && shift && alt && !meta,
                            _ => false,
                        };
                        if mods_match && binding.key == format!("{:?}", key) {
                            let _ = hk_state
                                .command_tx
                                .send(MainCommand::PlaySound(binding.sound_id.clone()));
                        }
                    }
                }
            },
            EventType::KeyRelease(key) => match key {
                Key::ControlLeft | Key::ControlRight => ctrl = false,
                Key::ShiftLeft | Key::ShiftRight => shift = false,
                Key::Alt | Key::AltGr => alt = false,
                Key::MetaLeft | Key::MetaRight => meta = false,
                _ => {}
            },
            _ => {}
        });
    });

    thread::spawn(move || {
        let cors = CorsOptions::default()
            .allowed_origins(AllowedOrigins::all()) // Be specific in production!
            .allowed_methods(
                vec![
                    Method::Get,
                    Method::Post,
                    Method::Put,
                    Method::Delete,
                    Method::Options,
                ]
                .into_iter()
                .map(From::from)
                .collect(),
            )
            .allowed_headers(AllowedHeaders::all())
            .allow_credentials(true)
            .to_cors()
            .expect("CORS configuration failed");
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async {
            rocket::build()
                .configure(
                    rocket::Config::figment()
                        .merge(("address", "0.0.0.0"))
                        .merge(("port", 6677u16)),
                )
                .manage(rocket_state)
                .mount(
                    "/api",
                    routes![
                        myinstants::index,
                        myinstants::recent,
                        myinstants::trending,
                        myinstants::search,
                        myinstants::best,
                        myinstants::detail,
                        myinstants::favorites,
                        myinstants::uploaded,
                        library::save_sound,
                        library::upload_mp3,
                        library::list_sounds,
                        library::delete_sound,
                        library::play_sound,
                        library::bind_hotkey,
                        library::list_hotkeys,
                        library::unbind_hotkey,
                        library::set_volume,
                        library::get_settings,
                        library::update_settings,
                        library::list_devices,
                        library::play_url,
                        library::stop_playback,
                        library::now_playing,
                        library::local_ip_endpoint,
                    ],
                )
                .attach(cors)
                .mount("/", routes![library::serve_mp3, library::serve_static])
                .launch()
                .await
                .ok();
        });
    });

    let tray_menu = {
        let menu = tray_icon::menu::Menu::new();
        let quit_item = tray_icon::menu::MenuItem::new("Quit", true, None);
        let _ = menu.append(&quit_item);
        menu
    };

    let _tray = TrayIconBuilder::new()
        .with_tooltip("Popasound")
        .with_icon(create_icon())
        .with_menu(Box::new(tray_menu))
        .with_menu_on_left_click(false)
        .build()
        .expect("failed to create tray icon");

    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop
        .run(move |_event, _| {
            if let Ok(event) = TrayIconEvent::receiver().try_recv() {
                match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                    | TrayIconEvent::DoubleClick { .. } => {
                        let _ = open::that("http://localhost:6677");
                    }
                    _ => {}
                }
            }

            if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                std::process::exit(0);
            }

            if let Ok(cmd) = command_rx.try_recv() {
                match cmd {
                    MainCommand::RegisterHotkey { .. } => {}
                    MainCommand::UnregisterHotkey(..) => {}
                    MainCommand::PlaySound(sound_id) => {
                        if let Some(ref mixer_sink) = sink {
                            if let Some(pos) =
                                active_playbacks.iter().position(|p| p.sound_id == sound_id)
                            {
                                let removed = active_playbacks.remove(pos);
                                removed.player.stop();
                                app_state
                                    .now_playing
                                    .lock()
                                    .unwrap()
                                    .retain(|pid| pid != &removed.play_id);
                            }
                            let data = app_state.data.lock().unwrap();
                            let settings = app_state.settings.lock().unwrap();
                            let sound_vol = data
                                .sounds
                                .iter()
                                .find(|s| s.id == sound_id)
                                .map(|s| s.volume)
                                .unwrap_or(1.0);
                            let mp3_path = library::data_dir().join("mp3")
                                .join(format!("{}.mp3", sound_id));
                            if let Ok(file) = File::open(&mp3_path) {
                                let reader = BufReader::new(file);
                                if let Ok(source) = rodio::Decoder::new(reader) {
                                    let vol =
                                        (settings.global_volume * sound_vol).clamp(0.0, 1.0) as f32;
                                    let new_player =
                                        Arc::new(rodio::Player::connect_new(&mixer_sink.mixer()));
                                    new_player.set_volume(vol);
                                    new_player.append(source);
                                    let sid = sound_id.clone();
                                    let seq = app_state.play_gen.fetch_add(1, Ordering::SeqCst);
                                    let play_id = format!("{}:{}", seq, sid);
                                    app_state.now_playing.lock().unwrap().push(play_id.clone());
                                    let np = app_state.clone();
                                    let p = Arc::clone(&new_player);
                                    let pid = play_id.clone();
                                    thread::spawn(move || {
                                        p.sleep_until_end();
                                        let mut guard = np.now_playing.lock().unwrap();
                                        guard.retain(|x| x != &pid);
                                    });
                                    active_playbacks.push(CurrentPlayback {
                                        player: new_player,
                                        sound_volume: sound_vol,
                                        sound_id,
                                        play_id,
                                    });
                                }
                            }
                        }
                    }
                    MainCommand::PlayAudio {
                        data,
                        volume,
                        title,
                    } => {
                        if let Some(ref mixer_sink) = sink {
                            if let Some(pos) =
                                active_playbacks.iter().position(|p| p.sound_id == title)
                            {
                                let removed = active_playbacks.remove(pos);
                                removed.player.stop();
                                app_state
                                    .now_playing
                                    .lock()
                                    .unwrap()
                                    .retain(|pid| pid != &removed.play_id);
                            }
                            let settings = app_state.settings.lock().unwrap();
                            let cursor = Cursor::new(data);
                            if let Ok(source) = rodio::Decoder::new(cursor) {
                                let vol = (settings.global_volume * volume).clamp(0.0, 1.0) as f32;
                                let new_player =
                                    Arc::new(rodio::Player::connect_new(&mixer_sink.mixer()));
                                new_player.set_volume(vol);
                                new_player.append(source);
                                let tid = title.clone();
                                let seq = app_state.play_gen.fetch_add(1, Ordering::SeqCst);
                                let play_id = format!("{}:{}", seq, tid);
                                app_state.now_playing.lock().unwrap().push(play_id.clone());
                                let np = app_state.clone();
                                let p = Arc::clone(&new_player);
                                let pid = play_id.clone();
                                thread::spawn(move || {
                                    p.sleep_until_end();
                                    let mut guard = np.now_playing.lock().unwrap();
                                    guard.retain(|x| x != &pid);
                                });
                                active_playbacks.push(CurrentPlayback {
                                    player: new_player,
                                    sound_volume: volume,
                                    sound_id: title,
                                    play_id,
                                });
                            }
                        }
                    }
                    MainCommand::Stop => {
                        for pb in active_playbacks.drain(..) {
                            pb.player.stop();
                        }
                        app_state.now_playing.lock().unwrap().clear();
                    }
                    MainCommand::SetDevice(device_name) => {
                        for pb in active_playbacks.drain(..) {
                            pb.player.stop();
                        }
                        app_state.now_playing.lock().unwrap().clear();
                        sink = create_sink(&device_name);
                    }
                    MainCommand::SetGlobalVolume(vol) => {
                        for playback in &active_playbacks {
                            let v = (vol * playback.sound_volume).clamp(0.0, 1.0) as f32;
                            playback.player.set_volume(v);
                        }
                    }
                }
            }
        })
        .unwrap();
}
