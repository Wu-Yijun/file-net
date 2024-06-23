use std::{
    collections::HashMap,
    net::{TcpListener, TcpStream},
    sync::{
        atomic::AtomicUsize,
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{connect::connect_loop, file::FileStateExtend, MyMessage};

use crate::connect;
use connect::{tcp_read, tcp_write, TCPSignal};

#[derive(Debug)]
pub enum MyCommand {
    TrayShow,
    TrayHide,
    AcceptListener(TcpListener, TcpStream),
    AcceptConnector(TcpStream),

    AddTcpSender(TcpStream),
    AddTcpReceiver(TcpStream),

    SendFiles(Vec<FileStateExtend>),
    SendFileError(usize, SendFileErrorType),
    SendFileOk(usize, SendFileOkType),

    ReceiveFileError(usize, ReceiveFileErrorType),
    ReceiveFileOk(usize, ReceiveFileOkType),

    ConnectLoopStop,
}

#[derive(Debug)]
pub enum SendFileErrorType {
    CannotReadFile,
    SendError,
}
#[derive(Debug)]
pub enum SendFileOkType {
    SendDone,
    SendProgress(f32),
}
impl SendFileOkType {
    pub fn is_ok(&self) -> bool {
        if let SendFileOkType::SendDone = self {
            true
        } else {
            false
        }
    }
}

#[derive(Debug)]
pub enum ReceiveFileErrorType {
    ReceiveError,
    CannotWriteFile,
}
#[derive(Debug)]
pub enum ReceiveFileOkType {
    ReceiveDone,
    ReceiveProgress(f32),
}
impl ReceiveFileOkType {
    pub fn is_ok(&self) -> bool {
        if let ReceiveFileOkType::ReceiveDone = self {
            true
        } else {
            false
        }
    }
}

pub enum MyConnectCommand {
    ToStop,
    AddTcpStream,
}

struct MyBlockSender {
    pub streams: Arc<Mutex<Vec<TcpStream>>>,
    pub msg: Sender<MyCommand>,
    counter: Arc<AtomicUsize>,
}

impl MyBlockSender {
    fn new(msg: Sender<MyCommand>) -> Self {
        Self {
            streams: Arc::new(Mutex::new(Vec::new())),
            msg,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn push(&mut self, ts: TcpStream) {
        self.streams.lock().unwrap().push(ts)
    }
    fn pop(&mut self) -> Option<TcpStream> {
        self.streams.lock().unwrap().pop()
    }
    /// return a run id
    pub fn send(&mut self, file: FileStateExtend) -> usize {
        let mut slf = self.clone();
        let id = self.next_id();
        thread::spawn(move || {
            let data = match file.get() {
                Ok(data) => data,
                Err(e) => {
                    println!("Read file error: {e}");
                    slf.msg
                        .send(MyCommand::SendFileError(
                            id,
                            SendFileErrorType::CannotReadFile,
                        ))
                        .unwrap();
                    return;
                }
            };
            if let Some(mut ts) = slf.pop() {
                println!("Trying to send file......");
                tcp_write(&mut ts, &data).unwrap();
                println!("Send ok. Trying to recv response......");
                let signal: TCPSignal = tcp_read(&mut ts).unwrap().into();
                println!("Recv ok.");
                if signal.is_ok() {
                    slf.msg
                        .send(MyCommand::SendFileOk(id, SendFileOkType::SendDone))
                        .unwrap();
                    return;
                } else {
                    println!("Send file error: {:?}", signal);
                    slf.msg
                        .send(MyCommand::SendFileError(id, SendFileErrorType::SendError))
                        .unwrap();
                    return;
                }
            } else {
                println!("Send file error: Cannot find tcp stream!");
                slf.msg
                    .send(MyCommand::SendFileError(id, SendFileErrorType::SendError))
                    .unwrap();
                return;
            }
        });
        id
    }

    fn next_id(&self) -> usize {
        self.counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

impl Clone for MyBlockSender {
    fn clone(&self) -> Self {
        Self {
            streams: Arc::clone(&self.streams),
            msg: self.msg.clone(),
            counter: self.counter.clone(),
        }
    }
}

struct MySenderState {}
impl MySenderState {
    pub fn new() -> Self {
        Self {}
    }
}

struct MyBlockReceiver {
    pub streams: Arc<Mutex<Vec<TcpStream>>>,
    pub msg: Sender<MyCommand>,
    counter: Arc<AtomicUsize>,
}

impl MyBlockReceiver {
    fn new(msg: Sender<MyCommand>) -> Self {
        Self {
            streams: Arc::new(Mutex::new(Vec::new())),
            msg,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn push(&mut self, ts: TcpStream) {
        self.streams.lock().unwrap().push(ts)
    }
    fn pop(&mut self) -> Option<TcpStream> {
        self.streams.lock().unwrap().pop()
    }
    /// return a run id
    pub fn recv(&mut self) -> usize {
        let mut slf = self.clone();
        let id = self.next_id();
        thread::spawn(move || {});
        id
    }

    fn next_id(&self) -> usize {
        self.counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

impl Clone for MyBlockReceiver {
    fn clone(&self) -> Self {
        Self {
            streams: Arc::clone(&self.streams),
            msg: self.msg.clone(),
            counter: self.counter.clone(),
        }
    }
}

struct MyReceiverState {}
impl MyReceiverState {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct CommandLoop {
    handle: isize,
    cmd: Receiver<MyCommand>,
    cmd_s: Sender<MyCommand>,
    msg_sender: Sender<MyMessage>,

    connect_loop: Option<JoinHandle<()>>,
    connect_sender: Option<Sender<MyConnectCommand>>,

    block_sender: MyBlockSender,
    block_sender_state: HashMap<usize, MySenderState>,

    block_receiver: MyBlockReceiver,
    block_receiver_state: HashMap<usize, MyReceiverState>,
}

impl CommandLoop {
    pub fn new(
        handle: isize,
        sm: Sender<MyMessage>,
        sc: Sender<MyCommand>,
        rc: Receiver<MyCommand>,
    ) -> Self {
        Self {
            block_sender: MyBlockSender::new(sc.clone()),
            block_sender_state: HashMap::new(),
            block_receiver: MyBlockReceiver::new(sc.clone()),
            block_receiver_state: HashMap::new(),

            handle: handle,
            cmd: rc,
            cmd_s: sc,
            msg_sender: sm,

            connect_loop: None,
            connect_sender: None,
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
                        self.run_connect_loop(ts, Some(tls));
                    }
                    MyCommand::AcceptConnector(ts) => {
                        // println!("MyCommand::AcceptConnector");
                        self.run_connect_loop(ts, None);
                    }
                    MyCommand::AddTcpSender(ts) => self.block_sender.push(ts),
                    MyCommand::AddTcpReceiver(ts) => self.block_sender.push(ts),
                    MyCommand::SendFiles(files) => {
                        for f in files {
                            let id = self.block_sender.send(f);
                            self.block_sender_state.insert(id, MySenderState::new());
                        }
                    }
                    MyCommand::SendFileOk(id, tp) => {
                        println!("Send file {id} ok with {:?}", tp);
                        if tp.is_ok() {
                            self.block_sender_state.remove(&id);
                        }
                    }
                    MyCommand::SendFileError(id, tp) => {
                        println!("Send file {id} error with {:?}", tp);
                        // todo: Move add tcp stream inside run not here
                        self.connect_sender.as_ref().unwrap().send(MyConnectCommand::AddTcpStream).unwrap();
                    }
                    e => println!("[Unknown Command]{:#?}", e),
                }
            }
        })
    }

    fn run_connect_loop(&mut self, mut ts: TcpStream, host: Option<TcpListener>) {
        let (sc, sx) = mpsc::channel();
        self.connect_sender = Some(sc);
        let cmd_s = self.cmd_s.clone();

        if let Err(e) = ts.set_read_timeout(Some(Duration::from_millis(2000))) {
            println!("[Connect Loop fail to][Set read timeout]: {e}");
        }
        if let Err(e) = ts.set_write_timeout(Some(Duration::from_millis(2000))) {
            println!("[Connect Loop fail to][Set write timeout]: {e}");
        }
        println!("[Ready for connect loop]");
        if host.is_none() {
            if let Err(e) = tcp_write(&mut ts, &TCPSignal::AC.into()) {
                println!("[Signal][Send] Error {e}");
            }
        }
        println!("[Enter connect loop]");
        self.connect_loop = Some(thread::spawn(move || connect_loop(ts, cmd_s, sx, host)));
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
