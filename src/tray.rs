//! Notification area icon with a small menu to reopen or quit the application.

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::app_icon::render_logo_rgba;

const TRAY_ICON_SIZE: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayRequest {
    ShowWindow,
    Quit,
}

/// Creates the tray icon. `on_request` runs on the UI thread as soon as the user clicks,
/// even while the window is hidden.
pub fn create_tray_icon(on_request: impl Fn(TrayRequest) + Send + Sync + Clone + 'static) -> anyhow::Result<TrayIcon> {
    let open_item = MenuItem::new("Open Joystick Manager", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&open_item, &PredefinedMenuItem::separator(), &quit_item])?;

    forward_menu_clicks(&open_item, &quit_item, on_request.clone());
    forward_icon_clicks(on_request);

    let tray_icon = TrayIconBuilder::new()
        .with_tooltip("Joystick Manager")
        .with_icon(Icon::from_rgba(
            render_logo_rgba(TRAY_ICON_SIZE),
            TRAY_ICON_SIZE,
            TRAY_ICON_SIZE,
        )?)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()?;
    Ok(tray_icon)
}

fn forward_menu_clicks(
    open_item: &MenuItem,
    quit_item: &MenuItem,
    on_request: impl Fn(TrayRequest) + Send + Sync + 'static,
) {
    let open_item_id = open_item.id().clone();
    let quit_item_id = quit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_item_id {
            on_request(TrayRequest::ShowWindow);
        } else if event.id == quit_item_id {
            on_request(TrayRequest::Quit);
        }
    }));
}

fn forward_icon_clicks(on_request: impl Fn(TrayRequest) + Send + Sync + 'static) {
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let is_left_click = matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
        );
        if is_left_click {
            on_request(TrayRequest::ShowWindow);
        }
    }));
}
