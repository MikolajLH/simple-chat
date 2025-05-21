use std::{net::{IpAddr, SocketAddr, TcpStream}, sync::mpsc::{self, Receiver, Sender}};



// Client represens the client side connection to the specific remote server
struct Client {
    server_addr: Option<SocketAddr>,
    running: bool,
    tx: Sender<ClientEvent>,
    rx: Receiver<ClientEvent>,
}

enum ClientEvent {

}

impl Client {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        return Self{
            server_addr: None,
            running: false,
            tx, rx
        };
    }

    pub fn connect<F>(&mut self, callback: F) where F: Fn(ClientEvent) -> () {
        
    }

    
    pub fn th_handle_receive(conn: TcpStream) {

    }
}