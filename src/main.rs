use arboard::Clipboard;
use eframe::egui::{self, Widget};
use if_addrs::IfAddr;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;
use std::{
    fmt::Debug,
    isize,
    process::exit,
    sync::mpsc::{Receiver, Sender},
    thread::{self, JoinHandle},
};
use trayicon::{Icon, MenuBuilder, MenuItem, TrayIcon, TrayIconBuilder};
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};

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

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
enum TCPSignal {
    Accept {
        ipAddr: String,
        name: String,
    },
    Parden,
    Shut,
    #[default]
    ErrorInto,
}

impl From<&Vec<u8>> for TCPSignal {
    fn from(bytes: &Vec<u8>) -> Self {
        bincode::deserialize(bytes).unwrap_or_default()
    }
}
impl From<Vec<u8>> for TCPSignal {
    fn from(bytes: Vec<u8>) -> Self {
        (&bytes).into()
    }
}
impl Into<Vec<u8>> for &TCPSignal {
    fn into(self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
}
impl Into<Vec<u8>> for TCPSignal {
    fn into(self) -> Vec<u8> {
        (&self).into()
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
enum TrayEvents {
    RightClickTrayIcon,
    LeftClickTrayIcon,
    DoubleClickTrayIcon,
    Exit,
    ChangeToRed,
    ChangeToGreen,
    ChangeMenu,
    ToolTip,
    HideWindow,
    ShowWindow,
    DisabledItem1,
    CheckItem1,
    SubItem1,
    SubItem2,
    SubItem3,
}

struct MyTray {
    tray: TrayIcon<TrayEvents>,
    icons: Vec<Icon>,
    cmd_sender: Sender<MyCommand>,
    rx: Receiver<TrayEvents>,
}
impl MyTray {
    fn new(sc: Sender<MyCommand>) -> Self {
        let (sx, rx) = std::sync::mpsc::channel::<TrayEvents>();
        let icon = vec![
            include_bytes!("../../assets/icon1.ico"),
            include_bytes!("../../assets/icon2.ico"),
        ];
        let icons: Vec<Icon> = icon
            .into_iter()
            .map(|i| Icon::from_buffer(i, None, None).unwrap())
            .collect();
        let tray = TrayIconBuilder::new()
            .sender(move |e: &TrayEvents| {
                // let _ = proxy.send_event(e.clone());
                sx.send(e.clone()).unwrap();
            })
            .icon(icons[0].clone())
            .tooltip("Cool Tray 👀 Icon")
            .on_click(TrayEvents::LeftClickTrayIcon)
            .on_double_click(TrayEvents::DoubleClickTrayIcon)
            .on_right_click(TrayEvents::RightClickTrayIcon)
            .menu(
                MenuBuilder::new()
                    .item("Item 1 Change Icon Red", TrayEvents::ChangeToRed)
                    .item("Item 2 Change Icon Green", TrayEvents::ChangeToGreen)
                    .item("Item 3 Replace Menu 👍", TrayEvents::ChangeMenu)
                    .item("Item 4 Set Tooltip", TrayEvents::ToolTip)
                    .separator()
                    .item("Hide Window", TrayEvents::HideWindow)
                    .item("Show Window", TrayEvents::ShowWindow)
                    .separator()
                    .submenu(
                        "Sub Menu",
                        MenuBuilder::new()
                            .item("Sub item 1", TrayEvents::SubItem1)
                            .item("Sub Item 2", TrayEvents::SubItem2)
                            .item("Sub Item 3", TrayEvents::SubItem3),
                    )
                    .checkable(
                        "This checkbox toggles disable",
                        true,
                        TrayEvents::CheckItem1,
                    )
                    .with(MenuItem::Item {
                        name: "Item Disabled".into(),
                        disabled: true, // Disabled entry example
                        id: TrayEvents::DisabledItem1,
                        icon: Some(icons[0].clone()),
                    })
                    .separator()
                    .item("Exit", TrayEvents::Exit),
            )
            .build()
            .unwrap();
        Self {
            tray,
            icons,
            cmd_sender: sc,
            rx,
        }
    }

    fn run(mut self) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(r) = self.rx.recv() {
                match r {
                    TrayEvents::Exit => {
                        exit(0);
                    }
                    TrayEvents::RightClickTrayIcon => {
                        self.tray.show_menu().unwrap();
                    }
                    TrayEvents::LeftClickTrayIcon => {
                        self.tray.show_menu().unwrap();
                    }
                    TrayEvents::HideWindow => {
                        self.cmd_sender.send(MyCommand::TrayHide).unwrap();
                    }
                    TrayEvents::ShowWindow => {
                        self.cmd_sender.send(MyCommand::TrayShow).unwrap();
                    }
                    e => println!("{:#?}", e),
                }
            }
        })
    }
}

