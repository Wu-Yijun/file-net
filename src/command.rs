use std::{
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::MyMessage;

use crate::connect;
use connect::{tcp_read, tcp_write, TCPSignal};

#[derive(Debug)]
pub enum MyCommand {
    TrayShow,
    TrayHide,
    AcceptListener(TcpListener, TcpStream),
    AcceptConnector(TcpStream),

    ConnectLoopStop,
}

struct MyConnectCommand {}

pub struct CommandLoop {
    handle: isize,
    cmd: Receiver<MyCommand>,
    cmd_s: Sender<MyCommand>,
    msg_sender: Sender<MyMessage>,

    connect_loop: Option<JoinHandle<()>>,
    connect_sender: Option<Sender<MyConnectCommand>>,
    tls: Option<TcpListener>,
}

impl CommandLoop {
    pub fn new(
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

    pub fn run(mut self) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(cmd) = self.cmd.recv() {
                match cmd {
                    MyCommand::TrayShow => self.to_show(),
                    MyCommand::TrayHide => self.to_hide(),
                    MyCommand::AcceptListener(tls, ts) => {
                        // println!("MyCommand::AcceptListener");
                        self.tls = Some(tls);
                        self.run_connect_loop(ts, true);
                    }
                    MyCommand::AcceptConnector(ts) => {
                        // println!("MyCommand::AcceptConnector");
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
            ip_addr: String::new(),
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
                            cmd_s.send(MyCommand::ConnectLoopStop).unwrap();
                        }
                        TCPSignal::ErrorInto => {
                            println!("[Signal] Error!");
                        }
                        #[allow(unreachable_patterns)]
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
            match sx.try_recv() {
                Ok(MyConnectCommand {}) => {
                    println!("[Connect Loop] Stop");
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => (),
                Err(e)=>{
                    println!("[Connect Loop] Error {e}");
                    return;
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
