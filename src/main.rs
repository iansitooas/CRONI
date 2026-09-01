#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod blocker;
mod config;
mod default_browser;
mod downloads;
#[cfg(target_os = "windows")]
mod native_chrome;
mod navigation;
mod ui;
mod updater;

use anyhow::Result;
use app::{BrowserApp, UserEvent};
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    if updater::run_apply_mode_if_requested()? {
        return Ok(());
    }
    updater::clean_stale_updater();
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut app = BrowserApp::new(event_loop.create_proxy())?;
    event_loop.run_app(&mut app)?;
    Ok(())
}