#[derive(Debug)]
enum MyCommand {
    TrayShow,
    TrayHide,
    AcceptListener(TcpListener, TcpStream),
    AcceptConnector(TcpStream),

    ConnectLoopStop,
}

struct MyConnectCommand {}

struct CommandLoop {
    handle: isize,
    cmd: Receiver<MyCommand>,
    cmd_s: Sender<MyCommand>,
    msg_sender: Sender<MyMessage>,

    connect_loop: Option<JoinHandle<()>>,
    connect_sender: Option<Sender<MyConnectCommand>>,
    tls: Option<TcpListener>,
}

impl CommandLoop {
    fn new(
        handle: isize,
        sm: Sender<MyMessage>,
        sc: Sender<MyCommand>,
        rc: Receiver<MyCommand>,
    ) -> Self {
        Self {
            handle: handle,
            cmd: rc,
            cmd_s: sc,
            msg_sender: sm,

            connect_loop: None,
            connect_sender: None,
            tls: None,
        }
    }

    fn run(mut self) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(cmd) = self.cmd.recv() {
                match cmd {
                    MyCommand::TrayShow => self.to_show(),
                    MyCommand::TrayHide => self.to_hide(),
                    MyCommand::AcceptListener(tls, mut ts) => {
                        self.run_connect_loop(ts, true);
                    }
                    MyCommand::AcceptConnector(mut ts) => {
                        self.run_connect_loop(ts, false);
                    }
                    e => println!("[Unknown Command]{:#?}", e),
                }
            }
        })
    }

    fn run_connect_loop(&mut self, mut ts: TcpStream, host: bool) {
        let (sc, sx) = mpsc::channel();
        self.connect_sender = Some(sc);
        let cmd_s = self.cmd_s.clone();

        if let Err(e) = ts.set_read_timeout(Some(Duration::from_millis(2000))) {
            println!("[Connect Loop fail to][Set read timeout]: {e}");
        }
        if let Err(e) = ts.set_write_timeout(Some(Duration::from_millis(2000))) {
            println!("[Connect Loop fail to][Set write timeout]: {e}");
        }
        let mut error_cnt = 0;
        const SIGNAL_AC_DATA: TCPSignal = TCPSignal::Accept {
            ipAddr: String::new(),
            name: String::new(),
        };
        println!("[Ready for connect loop]");
        if !host {
            if let Err(e) = tcp_write(&mut ts, &SIGNAL_AC_DATA.into()) {
                println!("[Signal][Send] Error {e}");
            }
        }
        println!("[Enter connect loop]");
        self.connect_loop = Some(thread::spawn(move || loop {
            match tcp_read(&mut ts) {
                Ok(data) => {
                    let signal: TCPSignal = data.into();
                    match signal {
                        TCPSignal::Accept { .. } => {
                            println!("[Signal Loop Ac]");
                            error_cnt = 0;
                            thread::sleep(Duration::from_millis(1000));
                            if let Err(e) = tcp_write(&mut ts, &SIGNAL_AC_DATA.into()) {
                                println!("[Signal][Send] Error {e}");
                            }
                        }
                        TCPSignal::Parden => {
                            println!("[Signal] To send again");
                            thread::sleep(Duration::from_millis(200));
                            if let Err(e) = tcp_write(&mut ts, &SIGNAL_AC_DATA.into()) {
                                println!("[Signal][Send] Error {e}");
                            }
                        }
                        TCPSignal::Shut => {
                            println!("[Signal] To close");
                        }
                        TCPSignal::ErrorInto => {
                            println!("[Signal] Error!");
                        }
                        e => {
                            println!("[Unknown][Signal]: {:#?}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("[Signal read Error]: {e}");
                    let data = TCPSignal::Parden.into();
                    match tcp_write(&mut ts, &data) {
                        Err(e) => {
                            error_cnt += 1;
                            println!("[Signal write Error][Parden]: {e}");
                            if error_cnt > 3 {
                                println!("[Error] Cannot resume connection. Stop connection...");
                                cmd_s.send(MyCommand::ConnectLoopStop).unwrap();
                                return;
                            }
                            thread::sleep(Duration::from_millis(2000));
                        }
                        _ => (),
                    }
                }
            }
        }));
    }

    fn to_hide(&mut self) {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                self.handle,
                windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
            );
        }
        self.msg_sender
            .send(format!("[COMMAND] Window hide").into())
            .unwrap();
    }
    fn to_show(&mut self) {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                self.handle,
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW,
            );
        }
        self.msg_sender
            .send(format!("[COMMAND] Window show").into())
            .unwrap();
    }
}

