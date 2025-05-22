use std::{collections::HashMap, io, net::{IpAddr, SocketAddr, TcpListener, TcpStream}, sync::{mpsc::{self, Receiver, Sender}, Arc, Mutex}, thread};

use crate::protocol;


pub struct ConnectedClientInfo {
    write_conn: TcpStream,
    addr: SocketAddr,
    nickname: Option<String>,
}

pub struct Server {
    host: IpAddr, port: u16,
    running: bool,
    listener_running: Arc<Mutex<bool>>,
    connected_clients: HashMap<usize, ConnectedClientInfo>,
    tx: Sender<ServerEvent>,
    rx: Receiver<ServerEvent>,
}

pub enum ServerEvent {
    NewConnection(usize, TcpStream),
    ServerError(io::Error),
    MessageFromClient(usize, String),
    MessageFromHost(String),
    ClientDisconnected(Option<String>, usize),
    Shutdown,
}

pub enum ServerMessage {
    Info(String),
    Error(String),
    Msg(String),
    CriticalError(String),
}

impl Server {
    pub fn new(host: IpAddr, port: u16) -> Self {
        let listener_running = Arc::new(Mutex::new(false));
        let (tx, rx) = mpsc::channel();
        return Self {
            host, port,
            running: false,
            listener_running,
            connected_clients: HashMap::new(),
            tx, rx
        };
    }

    pub fn clone_tx(&self) -> Sender<ServerEvent> {
        return self.tx.clone();
    }

    pub fn run<F>(&mut self, callback: F) where F: Fn(ServerMessage) -> ()  {
        callback(ServerMessage::Info(format!("Server is starting...")));

        let listener = match TcpListener::bind((self.host, self.port)) {
            Ok(listener) => listener,
            Err(err) => {
                callback(ServerMessage::CriticalError(err.to_string()));
                return ();
            }
        };
        let tx = self.tx.clone();
        let stay_active = self.listener_running.clone();

        *self.listener_running.lock().unwrap() = true;
        thread::spawn(move || Server::th_connections_listener(listener, tx, stay_active));
        callback(ServerMessage::Info(format!("Listening for new connection at {}:{}", self.host, self.port)));
        
        self.running = true;
        while self.running {
            let server_event = match self.rx.recv() {
                Ok(e) => e,
                Err(err) => {
                    callback(ServerMessage::CriticalError(err.to_string()));
                    return ();
                }
            };
            let msg = match server_event {
                ServerEvent::NewConnection(cid, conn) => self.evnt_handle_new_connection(cid, conn),
                ServerEvent::Shutdown => self.evnt_handle_shutdown(),
                ServerEvent::MessageFromClient(id, msg) => self.evnt_handle_message_from_client(id, msg),
                ServerEvent::MessageFromHost(msg) => self.evnt_handle_message_from_host(msg),
                ServerEvent::ClientDisconnected(err, client_id) => self.evnt_handle_client_diconnected(err, client_id),

                ServerEvent::ServerError(err) => ServerMessage::Error(err.to_string()),
            };
            callback(msg);
        }
    }
}

impl Server {
    fn evnt_handle_new_connection(&mut self, cid: usize, conn: TcpStream) -> ServerMessage {
        let read_conn = conn;

        let write_conn = match read_conn.try_clone() {
            Ok(sock) => sock,
            Err(err) => {
                return ServerMessage::Error(err.to_string());
            }
        };

        let addr = match read_conn.peer_addr() {
            Ok(addr) => addr,
            Err(err) => {
                return ServerMessage::Error(err.to_string());
            }
        };

        let msg = format!("new connection at {addr}");

        self.connected_clients.insert(
            cid, 
            ConnectedClientInfo { 
                write_conn, 
                addr, 
                nickname: None });
        
        let tx = self.tx.clone();
        thread::spawn(move || Server::th_handle_client(cid, read_conn, tx));
        return ServerMessage::Info(msg);
    }

    fn evnt_handle_shutdown(&mut self) -> ServerMessage {
        self.running = false;
        self.shutdown_listener();
        return ServerMessage::Info(format!("Server shutdown"));
    }


    fn evnt_handle_message_from_client(&mut self, id: usize, msg: String) -> ServerMessage {
        let cilent_info = match self.connected_clients.get_mut(&id) {
            Some(c) => c,
            None => {
                return ServerMessage::Error(format!("No client info, that's weird"));
            }
        };

        let nick = cilent_info.nickname.clone().unwrap_or(String::new());
        // TODO: add function that formats message
        let fmsg = format!("({})[{};{}]: {msg}", cilent_info.addr, id, nick);
        self.send_to_others(Some(id), &fmsg);
        return ServerMessage::Msg(fmsg);
    }

