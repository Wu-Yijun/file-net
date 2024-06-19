use std::{
    error::Error,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    thread::{self, JoinHandle},
    time::Duration,
};

use if_addrs::Ifv4Addr;

pub struct MyTcplistener {
    pub ip4: [(u8, String); 4],
    pub port: (u16, String),
    pub state: ListenerState,
    pub name: String,
    pub handle: Option<JoinHandle<(Option<TcpListener>, Option<TcpStream>, Vec<u8>)>>,
}
impl MyTcplistener {
    pub const NULL: MyTcplistener = Self {
        ip4: [
            (0, String::new()),
            (0, String::new()),
            (0, String::new()),
            (0, String::new()),
        ],
        port: (0, String::new()),
        state: ListenerState::READY,
        name: String::new(),
        handle: None,
    };
    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    pub fn to_string(&self) -> String {
        format!(
            "{}.{}.{}.{}:{}",
            self.ip4[0].0, self.ip4[1].0, self.ip4[2].0, self.ip4[3].0, self.port.0
        )
    }

    pub fn get_tls(&mut self, host: bool) -> (Option<TcpListener>, Option<TcpStream>) {
        let res = std::mem::replace(&mut self.handle, None);
        match res.unwrap().join() {
            Ok((tls, Some(mut stream), data)) if !data.is_empty() => {
                let signal: TCPSignal = data.into();
                match signal {
                    TCPSignal::Accept { ip_addr, name } => {
                        println!("[Tcp Connect Accept!] Connect to {name} with ip {ip_addr}");
                        // self.info = format!("Connect to {name} with ip {ipAddr}");
                    }
                    e => {
                        println!("[Tcp Connect Error!]: {:?}", e);
                        self.state = ListenerState::FAIL;
                        return (None, None);
                    }
                }
                if host {
                    let data: Vec<u8> = TCPSignal::Accept {
                        ip_addr: self.to_string(),
                        name: "host".to_string(),
                    }
                    .into();
                    if let Err(e) = tcp_write(&mut stream, &data) {
                        println!("[Tcp Connect Accept Response Error]: {e}");
                        self.state = ListenerState::FAIL;
                        return (None, None);
                    }
                }
                return (tls, Some(stream));
            }
            Ok(e) => {
                self.state = ListenerState::FAIL;
                println!("[TcpListener Receive Fail]: {:?}", e);
                (None, None)
            }
            Err(e) => {
                println!("[TcpListener Receive Error]: {:?}", e);
                self.state = ListenerState::FAIL;
                (None, None)
            }
        }
    }

