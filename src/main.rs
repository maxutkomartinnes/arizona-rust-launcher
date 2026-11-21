#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use arizona_rust_launcher::{
    autostart, drivers, identity, install, launch,
    model::{GameId, Manifest, ModCatalog, Notification, Project, Server, Servers},
    mods,
    net::Net,
    plan::{self, Options, Plan},
    safety,
    settings::Settings,
    theme, updater,
};
use eframe::egui;
use fs2::FileExt;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

#[cfg(windows)]
fn show_existing_window() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindowAsync, SW_RESTORE, SW_SHOW,
    };

    let title: Vec<u16> = "Arizona Rust Launcher\0".encode_utf16().collect();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // The first instance can hold the lock before its window is created.
        let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if !window.is_null() {
            unsafe {
                ShowWindowAsync(window, SW_SHOW);
                ShowWindowAsync(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(windows))]
fn show_existing_window() -> bool {
    false
}

#[cfg(windows)]
fn close_launcher_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};

    let title: Vec<u16> = "Arizona Rust Launcher\0".encode_utf16().collect();
    let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if !window.is_null() {
        unsafe {
            PostMessageW(window, WM_CLOSE, 0, 0);
        }
    }
}

#[cfg(not(windows))]
fn close_launcher_window() {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Verify,
    Install,
    Repair,
    DeepRepair,
    Play,
}
enum Event {
    Api(Result<(Vec<Project>, Servers), String>),
    Mods(Result<ModCatalog, String>),
    Notifications(Result<Vec<Notification>, String>),
    Preview(Result<Plan, String>, Action),
    Progress(u64, u64, String),
    Done(Result<String, String>),
    GameStarted(u32),
    GameEnded(bool),
    TrayShow,
    Update(Result<updater::Release, String>),
    Theme(GameId, Result<HashMap<String, PathBuf>, String>),
}
struct Launcher {
    settings: Settings,
    game: GameId,
    root_text: String,
    nick: String,
    selected_server_number: Option<u32>,
    server_page: usize,
    tab: usize,
    projects: Vec<Project>,
    servers: Servers,
    catalog: Option<ModCatalog>,
    notifications: Vec<Notification>,
    last_poll: Instant,
    last_api_poll: Instant,
    preview: Option<(Plan, Action)>,
    busy: bool,
    running_game: bool,
    window_hidden: bool,
    tray_hidden: bool,
    quit_requested: Arc<AtomicBool>,
    tray: Option<TrayIcon>,
    cancel: Arc<AtomicBool>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    message: String,
    progress: (u64, u64, String),
    update_url: Option<String>,
    test_password: String,
    theme_textures: HashMap<String, egui::TextureHandle>,
}
fn game_root(settings: &Settings, game: GameId) -> PathBuf {
    let s = settings.game(game.id());
    if !s.game_path.is_empty() {
        return PathBuf::from(s.game_path);
    }
    dirs::home_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("Games")
        .join(game.id())
}
fn game_label(game: GameId) -> &'static str {
    match game {
        GameId::Arizona => "Arizona",
        GameId::ArizonaStaging => "Arizona · тест",
        GameId::Rodina => "Rodina",
        GameId::RodinaStaging => "Rodina · тест",
        GameId::ArizonaV => "Arizona V",
        GameId::ArizonaVStaging => "Arizona V · тест",
        GameId::Village => "Village",
        GameId::VillageStaging => "Village · тест",
        GameId::Trilogy => "Trilogy",
    }
}
fn external_game_url(game: GameId) -> Option<&'static str> {
    match game {
        GameId::Trilogy => Some("https://vk.ru/wall-182234941_2787962"),
        GameId::Village => Some("https://vk.ru/wall-89556237_271418"),
        _ => None,
    }
}
fn server_position(servers: &[Server], selected: Option<u32>) -> Option<usize> {
    if servers.is_empty() {
        return None;
    }
    Some(
        selected
            .and_then(|id| servers.iter().position(|server| server.number == id))
            .unwrap_or(0),
    )
}
fn cover_uv(image_size: egui::Vec2, target_size: egui::Vec2) -> egui::Rect {
    let image_ratio = image_size.x / image_size.y.max(1.0);
    let target_ratio = target_size.x / target_size.y.max(1.0);
    if image_ratio > target_ratio {
        let visible = target_ratio / image_ratio;
        egui::Rect::from_min_max(
            egui::pos2((1.0 - visible) * 0.5, 0.0),
            egui::pos2((1.0 + visible) * 0.5, 1.0),
        )
    } else {
        let visible = image_ratio / target_ratio;
        egui::Rect::from_min_max(
            egui::pos2(0.0, (1.0 - visible) * 0.5),
            egui::pos2(1.0, (1.0 + visible) * 0.5),
        )
    }
}
impl Launcher {
    fn new(ctx: egui::Context) -> Self {
        let settings = Settings::load().unwrap_or_default();
        let game = GameId::from_id(&settings.last_selected_game_id).unwrap_or(GameId::Arizona);
        let root_text = game_root(&settings, game).to_string_lossy().to_string();
        let nick = settings.game(game.id()).user_name;
        let selected_server_number = settings.game(game.id()).last_selected_server_id;
        let (tx, rx) = mpsc::channel();
        let quit_requested = Arc::new(AtomicBool::new(false));
        let tray = create_tray(&tx, &ctx, quit_requested.clone()).ok();
        let mut app = Self {
            settings,
            game,
            root_text,
            nick,
            selected_server_number,
            server_page: 0,
            tab: 0,
            projects: serde_json::from_str(include_str!("../fixtures/projects.json"))
                .unwrap_or_default(),
            servers: serde_json::from_str(include_str!("../fixtures/servers.json"))
                .unwrap_or_default(),
            catalog: None,
            notifications: Vec::new(),
            last_poll: Instant::now(),
            last_api_poll: Instant::now(),
            preview: None,
            busy: false,
            running_game: false,
            window_hidden: false,
            tray_hidden: false,
            quit_requested,
            tray,
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            message: String::new(),
            progress: (0, 0, String::new()),
            update_url: None,
            test_password: String::new(),
            theme_textures: HashMap::new(),
        };
        app.sync_server_selection();
        app.refresh_api();
        app.refresh_notifications();
        app.refresh_theme();
        app
    }
    fn select_game(&mut self, g: GameId) {
        self.game = g;
        self.settings.last_selected_game_id = g.id().into();
        self.root_text = game_root(&self.settings, g).to_string_lossy().into();
        self.nick = self.settings.game(g.id()).user_name;
        self.selected_server_number = self.settings.game(g.id()).last_selected_server_id;
        self.server_page = 0;
        self.sync_server_selection();
        self.catalog = None;
        self.preview = None;
        self.theme_textures.clear();
        let _ = self.settings.save();
        self.refresh_theme();
    }
    fn save_config(&mut self) {
        let g = self.settings.game_mut(self.game.id());
        g.game_path = self.root_text.clone();
        g.user_name = self.nick.clone();
        match self.settings.save() {
            Ok(()) => self.message = "Настройки сохранены".into(),
            Err(e) => self.message = e.to_string(),
        }
    }
    fn sync_server_selection(&mut self) {
        let Some(servers) = self.servers.get(self.game.id()) else {
            return;
        };
        if servers.is_empty() {
            self.selected_server_number = None;
            self.server_page = 0;
            return;
        }
        let position = server_position(servers, self.selected_server_number).unwrap_or(0);
        self.selected_server_number = Some(servers[position].number);
        self.server_page = position / 8;
    }
    fn refresh_api(&mut self) {
        self.last_api_poll = Instant::now();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = Net::new()
                .and_then(|net| Ok((net.projects()?, net.servers()?)))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Api(result));
        });
    }
    fn refresh_theme(&self) {
        let tx = self.tx.clone();
        let game = self.game;
        thread::spawn(move || {
            let result = Net::new()
                .and_then(|net| theme::sync(&net, game))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Theme(game, result));
        });
    }
    fn refresh_mods(&mut self) {
        let tx = self.tx.clone();
        let g = self.game;
        thread::spawn(move || {
            let r = Net::new()
                .and_then(|net| net.mod_catalog(g))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Mods(r));
        });
    }
    fn refresh_notifications(&mut self) {
        self.last_poll = Instant::now();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = Net::new()
                .and_then(|net| {
                    let mut n = net.notifications(None)?;
                    if let Ok(id) = identity::machine_fingerprint() {
                        if let Ok(mut own) = net.notifications(Some(&id)) {
                            n.append(&mut own);
                        }
                    }
                    n.sort_by_key(|n| n.id);
                    Ok(n)
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Notifications(result));
        });
    }
    fn install_drivers(&mut self) {
        if self.busy {
            return;
        }
        let tx = self.tx.clone();
        let root = PathBuf::from(&self.root_text);
        self.busy = true;
        thread::spawn(move || {
            let result = drivers::install(&root, |name| {
                let _ = tx.send(Event::Progress(0, 0, name.into()));
            })
            .map(|_| "Установка драйверов завершена".into())
            .map_err(|e| e.to_string());
            let _ = tx.send(Event::Done(result));
        });
    }
    fn prepare(&mut self, action: Action) {
        if self.busy || self.running_game {
            return;
        }
        let root = PathBuf::from(self.root_text.trim());
        if let Err(error) = safety::plan_root(&root) {
            self.message = format!("Выберите отдельную папку игры кнопкой «Выбрать папку». Текущий путь не подходит: {error}");
            return;
        }
        if action != Action::Verify {
            if let Err(error) = safety::root(&root) {
                self.message = format!("Ошибка доступа к папке игры: {error:#}");
                return;
            }
        }
        if action == Action::Play && self.current_server().is_none() {
            self.message = "Выберите сервер перед запуском".into();
            return;
        }
        self.save_config();
        self.busy = true;
        self.preview = None;
        self.message = "Получение манифеста и проверка файлов...".into();
        self.cancel.store(false, Ordering::Relaxed);
        let tx = self.tx.clone();
        let root = PathBuf::from(&self.root_text);
        let game = self.game;
        let future = self
            .settings
            .game(game.id())
            .options
            .get("newGraphics")
            .copied()
            .unwrap_or(false);
        thread::spawn(move || {
            let r = Net::new()
                .and_then(|net| {
                    let m = net.manifest(game)?;
                    plan::build(
                        &m,
                        &root,
                        Options {
                            deep: action == Action::DeepRepair,
                            repair: matches!(action, Action::Repair | Action::DeepRepair),
                            future_graphics: future,
                        },
                    )
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Preview(r, action));
        });
    }
    fn current_server(&self) -> Option<Server> {
        self.servers
            .get(self.game.id())
            .and_then(|v| server_position(v, self.selected_server_number).and_then(|i| v.get(i)))
            .cloned()
    }
    fn execute(&mut self) {
        let Some((plan, action)) = self.preview.take() else {
            return;
        };
        let root = PathBuf::from(&self.root_text);
        let game = self.game;
        let settings = self.settings.game(game.id());
        let server = self.current_server();
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        let password = self.test_password.clone();
        self.busy = true;
        self.message = "Загрузка файлов...".into();
        thread::spawn(move || {
            let r = (|| -> anyhow::Result<String> {
                let net = Net::new()?;
                install::apply(
                    &net,
                    &root,
                    &plan,
                    game.cdns(),
                    "game",
                    &cancel,
                    true,
                    |done, total, file| {
                        let _ = tx.send(Event::Progress(done, total, file.into()));
                    },
                )?;
                if action == Action::Play {
                    let server = server.ok_or_else(|| anyhow::anyhow!("Выберите сервер"))?;
                    let indexes = net.cdn_indexes();
                    let mut child = launch::launch(
                        game,
                        &root,
                        &settings,
                        &server,
                        &indexes,
                        if password.is_empty() {
                            None
                        } else {
                            Some(&password)
                        },
                    )?;
                    let pid = child.id();
                    let _ = tx.send(Event::GameStarted(pid));
                    let watcher = tx.clone();
                    thread::spawn(move || {
                        let started = Instant::now();
                        let _ = child.wait();
                        let _ = watcher.send(Event::GameEnded(
                            started.elapsed() < Duration::from_secs(300),
                        ));
                    });
                    Ok(format!("Игра запущена, PID {pid}"))
                } else {
                    Ok("Файлы готовы".into())
                }
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Done(r));
        });
    }
    fn install_mod(&mut self, id: u64) {
        if self.busy {
            return;
        }
        let Some(catalog) = self.catalog.clone() else {
            return;
        };
        self.save_config();
        let game = self.game;
        let root = PathBuf::from(&self.root_text);
        let mut settings = self.settings.clone();
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        self.busy = true;
        self.cancel.store(false, Ordering::Relaxed);
        thread::spawn(move || {
            let r = Net::new()
                .and_then(|net| {
                    mods::install_mod(
                        &net,
                        game,
                        id,
                        &catalog,
                        &root,
                        &mut settings,
                        &cancel,
                        |p| {
                            let _ = tx.send(Event::Progress(0, 0, p));
                        },
                    )
                })
                .map(|_| format!("Мод {id} установлен"))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Done(r));
        });
    }
    fn uninstall_mod(&mut self, id: u64) {
        if self.busy {
            return;
        }
        let root = PathBuf::from(&self.root_text);
        let game = self.game;
        let mut settings = self.settings.clone();
        let tx = self.tx.clone();
        self.busy = true;
        thread::spawn(move || {
            let r = mods::uninstall_mod(game, id, &root, &mut settings)
                .map(|_| format!("Мод {id} удалён"))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Done(r));
        });
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                Event::Api(Ok((p, s))) => {
                    self.projects = p;
                    self.servers = s;
                    self.sync_server_selection();
                }
                Event::Api(Err(e)) => {
                    self.message = format!("API недоступен, показаны сохранённые данные: {e}");
                    self.last_api_poll = Instant::now() - Duration::from_secs(30);
                }
                Event::Mods(Ok(c)) => self.catalog = Some(c),
                Event::Mods(Err(e)) => self.message = e,
                Event::Notifications(Ok(n)) => {
                    if let Some(last) = n.last() {
                        if last.id > self.settings.last_notification_id {
                            self.settings.last_notification_id = last.id;
                            let _ = self.settings.save();
                        }
                    }
                    self.notifications = n;
                }
                Event::Notifications(Err(_)) => {}
                Event::Preview(Ok(p), a) => {
                    self.busy = false;
                    self.message = format!(
                        "План: {} загрузок, {} удалений, {:.2} ГиБ",
                        p.download.len(),
                        p.delete.len(),
                        p.total_bytes as f64 / 1073741824.0
                    );
                    self.preview = Some((p, a));
                }
                Event::Preview(Err(e), _) => {
                    self.busy = false;
                    self.message = e;
                }
                Event::Progress(d, t, f) => self.progress = (d, t, f),
                Event::Done(r) => {
                    self.busy = false;
                    self.message = r.unwrap_or_else(|e| format!("Ошибка: {e}"));
                    self.settings = Settings::load().unwrap_or_else(|_| self.settings.clone());
                    self.progress = (0, 0, String::new());
                }
                Event::GameStarted(pid) => {
                    self.running_game = true;
                    self.message = format!("Игра запущена, PID {pid}");
                }
                Event::GameEnded(early) => {
                    self.running_game = false;
                    self.message = if early {
                        "Игра быстро завершилась. Проверьте crashlog и при необходимости выполните починку.".into()
                    } else {
                        "Игра завершилась".into()
                    };
                }
                Event::TrayShow => {
                    self.tray_hidden = false;
                    self.window_hidden = self.running_game;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Event::Update(Ok(r)) => {
                    self.message = format!("Последний релиз Electron: {}", r.tag_name);
                    self.update_url = Some(r.html_url);
                }
                Event::Update(Err(e)) => self.message = e,
                Event::Theme(game, Ok(paths)) if game == self.game => {
                    for (name, path) in paths {
                        if !["logo", "character", "bg"].contains(&name.as_str()) {
                            continue;
                        }
                        if let Ok((rgba, size)) = theme::rgba(&path) {
                            let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                            let texture = ctx.load_texture(
                                format!("{}-{name}", self.game.id()),
                                image,
                                Default::default(),
                            );
                            self.theme_textures.insert(name, texture);
                        }
                    }
                }
                Event::Theme(_, _) => {}
            }
        }
    }
    fn ui_home(&mut self, ui: &mut egui::Ui) {
        let (banner, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 130.0),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        if let Some(bg) = self.theme_textures.get("bg") {
            painter.image(
                bg.id(),
                banner,
                cover_uv(bg.size_vec2(), banner.size()),
                egui::Color32::WHITE,
            );
        } else {
            painter.rect_filled(banner, 8.0, egui::Color32::from_rgb(28, 42, 66));
        }
        if let Some(tex) = self.theme_textures.get("logo") {
            let size = tex.size_vec2();
            let height = (160.0 * size.y / size.x.max(1.0)).min(90.0);
            let rect = egui::Rect::from_min_size(
                banner.min + egui::vec2(18.0, 18.0),
                egui::vec2(160.0, height),
            );
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            painter.text(
                banner.min + egui::vec2(20.0, 28.0),
                egui::Align2::LEFT_TOP,
                format!("Arizona Games · {}", self.game.id()),
                egui::FontId::proportional(27.0),
                egui::Color32::WHITE,
            );
        }
        if let Some(tex) = self.theme_textures.get("character") {
            let size = tex.size_vec2();
            let width = 120.0 * size.x / size.y.max(1.0);
            let rect = egui::Rect::from_min_size(
                egui::pos2(banner.max.x - width - 16.0, banner.min.y + 5.0),
                egui::vec2(width, 120.0),
            );
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        ui.horizontal(|ui| {
            ui.label("Папка игры");
            ui.add(egui::TextEdit::singleline(&mut self.root_text).desired_width(460.0));
            if ui.button("Выбрать папку...").clicked() {
                let current = PathBuf::from(&self.root_text);
                let start = if current.is_dir() {
                    Some(current.as_path())
                } else {
                    current.parent().filter(|p| p.is_dir())
                };
                let mut dialog = rfd::FileDialog::new();
                if let Some(start) = start {
                    dialog = dialog.set_directory(start);
                }
                if let Some(folder) = dialog.pick_folder() {
                    self.root_text = folder.to_string_lossy().into_owned();
                    self.save_config();
                }
            }
            if ui.button("Папка в профиле").clicked() {
                self.root_text = dirs::home_dir()
                    .unwrap_or_else(|| std::env::temp_dir())
                    .join("Games")
                    .join(self.game.id())
                    .to_string_lossy()
                    .into_owned();
                self.save_config();
            }
            if ui.button("Открыть").clicked() {
                let _ = open::that(&self.root_text);
            }
        });
        if let Err(e) = safety::plan_root(PathBuf::from(self.root_text.trim()).as_path()) {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("Выберите отдельную папку клиента, например C:\\Games\\Arizona. {e}"),
            );
        }
        ui.horizontal(|ui| {
            ui.label("Ник");
            ui.text_edit_singleline(&mut self.nick);
            if ui.button("Сохранить").clicked() {
                self.save_config();
            }
        });
        if self
            .settings
            .game(self.game.id())
            .options
            .get("testBranch")
            .copied()
            .unwrap_or(false)
        {
            ui.horizontal(|ui| {
                ui.label("Пароль тестовой ветки");
                ui.add(egui::TextEdit::singleline(&mut self.test_password).password(true));
            });
        }
        ui.separator();
        ui.heading("Серверы");
        if let Some(server) = self.current_server() {
            ui.colored_label(
                egui::Color32::LIGHT_BLUE,
                format!("Выбран: №{} · {}", server.number, server.name),
            );
        }
        let servers = self
            .servers
            .get(self.game.id())
            .cloned()
            .unwrap_or_default();
        const PAGE_SIZE: usize = 8;
        let pages = servers.len().div_ceil(PAGE_SIZE).max(1);
        self.server_page = self.server_page.min(pages - 1);
        ui.horizontal(|ui| {
            ui.label(format!("Всего: {}", servers.len()));
            if ui
                .add_enabled(self.server_page > 0, egui::Button::new("◀ Назад"))
                .clicked()
            {
                self.server_page -= 1;
            }
            ui.label(format!("Страница {} / {}", self.server_page + 1, pages));
            if ui
                .add_enabled(self.server_page + 1 < pages, egui::Button::new("Далее ▶"))
                .clicked()
            {
                self.server_page += 1;
            }
            ui.weak("Колесо мыши над списком тоже листает страницы");
        });
        let start = self.server_page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(servers.len());
        let list = ui.vertical(|ui| {
            ui.set_min_height(190.0);
            for s in &servers[start..end] {
                let label = format!(
                    "{:>2} · {:<18}  {}/{}",
                    s.number, s.name, s.online, s.maxplayers
                );
                if ui
                    .selectable_label(self.selected_server_number == Some(s.number), label)
                    .clicked()
                {
                    self.selected_server_number = Some(s.number);
                    self.settings
                        .game_mut(self.game.id())
                        .last_selected_server_id = Some(s.number);
                    let _ = self.settings.save();
                    self.message = format!("Выбран сервер {} · {}", s.number, s.name);
                }
            }
        });
        let wheel = ui.ctx().input(|i| i.raw_scroll_delta.y);
        if list
            .response
            .rect
            .contains(ui.ctx().pointer_hover_pos().unwrap_or_default())
            && wheel.abs() > 1.0
        {
            if wheel < 0.0 {
                self.server_page = (self.server_page + 1).min(pages - 1);
            } else {
                self.server_page = self.server_page.saturating_sub(1);
            }
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Играть"))
                .clicked()
            {
                self.prepare(Action::Play)
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Установить / обновить"))
                .clicked()
            {
                self.prepare(Action::Install)
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Проверить"))
                .clicked()
            {
                self.prepare(Action::Verify)
            }
            if self.busy && ui.button("Отмена").clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        });
        ui.separator();
        ui.heading("Новости");
        if let Some(project) = self.projects.iter().find(|p| p.game_id == self.game.id()) {
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for n in &project.news {
                        ui.horizontal(|ui| {
                            if ui.link(&n.title).clicked() {
                                if n.url.starts_with("https://") {
                                    let _ = open::that(&n.url);
                                }
                            }
                            ui.label(&n.date);
                        });
                        ui.label(&n.description);
                        ui.separator();
                    }
                });
        }
    }
    fn ui_mods(&mut self, ui: &mut egui::Ui) {
        ui.heading("Модификации");
        if ui.button("Обновить каталог").clicked() {
            self.refresh_mods()
        }
        if self.catalog.is_none() {
            ui.label("Каталог ещё не загружен");
            return;
        }
        let catalog = self.catalog.as_ref().unwrap().clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for m in &catalog.mods {
                if m.visible == Some(false) {
                    continue;
                }
                let installed = self
                    .settings
                    .installed_mods
                    .get(self.game.id())
                    .is_some_and(|x| x.iter().any(|x| x.id == m.id));
                ui.horizontal(|ui| {
                    ui.label(format!("{} — {} ({})", m.title, m.author, m.version));
                    if installed {
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Удалить"))
                            .clicked()
                        {
                            self.uninstall_mod(m.id)
                        }
                    } else if ui
                        .add_enabled(!self.busy, egui::Button::new("Установить"))
                        .clicked()
                    {
                        self.install_mod(m.id)
                    }
                });
                ui.small(&m.short_description);
                ui.separator();
            }
        });
    }
    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Настройки игры");
        let cfg = self.settings.game_mut(self.game.id());
        ui.horizontal(|ui| {
            ui.label("Память, МиБ");
            ui.add(
                egui::DragValue::new(&mut cfg.memory)
                    .range(256..=16384)
                    .speed(64),
            );
        });
        for (key, label) in [
            ("wideScreen", "Широкий экран"),
            ("autoLogin", "Автовход"),
            ("preload", "Предзагрузка"),
            ("windowed", "Оконный режим"),
            ("newGraphics", "Новая графика"),
            ("shitPc", "Слабый ПК"),
            ("cefDirtyRects", "CEF dirty rects"),
            ("authCef", "CEF auth"),
            ("grass", "Трава"),
            ("oldResolution", "16 bpp"),
            ("hdrResolution", "HDR"),
            ("testBranch", "Тестовая ветка"),
            ("seasons", "Сезоны Trilogy"),
        ] {
            let mut value = cfg.options.get(key).copied().unwrap_or(false);
            if ui.checkbox(&mut value, label).changed() {
                cfg.options.insert(key.into(), value);
            }
        }
        if ui.button("Сохранить настройки").clicked() {
            self.save_config()
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Починить игру"))
                .clicked()
            {
                self.prepare(Action::Repair)
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Глубокая проверка"))
                .clicked()
            {
                self.prepare(Action::DeepRepair)
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Установить драйверы"))
                .clicked()
            {
                self.install_drivers()
            }
        });
        ui.small("Перед удалением файлов появится полный список. Используйте отдельную папку для каждой игры.");
        ui.separator();
        if ui.button("Проверить релиз исходного лаунчера").clicked()
        {
            let tx = self.tx.clone();
            thread::spawn(move || {
                let r = Net::new()
                    .and_then(|net| updater::latest(&net))
                    .map_err(|e| e.to_string());
                let _ = tx.send(Event::Update(r));
            });
        }
        if let Some(url) = &self.update_url {
            if ui.link("Открыть страницу релиза").clicked() {
                let _ = open::that(url);
            }
        }
    }
    fn ui_autostart(&mut self, ui: &mut egui::Ui) {
        let mut enabled = self.settings.auto_launch;
        if ui
            .checkbox(&mut enabled, "Запускать вместе с Windows")
            .changed()
        {
            match autostart::set(enabled) {
                Ok(()) => {
                    self.settings.auto_launch = enabled;
                    let _ = self.settings.save();
                }
                Err(e) => self.message = e.to_string(),
            }
        }
    }
    fn ui_notifications(&mut self, ui: &mut egui::Ui) {
        ui.heading("Уведомления");
        if ui.button("Обновить").clicked() {
            self.refresh_notifications()
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for n in self.notifications.iter().rev().take(30) {
                ui.heading(&n.title);
                ui.label(&n.text);
                ui.small(&n.project);
                ui.separator();
            }
        });
    }
    fn ui_preview(&mut self, ctx: &egui::Context) {
        let Some((plan, action)) = &self.preview else {
            return;
        };
        let mut apply = false;
        let mut close = false;
        egui::Window::new("План файловых операций")
            .collapsible(false)
            .resizable(true)
            .default_width(700.0)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Загрузить: {} файлов ({:.2} ГиБ). Удалить: {}. Изменить время: {}.",
                    plan.download.len(),
                    plan.total_bytes as f64 / 1073741824.0,
                    plan.delete.len(),
                    plan.touch.len()
                ));
                if !plan.delete.is_empty() {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "Подтвердите удаление только после проверки списка:",
                    );
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for p in &plan.delete {
                                ui.label(p.display().to_string());
                            }
                        });
                }
                ui.horizontal(|ui| {
                    if *action != Action::Verify && ui.button("Выполнить план").clicked()
                    {
                        apply = true
                    }
                    if ui.button("Закрыть").clicked() {
                        close = true
                    }
                });
            });
        if apply {
            self.execute()
        }
        if close {
            self.preview = None
        }
    }
}
impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.quit_requested.load(Ordering::SeqCst) {
            self.tray.take();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.poll(ctx);
        if self.tray.is_some() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.tray_hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        } else if !self.tray_hidden {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        if self.running_game != self.window_hidden {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(self.running_game));
            self.window_hidden = self.running_game;
        }
        if self.last_poll.elapsed() > Duration::from_secs(60) {
            self.refresh_notifications()
        }
        if self.last_api_poll.elapsed() > Duration::from_secs(60) {
            self.refresh_api()
        }
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.set_min_height(48.0);
            ui.horizontal_centered(|ui| {
                ui.add_space(8.0);
                ui.strong("ARIZONA GAMES");
                ui.separator();
                egui::ComboBox::from_id_salt("game")
                    .selected_text(game_label(self.game))
                    .show_ui(ui, |ui| {
                        for g in GameId::ALL {
                            let external = external_game_url(g);
                            let label = if external.is_some() {
                                format!("{} ↗", game_label(g))
                            } else {
                                game_label(g).to_owned()
                            };
                            if ui
                                .selectable_label(self.game == g && external.is_none(), label)
                                .clicked()
                            {
                                if let Some(url) = external {
                                    if let Err(error) = open::that(url) {
                                        self.message =
                                            format!("Не удалось открыть страницу: {error}");
                                    }
                                } else {
                                    self.select_game(g);
                                }
                            }
                        }
                    });
                ui.separator();
                for (i, label) in ["Главная", "Моды", "Настройки", "Уведомления"]
                    .iter()
                    .enumerate()
                {
                    if ui
                        .add_sized(
                            [105.0, 30.0],
                            egui::Button::new(*label).selected(self.tab == i),
                        )
                        .clicked()
                    {
                        self.tab = i;
                        if i == 1 && self.catalog.is_none() {
                            self.refresh_mods();
                        }
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                0 => self.ui_home(ui),
                1 => self.ui_mods(ui),
                2 => {
                    self.ui_settings(ui);
                    self.ui_autostart(ui)
                }
                _ => self.ui_notifications(ui),
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if self.progress.1 > 0 {
                ui.add(
                    egui::ProgressBar::new(self.progress.0 as f32 / self.progress.1 as f32).text(
                        format!(
                            "{} / {} MiB · {}",
                            self.progress.0 / 1048576,
                            self.progress.1 / 1048576,
                            self.progress.2
                        ),
                    ),
                );
            } else if !self.progress.2.is_empty() {
                ui.label(&self.progress.2);
            }
            ui.label(&self.message);
        });
        self.ui_preview(ctx);
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let result = run(&args);
    if args.len() == 1 {
        if let Err(error) = &result {
            let _ = rfd::MessageDialog::new()
                .set_title("Arizona Rust Launcher — ошибка запуска")
                .set_description(format!("{error:#}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
    }
    result
}

fn run(args: &[String]) -> anyhow::Result<()> {
    if args.get(1).is_some_and(|s| s == "--probe-api") {
        let net = Net::new()?;
        let projects = net.projects()?;
        let servers = net.servers()?;
        let manifest = net.manifest(GameId::Arizona)?;
        let mods = net.mod_catalog(GameId::Arizona)?;
        println!(
            "projects: {}, server groups: {}, Arizona manifest roots: {}, mods: {}",
            projects.len(),
            servers.len(),
            manifest.data.len(),
            mods.mods.len()
        );
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "--probe-theme") {
        let game = args
            .get(2)
            .and_then(|s| GameId::from_id(s))
            .unwrap_or(GameId::Arizona);
        let assets = theme::sync(&Net::new()?, game)?;
        println!("theme assets cached: {}", assets.len());
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "--dry-run") {
        if args.len() < 5 {
            anyhow::bail!(
                "usage: --dry-run GAME_ID ROOT MANIFEST_JSON [--repair] [--deep] [--future]"
            )
        };
        GameId::from_id(&args[2]).ok_or_else(|| anyhow::anyhow!("unknown game ID"))?;
        let m: Manifest = serde_json::from_slice(&fs::read(&args[4])?)?;
        let p = plan::build(
            &m,
            &PathBuf::from(&args[3]),
            Options {
                repair: args.contains(&"--repair".into()),
                deep: args.contains(&"--deep".into()),
                future_graphics: args.contains(&"--future".into()),
            },
        )?;
        println!("{}", serde_json::to_string_pretty(&p)?);
        return Ok(());
    }
    let settings_path = Settings::path()?;
    let config_dir = settings_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("settings parent missing"))?;
    fs::create_dir_all(config_dir)?;
    let instance_lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(config_dir.join("launcher.lock"))?;
    if let Err(error) = FileExt::try_lock_exclusive(&instance_lock) {
        if show_existing_window() {
            return Ok(());
        }
        FileExt::try_lock_exclusive(&instance_lock)
            .map_err(|_| anyhow::anyhow!("Лаунчер уже запущен, но его окно не найдено: {error}"))?;
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 750.0])
            .with_min_inner_size([640.0, 410.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Arizona Rust Launcher",
        options,
        Box::new(|cc| Ok(Box::new(Launcher::new(cc.egui_ctx.clone())))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

fn create_tray(
    tx: &Sender<Event>,
    ctx: &egui::Context,
    quit_requested: Arc<AtomicBool>,
) -> anyhow::Result<TrayIcon> {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id("show", "Открыть", true, None))?;
    menu.append(&MenuItem::with_id("quit", "Выход", true, None))?;
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32 {
        for x in 0..32 {
            let inside = (x as i32 - 16).pow(2) + (y as i32 - 16).pow(2) < 225;
            pixels.extend_from_slice(if inside {
                &[217, 34, 50, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
    }
    let icon = Icon::from_rgba(pixels, 32, 32).map_err(|e| anyhow::anyhow!("{e}"))?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Arizona Rust Launcher")
        .with_icon(icon)
        .build()?;
    let tx = tx.clone();
    let ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let msg = match event.id.0.as_str() {
            "show" => Event::TrayShow,
            "quit" => {
                quit_requested.store(true, Ordering::SeqCst);
                close_launcher_window();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx.request_repaint();
                return;
            }
            _ => return,
        };
        let _ = tx.send(msg);
        ctx.request_repaint();
    }));
    Ok(tray)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_projects_open_direct_pages() {
        assert_eq!(
            external_game_url(GameId::Trilogy),
            Some("https://vk.ru/wall-182234941_2787962")
        );
        assert_eq!(
            external_game_url(GameId::Village),
            Some("https://vk.ru/wall-89556237_271418")
        );
        assert_eq!(external_game_url(GameId::Arizona), None);
    }

    #[test]
    fn server_selection_survives_reordered_api_list() {
        let groups: Servers =
            serde_json::from_str(include_str!("../fixtures/servers.json")).unwrap();
        let mut servers = groups.get("arizona").unwrap().clone();
        let chosen_number = servers[3].number;
        servers.reverse();
        let position = server_position(&servers, Some(chosen_number)).unwrap();
        assert_eq!(servers[position].number, chosen_number);
        assert_eq!(server_position(&servers, Some(u32::MAX)), Some(0));
    }
}