type MyTcplistener = (
    [(u8, String); 4],
    (u16, String),
    ListenerState,
    String,
    Option<JoinHandle<(Option<TcpListener>, Option<TcpStream>, Vec<u8>)>>,
);

struct MyApplication {
    frames: u64,
    cmd_sender: Sender<MyCommand>,
    msg: Receiver<MyMessage>,

    info: String,
    /// ([127,0,0,1], port, state, name)
    listeners: Vec<MyTcplistener>,
    connector: MyTcplistener,

    is_listened: bool,
    is_connected: bool,
}

#[derive(Debug, PartialEq)]
enum ListenerState {
    READY = 0,
    TOLISTEN = 1,
    TOSTOP = 2,
    LISTENING = 3,
    ACCEPTED = 4,
    FAIL = 5,
    TODELETE = -1,
}

impl MyApplication {
    fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Self {
        let RawWindowHandle::Win32(handle) = cc.window_handle().unwrap().as_raw() else {
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
            msg: rm,
            listeners: vec![Self::get_null_ip()],
            connector: Self::get_null_ip(),
            is_listened: false,
            is_connected: false,
        }
    }

    fn get_null_ip() -> MyTcplistener {
        (
            [
                (0, "".to_string()),
                (0, "".to_string()),
                (0, "".to_string()),
                (0, "".to_string()),
            ],
            (0, "".to_string()),
            ListenerState::READY,
            "".to_string(),
            None,
        )
    }

    fn detecting_all_ip(&mut self) {
        self.listeners.clear();
        let ips = if_addrs::get_if_addrs().unwrap_or_default();
        for ip in ips.into_iter() {
            if let IfAddr::V4(i) = ip.addr {
                let ip4 = i.ip.octets();
                let port = 0;
                let name = ip.name;
                self.listeners.push((
                    [
                        (ip4[0], ip4[0].to_string()),
                        (ip4[1], ip4[1].to_string()),
                        (ip4[2], ip4[2].to_string()),
                        (ip4[3], ip4[3].to_string()),
                    ],
                    (port, port.to_string()),
                    ListenerState::READY,
                    name,
                    None,
                ));
            }
        }
        self.listeners.push(Self::get_null_ip());
    }

