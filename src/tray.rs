//! KDE system tray entry, via StatusNotifierItem.

use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem};

use crate::ipc::{CmdSender, Command};

pub struct Tray {
    pub visible: bool,
    pub tx: CmdSender,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "ultrawide-side-saver".into()
    }

    fn title(&self) -> String {
        "Ultrawide Side Saver".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Ultrawide Side Saver".into(),
            description: if self.visible {
                "Side bars on".into()
            } else {
                "Side bars off".into()
            },
            ..Default::default()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![icon(22, self.visible), icon(44, self.visible)]
    }

    /// Left-clicking the tray icon toggles, same as the keyboard shortcut.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.tx.send(Command::Toggle);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            CheckmarkItem {
                label: "Side bars".into(),
                checked: self.visible,
                activate: Box::new(|this: &mut Self| this.tx.send(Command::Toggle)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Reload config".into(),
                activate: Box::new(|this: &mut Self| this.tx.send(Command::Reload)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|this: &mut Self| this.tx.send(Command::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// A tiny ultrawide-monitor glyph with its side bars lit or dark, drawn directly
/// rather than shipped as a file so the binary has no icon-theme dependency.
///
/// Returns ARGB32 in network byte order, as StatusNotifierItem requires.
fn icon(size: i32, active: bool) -> Icon {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];

    let s = size as f32 / 22.0;
    let px = |v: f32| (v * s).round() as i32;

    // A 20x10 screen centred in the 22x22 canvas: roughly 2:1, i.e. ultrawide.
    let (x0, y0, x1, y1) = (px(1.0), px(6.0), px(21.0), px(16.0));
    let bar_w = px(3.0).max(1);
    let frame = [0xB0u8, 0xC8, 0xCC, 0xD0];
    let bar = if active {
        [0xFFu8, 0x5A, 0xC8, 0xB4]
    } else {
        [0xFFu8, 0x50, 0x54, 0x58]
    };
    let centre = [0xFFu8, 0x10, 0x11, 0x13];

    let mut put = |x: i32, y: i32, c: [u8; 4]| {
        if x < 0 || y < 0 || x >= size || y >= size {
            return;
        }
        let i = (y as usize * n + x as usize) * 4;
        data[i..i + 4].copy_from_slice(&c);
    };

    for y in y0..y1 {
        for x in x0..x1 {
            let on_frame = x < x0 + px(1.0).max(1)
                || x >= x1 - px(1.0).max(1)
                || y < y0 + px(1.0).max(1)
                || y >= y1 - px(1.0).max(1);
            let inner_l = x0 + px(1.0).max(1);
            let inner_r = x1 - px(1.0).max(1);
            let c = if on_frame {
                frame
            } else if x < inner_l + bar_w || x >= inner_r - bar_w {
                bar
            } else {
                centre
            };
            put(x, y, c);
        }
    }

    Icon {
        width: size,
        height: size,
        data,
    }
}