    fn evnt_handle_message_from_host(&mut self, msg: String) -> ServerMessage {
        // TODO: add function that formats message
        let fmsg = format!("({}:{})[0;]: {msg}", self.host, self.port);
        self.send_to_others(None, &fmsg);
        return ServerMessage::Msg(fmsg);
    }

    fn evnt_handle_client_diconnected(&mut self, err: Option<String>, client_id: usize) -> ServerMessage {        
        self.connected_clients.remove(&client_id);
        return match err {
            Some(err_msg) => return ServerMessage::Error(format!("Client {client_id} unexpectedly disconnected:\n{err_msg}")),
            None => ServerMessage::Info(format!("Client {client_id} disconnected")), 
        };
    }

}

impl Server {
    fn shutdown_listener(&mut self) {
        *self.listener_running.lock().unwrap() = false;
        let _ = TcpStream::connect((self.host, self.port));
    }

    fn send_to_others(&mut self, sender_id: Option<usize>, msg: &str) {
        for (id, client) in self.connected_clients.iter_mut() {
            if !sender_id.is_none_or(|v| v != *id) {
                continue;
            }

            let good = match protocol::send_msg_tcp(&mut client.write_conn, &msg) {
                protocol::TcpSnd::Good => Ok(()),
                protocol::TcpSnd::IOError(err) => self.tx.send(ServerEvent::ClientDisconnected(Some(err.to_string()), id.clone())),
                protocol::TcpSnd::MsgTooLong => self.tx.send(ServerEvent::ServerError(io::Error::other(format!("Client's {id} message is too long to send"))))
            };

            if good.is_err(){
                self.running = false;
            }
        }
    }
}

impl Server {
    fn th_connections_listener(
        listener: TcpListener,
        tx: Sender<ServerEvent>,
        stay_active: Arc<Mutex<bool>>) -> () {

        // IDEA: Consider using some reliable logging utilitiles for managing channel error for debuging purposes
        
        // Blocks while waiting for new connections
        // When the server wants to kill the listener thread
        // it first set the shared stay_actice flat to false
        // and then it makes a dummy connection to itself so the listener thread wakes up and checks the shared flag
        let mut clients_count: usize = 0;
        for new_conn in listener.incoming() {
            if *stay_active.lock().unwrap() == false {
                // It doesn't matter if the channel no longer works, since the thread is shutting down anyway
                // let _ = tx.send(ServerEvent::);
                return ();
            }
            
            if let Err(_) = match new_conn {
                Ok(conn) => {
                    clients_count += 1;
                    tx.send(ServerEvent::NewConnection(clients_count, conn))
                },
                Err(err) => tx.send(ServerEvent::ServerError(err)),
            } { return (); } 
            // If the channel no longer works, 
            // this thread is useless since it can't communicate with the main server thread,
            // so it just shutdowns on it's own
        }
    }

    fn th_handle_client(id: usize, conn: TcpStream, tx: Sender<ServerEvent>) -> () {
        let mut conn = conn;
        let mut connected = true;
        while connected {
            let tcp_msg = protocol::recv_msg_tcp(&mut conn);
            let event = match tcp_msg {
                protocol::TcpRcv::GracefullyClosed => {
                    connected = false;
                    ServerEvent::ClientDisconnected(None, id)
                },
                protocol::TcpRcv::IOError(err) => {
                    connected = false;
                    ServerEvent::ClientDisconnected(Some(err.to_string()), id)
                },
                protocol::TcpRcv::InvalidUtf(err) =>  ServerEvent::ServerError(io::Error::other(err)),
                protocol::TcpRcv::ProtocolError => ServerEvent::ServerError(io::Error::other(format!("Protocol error"))),
                protocol::TcpRcv::Msg(msg) => ServerEvent::MessageFromClient(id, msg),
            };

            if tx.send(event).is_err() {
                connected = false;
            }
        }
    }
}


impl ServerMessage {
    pub fn to_string(&self) -> String {
        return match self {
            Self::Error(err) => format!("Server Error:\n{err}"),
            Self::Info(info) => format!("Server Info:\n{info}"),
            Self::Msg(msg) => format!("{msg}"),
            Self::CriticalError(err) => format!("Server Critical Error:\n{err}"),
        };
    }
}