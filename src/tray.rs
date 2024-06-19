use std::{process::exit, sync::mpsc::{Receiver, Sender}, thread::{self, JoinHandle}};

use trayicon::{Icon, MenuBuilder, MenuItem, TrayIcon, TrayIconBuilder};

use crate::command::MyCommand;


#[derive(Clone, Eq, PartialEq, Debug)]
enum TrayEvents {
    RightClickTrayIcon,
    LeftClickTrayIcon,
    DoubleClickTrayIcon,
    Exit,
    ChangeToRed,
    ChangeToGreen,
    ChangeMenu,
    ToolTip,
    HideWindow,
    ShowWindow,
    DisabledItem1,
    CheckItem1,
    SubItem1,
    SubItem2,
    SubItem3,
}

pub struct MyTray {
    tray: TrayIcon<TrayEvents>,
    #[allow(dead_code)]
    icons: Vec<Icon>,
    cmd_sender: Sender<MyCommand>,
    rx: Receiver<TrayEvents>,
}
impl MyTray {
    pub fn new(sc: Sender<MyCommand>) -> Self {
        let (sx, rx) = std::sync::mpsc::channel::<TrayEvents>();
        let icon = vec![
            include_bytes!("../assets/icon.ico"),
            include_bytes!("../assets/icon.ico"),
        ];
        let icons: Vec<Icon> = icon
            .into_iter()
            .map(|i| Icon::from_buffer(i, None, None).unwrap())
            .collect();
        let tray = TrayIconBuilder::new()
            .sender(move |e: &TrayEvents| {
                // let _ = proxy.send_event(e.clone());
                sx.send(e.clone()).unwrap();
            })
            .icon(icons[0].clone())
            .tooltip("Cool Tray 👀 Icon")
            .on_click(TrayEvents::LeftClickTrayIcon)
            .on_double_click(TrayEvents::DoubleClickTrayIcon)
            .on_right_click(TrayEvents::RightClickTrayIcon)
            .menu(
                MenuBuilder::new()
                    .item("Item 1 Change Icon Red", TrayEvents::ChangeToRed)
                    .item("Item 2 Change Icon Green", TrayEvents::ChangeToGreen)
                    .item("Item 3 Replace Menu 👍", TrayEvents::ChangeMenu)
                    .item("Item 4 Set Tooltip", TrayEvents::ToolTip)
                    .separator()
                    .item("Hide Window", TrayEvents::HideWindow)
                    .item("Show Window", TrayEvents::ShowWindow)
                    .separator()
                    .submenu(
                        "Sub Menu",
                        MenuBuilder::new()
                            .item("Sub item 1", TrayEvents::SubItem1)
                            .item("Sub Item 2", TrayEvents::SubItem2)
                            .item("Sub Item 3", TrayEvents::SubItem3),
                    )
                    .checkable(
                        "This checkbox toggles disable",
                        true,
                        TrayEvents::CheckItem1,
                    )
                    .with(MenuItem::Item {
                        name: "Item Disabled".into(),
                        disabled: true, // Disabled entry example
                        id: TrayEvents::DisabledItem1,
                        icon: Some(icons[0].clone()),
                    })
                    .separator()
                    .item("Exit", TrayEvents::Exit),
            )
            .build()
            .unwrap();
        Self {
            tray,
            icons,
            cmd_sender: sc,
            rx,
        }
    }

    pub fn run(mut self) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(r) = self.rx.recv() {
                match r {
                    TrayEvents::Exit => {
                        exit(0);
                    }
                    TrayEvents::RightClickTrayIcon => {
                        self.tray.show_menu().unwrap();
                    }
                    TrayEvents::LeftClickTrayIcon => {
                        self.tray.show_menu().unwrap();
                    }
                    TrayEvents::HideWindow => {
                        self.cmd_sender.send(MyCommand::TrayHide).unwrap();
                    }
                    TrayEvents::ShowWindow => {
                        self.cmd_sender.send(MyCommand::TrayShow).unwrap();
                    }
                    e => println!("{:#?}", e),
                }
            }
        })
    }
}


// impl ApplicationHandler<TrayEvents> for MyApplication {
//     fn resumed(&mut self, event_loop: &ActiveEventLoop) {
//         self.window = Some(
//             event_loop
//                 .create_window(Window::default_attributes())
//                 .unwrap(),
//         );
//     }

//     // Platform specific events
//     fn window_event(
//         &mut self,
//         event_loop: &ActiveEventLoop,
//         _window_id: winit::window::WindowId,
//         event: WindowEvent,
//     ) {
//         match event {
//             WindowEvent::CloseRequested => {
//                 event_loop.exit();
//             }
//             e => {
//                 println!("Window event: {:?}", e);
//             }
//         }
//     }

//     // Application specific events
//     fn user_event(&mut self, event_loop: &ActiveEventLoop, event: TrayEvents) {
//         match event {
//             TrayEvents::Exit => event_loop.exit(),
//             TrayEvents::RightClickTrayIcon => {
//                 self.tray_icon.show_menu().unwrap();
//             }
//             TrayEvents::CheckItem1 => {
//                 // You can mutate single checked, disabled value followingly.
//                 //
//                 // However, I think better way is to use reactively
//                 // `set_menu` by building the menu based on application
//                 // state.
//                 if let Some(old_value) = self
//                     .tray_icon
//                     .get_menu_item_checkable(TrayEvents::CheckItem1)
//                 {
//                     // Set checkable example
//                     let _ = self
//                         .tray_icon
//                         .set_menu_item_checkable(TrayEvents::CheckItem1, !old_value);

//                     // Set disabled example
//                     let _ = self
//                         .tray_icon
//                         .set_menu_item_disabled(TrayEvents::DisabledItem1, !old_value);
//                 }
//             }
//             TrayEvents::ChangeToRed => {
//                 self.tray_icon.set_icon(&self.second_icon).unwrap();
//             }
//             TrayEvents::ChangeToGreen => {
//                 self.tray_icon.set_icon(&self.first_icon).unwrap();
//             }
//             TrayEvents::ChangeMenu => {
//                 self.tray_icon
//                     .set_menu(
//                         &MenuBuilder::new()
//                             .item("Another item", TrayEvents::ChangeToRed)
//                             .item("Exit", TrayEvents::Exit),
//                     )
//                     .unwrap();
//             }
//             TrayEvents::ToolTip => {
//                 self.tray_icon.set_tooltip("Menu changed!").unwrap();
//             }
//             TrayEvents::LeftClickTrayIcon => {
//                 self.tray_icon.show_menu().unwrap();
//             }
//             // Events::DoubleClickTrayIcon => todo!(),
//             // Events::DisabledItem1 => todo!(),
//             // Events::SubItem1 => todo!(),
//             // Events::SubItem2 => todo!(),
//             // Events::SubItem3 => todo!(),
//             _ => {}
//         }
//     }
// }
