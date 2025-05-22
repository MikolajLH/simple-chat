use std::{
    io, net::Ipv4Addr, sync::mpsc::{self, Receiver, Sender}, thread
};

use crossterm::event::{KeyCode, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    text::{Line, Text},
    widgets::{Block, List, Paragraph},
};

use crate::{client, server::{self}};

// Outline:
// The application main thread runs an event loop,
// The current idea is that there will be two even source living in separate threads:
// 1. Input event listener: That handles the local inputs such as key input and so on.
// 2. Server event listener: This abstract the connection to server (so remote or hosted) and listens for the server events.
// When either of the threads received event it will pass it to the application event loop by the channel
// TODO: There needs to be a way for the application to disconnect from the server (or close it if it's hosted)
// First idea is to close the receiver, but now I realize that this will also make the input listner unrecovable
// second idea is to emit the special kind of server event, and send it to server, then the server will send back confirmation
// and when the server event listener recieves this confirmation, it will shutdown itself after passing this event to application.
// To summorize, there are three threads: event loop, ServerEvent Listener, InputEvent listnener.
// Of course the Client and Server functionalites will have more threads and so on,
// but the abstracion should make it so from the application point of view it only consist of this three threads.

// Enum encapsulating information about what's happening inside the application
// It has only informative meaning, as to describe the result of diffrent functions called inside the application
// It's supposed to be pushed into messages vector and be displayed inside console
enum AppMessage {
    LogMsg(String),
    ServerMsg(String),
    ClientMsg(String),
    ErrorMsg(io::Error),
    Empty,
    NotImplemented,
}
// When the user inputs a string inside console, it is parsed and mapped into value from this enum
// This enum acts as an interface of gateway for a user to interact with application
enum AppCommand {
    Unknown(String),
    Nothing,
    NewMessage(String),
    
    ClearScreen(Vec<String>),
    CreateHostedServer(Vec<String>),
    ShutdownHostedServer(Vec<String>),

    JoinRemoteServer(Vec<String>),
    DisconnectFromRemoteServer(Vec<String>),
}

// The application can be in three(?) states:
// - Disconnected
// - Connected to remote server
// - hosting server by itself
// This enum encapsulates the connection state of the application
enum ConnectionStatus {
    Disconnected,
    Remote(Sender<client::ClientEvent>),
    Hosted(Sender<server::ServerEvent>),
}


// Struct represeting the state of TUI Application
// exit - while this flag is true, the application runs
// messages - logs of what has happend inside the application that will be renderd in the console,
// rx, tx - channel for sending AppEvents to event loop, IDEA: maybe there is no need for the rx to be owned by the TUIAPP struct,
// instead it should be owned by the run function as the channel model is single consumer multiple producents,
// IDEA: shutting down the rx, could be a signal for all the producer threads to finish gracefully.
// IDEA: consider removing rx from the fildes, and make the tx optional instead, in the new function, it will be set to None, and in the run function it will be set to Some :P
// This way the application can clone it when creating the server/client thread.
// connection_status - interface for the connection to the server. This fields should allow the application to receive ServerEvents.
// input - state of the TUI part that is responsible for accepting user input
pub struct TUIApp {
    exit: bool,
    messages: Vec<AppMessage>,
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
    connection_status: ConnectionStatus,
    input: InputField,
}

// struct representing the input field in the application
// It provides functionality that coupled with corresponding crossterm event handlers
// will simulate the behaviour of text input form, so backspace, moving the cursor and of course submiting the message
// It emits a string that is to be parsed into AppCommand enum type
struct InputField {
    cursor_index: usize,
    buff: String,
}

// Enum aggregating different events that can happen inside the application
// Each event has to be handled.
// The application works inside an event loop, so through the events the application will change its state.
enum AppEvent {
    KeyInput(crossterm::event::KeyEvent),
    ServerEvent(server::ServerMessage),
    ClientEvent(client::ClientMessage),
    Error(io::Error),
}

