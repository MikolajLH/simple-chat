use std::{io, net::{IpAddr, TcpListener, TcpStream}, sync::{mpsc::{self, Receiver, Sender}, Arc, Mutex}, thread};

use crate::protocol;


pub struct Server {
    host: IpAddr, port: u16,
    running: bool,
    listener_running: Arc<Mutex<bool>>,
    tx: Sender<ServerEvent>,
    rx: Receiver<ServerEvent>,
}

pub enum ServerEvent {
    NewConnection(TcpStream),
    ServerError(io::Error),
    MessageFromClient(usize, String),
    Shutdown,
}

pub enum ServerMessage {
    Info(String),
    Error(String),
}

impl Server {
    pub fn new(host: IpAddr, port: u16) -> Self {
        let listener_running = Arc::new(Mutex::new(false));
        let (tx, rx) = mpsc::channel();
        return Self {
            host, port,
            running: false,
            listener_running,
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
                callback(ServerMessage::Error(err.to_string()));
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
                    callback(ServerMessage::Error(err.to_string()));
                    return ();
                }
            };
            let msg = match server_event {
                ServerEvent::NewConnection(conn) => self.evnt_handle_new_connection(conn),
                ServerEvent::Shutdown => self.evnt_handle_shutdown(),
                ServerEvent::MessageFromClient(id, msg) => self.evnt_handle_message_from_client(id, msg),


                ServerEvent::ServerError(err) => ServerMessage::Error(err.to_string()),
            };
            callback(msg);
        }
    }
}

impl Server {
    fn evnt_handle_new_connection(&mut self, conn: TcpStream) -> ServerMessage {
        // TODO
        return match conn.peer_addr() {
            Ok(addr) => ServerMessage::Info(format!("new connection at {addr}")),
            Err(err) => ServerMessage::Error(err.to_string()),
        };
    }

    fn evnt_handle_shutdown(&mut self) -> ServerMessage {
        self.running = false;
        self.shutdown_listener();
        return ServerMessage::Info(format!("Server shutdown"));
    }


    fn evnt_handle_message_from_client(&mut self, id: usize, msg: String) -> ServerMessage {


        return  ServerMessage::Error(format!("Not implemented!"));
    }
}

impl Server {
    fn shutdown_listener(&mut self) {
        *self.listener_running.lock().unwrap() = false;
        let _ = TcpStream::connect((self.host, self.port));
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
        for new_conn in listener.incoming() {
            if *stay_active.lock().unwrap() == false {
                // It doesn't matter if the channel no longer works, since the thread is shutting down anyway
                // let _ = tx.send(ServerEvent::);
                return ();
            }

            if let Err(_) = match new_conn {
                Ok(conn) => tx.send(ServerEvent::NewConnection(conn)),
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
            if tcp_msg.connection_closed() {
                connected = false;
            }
            if tx.send(ServerEvent::MessageFromClient(id, format!("New Message :3"))).is_err() {
                connected = false;
            }
        }
    }
}


impl ServerMessage {
    pub fn to_string(&self) -> String {
        return match self {
            Self::Error(err) => format!("Error: {err}"),
            Self::Info(info) => format!("Info: {info}"),
        };
    }
}