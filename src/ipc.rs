//! Control channel. The daemon owns a D-Bus object; `ultrawide-side-saver toggle`
//! is just a client for it, which is also what the KDE global shortcut invokes.

use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};

pub const BUS_NAME: &str = "com.gamerh2.UltrawideSideSaver";
pub const OBJECT_PATH: &str = "/com/gamerh2/UltrawideSideSaver";
pub const INTERFACE: &str = "com.gamerh2.UltrawideSideSaver1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Show,
    Hide,
    Toggle,
    Reload,
    Quit,
}

impl Command {
    pub fn dbus_method(self) -> &'static str {
        match self {
            Command::Show => "Show",
            Command::Hide => "Hide",
            Command::Toggle => "Toggle",
            Command::Reload => "Reload",
            Command::Quit => "Quit",
        }
    }
}

/// A `Send + Sync` handle onto the calloop channel that wakes the render thread.
/// calloop's `Sender` is `!Sync`, and both the tray and D-Bus threads need one.
#[derive(Clone)]
pub struct CmdSender(Arc<Mutex<calloop::channel::Sender<Command>>>);

impl CmdSender {
    pub fn new(tx: calloop::channel::Sender<Command>) -> Self {
        Self(Arc::new(Mutex::new(tx)))
    }

    pub fn send(&self, cmd: Command) {
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(cmd);
        }
    }
}

struct Service {
    tx: CmdSender,
}

#[zbus::interface(name = "com.gamerh2.UltrawideSideSaver1")]
impl Service {
    fn show(&self) {
        self.tx.send(Command::Show);
    }
    fn hide(&self) {
        self.tx.send(Command::Hide);
    }
    fn toggle(&self) {
        self.tx.send(Command::Toggle);
    }
    fn reload(&self) {
        self.tx.send(Command::Reload);
    }
    fn quit(&self) {
        self.tx.send(Command::Quit);
    }
}

/// Claim the well-known name and serve the control interface. The returned
/// connection must be kept alive for the lifetime of the daemon.
///
/// Claiming the name is also the single-instance check. The name is requested
/// explicitly rather than through `Builder::name`, because the builder's default
/// flags include `ReplaceExisting`/`AllowReplacement`: a second daemon would then
/// silently steal the name from the first and both would keep drawing.
pub fn serve(tx: CmdSender) -> Result<zbus::blocking::Connection> {
    use zbus::fdo::{RequestNameFlags, RequestNameReply};

    let conn = zbus::blocking::connection::Builder::session()
        .context("connecting to the session bus")?
        .serve_at(OBJECT_PATH, Service { tx })
        .context("exporting the control interface")?
        .build()
        .context("building the D-Bus connection")?;

    match conn
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .context("requesting the bus name")?
    {
        RequestNameReply::PrimaryOwner => Ok(conn),
        _ => anyhow::bail!("another instance is already running (it owns {BUS_NAME})"),
    }
}

/// Send one command to a running daemon.
pub fn send(cmd: Command) -> Result<()> {
    let conn = zbus::blocking::Connection::session().context("connecting to the session bus")?;
    conn.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        cmd.dbus_method(),
        &(),
    )
    .with_context(|| format!("calling {}() - is the daemon running?", cmd.dbus_method()))?;
    Ok(())
}