// public interface of the application
impl TUIApp {
    // create new Application instance
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        return Self {
            exit: false,
            messages: Vec::new(),
            tx,
            rx,
            input: InputField {
                cursor_index: 0,
                buff: String::new(),
            },
            connection_status: ConnectionStatus::Disconnected,
        };
    }

    // spawns a crossterm even listner thread
    // and starts an event loop:
    // 1. receive event
    // 2. handle event
    // 3. render application
    // 4. repeat
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let ctx = self.tx.clone();
        std::thread::spawn(move || TUIApp::th_crossterm_event_listener(ctx));

        while !self.exit {
            let app_event = self.rx.recv().map_err(io::Error::other)?;
            let good = match app_event {
                AppEvent::KeyInput(e) => self.evnt_handle_key_input(e),
                AppEvent::Error(err) => self.evnt_handle_error(err),
                AppEvent::ServerEvent(server_event) => self.evnt_handle_server_event(server_event),
                AppEvent::ClientEvent(client_event) => self.evnt_handle_client_event(client_event),
            };
            // TODO: either map the error directly to AppMessage and push it to the vector or emit another AppEvent::Error
            good.expect("error occured while handling app events");
            terminal.draw(|frame| self.draw(frame))?;
        }
        return Ok(());
    }
}

// Event handlers
impl TUIApp {
    // IDEA: event handlers return the result type, but I wonder if that is necessery,
    // and also, for the application architecture to be consistent the error returned by an event handler should be mapped to AppEvent error,
    // that will once again be handled by an event handler, so I guess there need to be certainty that the error event handler won't emit any errors?

    // this function maps the key input to corresponding action on the server
    // TODO: For now the key inputs are only relevant for the inputField, consider adding functionality for more keyEvents, maybe some editing mode or something
    fn evnt_handle_key_input(&mut self, key_event: crossterm::event::KeyEvent) -> io::Result<()> {
        if key_event.kind == KeyEventKind::Press {
            match key_event.code {
                KeyCode::Esc => self.exit = true,
                KeyCode::Backspace => self.input.delete_char(),
                KeyCode::Char(c) => self.input.enter_char(c),
                KeyCode::Left => self.input.move_cursor_left(),
                KeyCode::Right => self.input.move_cursor_right(),
                KeyCode::Enter => self.submit(),
                _ => {}
            }
        }
        return Ok(());
    }

    // IDEA: this function is supposed to handle the error event, but I don't see anyother way of handling the error
    // than just wrapping it in AppMessage and pushing it to the vector,
    fn evnt_handle_error(&mut self, err: io::Error) -> io::Result<()> {
        self.messages.push(AppMessage::ErrorMsg(err));
        return Ok(());
    }

    //
    fn evnt_handle_server_event(&mut self, server_event: server::ServerMessage) -> io::Result<()> {
        self.messages.push(AppMessage::LogMsg(server_event.to_string()));

        return Ok(());
    }

    fn evnt_handle_client_event(&mut self, client_event: client::ClientMessage) -> io::Result<()> {
        self.messages.push(AppMessage::LogMsg(client_event.to_string()));

        return Ok(());
    }
}

// Commands handlers
impl TUIApp {
    // IDEA: commands handlers should probably return result type in case of an error?

    // the flow is:
    // submit ->
    //    1. parse_app_command
    //    2. handle_app_command
    //    3. cmd_<command> ( -> emits corresponding event)
    //    4. push AppMessage to messages vector.
    fn submit(&mut self) {
        let input_msg = self.input.get_and_clear();
        let cmd = self.parse_app_command(&input_msg);
        let app_msg = self.handle_app_command(cmd);

        self.messages.push(app_msg);
    }

    // parses string into AppCommand type
    fn parse_app_command(&self, cmd_str: &str) -> AppCommand {
        let mut parts = cmd_str.split_whitespace();
        let command = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(String::from).collect();
        if command.is_empty(){
            return AppCommand::Nothing;
        }
        if command.starts_with(':') {
            let cmd = match command.as_str() {
                ":cls" => AppCommand::ClearScreen(args),    
                ":create" => AppCommand::CreateHostedServer(args),
                ":shutdown" => AppCommand::ShutdownHostedServer(args),
                ":join" => AppCommand::JoinRemoteServer(args),
                ":exit" => AppCommand::DisconnectFromRemoteServer(args),
                _ => AppCommand::Unknown(command),
            };
            return cmd;
        }

        return AppCommand::NewMessage(String::from(cmd_str));
    }

