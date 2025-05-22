use std::{io::{self, Read, Write}, net::TcpStream};

//ENDIANNESS: "big"
//FORMAT: "utf-8"
const TCP_HEADER_LENGTH: usize = 4;

pub enum TcpRcv {
    Msg(String),
    InvalidUtf(std::string::FromUtf8Error),
    IOError(io::Error),
    ProtocolError,
    GracefullyClosed,
}

pub fn recv_msg_tcp(conn: &mut TcpStream) -> TcpRcv {
    let mut header = [0_u8; TCP_HEADER_LENGTH];

    match conn.read(&mut header) {
        Err(e) => return TcpRcv::IOError(e),
        Ok(n) => match n {
            TCP_HEADER_LENGTH => (),
            0 => return TcpRcv::GracefullyClosed,
            _ => return TcpRcv::ProtocolError,
        },
    };

    let mut bmsg: Vec<u8> = Vec::new();
    let msg_length = u32::from_be_bytes(header) as usize;
    let mut recv_count = 0_usize;
    while recv_count < msg_length {
        const CHUNK_SIZE: usize = 64;
        let mut buff = [0_u8; CHUNK_SIZE];
        match conn.read(&mut buff) {
            Err(err) => return TcpRcv::IOError(err),
            Ok(n) => {
                if n == 0 {
                    return TcpRcv::GracefullyClosed;
                } else {
                    recv_count += n;
                }
            }
        }
        bmsg.extend_from_slice(&buff);
    }

    return match String::from_utf8(bmsg) {
        Err(err) => TcpRcv::InvalidUtf(err),
        Ok(msg) => TcpRcv::Msg(msg),
    };
}

pub enum TcpSnd {
    Good,
    MsgTooLong,
    IOError(io::Error),
}
impl TcpSnd {
    pub fn to_string(&self) -> String {
        match self {
            Self::Good => String::from("<Sent successfull>"),
            Self::MsgTooLong => {
                String::from("<MsgTooLong: Msg too long, max message length is 2^32 bytes>")
            }
            Self::IOError(e) => format!("<IOError: {e:?}>"),
        }
    }
}
pub fn send_msg_tcp(conn: &mut TcpStream, msg: &str) -> TcpSnd {
    let mut bmsg: Vec<u8> = Vec::new();

    let msg_bytes = msg.as_bytes();
    let msg_len: u32 = match msg_bytes.len().try_into() {
        Ok(n) => n,
        Err(_) => return TcpSnd::MsgTooLong,
    };

    let header: [u8; TCP_HEADER_LENGTH] = msg_len.to_be_bytes();
    bmsg.extend_from_slice(&header);
    bmsg.extend_from_slice(msg_bytes);

    match conn.write_all(&bmsg) {
        Ok(()) => return TcpSnd::Good,
        Err(e) => return TcpSnd::IOError(e),
    }
}