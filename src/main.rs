mod app;
mod config;
mod ipc;
mod palette;
mod render;
mod tray;

use anyhow::Result;
use config::Config;
use ipc::Command;

const USAGE: &str = "\
ultrawide-side-saver - animated side bars for the unused edges of an ultrawide OLED

USAGE:
    ultrawide-side-saver [run]      Start the daemon (tray icon + D-Bus control)
    ultrawide-side-saver toggle     Toggle the bars on the running daemon
    ultrawide-side-saver show
    ultrawide-side-saver hide
    ultrawide-side-saver reload     Re-read the config file
    ultrawide-side-saver quit       Stop the daemon
    ultrawide-side-saver init-config  Write a commented default config
    ultrawide-side-saver outputs    List outputs and the bar width for each
    ultrawide-side-saver --help
";

fn main() {
    if let Err(e) = real_main() {
        eprintln!("ultrawide-side-saver: {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("run") => app::run(Config::load()?),
        Some("toggle") => ipc::send(Command::Toggle),
        Some("show") => ipc::send(Command::Show),
        Some("hide") => ipc::send(Command::Hide),
        Some("reload") => ipc::send(Command::Reload),
        Some("quit") => ipc::send(Command::Quit),
        Some("init-config") => {
            let path = Config::write_default_if_missing()?;
            println!("{}", path.display());
            Ok(())
        }
        Some("outputs") => outputs(),
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("ultrawide-side-saver {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => {
            eprint!("unknown command {other:?}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}

/// Print what the daemon would do for each connected output, so the config can
/// be checked without starting anything.
fn outputs() -> Result<()> {
    let cfg = Config::load()?;
    println!(
        "content_aspect = {} ({:.4}), palette = {} (available: {})",
        cfg.content_aspect,
        cfg.aspect()?,
        cfg.palette,
        palette::names().join(", ")
    );
    for (name, w, h) in app::list_outputs()? {
        let bar = cfg.bar_width_for(w, h)?;
        let note = if bar == 0 {
            "no room for bars".to_string()
        } else {
            format!("{bar}px bars, {}px content", w - bar * 2)
        };
        println!("  {name:<12} {w}x{h}  ->  {note}");
    }
    Ok(())
}