    // maps the AppCommand into function that handles it
    fn handle_app_command(&mut self, cmd: AppCommand) -> AppMessage {
        return match cmd {
            AppCommand::ClearScreen(args) => self.cmd_clear_screen(args),
            AppCommand::Unknown(unknown) => self.cmd_unknown_command(unknown),
            AppCommand::NewMessage(msg) => self.cmd_new_message(msg),
            AppCommand::CreateHostedServer(args) => self.cmd_start_hosted_server(args),
            AppCommand::ShutdownHostedServer(args) => self.cmd_stop_hosted_server(args),

            AppCommand::JoinRemoteServer(args) => self.cmd_connect_to_remote_server(args),
            AppCommand::DisconnectFromRemoteServer(args) => self.cmd_disconnect_from_remote_server(args),
            AppCommand::Nothing => AppMessage::Empty,
        };
    }

    fn cmd_new_message(&mut self, msg: String) -> AppMessage {
        return match &self.connection_status {
            ConnectionStatus::Disconnected => {
                AppMessage::ErrorMsg(io::Error::other(format!("Unknown command, you are not connected, type :help to see possible commands")))
            },
            ConnectionStatus::Hosted(htx) => {
                if htx.send(server::ServerEvent::MessageFromHost(msg)).is_err(){

                }

                AppMessage::Empty
            },
            ConnectionStatus::Remote(rtx) => {
                if rtx.send(client::ClientEvent::SendMsg(msg)).is_err() {
                    
                }
                AppMessage::Empty
            }
        };
    }

    fn cmd_unknown_command(&self, unknown: String) -> AppMessage {
        return AppMessage::ErrorMsg(io::Error::other(format!("Unknown command: {unknown}")));
    }

    fn cmd_start_hosted_server(&mut self, args: Vec<String>) -> AppMessage {
        
        if !matches!(self.connection_status, ConnectionStatus::Disconnected) {
            return AppMessage::ErrorMsg(io::Error::other(format!("Already started")))
        }
        //let _ = format!("{host}:{port}");
        //return AppMessage::NotImplemented;
        // // TODO: this function needs rewrite, its current form is to allow testing
        let mut srv = server::Server::new(std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        let app_tx = self.tx.clone();
        let srv_tx = srv.clone_tx();

        std::thread::spawn(move || {
            srv.run(move |msg| {
                app_tx
                    .send(AppEvent::ServerEvent(msg))
                    .expect("Sender stopped working unexpectedly");
            });
        });

        self.connection_status = ConnectionStatus::Hosted(srv_tx);
        return AppMessage::LogMsg(format!("Server running!"));
    }

    fn cmd_stop_hosted_server(&mut self, args: Vec<String>) -> AppMessage {
        // TODO: implement this function
        if let ConnectionStatus::Hosted(tx) = &self.connection_status {
            tx.send(server::ServerEvent::Shutdown).unwrap();
            return AppMessage::LogMsg(format!("Sent shutdown"));
        }
        return AppMessage::NotImplemented;
    }

    fn cmd_connect_to_remote_server(&mut self, args: Vec<String>) -> AppMessage {
        if !matches!(self.connection_status, ConnectionStatus::Disconnected) {
            return AppMessage::ErrorMsg(io::Error::other(format!("Something is wrong")))
        }

        let mut clt = client::Client::new();

        let app_tx = self.tx.clone();
        let clt_tx = clt.clone_tx();

        std::thread::spawn(move || {
            clt.connect_and_run(
                std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
                move |msg| {
                app_tx
                    .send(AppEvent::ClientEvent(msg))
                    .expect("Sender stopped working unexpectedly");
            });
        });

        self.connection_status = ConnectionStatus::Remote(clt_tx);
        return AppMessage::LogMsg(format!("Client is running!"));
    }

    fn cmd_disconnect_from_remote_server(&mut self, args: Vec<String>) -> AppMessage {
        if let ConnectionStatus::Remote(tx) = &self.connection_status {            
            tx.send(client::ClientEvent::DisconnectAndExit).unwrap();
            return AppMessage::LogMsg(format!("Sent shutdown"));
        }
        return AppMessage::NotImplemented;
    }

    // pulls the history from the chat
    // IDEA: filter how many messages to pull and maybe from whom?
    // fn cmd_pull_server_messages(&mut self) -> AppMessage {
    //     // TODO: implement this function
    //     return AppMessage::NotImplemented;
    // }

    // Idea: optional argument to clear only selected kinds of AppMessages, like only remove AppMessages::Error
    fn cmd_clear_screen(&mut self, args: Vec<String>) -> AppMessage {
        self.messages.clear();
        return AppMessage::Empty;
    }
}

// Section for the internal functions of the application that are the category of their own
impl TUIApp {
    fn draw(&mut self, frame: &mut Frame) {
        let [messages_area, input_area] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(frame.area());

        let text: List = self.messages.iter()
            .map(|m| m.style())
            .filter(|m| m.is_some())
            .map(|m| m.unwrap()).collect();

        //let messages = List::new()
        //let top_p = Paragraph::new("Top").block(Block::new().borders(Borders::ALL));
        frame.render_widget(
            text.block(Block::bordered().title("Messages")),
            messages_area,
        );

        frame.render_widget(
            Paragraph::new(self.input.get())
                .style(Style::default().fg(Color::Yellow))
                .block(Block::bordered().title("Input")),
            input_area,
        );
    }