    fn get_nth_ip(&self, n: usize) -> String {
        if n >= self.listeners.len() {
            return "0.0.0.0:0".to_string();
        }
        let ip = &self.listeners[n];
        Self::get_struct_ip(ip)
    }
    fn get_struct_ip(ip: &MyTcplistener) -> String {
        format!(
            "{}.{}.{}.{}:{}",
            ip.0[0].0, ip.0[1].0, ip.0[2].0, ip.0[3].0, ip.1 .0
        )
    }

    fn handle_listener(&mut self) {
        for ls in self.listeners.iter_mut() {
            match ls.2 {
                ListenerState::TOLISTEN => {
                    if ls.0[0].0 == 0 && ls.0[1].0 == 0 && ls.0[2].0 == 0 && ls.0[3].0 == 0 {
                        // not ready for lis
                        ls.2 = ListenerState::READY;
                        continue;
                    }
                    // start listening
                    let ip = Self::get_struct_ip(ls);
                    println!("Start listening to {ip}.");
                    match TcpListener::bind(ip) {
                        Ok(l) if l.local_addr().is_ok() => {
                            let add = l.local_addr().unwrap();
                            println!("[Listen Start] At {:?}.", add);
                            ls.2 = ListenerState::LISTENING;
                            ls.1 .0 = add.port();
                            ls.1 .1 = ls.1 .0.to_string();
                            ls.4 = Some(thread::spawn(move || match l.accept() {
                                Ok(mut s) => {
                                    println!("[Accect From]: {:?}", s.1);
                                    let data = tcp_read(&mut s.0).unwrap_or_default();
                                    (Some(l), Some(s.0), data)
                                }
                                Err(e) => {
                                    println!("[Accept Error]: {e}");
                                    (None, None, vec![])
                                }
                            }));
                        }
                        Ok(l) => {
                            println!("[Listen Start][Error]:{:?}", l.local_addr().err().unwrap());
                            ls.2 = ListenerState::TODELETE;
                        }
                        Err(e) => {
                            println!("[Listen Error]:{e}");
                            ls.2 = ListenerState::TODELETE;
                        }
                    }
                }
                ListenerState::LISTENING => {
                    match &ls.4 {
                        Some(h) if h.is_finished() => {
                            ls.2 = ListenerState::ACCEPTED;
                        }
                        _ => continue,
                    }
                    let res = std::mem::replace(&mut ls.4, None);
                    match res.unwrap().join() {
                        Ok((Some(tls), Some(mut stream), data)) if !data.is_empty() => {
                            let signal: TCPSignal = data.into();
                            match signal {
                                TCPSignal::Accept { ipAddr, name } => {
                                    println!("[Tcp Connect Accept!]");
                                    self.info = format!("Connect to {name} with ip {ipAddr}");
                                }
                                e => {
                                    println!("[Tcp Connect Error!]: {:?}", e);
                                    ls.2 = ListenerState::FAIL;
                                    continue;
                                }
                            }
                            let data: Vec<u8> = TCPSignal::Accept {
                                ipAddr: Self::get_struct_ip(ls),
                                name: "host".to_string(),
                            }
                            .into();
                            if let Err(e) = tcp_write(&mut stream, &data) {
                                println!("[Tcp Connect Accept Response Error]: {e}");
                                ls.2 = ListenerState::FAIL;
                                continue;
                            }
                            self.is_listened = true;
                            self.cmd_sender
                                .send(MyCommand::AcceptListener(tls, stream))
                                .unwrap();
                            break;
                        }
                        Ok(_) => {
                            ls.2 = ListenerState::FAIL;
                        }
                        Err(e) => {
                            println!("[TcpListener Receive Error]: {:?}", e);
                            ls.2 = ListenerState::FAIL;
                        }
                    }
                }
                ListenerState::TOSTOP => {
                    ls.4 = None;
                    ls.2 = ListenerState::READY;
                    ls.1 .0 = 0;
                    ls.1 .1 = 0.to_string();
                }
                _ => (),
            }
        }
        self.listeners.retain(|l| l.2 != ListenerState::TODELETE);
        if self.listeners.len() == 0 {
            self.listeners.push(Self::get_null_ip());
        }
    }

