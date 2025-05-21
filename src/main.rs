mod application;
mod client;
mod protocol;
mod server;

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();

    let mut app = application::TUIApp::new();
    let res = app.run(&mut terminal);

    ratatui::restore();
    return res;
}
