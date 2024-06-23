use std::{
    fmt::Debug,
    process::exit,
    sync::mpsc::{Receiver, Sender},
};

use arboard::Clipboard;
use command::{CommandLoop, MyCommand};
use connect::{ListenerState, MyTcplistener};
use eframe::egui::{self, Align2, Widget};
use file::{FileManager, FileStateExtend};
use tray::MyTray;

mod command;
mod connect;
mod file;
mod tray;

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Test windows tray",
        options,
        Box::new(|cc| Box::new(MyApplication::new(cc))),
    )
    .unwrap();
}

struct MyApplication {
    frames: u64,
    cmd_sender: Sender<MyCommand>,
    msg: Receiver<MyMessage>,
    dark_mode: bool,

    info: String,
    /// ([127,0,0,1], port, state, name)
    listeners: Vec<MyTcplistener>,
    connector: MyTcplistener,

    is_listened: bool,
    is_connected: bool,
    page: AppPage,

    files: FileManager,
}

impl MyApplication {
    const SIDE_BAR_SIZE: f32 = 40.0;
    const FILE_LIST_HEIGHT: f32 = 15.0;
}

impl eframe::App for MyApplication {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        // 切换视觉样式
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        egui::TopBottomPanel::top("menu bar").show(ctx, |ui| self.draw_menu_bar(ui));
        egui::SidePanel::left("page bar")
            .resizable(false)
            .exact_width(Self::SIDE_BAR_SIZE)
            .show(ctx, |ui| self.draw_side_bar(ui));
        egui::CentralPanel::default().show(ctx, |ui| match self.page {
            AppPage::Connect => self.draw_connect_control(ui),
            AppPage::File => self.draw_file_control(ui),
            AppPage::Setting => self.draw_setting(ui),
            AppPage::About => self.draw_about(ui),
        });
        self.frames += 1;

        match self.msg.try_recv() {
            // no message
            Err(std::sync::mpsc::TryRecvError::Empty) => (),

            Ok(MyMessage::Text(t)) => self.info = t,
            Ok(MyMessage::ConnectInterrupt(is_host)) => {
                //...
            }
            // ...

            // unexpected
            e => println!("{:#?}", e),
        }

        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }
}

impl MyApplication {
    fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Self {
        let wgpu::rwh::RawWindowHandle::Win32(handle) =
            wgpu::rwh::HasWindowHandle::window_handle(&cc)
                .unwrap()
                .as_raw()
        else {
            panic!("Unsupported platform");
        };
        let (sc, rc) = std::sync::mpsc::channel::<MyCommand>();
        let (sm, rm) = std::sync::mpsc::channel::<MyMessage>();
        let tray = MyTray::new(sc.clone());
        tray.run();
        let cmd = CommandLoop::new(handle.hwnd.into(), sm, sc.clone(), rc);
        cmd.run();

        Self {
            frames: 0,
            cmd_sender: sc,
            info: String::new(),
            dark_mode: false,
            msg: rm,
            listeners: vec![MyTcplistener::NULL],
            connector: MyTcplistener::NULL,
            is_listened: false,
            is_connected: false,
            page: AppPage::default(),

            files: FileManager::new(),
        }
    }

    fn detecting_all_ip4(&mut self) {
        self.listeners.clear();
        let ips = if_addrs::get_if_addrs().unwrap_or_default();
        for ip in ips.into_iter() {
            if let if_addrs::IfAddr::V4(i) = ip.addr {
                self.listeners
                    .push(Into::<MyTcplistener>::into(i).with_name(ip.name));
            }
        }
        self.listeners.push(MyTcplistener::NULL);
    }

    fn handle_listener(&mut self) {
        for ls in self.listeners.iter_mut() {
            if ls.handle_listener() {
                if let (Some(tls), Some(ts)) = ls.get_tls(true) {
                    self.is_listened = true;
                    self.cmd_sender
                        .send(MyCommand::AcceptListener(tls, ts))
                        .unwrap();
                    break;
                }
            }
        }
        self.listeners
            .retain(|l| l.state != ListenerState::TODELETE);
        if self.listeners.len() == 0 {
            self.listeners.push(MyTcplistener::NULL);
        }
    }

    fn handle_connector(&mut self) {
        if self.connector.handle_connector() {
            if let (None, Some(ts)) = self.connector.get_tls(false) {
                self.is_connected = true;
                self.cmd_sender
                    .send(MyCommand::AcceptConnector(ts))
                    .unwrap();
            }
        }
    }