    fn handle_connector(&mut self) {
        let ls = &mut self.connector;
        match ls.2 {
            ListenerState::TOLISTEN => {
                if ls.0[0].0 == 0 && ls.0[1].0 == 0 && ls.0[2].0 == 0 && ls.0[3].0 == 0 {
                    // not ready for connect
                    self.info = format!("[Cannot Connect] Please enter ip4");
                    ls.2 = ListenerState::READY;
                    return;
                }
                if ls.1 .0 == 0 {
                    // not ready for connect
                    self.info = format!("[Cannot Connect] Please enter port");
                    ls.2 = ListenerState::READY;
                    return;
                }
                // start connect
                let addr = std::net::Ipv4Addr::new(ls.0[0].0, ls.0[1].0, ls.0[2].0, ls.0[3].0);
                let addr = std::net::SocketAddr::new(addr.into(), ls.1 .0);
                println!("Start connecting to {addr}.");
                let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(2500))
                else {
                    println!("Couldn't connect to server...");
                    ls.2 = ListenerState::FAIL;
                    return;
                };
                println!("Connected to {:?}.", stream.peer_addr());
                ls.2 = ListenerState::LISTENING;
                let ip = Self::get_struct_ip(ls);
                ls.4 = Some(thread::spawn(move || {
                    let data: Vec<u8> = TCPSignal::Accept {
                        ipAddr: ip,
                        name: "客户端".to_string(),
                    }
                    .into();
                    match tcp_write(&mut stream, &data) {
                        Ok(()) => {
                            let Ok(data) = tcp_read(&mut stream) else {
                                return (None, None, vec![]);
                            };
                            (None, Some(stream), data)
                        }
                        Err(e) => {
                            println!("[Connect Send Error]: {e}");
                            (None, None, vec![])
                        }
                    }
                }));
            }
            ListenerState::LISTENING => {
                match &ls.4 {
                    Some(h) if h.is_finished() => {
                        ls.2 = ListenerState::ACCEPTED;
                    }
                    _ => return,
                }
                let res = std::mem::replace(&mut ls.4, None);
                match res.unwrap().join() {
                    Ok((None, Some(mut stream), data)) if !data.is_empty() => {
                        let signal: TCPSignal = data.into();
                        match signal {
                            TCPSignal::Accept { ipAddr, name } => {
                                println!("[Tcp Connect Accept!]");
                                self.info = format!("Connect to {name} with ip {ipAddr}");
                            }
                            e => {
                                println!("[Tcp Connect Error!]: {:?}", e);
                                ls.2 = ListenerState::FAIL;
                                return;
                            }
                        }
                        self.is_connected = true;
                        self.cmd_sender
                            .send(MyCommand::AcceptConnector(stream))
                            .unwrap();
                        return;
                    }
                    Ok(_) => {
                        ls.2 = ListenerState::FAIL;
                    }
                    Err(e) => {
                        println!("[TcpListener Receive Error]: {:?}", e);
                        ls.2 = ListenerState::FAIL;
                    }
                }
            }
            ListenerState::TOSTOP => {
                ls.4 = None;
                ls.2 = ListenerState::READY;
                ls.1 .0 = 0;
                ls.1 .1 = 0.to_string();
            }
            _ => (),
        }
    }

    fn draw_ip(ui: &mut egui::Ui, ls: &mut MyTcplistener) {
        const IP_SIZE: f32 = 25.0;
        const PORT_WIDTH: f32 = 35.0;
        let mut str_next: String = "".to_string();
        let mut to_next: bool = false;
        let mut sep = [".", ".", ".", ":"].into_iter();
        for ip4 in ls.0.iter_mut() {
            let response = egui::TextEdit::singleline(&mut ip4.1)
                .desired_width(IP_SIZE)
                .interactive(ls.2 == ListenerState::READY)
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
        let response = egui::TextEdit::singleline(&mut ls.1 .1)
            .desired_width(PORT_WIDTH)
            .interactive(ls.2 == ListenerState::READY)
            .show(ui)
            .response;
        if to_next {
            response.request_focus();
            ls.1 .0 = 0;
            ls.1 .1.clear();
        }
        if !str_next.is_empty() {
            ls.1 .1 = str_next.clone();
        }
        if !str_next.is_empty() || response.changed() {
            if ls.1 .1.len() == 0 {
                ls.1 .0 = 0;
            } else {
                let num: isize = ls.1 .1.parse().unwrap_or(ls.1 .0 as isize);
                ls.1 .0 = num.max(0).min(u16::MAX as isize) as u16;
                ls.1 .1 = ls.1 .0.to_string();
            }
        }
        if ls.2 == ListenerState::READY {
            if egui::Button::new("Start").ui(ui).clicked(){
                ls.2 = ListenerState::TOLISTEN;
            }
        } else {
            if egui::Button::new("Stop").ui(ui).clicked(){
                ls.2 = ListenerState::TOSTOP;
            }
        }
        if ui.add_enabled(ls.2 != ListenerState::READY, egui::Button::new("Copy")).clicked() {
            let text = Self::get_struct_ip(ls);
            println!("[Copied]{}", Self::get_struct_ip(ls));
            Clipboard::new().unwrap().set_text(text).unwrap();
        }
        if ui.add_enabled(ls.2 == ListenerState::READY, egui::Button::new("Clear")).clicked() {
            let _ = std::mem::replace(ls, Self::get_null_ip());
        };
        if ui.add_enabled(ls.2 == ListenerState::READY, egui::Button::new("Paste")).clicked() {
            if let Ok(text) = Clipboard::new().unwrap().get_text() {
                for i in text.split(&['.', ':']).enumerate() {
                    if i.0 < 4 {
                        ls.0[i.0].0 = i.1.parse().unwrap_or_default();
                        ls.0[i.0].1 = ls.0[i.0].0.to_string();
                    } else if i.0 == 4 {
                        ls.1 .0 = i.1.parse().unwrap_or_default();
                        ls.1 .1 = ls.1 .0.to_string();
                    }
                }
                println!("[Pasted]{}", Self::get_struct_ip(ls));
            }
        }
    }
}

