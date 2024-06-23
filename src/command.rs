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

use crate::{
    connect::connect_loop,
    file::{FileBlock, FileBlocks, FileState, FileStateExtend},
    MyMessage,
};

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

    ReceiveFile(FileState, FileBlocks),
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
    TCPSignal(TCPSignal),
}
impl From<TCPSignal> for MyConnectCommand {
    fn from(signal: TCPSignal) -> Self {
        Self::TCPSignal(signal)
    }
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
            counter: Arc::new(AtomicUsize::new(1)),
        }
    }
    fn push(&mut self, ts: TcpStream) {
        self.streams.lock().unwrap().push(ts)
    }
    fn pop(&mut self) -> Option<TcpStream> {
        self.streams.lock().unwrap().pop()
    }
    /// return a run id
    pub fn send(&mut self, file: FileStateExtend) -> FileBlocks {
        let mut slf = self.clone();
        let id = self.next_id();
        let data = match file.f.get() {
            Ok(data) => data,
            Err(e) => {
                println!("Read file error: {e}");
                self.msg
                    .send(MyCommand::SendFileError(
                        id,
                        SendFileErrorType::CannotReadFile,
                    ))
                    .unwrap();
                return FileBlocks::default();
            }
        };
        let mut fb = FileBlocks::new(id);
        fb.load(data);
        let res = fb.info();
        printlnl!("FILE;; {:#?}", res);
        thread::spawn(move || {
            let mut pos = 0;
            while !fb.is_finished() {
                if let Some(mut ts) = slf.pop() {
                    // println!("Trying to send file......");
                    let fdata = fb.get(pos);
                    let file: FileBlock = (&fdata).into();
                    printlnl!("Sending File Block Info: {}:{}", file.file_id, file.index);
                    tcp_write(&mut ts, &fdata).unwrap();
                    // println!("Send ok. Trying to recv response......");
                    let recdata = match tcp_read(&mut ts) {
                        Ok(d) => d,
                        Err(e) => {
                            printlnl!("Error {e}");
                            break;
                        }
                    };
                    let signal: TCPSignal = recdata.into();
                    println!("Recv ok.");
                    slf.push(ts);

                    if signal.is_ok() {
                        slf.msg
                            .send(MyCommand::SendFileOk(
                                id,
                                SendFileOkType::SendProgress(pos as f32 / fb.block_num as f32),
                            ))
                            .unwrap();
                        fb.done(pos);
                        pos += 1;
                    } else {
                        printlnl!("Send file error: {:?}", signal);
                        slf.msg
                            .send(MyCommand::SendFileError(id, SendFileErrorType::SendError))
                            .unwrap();
                        continue;
                    }
                } else {
                    printlnl!("Send file error: Cannot find tcp stream!");
                    thread::sleep(Duration::from_millis(1500));
                    slf.msg
                        .send(MyCommand::SendFileError(id, SendFileErrorType::SendError))
                        .unwrap();
                    continue;
                }
            }
            slf.msg
                .send(MyCommand::SendFileOk(id, SendFileOkType::SendDone))
                .unwrap();
            println!("Everything Sent");
        });
        res
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
    pub streams: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// file id  -->  sender of file id receiver
    pub allocate_map: Arc<Mutex<HashMap<usize, Sender<FileBlock>>>>,
    pub msg: Sender<MyCommand>,
    counter: Arc<AtomicUsize>,
}

impl MyBlockReceiver {
    fn new(msg: Sender<MyCommand>) -> Self {
        // let (sender, receiver) = std::sync::m::channel();
        // thread::spawn(move||)
        Self {
            streams: Arc::new(Mutex::new(Vec::new())),
            msg,
            counter: Arc::new(AtomicUsize::new(1)),
            allocate_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    fn push(&mut self, mut ts: TcpStream) {
        let map = Arc::clone(&self.allocate_map);
        self.streams
            .lock()
            .unwrap()
            .push(thread::spawn(move || loop {
                match tcp_read(&mut ts) {
                    Ok(data) => {
                        let fb: FileBlock = (&data).into();
                        if fb.is_valid() {
                            if let Some(s) = map.lock().unwrap().get(&fb.file_id) {
                                s.send(fb).unwrap();
                                tcp_write(&mut ts, &TCPSignal::AC.into()).unwrap();
                                // printlnl!("OK");
                            } else {
                                printlnl!("[Error] file id {} not avaliable", fb.file_id);
                                thread::sleep(Duration::from_millis(200));
                                tcp_write(&mut ts, &TCPSignal::Parden.into()).unwrap();
                            }
                        } else {
                            printlnl!("[Error] File block not valid: {:?}", fb);
                            thread::sleep(Duration::from_millis(200));
                            tcp_write(&mut ts, &TCPSignal::Parden.into()).unwrap();
                            panic!();
                        }
                    }
                    Err(e) => {
                        printlnl!("[Error] {e}");
                        panic!();
                    }
                }
            }))
    }
    /// return a run id
    pub fn recv(&mut self, f: FileState, mut fb: FileBlocks) -> usize {
        // let mut slf = self.clone();
        // printlnl!("{:#?}", fb);
        let (send, recv) = mpsc::channel();
        self.allocate_map.lock().unwrap().insert(fb.id, send);
        let id = self.next_id();
        thread::spawn(move || {
            while !fb.is_finished() {
                match recv.recv() {
                    Ok(b) => {
                        println!("Receive block {:?} of file {:?}!", b.index, b.file_id);
                        fb.set(b);
                    }
                    Err(e) => {
                        printlnl!("[Error] {e}");
                        panic!();
                    }
                }
            }
            println!("FB finish!");
            // save to file
            fb.save(&f);
        });
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
            allocate_map: Arc::clone(&self.allocate_map),
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
                    MyCommand::AddTcpReceiver(ts) => self.block_receiver.push(ts),
                    MyCommand::SendFiles(files) => {
                        // todo: Move add tcp stream inside run not here
                        self.connect_sender
                            .as_ref()
                            .unwrap()
                            .send(MyConnectCommand::AddTcpStream)
                            .unwrap();
                        for mut f in files {
                            let id = self.block_sender.send(f.clone());
                            if id.id != 0 {
                                f.f.is_local = false;
                                self.block_sender_state.insert(id.id, MySenderState::new());
                                self.connect_sender
                                    .as_ref()
                                    .unwrap()
                                    .send(TCPSignal::PostFile(f.f, id).into())
                                    .unwrap();
                            } else {
                                // error send file
                                printlnl!("error send file");
                            }
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
                    }
                    MyCommand::ReceiveFile(f, mut fb) => {
                        fb.init();
                        if self.block_receiver_state.contains_key(&fb.id) {
                            printlnl!("[Error] Cannot have two runs with same id!");
                        } else {
                            let id = self.block_receiver.recv(f, fb);
                            self.block_receiver_state.insert(id, MyReceiverState::new());
                        }
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