    /// return true if the listener is connected to listen
    pub fn handle_listener(&mut self) -> bool {
        match self.state {
            ListenerState::TOLISTEN => {
                if self.ip4[0].0 == 0
                    && self.ip4[1].0 == 0
                    && self.ip4[2].0 == 0
                    && self.ip4[3].0 == 0
                {
                    // not ready for lis
                    self.state = ListenerState::READY;
                    return false;
                }
                // start listening
                let ip = self.to_string();
                println!("Start listening to {ip}.");
                match TcpListener::bind(ip) {
                    Ok(l) if l.local_addr().is_ok() => {
                        let add = l.local_addr().unwrap();
                        println!("[Listen Start] At {:?}.", add);
                        self.state = ListenerState::LISTENING;
                        self.port.0 = add.port();
                        self.port.1 = self.port.0.to_string();
                        self.handle = Some(thread::spawn(move || match l.accept() {
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
                        self.state = ListenerState::TODELETE;
                    }
                    Err(e) => {
                        println!("[Listen Error]:{e}");
                        self.state = ListenerState::TODELETE;
                    }
                }
            }
            ListenerState::LISTENING => match &self.handle {
                Some(h) if h.is_finished() => {
                    self.state = ListenerState::ACCEPTED;
                    return true;
                }
                None => {
                    self.state = ListenerState::FAIL;
                    return false;
                }
                _ => return false,
            },
            ListenerState::TOSTOP => {
                self.handle = None;
                self.state = ListenerState::READY;
                self.port.0 = 0;
                self.port.1 = 0.to_string();
            }
            _ => (),
        }
        return false;
    }

    pub fn handle_connector(&mut self) -> bool {
        match self.state {
            ListenerState::TOLISTEN => {
                if self.ip4[0].0 == 0
                    && self.ip4[1].0 == 0
                    && self.ip4[2].0 == 0
                    && self.ip4[3].0 == 0
                {
                    // not ready for connect
                    println!("[Cannot Connect] Please enter ip4");
                    self.state = ListenerState::READY;
                    return false;
                }
                if self.port.0 == 0 {
                    // not ready for connect
                    println!("[Cannot Connect] Please enter port");
                    self.state = ListenerState::READY;
                    return false;
                }
                // start connect
                let addr = self.into();
                println!("Start connecting to {addr}.");
                let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(2500))
                else {
                    println!("Couldn't connect to server...");
                    self.state = ListenerState::FAIL;
                    return false;
                };
                println!("Connected to {:?}.", stream.peer_addr());
                self.state = ListenerState::LISTENING;
                let ip = self.to_string();
                self.handle = Some(thread::spawn(move || {
                    let data: Vec<u8> = TCPSignal::Accept {
                        ip_addr: ip,
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
                match &self.handle {
                    Some(h) if h.is_finished() => {
                        self.state = ListenerState::ACCEPTED;
                    }
                    _ => return false,
                }
                return true;
            }
            ListenerState::TOSTOP => {
                self.handle = None;
                self.state = ListenerState::READY;
                self.port.0 = 0;
                self.port.1 = 0.to_string();
            }
            _ => (),
        }
        return false;
    }
}

impl From<Ifv4Addr> for MyTcplistener {
    fn from(ifv4: Ifv4Addr) -> Self {
        let ip4 = ifv4.ip.octets();
        let ip4 = [
            (ip4[0], ip4[0].to_string()),
            (ip4[1], ip4[1].to_string()),
            (ip4[2], ip4[2].to_string()),
            (ip4[3], ip4[3].to_string()),
        ];
        let port = (0, String::new());
        Self {
            ip4,
            port,
            state: ListenerState::READY,
            name: String::new(),
            handle: None,
        }
    }
}

impl Into<SocketAddr> for &MyTcplistener {
    fn into(self) -> SocketAddr {
        let ip =
            std::net::Ipv4Addr::new(self.ip4[0].0, self.ip4[1].0, self.ip4[2].0, self.ip4[3].0);
        std::net::SocketAddr::new(ip.into(), self.port.0)
    }
}
impl Into<SocketAddr> for &mut MyTcplistener {
    fn into(self) -> SocketAddr {
        let s: &MyTcplistener = self;
        s.into()
    }
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub enum TCPSignal {
    Accept {
        ip_addr: String,
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

#[derive(Debug, PartialEq)]
pub enum ListenerState {
    READY = 0,
    TOLISTEN = 1,
    TOSTOP = 2,
    LISTENING = 3,
    ACCEPTED = 4,
    FAIL = 5,
    TODELETE = -1,
}

pub fn tcp_write(stream: &mut TcpStream, data: &Vec<u8>) -> Result<(), Box<dyn Error>> {
    stream.write_all(&data.len().to_le_bytes())?;
    stream.write_all(data)?;
    Ok(())
}

pub fn tcp_read(stream: &mut TcpStream) -> Result<Vec<u8>, Box<dyn Error>> {
    const SIZE_LEN: usize = std::mem::size_of::<usize>();
    let mut size_data = [0; SIZE_LEN];
    stream.read_exact(&mut size_data)?;
    let len = usize::from_le_bytes(size_data);
    let mut res = vec![0; len];
    stream.read_exact(&mut res)?;
    Ok(res)
}
