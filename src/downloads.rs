use crate::{app::UserEvent, config::StoredDownload};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct DownloadItem {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub path: String,
    pub received: u64,
    pub total: Option<u64>,
    pub status: String,
}

impl DownloadItem {
    pub fn from_stored(id: u64, stored: StoredDownload) -> Self {
        Self {
            id,
            name: stored.name,
            url: stored.url,
            path: stored.path,
            received: stored.received,
            total: stored.total,
            status: if stored.status == "downloading" || stored.status == "cancelling" {
                "interrupted".into()
            } else {
                stored.status
            },
        }
    }

    pub fn to_stored(&self) -> StoredDownload {
        StoredDownload {
            name: self.name.clone(),
            url: self.url.clone(),
            path: self.path.clone(),
            received: self.received,
            total: self.total,
            status: self.status.clone(),
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use anyhow::{Context, Result};
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
        path::Path,
        rc::Rc,
        time::{Duration, Instant},
    };
    use webview2_com::{
        take_pwstr, BytesReceivedChangedEventHandler, DownloadStartingEventHandler,
        Microsoft::Web::WebView2::Win32::*, StateChangedEventHandler,
    };
    use windows::core::{Interface, PWSTR};
    use winit::event_loop::EventLoopProxy;
    use wry::{WebView, WebViewExtWindows};

    pub type OperationMap = Rc<RefCell<HashMap<u64, ICoreWebView2DownloadOperation>>>;
    pub type DownloadIdCounter = Rc<Cell<u64>>;

    pub fn new_operation_map() -> OperationMap {
        Rc::new(RefCell::new(HashMap::new()))
    }

    pub fn new_id_counter(next_id: u64) -> DownloadIdCounter {
        Rc::new(Cell::new(next_id))
    }

    pub fn attach_download_manager(
        view: &WebView,
        proxy: EventLoopProxy<UserEvent>,
        next_id: DownloadIdCounter,
        operations: OperationMap,
    ) -> Result<()> {
        let webview: ICoreWebView2_4 = view
            .webview()
            .cast()
            .context("esta versión de WebView2 no admite el gestor de descargas")?;

        let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let operation = unsafe { args.DownloadOperation()? };
            let id = next_id.get();
            next_id.set(id.saturating_add(1));

            let url = read_text(|target| unsafe { operation.Uri(target) })?;
            let path = read_text(|target| unsafe { args.ResultFilePath(target) })?;
            let name = Path::new(&path)
                .file_name()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("Descarga")
                .to_string();
            let (received, total, _) = operation_snapshot(&operation)?;

            // CRONI muestra su propio panel, por lo que ocultamos el panel nativo.
            // La descarga continúa normalmente y la operación queda disponible para cancelar.
            unsafe { args.SetHandled(true)? };
            operations.borrow_mut().insert(id, operation.clone());
            let _ = proxy.send_event(UserEvent::DownloadStarted {
                id,
                name,
                url,
                path,
                received,
                total,
            });

            let progress_proxy = proxy.clone();
            let last_update = Rc::new(Cell::new(Instant::now() - Duration::from_secs(1)));
            let progress_clock = last_update.clone();
            unsafe {
                operation.add_BytesReceivedChanged(
                    &BytesReceivedChangedEventHandler::create(Box::new(move |sender, _| {
                        let Some(sender) = sender else {
                            return Ok(());
                        };
                        if progress_clock.get().elapsed() >= Duration::from_millis(160) {
                            progress_clock.set(Instant::now());
                            send_progress(&progress_proxy, id, &sender)?;
                        }
                        Ok(())
                    })),
                    &mut 0,
                )?;
            }

            let state_proxy = proxy.clone();
            let state_operations = operations.clone();
            unsafe {
                operation.add_StateChanged(
                    &StateChangedEventHandler::create(Box::new(move |sender, _| {
                        let Some(sender) = sender else {
                            return Ok(());
                        };
                        let (_, _, status) = operation_snapshot(&sender)?;
                        send_progress(&state_proxy, id, &sender)?;
                        if status != "downloading" {
                            state_operations.borrow_mut().remove(&id);
                        }
                        Ok(())
                    })),
                    &mut 0,
                )?;
            }
            Ok(())
        }));

        unsafe { webview.add_DownloadStarting(&handler, &mut 0) }
            .context("no se pudo activar el seguimiento de descargas")?;
        Ok(())
    }

    fn read_text(
        get_value: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>,
    ) -> windows::core::Result<String> {
        let mut value = PWSTR::null();
        get_value(&mut value)?;
        Ok(take_pwstr(value))
    }

    fn operation_snapshot(
        operation: &ICoreWebView2DownloadOperation,
    ) -> windows::core::Result<(u64, Option<u64>, &'static str)> {
        let mut received = 0_i64;
        let mut total = -1_i64;
        let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
        unsafe {
            operation.BytesReceived(&mut received)?;
            operation.TotalBytesToReceive(&mut total)?;
            operation.State(&mut state)?;
        }
        let status = if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
            "completed"
        } else if state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
            let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
            unsafe { operation.InterruptReason(&mut reason)? };
            if reason == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED {
                "cancelled"
            } else {
                "interrupted"
            }
        } else {
            "downloading"
        };
        Ok((
            received.max(0) as u64,
            (total >= 0).then_some(total as u64),
            status,
        ))
    }

    fn send_progress(
        proxy: &EventLoopProxy<UserEvent>,
        id: u64,
        operation: &ICoreWebView2DownloadOperation,
    ) -> windows::core::Result<()> {
        let (received, total, status) = operation_snapshot(operation)?;
        let _ = proxy.send_event(UserEvent::DownloadProgress {
            id,
            received,
            total,
            status: status.into(),
        });
        Ok(())
    }

    pub fn cancel(operations: &OperationMap, id: u64) -> Result<bool> {
        let operation = operations.borrow().get(&id).cloned();
        if let Some(operation) = operation {
            unsafe { operation.Cancel() }.context("no se pudo cancelar la descarga")?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(target_os = "windows")]
pub use platform::*;