impl eframe::App for MyApplication {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("menu bar").show(ctx, |ui| {
            const SHORT_CUT_OPEN_FILE: egui::KeyboardShortcut =
                egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::O);
            const SHORT_CUT_HIDE: egui::KeyboardShortcut =
                egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::H);
            const SHORT_CUT_CLOSE: egui::KeyboardShortcut =
                egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::W);
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
                ui.menu_button("Edit", |ui| {});
                ui.menu_button("View", |ui| {});

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
                    }
                }
            })
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            for ls in self.listeners.iter_mut() {
                ui.horizontal(|ui| {
                    ui.label("Listening on ").on_hover_text(ls.3.clone());
                    Self::draw_ip(ui, ls);
                    if ui.button("Delete").clicked() {
                        ls.2 = ListenerState::TODELETE;
                    };
                });
            }
            ui.horizontal(|ui| {
                if ui.button("Auto detecting ip.").clicked() {
                    self.detecting_all_ip();
                }
                if ui.button("Connecting All").clicked() {
                    for ls in self.listeners.iter_mut() {
                        if ls.2 == ListenerState::READY {
                            ls.2 = ListenerState::TOLISTEN;
                        }
                    }
                }
                if ui.button("Disconnecting All").clicked() {
                    for ls in self.listeners.iter_mut() {
                        if ls.2 == ListenerState::LISTENING || ls.2 == ListenerState::ACCEPTED {
                            ls.2 = ListenerState::TOSTOP;
                        }
                    }
                }
                if ui.button("Clear All").clicked() {
                    for ls in self.listeners.iter_mut() {
                        ls.2 = ListenerState::TODELETE;
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
            })
        });
        self.frames += 1;

        match self.msg.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => (),
            Ok(MyMessage::Text(t)) => self.info = t,
            e => println!("{:#?}", e),
        }

        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }
}

