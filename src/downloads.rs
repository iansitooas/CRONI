use crate::{app::UserEvent, config::StoredDownload};
use serde::Serialize;

pub const INITIALIZATION_SCRIPT: &str = include_str!("../assets/downloads.js");

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
        Microsoft::Web::WebView2::Win32::*, SaveAsUIShowingEventHandler,
        ShowSaveAsUICompletedHandler, StateChangedEventHandler,
    };
    use windows::{
        core::{w, Interface, HRESULT, HSTRING, PWSTR},
        Win32::{
            Foundation::{ERROR_CANCELLED, HWND},
            System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            UI::Shell::{
                Common::COMDLG_FILTERSPEC, FileSaveDialog, IFileSaveDialog, IShellItem,
                SHCreateItemFromParsingName, FOS_FORCEFILESYSTEM, FOS_NODEREFERENCELINKS,
                FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
            },
        },
    };
    use winit::{
        event_loop::EventLoopProxy,
        raw_window_handle::{HasWindowHandle, RawWindowHandle},
        window::Window,
    };
    use wry::{WebView, WebViewExtWindows};

    pub type OperationMap = Rc<RefCell<HashMap<u64, ICoreWebView2DownloadOperation>>>;
    pub type DownloadIdCounter = Rc<Cell<u64>>;
    pub type PendingDownloadMap = Rc<RefCell<HashMap<u64, PendingDownload>>>;

    pub struct PendingDownload {
        args: ICoreWebView2DownloadStartingEventArgs,
        deferral: ICoreWebView2Deferral,
        accepted: bool,
    }

    impl Drop for PendingDownload {
        fn drop(&mut self) {
            unsafe {
                if !self.accepted {
                    let _ = self.args.SetCancel(true);
                }
                let _ = self.deferral.Complete();
            }
        }
    }

    pub fn new_pending_map() -> PendingDownloadMap {
        Rc::new(RefCell::new(HashMap::new()))
    }

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
        pending: PendingDownloadMap,
    ) -> Result<()> {
        let webview: ICoreWebView2_4 = view
            .webview()
            .cast()
            .context("esta versión de WebView2 no admite el gestor de descargas")?;

        let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let deferral = unsafe { args.GetDeferral()? };
            let id = next_id.get();
            next_id.set(id.saturating_add(1));
            pending.borrow_mut().insert(
                id,
                PendingDownload {
                    args,
                    deferral,
                    accepted: false,
                },
            );
            // Return from the WebView callback before showing a modal dialog.
            if proxy
                .send_event(UserEvent::ChooseDownloadDestination { id })
                .is_err()
            {
                pending.borrow_mut().remove(&id);
            }
            Ok(())
        }));
        unsafe { webview.add_DownloadStarting(&handler, &mut 0) }
            .context("no se pudo activar el seguimiento de descargas")?;
        // Save As (documents, built-in viewers and context menus) is a separate
        // WebView2 route. Keep its native destination picker, never silent-save.
        // Older runtimes keep their default dialog without requiring this API.
        if let Ok(save_ui) = view.webview().cast::<ICoreWebView2_25>() {
            unsafe {
                save_ui.add_SaveAsUIShowing(
                    &SaveAsUIShowingEventHandler::create(Box::new(|_, args| {
                        if let Some(args) = args {
                            args.SetSuppressDefaultDialog(false)?;
                        }
                        Ok(())
                    })),
                    &mut 0,
                )?;
            }
        }
        Ok(())
    }

    pub fn save_page_as(view: &WebView) -> Result<()> {
        let webview: ICoreWebView2_25 = view
            .webview()
            .cast()
            .context("actualiza WebView2 para guardar documentos desde este menú")?;
        unsafe {
            webview.ShowSaveAsUI(&ShowSaveAsUICompletedHandler::create(Box::new(
                |result, _| result,
            )))?;
        }
        Ok(())
    }

    pub fn choose_destination(
        id: u64,
        pending: &PendingDownloadMap,
        operations: &OperationMap,
        proxy: &EventLoopProxy<UserEvent>,
        window: &Window,
    ) -> Result<()> {
        // Do not hold a RefCell borrow while the modal dialog pumps Windows events.
        let request = pending.borrow_mut().remove(&id);
        let Some(mut request) = request else {
            return Ok(());
        };
        let suggested = read_text(|target| unsafe { request.args.ResultFilePath(target) })?;
        let raw = window.window_handle()?;
        let RawWindowHandle::Win32(raw) = raw.as_raw() else {
            anyhow::bail!("no se encontró la ventana de Windows");
        };
        let owner = HWND(raw.hwnd.get() as *mut std::ffi::c_void);
        let Some(path) = save_path(owner, &suggested)? else {
            return Ok(()); // Drop cancels and completes the deferred download.
        };
        unsafe {
            request.args.SetResultFilePath(&HSTRING::from(path))?;
        }
        track_download(id, &request.args, proxy, operations)?;
        request.accepted = true;
        Ok(())
    }

    fn save_path(owner: HWND, suggested: &str) -> windows::core::Result<Option<String>> {
        // WebView2 initializes COM on this UI thread. Use the native shell dialog.
        unsafe {
            let dialog: IFileSaveDialog =
                CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                dialog.GetOptions()?
                    | FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST
                    | FOS_NODEREFERENCELINKS
                    | FOS_OVERWRITEPROMPT,
            )?;
            dialog.SetTitle(&HSTRING::from("Guardar descarga como"))?;
            // No extension or MIME whitelist; .lnk files are saved, not followed.
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: w!("Todos los archivos (*.*)"),
                pszSpec: w!("*.*"),
            }])?;
            let suggested = Path::new(suggested);
            let name = suggested
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Descarga");
            dialog.SetFileName(&HSTRING::from(name))?;
            if let Some(extension) = suggested.extension().and_then(|value| value.to_str()) {
                dialog.SetDefaultExtension(&HSTRING::from(extension))?;
            }
            if let Some(folder) = suggested.parent().filter(|folder| folder.is_dir()) {
                let folder = HSTRING::from(folder.to_string_lossy().as_ref());
                if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&folder, None) {
                    let _ = dialog.SetDefaultFolder(&item);
                }
            }
            match dialog.Show(Some(owner)) {
                Ok(()) => {}
                Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                    return Ok(None)
                }
                Err(error) => return Err(error),
            }
            let item = dialog.GetResult()?;
            let path = take_pwstr(item.GetDisplayName(SIGDN_FILESYSPATH)?);
            Ok(Some(path))
        }
    }

    fn track_download(
        id: u64,
        args: &ICoreWebView2DownloadStartingEventArgs,
        proxy: &EventLoopProxy<UserEvent>,
        operations: &OperationMap,
    ) -> windows::core::Result<()> {
        let operation = unsafe { args.DownloadOperation()? };

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