    fn draw_connect_control(&mut self, ui: &mut egui::Ui) {
        for ls in self.listeners.iter_mut() {
            ui.horizontal(|ui| {
                ui.label("Listening on ").on_hover_text(ls.name.clone());
                Self::draw_ip(ui, ls);
                if ui.button("Delete").clicked() {
                    ls.state = ListenerState::TODELETE;
                };
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Auto detecting ip.").clicked() {
                self.detecting_all_ip4();
            }
            if ui.button("Connecting All").clicked() {
                for ls in self.listeners.iter_mut() {
                    if ls.state == ListenerState::READY {
                        ls.state = ListenerState::TOLISTEN;
                    }
                }
            }
            if ui.button("Disconnecting All").clicked() {
                for ls in self.listeners.iter_mut() {
                    if ls.state == ListenerState::LISTENING || ls.state == ListenerState::ACCEPTED {
                        ls.state = ListenerState::TOSTOP;
                    }
                }
            }
            if ui.button("Clear All").clicked() {
                for ls in self.listeners.iter_mut() {
                    ls.state = ListenerState::TODELETE;
                }
            }
        });
        self.handle_listener();
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Connect to ");
            let ls = &mut self.connector;
            Self::draw_ip(ui, ls);
            self.handle_connector();
        });
    }

    fn draw_ip(ui: &mut egui::Ui, ls: &mut MyTcplistener) {
        const IP_SIZE: f32 = 25.0;
        const PORT_WIDTH: f32 = 35.0;
        let mut str_next: String = "".to_string();
        let mut to_next: bool = false;
        let mut sep = [".", ".", ".", ":"].into_iter();
        for ip4 in ls.ip4.iter_mut() {
            let response = egui::TextEdit::singleline(&mut ip4.1)
                .desired_width(IP_SIZE)
                .interactive(ls.state == ListenerState::READY)
                .show(ui)
                .response;
            if to_next {
                response.request_focus();
                ip4.0 = 0;
                ip4.1.clear();
                to_next = false;
            }
            if !str_next.is_empty() {
                ip4.1 = str_next.clone();
            }
            if !str_next.is_empty() || response.changed() {
                let s = ip4.1.clone();
                let mut slice = 0;
                while s[slice..].starts_with(|c: char| c == '.' || c == ':') {
                    slice += 1;
                }
                if let Some((a, b)) = s[slice..].split_once(|c: char| c == '.' || c == ':') {
                    if !a.is_empty() {
                        ip4.1 = a.to_string();
                        str_next = b.to_string();
                        to_next = true;
                    }
                } else {
                    str_next.clear();
                }
                if ip4.1.len() == 0 {
                    ip4.0 = 0;
                } else {
                    let num: isize = ip4.1.parse().unwrap_or(ip4.0 as isize);
                    ip4.0 = num.max(0).min(u8::MAX as isize) as u8;
                    ip4.1 = ip4.0.to_string();
                }
                if ip4.1.len() == 3 {
                    to_next = true;
                }
            }
            ui.label(sep.next().unwrap());
        }
        let response = egui::TextEdit::singleline(&mut ls.port.1)
            .desired_width(PORT_WIDTH)
            .interactive(ls.state == ListenerState::READY)
            .show(ui)
            .response;
        if to_next {
            response.request_focus();
            ls.port.0 = 0;
            ls.port.1.clear();
        }
        if !str_next.is_empty() {
            ls.port.1 = str_next.clone();
        }
        if !str_next.is_empty() || response.changed() {
            if ls.port.1.len() == 0 {
                ls.port.0 = 0;
            } else {
                let num: isize = ls.port.1.parse().unwrap_or(ls.port.0 as isize);
                ls.port.0 = num.max(0).min(u16::MAX as isize) as u16;
                ls.port.1 = ls.port.0.to_string();
            }
        }
        if ls.state == ListenerState::READY {
            if egui::Button::new("Start").ui(ui).clicked() {
                ls.state = ListenerState::TOLISTEN;
            }
        } else {
            if egui::Button::new("Stop").ui(ui).clicked() {
                ls.state = ListenerState::TOSTOP;
            }
        }
        if ui
            .add_enabled(ls.state != ListenerState::READY, egui::Button::new("Copy"))
            .clicked()
        {
            println!("[Copied]{}", ls.to_string());
            Clipboard::new().unwrap().set_text(ls.to_string()).unwrap();
        }
        if ui
            .add_enabled(ls.state == ListenerState::READY, egui::Button::new("Clear"))
            .clicked()
        {
            let _ = std::mem::replace(ls, MyTcplistener::NULL);
        };
        if ui
            .add_enabled(ls.state == ListenerState::READY, egui::Button::new("Paste"))
            .clicked()
        {
            if let Ok(text) = Clipboard::new().unwrap().get_text() {
                for i in text.split(&['.', ':']).enumerate() {
                    if i.0 < 4 {
                        ls.ip4[i.0].0 = i.1.parse().unwrap_or_default();
                        ls.ip4[i.0].1 = ls.ip4[i.0].0.to_string();
                    } else if i.0 == 4 {
                        ls.port.0 = i.1.parse().unwrap_or_default();
                        ls.port.1 = ls.port.0.to_string();
                    }
                }
                println!("[Pasted]{}", ls.to_string());
            }
        }
    }

    fn draw_menu_bar(&mut self, ui: &mut egui::Ui) {
        const SHORT_CUT_OPEN_FILE: egui::KeyboardShortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::O);
        const SHORT_CUT_HIDE: egui::KeyboardShortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::H);
        const SHORT_CUT_CLOSE: egui::KeyboardShortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::W);
        const SHORT_CUT_UNDO: egui::KeyboardShortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Z);
        const SHORT_CUT_SETTING: egui::KeyboardShortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::P);
        const SHORT_CUT_DARKMODE: egui::KeyboardShortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::OpenBracket);
        const SHORT_CUT_ABOUT: egui::KeyboardShortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::A);
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(
                        egui::Button::new("Open File")
                            .shortcut_text(ui.ctx().format_shortcut(&SHORT_CUT_OPEN_FILE)),
                    )
                    .clicked()
                {
                    ui.close_menu();
                }

                if ui
                    .add(
                        egui::Button::new("Hide Window")
                            .shortcut_text(ui.ctx().format_shortcut(&SHORT_CUT_HIDE)),
                    )
                    .clicked()
                {
                    self.cmd_sender.send(MyCommand::TrayHide).unwrap();
                    ui.close_menu();
                }
                if ui
                    .add(
                        egui::Button::new("Quit")
                            .shortcut_text(ui.ctx().format_shortcut(&SHORT_CUT_CLOSE)),
                    )
                    .clicked()
                {
                    exit(0);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui
                    .add(
                        egui::Button::new("Undo")
                            .shortcut_text(ui.ctx().format_shortcut(&SHORT_CUT_UNDO)),
                    )
                    .clicked()
                {
                    ui.close_menu();
                }
                if ui
                    .add(
                        egui::Button::new("Preference")
                            .shortcut_text(ui.ctx().format_shortcut(&SHORT_CUT_SETTING)),
                    )
                    .clicked()
                {
                    ui.close_menu();
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(
                        egui::Button::new("Darkmode")
                            .shortcut_text(ui.ctx().format_shortcut(&SHORT_CUT_DARKMODE)),
                    )
                    .clicked()
                {
                    ui.close_menu();
                    self.dark_mode = !self.dark_mode;
                }
                ui.separator();
                if ui
                    .add(
                        egui::Button::new("About")
                            .shortcut_text(ui.ctx().format_shortcut(&SHORT_CUT_ABOUT)),
                    )
                    .clicked()
                {
                    ui.close_menu();
                }
            });

            ui.separator();
            ui.label(format!("Frames: {}", self.frames));
            if ui.label(format!("[INFO] {}", self.info)).clicked() {
                self.info.clear();
            }
        });
        ui.input(|r| {
            if r.modifiers.ctrl {
                if r.key_pressed(SHORT_CUT_HIDE.logical_key) {
                    self.cmd_sender.send(MyCommand::TrayHide).unwrap();
                } else if r.key_pressed(SHORT_CUT_CLOSE.logical_key) {
                    exit(0);
                } else if r.key_pressed(SHORT_CUT_DARKMODE.logical_key) {
                    self.dark_mode = !self.dark_mode;
                }
            }
        })
    }

    fn draw_side_bar(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                self.page != AppPage::Connect,
                egui::Button::new("Connect")
                    .min_size([Self::SIDE_BAR_SIZE, Self::SIDE_BAR_SIZE].into()),
            )
            .clicked()
        {
            self.page = AppPage::Connect;
        }
        ui.separator();
        if ui
            .add_enabled(
                self.page != AppPage::File,
                egui::Button::new("File")
                    .min_size([Self::SIDE_BAR_SIZE, Self::SIDE_BAR_SIZE].into()),
            )
            .clicked()
        {
            self.page = AppPage::File;
        }
        ui.separator();
        if ui
            .add_enabled(
                self.page != AppPage::Setting,
                egui::Button::new("Setting")
                    .min_size([Self::SIDE_BAR_SIZE, Self::SIDE_BAR_SIZE].into()),
            )
            .clicked()
        {
            self.page = AppPage::Setting;
        }
        ui.separator();
        if ui
            .add_enabled(
                self.page != AppPage::About,
                egui::Button::new("About")
                    .min_size([Self::SIDE_BAR_SIZE, Self::SIDE_BAR_SIZE].into()),
            )
            .clicked()
        {
            self.page = AppPage::About;
        }
    }
    fn draw_file_control(&mut self, ui: &mut egui::Ui) {
        ui.label("file-manager");
        ui.separator();
        let text_height = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y);

        let available_height = ui.available_height();
        let table = egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::auto())
            .column(egui_extras::Column::auto())
            .column(egui_extras::Column::auto())
            .column(egui_extras::Column::auto())
            .column(egui_extras::Column::auto())
            // .column(egui_extras::Column::initial(100.0).range(40.0..=300.0))
            // .column(
            //     egui_extras::Column::initial(100.0)
            //         .at_least(40.0)
            //         .clip(true),
            // )
            // .column(egui_extras::Column::remainder())
            .min_scrolled_height(0.0)
            .max_scroll_height(available_height)
            .sense(egui::Sense::click());

        let mut rows_clicked = None;
        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Select");
                });
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Type");
                });
                header.col(|ui| {
                    ui.strong("State");
                });
                header.col(|ui| {
                    ui.strong("Path");
                });
                // header.col(|ui| {
                //     ui.strong("Clipped text");
                // });
                // header.col(|ui| {
                //     ui.strong("Content");
                // });
            })
            .body(|mut body| {
                for file in self.files.current_files.iter_mut() {
                    body.row(text_height, |mut row| {
                        // row.set_selected(self.selection.contains(&row_index));
                        row.col(|ui| {
                            ui.checkbox(&mut file.is_selected, "");
                        });
                        row.col(|ui| {
                            ui.add(egui::Label::new(&file.f.name).selectable(false));
                        });
                        row.col(|ui| {
                            let text = if file.f.is_folder { "Folder" } else { "File" };
                            ui.add(egui::Label::new(text).selectable(false));
                        });
                        row.col(|ui| {
                            let text =
                                format!("local:{}, sync:{}", file.f.is_local, file.f.is_synced);
                            ui.add(egui::Label::new(text).selectable(false));
                        });
                        row.col(|ui| {
                            let s = file.f.is_linked.clone();
                            ui.add(
                                egui::Label::new(
                                    s.unwrap_or_default().to_str().unwrap_or_default(),
                                )
                                .selectable(false),
                            );
                        });
                        if row.response().clicked_by(egui::PointerButton::Secondary) {
                            rows_clicked = Some(file.clone());
                        }
                    });
                }
            });

        self.draw_file_control_menu(ui, rows_clicked);
    }
    fn draw_file_control_menu(&mut self, ui: &mut egui::Ui, rows_clicked: Option<FileStateExtend>) {
        if ui.button("Send").clicked() {
            let files: Vec<_> = self
                .files
                .current_files
                .iter()
                .filter_map(|f| {
                    if f.is_selected {
                        Some(f.to_owned())
                    } else {
                        None
                    }
                })
                .collect();
            if !files.is_empty() {
                self.cmd_sender.send(MyCommand::SendFiles(files)).unwrap();
            }
        }
        if let Some(file) = rows_clicked {
            println!("Click this row! {:?}", file);
            let pos = ui.input(|i| i.pointer.hover_pos()).unwrap_or_default();
            let mut ui = ui.child_ui(
                egui::Rect {
                    min: pos,
                    max: [f32::INFINITY, f32::INFINITY].into(),
                },
                ui.layout().to_owned(),
            );
            ui.label("text");
            ui.label("text2");
        }
    }
    fn draw_setting(&mut self, ui: &mut egui::Ui) {}
    fn draw_about(&mut self, ui: &mut egui::Ui) {}
}

#[derive(Debug)]
enum MyMessage {
    Text(String),
    ConnectInterrupt(bool),
}

impl From<String> for MyMessage {
    fn from(value: String) -> Self {
        MyMessage::Text(value)
    }
}

#[derive(Debug, Default, PartialEq)]
enum AppPage {
    #[default]
    Connect,
    File,
    Setting,
    About,
}