    fn th_crossterm_event_listener(tx: mpsc::Sender<AppEvent>) {
        let last = loop {
            let event = match crossterm::event::read() {
                Err(err) => break io::Error::other(err),
                Ok(e) => e,
            };

            // This part maps the crossterm event to AppEvent and sends it to the event loop
            // For now if sending the Event through channel returns an error the thread will call panic!.
            // IDEA: In case the channel is not working anymore, consider just gracefully shuting down the thread
            // since it probably mean that application is not working anyway, so there is no need to call panic.
            // IDEA: research thread safe writing to file for logging purposes
            // TODO: consider including more crossterm events besides the key_event e.g. resize etc.
            let res = match event {
                crossterm::event::Event::Key(key_event) => tx.send(AppEvent::KeyInput(key_event)),
                _ => Ok(()),
            };

            if res.is_err() {
                panic!("Sender Error: {}", res.unwrap_err());
            }
        };
        tx.send(AppEvent::Error(last))
            .expect("Sender Error, after loop completed");
    }
}

impl InputField {
    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        return new_cursor_pos.clamp(0, self.buff.chars().count());
    }

    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.cursor_index.saturating_sub(1);
        self.cursor_index = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.cursor_index.saturating_add(1);
        self.cursor_index = self.clamp_cursor(cursor_moved_right);
    }

    fn reset_cursor(&mut self) {
        self.cursor_index = 0;
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.cursor_index != 0;
        if is_not_cursor_leftmost {
            let current_index = self.cursor_index;
            let from_left_to_current_index = current_index - 1;

            let before_char_to_delete = self.buff.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.buff.chars().skip(current_index);

            self.buff = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    fn byte_index(&self) -> usize {
        self.buff
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.cursor_index)
            .unwrap_or(self.buff.len())
    }

    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.buff.insert(index, new_char);
        self.move_cursor_right();
    }

    fn get_and_clear(&mut self) -> String {
        let string = self.buff.clone();
        self.buff.clear();
        self.reset_cursor();
        return string;
    }

    fn get(&self) -> &str {
        return &self.buff;
    }
}

impl AppMessage {
    // The style command maps the AppMessage to ratatui's Text widget
    // TODO: Implement different styles for different enum kinds, e.g. Error should probably be red server maybe normal, logging maybe yellow and so on.
    fn style(&self) -> Option<Text> {
        match self {
            Self::LogMsg(msg) => {
                let mut text = Vec::new();
                for line in msg.split("\n") {
                    text.push(Line::from(line).green());
                }
                Some(Text::from(text))
            }
            Self::ErrorMsg(err) =>  Some(Text::from(format!("{err}")).red()),
            Self::Empty => None,
            Self::ServerMsg(msg) => Some(Text::from(format!("ServerMsg: {msg}")).yellow()),
            Self::ClientMsg(msg) => Some(Text::from(format!("ClientMsg: {msg}")).blue()),
            _ => Some(Text::from("not implemented").yellow()),
        }
    }
}
