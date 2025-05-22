use std::{net::{IpAddr, SocketAddr, TcpStream}, sync::{mpsc::{self, Receiver, Sender}}, thread};

use crate::protocol;



// Client represens the client side connection to the specific remote server
pub struct Client {
    server_addr: Option<SocketAddr>,
    conn: Option<TcpStream>,
    running: bool,
    tx: Sender<ClientEvent>,
    rx: Receiver<ClientEvent>,
}

pub enum ClientEvent {
    RecvMsg(protocol::TcpRcv),
    SendMsg(String),
    DisconnectAndExit,
}

pub enum ClientMessage {
    Msg(String),
    MsgAck,
    Internal(String),
    Error(String),
}

impl Client {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        return Self{
            server_addr: None,
            conn: None,
            running: false,
            tx, rx
        };
    }

    pub fn clone_tx(&self) -> Sender<ClientEvent> {
        return self.tx.clone();
    }

    pub fn connect_and_run<F>(&mut self, host: IpAddr, port: u16, callback: F) where F: Fn(ClientMessage) -> () {
        
        let server_addr = SocketAddr::from((host, port));
        let conn_tcp = match TcpStream::connect(server_addr){
            Ok(conn) => conn,
            Err(err) => {
                callback(ClientMessage::Error(err.to_string()));
                return;
            }
        };
        self.server_addr = Some(server_addr);
        let tx = self.tx.clone();
        let read_conn = match conn_tcp.try_clone() {
            Ok(conn) => conn,
            Err(err) => {
                callback(ClientMessage::Error(err.to_string()));
                return;
            }
        };
        self.conn = Some(conn_tcp);
        thread::spawn(move || Client::th_handle_receive(read_conn, tx));
        self.running = true;
        while self.running {
            let client_event = match self.rx.recv() {
                Ok(e) => e,
                Err(err) => {
                    callback(ClientMessage::Error(err.to_string()));
                    return;
                }
            };

            let msg = match client_event {
                ClientEvent::RecvMsg(rcv_msg) => self.evnt_handle_recv_msg(rcv_msg),
                ClientEvent::SendMsg(snd_msg) => self.evnt_handle_send_msg(snd_msg),
                ClientEvent::DisconnectAndExit => self.evnt_handle_disconnect_and_exit(),
            };
            callback(msg);
        }
    }

    
    pub fn th_handle_receive(conn: TcpStream, tx: Sender<ClientEvent>) {
        let mut conn = conn;
        let mut connected = true;
        while connected {
            let tcp_msg = protocol::recv_msg_tcp(&mut conn);
            connected = !matches!(tcp_msg, protocol::TcpRcv::IOError(_)) && !matches!(tcp_msg, protocol::TcpRcv::GracefullyClosed);
            if tx.send(ClientEvent::RecvMsg(tcp_msg)).is_err() {
                connected = false;
            }
        }
    }
}

impl Client {
    fn evnt_handle_recv_msg(&mut self, msg: protocol::TcpRcv) -> ClientMessage {
        let clnt_msg = match msg {
            protocol::TcpRcv::GracefullyClosed => ClientMessage::Internal(format!("gracefully closed")),
            protocol::TcpRcv::IOError(err) => ClientMessage::Error(err.to_string()),
            protocol::TcpRcv::InvalidUtf(err) => ClientMessage::Error(err.to_string()),
            protocol::TcpRcv::ProtocolError => ClientMessage::Error(format!("protocol error")),

            protocol::TcpRcv::Msg(msg) => ClientMessage::Msg(msg),
        };
        return clnt_msg;
    }

    fn evnt_handle_disconnect_and_exit(&mut self) -> ClientMessage {
        if let Some(write_conn) = &self.conn {
            self.running = false;
            if let Err(err) = write_conn.shutdown(std::net::Shutdown::Both) {
                return ClientMessage::Error(err.to_string());
            }
            return ClientMessage::Internal(format!("Client disconnected and exited"));
        }
        return ClientMessage::Error(format!("Client is not connected"));
    }


    fn evnt_handle_send_msg(&mut self, msg: String) -> ClientMessage {
        if let Some(write_conn) = &mut self.conn {
            let clnt_msg = match protocol::send_msg_tcp(write_conn, &msg) {
                protocol::TcpSnd::Good => ClientMessage::MsgAck,
                protocol::TcpSnd::IOError(err) => ClientMessage::Error(err.to_string()),
                protocol::TcpSnd::MsgTooLong => ClientMessage::Error(format!("Message too long")),
            };
            return clnt_msg;
        }
        return ClientMessage::Error(format!("Not implemented"));
    }
}

impl ClientMessage {
    pub fn to_string(&self) -> String {
        return match self {
            Self::Error(err) => format!("Client Error: {err}"),
            Self::Internal(info) => format!("Client Info: {info}"),
            Self::Msg(msg ) => format!("{msg}"),
            Self::MsgAck => format!(""),
        };
    }
}