#[derive(Debug)]
enum MyMessage {
    Text(String),
}

impl From<String> for MyMessage {
    fn from(value: String) -> Self {
        MyMessage::Text(value)
    }
}

// impl ApplicationHandler<TrayEvents> for MyApplication {
//     fn resumed(&mut self, event_loop: &ActiveEventLoop) {
//         self.window = Some(
//             event_loop
//                 .create_window(Window::default_attributes())
//                 .unwrap(),
//         );
//     }

//     // Platform specific events
//     fn window_event(
//         &mut self,
//         event_loop: &ActiveEventLoop,
//         _window_id: winit::window::WindowId,
//         event: WindowEvent,
//     ) {
//         match event {
//             WindowEvent::CloseRequested => {
//                 event_loop.exit();
//             }
//             e => {
//                 println!("Window event: {:?}", e);
//             }
//         }
//     }

//     // Application specific events
//     fn user_event(&mut self, event_loop: &ActiveEventLoop, event: TrayEvents) {
//         match event {
//             TrayEvents::Exit => event_loop.exit(),
//             TrayEvents::RightClickTrayIcon => {
//                 self.tray_icon.show_menu().unwrap();
//             }
//             TrayEvents::CheckItem1 => {
//                 // You can mutate single checked, disabled value followingly.
//                 //
//                 // However, I think better way is to use reactively
//                 // `set_menu` by building the menu based on application
//                 // state.
//                 if let Some(old_value) = self
//                     .tray_icon
//                     .get_menu_item_checkable(TrayEvents::CheckItem1)
//                 {
//                     // Set checkable example
//                     let _ = self
//                         .tray_icon
//                         .set_menu_item_checkable(TrayEvents::CheckItem1, !old_value);

//                     // Set disabled example
//                     let _ = self
//                         .tray_icon
//                         .set_menu_item_disabled(TrayEvents::DisabledItem1, !old_value);
//                 }
//             }
//             TrayEvents::ChangeToRed => {
//                 self.tray_icon.set_icon(&self.second_icon).unwrap();
//             }
//             TrayEvents::ChangeToGreen => {
//                 self.tray_icon.set_icon(&self.first_icon).unwrap();
//             }
//             TrayEvents::ChangeMenu => {
//                 self.tray_icon
//                     .set_menu(
//                         &MenuBuilder::new()
//                             .item("Another item", TrayEvents::ChangeToRed)
//                             .item("Exit", TrayEvents::Exit),
//                     )
//                     .unwrap();
//             }
//             TrayEvents::ToolTip => {
//                 self.tray_icon.set_tooltip("Menu changed!").unwrap();
//             }
//             TrayEvents::LeftClickTrayIcon => {
//                 self.tray_icon.show_menu().unwrap();
//             }
//             // Events::DoubleClickTrayIcon => todo!(),
//             // Events::DisabledItem1 => todo!(),
//             // Events::SubItem1 => todo!(),
//             // Events::SubItem2 => todo!(),
//             // Events::SubItem3 => todo!(),
//             _ => {}
//         }
//     }
// }

fn tcp_write(stream: &mut TcpStream, data: &Vec<u8>) -> Result<(), Box<dyn Error>> {
    stream.write_all(&data.len().to_le_bytes())?;
    stream.write_all(data)?;
    Ok(())
}

fn tcp_read(stream: &mut TcpStream) -> Result<Vec<u8>, Box<dyn Error>> {
    const SIZE_LEN: usize = std::mem::size_of::<usize>();
    let mut size_data = [0; SIZE_LEN];
    stream.read_exact(&mut size_data)?;
    let len = usize::from_le_bytes(size_data);
    let mut res = vec![0; len];
    stream.read_exact(&mut res)?;
    Ok(res)
}
