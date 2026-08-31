use crate::{app::UserEvent, config::app_data_dir};
use anyhow::{bail, Context, Result};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_void, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};
use winit::event_loop::EventLoopProxy;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_REPOSITORY: &str = match option_env!("CRONI_GITHUB_REPOSITORY") {
    Some(value) => value,
    None => "",
};
const MAX_RELEASE_JSON: usize = 1024 * 1024;
const MAX_EXECUTABLE_SIZE: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

pub fn is_configured() -> bool {
    valid_repository(GITHUB_REPOSITORY)
}

pub fn start_update_check(proxy: EventLoopProxy<UserEvent>) {
    if !is_configured() {
        return;
    }
    thread::spawn(move || {
        let event = match check_and_download() {
            Ok(Some((version, path, sha256))) => UserEvent::UpdateReady {
                version,
                path,
                sha256,
            },
            Ok(None) => UserEvent::UpdateCurrent,
            Err(error) => UserEvent::UpdateFailed(error.to_string()),
        };
        let _ = proxy.send_event(event);
    });
}

fn check_and_download() -> Result<Option<(String, PathBuf, String)>> {
    let api_url = format!("https://api.github.com/repos/{GITHUB_REPOSITORY}/releases/latest");
    let release: Release = serde_json::from_slice(&http_get(&api_url, MAX_RELEASE_JSON)?)
        .context("GitHub devolvió una respuesta de actualización inválida")?;
    let remote_text = release.tag_name.trim_start_matches(['v', 'V']);
    let remote = Version::parse(remote_text).context("la versión publicada no es válida")?;
    let current = Version::parse(APP_VERSION).context("la versión local no es válida")?;
    if remote <= current {
        return Ok(None);
    }

    let executable = release
        .assets
        .iter()
        .find(|asset| asset.name.eq_ignore_ascii_case("CRONI.exe"))
        .context("la versión nueva no contiene CRONI.exe")?;
    let checksum_asset = release
        .assets
        .iter()
        .find(|asset| asset.name.eq_ignore_ascii_case("CRONI.exe.sha256"))
        .context("la versión nueva no contiene la verificación SHA-256")?;
    ensure_github_download_url(&executable.browser_download_url)?;
    ensure_github_download_url(&checksum_asset.browser_download_url)?;

    let checksum_text = String::from_utf8(http_get(&checksum_asset.browser_download_url, 4096)?)
        .context("la verificación SHA-256 no es texto válido")?;
    let expected = checksum_text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("la verificación SHA-256 publicada no es válida");
    }

    let bytes = http_get(&executable.browser_download_url, MAX_EXECUTABLE_SIZE)?;
    if bytes.len() < 1024 || !bytes.starts_with(b"MZ") {
        bail!("el archivo de actualización no es un ejecutable de Windows válido");
    }
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != expected {
        bail!("la actualización no superó la verificación SHA-256");
    }

    let updates_dir = app_data_dir().join("Updates");
    fs::create_dir_all(&updates_dir).context("no se pudo crear la carpeta de actualizaciones")?;
    let path = updates_dir.join(format!("CRONI-{remote}.exe"));
    fs::write(&path, bytes).context("no se pudo guardar la actualización")?;
    Ok(Some((remote.to_string(), path, expected)))
}

pub fn stage_and_launch(update: &Path, expected_sha256: &str) -> Result<()> {
    verify_file(update, expected_sha256)?;
    let current = std::env::current_exe().context("no se encontró CRONI.exe")?;
    let parent = current
        .parent()
        .context("CRONI.exe no tiene una carpeta válida")?;
    let write_test = parent.join(".croni-update-write-test");
    fs::write(&write_test, b"CRONI")
        .context("CRONI no tiene permiso para actualizarse en esta carpeta")?;
    let _ = fs::remove_file(&write_test);

    let helper = helper_path();
    let _ = fs::remove_file(&helper);
    fs::copy(&current, &helper).context("no se pudo preparar el actualizador")?;
    Command::new(&helper)
        .arg("--croni-apply-update")
        .arg(std::process::id().to_string())
        .arg(update)
        .arg(&current)
        .arg(expected_sha256)
        .spawn()
        .context("no se pudo iniciar el actualizador")?;
    Ok(())
}

pub fn run_apply_mode_if_requested() -> Result<bool> {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.get(1) != Some(&OsString::from("--croni-apply-update")) {
        return Ok(false);
    }
    if args.len() != 6 {
        bail!("parámetros de actualización incompletos");
    }
    let update = PathBuf::from(&args[3]);
    let target = PathBuf::from(&args[4]);
    let checksum = args[5].to_string_lossy().into_owned();
    verify_file(&update, &checksum)?;

    let backup = target.with_extension("exe.old");
    let _ = fs::remove_file(&backup);
    let mut moved = false;
    for _ in 0..300 {
        match fs::rename(&target, &backup) {
            Ok(()) => {
                moved = true;
                break;
            }
            Err(_) => thread::sleep(Duration::from_millis(200)),
        }
    }
    if !moved {
        bail!("CRONI no se cerró a tiempo para aplicar la actualización");
    }

    if let Err(error) = fs::copy(&update, &target) {
        let _ = fs::rename(&backup, &target);
        return Err(error).context("no se pudo reemplazar CRONI.exe");
    }
    let _ = fs::remove_file(&update);
    Command::new(&target)
        .spawn()
        .context("CRONI se actualizó, pero no pudo volver a abrirse")?;
    let _ = fs::remove_file(&backup);
    Ok(true)
}

pub fn clean_stale_updater() {
    let _ = fs::remove_file(helper_path());
}

fn helper_path() -> PathBuf {
    std::env::temp_dir().join("CRONI-Updater.exe")
}

fn verify_file(path: &Path, expected_sha256: &str) -> Result<()> {
    let bytes = fs::read(path).context("no se pudo leer la actualización descargada")?;
    if bytes.len() < 1024 || !bytes.starts_with(b"MZ") {
        bail!("la actualización descargada no es un ejecutable válido");
    }
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != expected_sha256.to_ascii_lowercase() {
        bail!("la actualización descargada fue modificada");
    }
    Ok(())
}

fn valid_repository(repository: &str) -> bool {
    let mut parts = repository.split('/');
    let valid_part = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    };
    matches!((parts.next(), parts.next(), parts.next()), (Some(owner), Some(repo), None) if valid_part(owner) && valid_part(repo))
}

fn ensure_github_download_url(raw: &str) -> Result<()> {
    let url = url::Url::parse(raw).context("GitHub publicó una URL de descarga inválida")?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        bail!("GitHub publicó una URL de descarga no permitida");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn http_get(raw: &str, max_size: usize) -> Result<Vec<u8>> {
    use windows::{
        core::{HSTRING, PCWSTR},
        Win32::Networking::WinHttp::{
            WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
            WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
            WINHTTP_QUERY_STATUS_CODE,
        },
    };

    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    let url = url::Url::parse(raw).context("URL de actualización inválida")?;
    if url.scheme() != "https" {
        bail!("la actualización sólo admite HTTPS");
    }
    let host = url
        .host_str()
        .context("URL de actualización sin servidor")?;
    let port = url.port_or_known_default().unwrap_or(443);
    let mut resource = url.path().to_owned();
    if let Some(query) = url.query() {
        resource.push('?');
        resource.push_str(query);
    }

    let agent = HSTRING::from(format!("CRONI/{APP_VERSION}"));
    let host = HSTRING::from(host);
    let resource = HSTRING::from(resource);
    unsafe {
        let session = Handle(WinHttpOpen(
            &agent,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            bail!("no se pudo iniciar la conexión de actualización");
        }
        let connection = Handle(WinHttpConnect(session.0, &host, port, 0));
        if connection.0.is_null() {
            bail!("no se pudo conectar con GitHub");
        }
        let request = Handle(WinHttpOpenRequest(
            connection.0,
            &HSTRING::from("GET"),
            &resource,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            bail!("no se pudo crear la solicitud de actualización");
        }
        let headers = "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n"
            .encode_utf16()
            .collect::<Vec<_>>();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0)
            .context("no se pudo enviar la solicitud a GitHub")?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .context("GitHub no respondió a la actualización")?;

        let mut status = 0u32;
        let mut status_size = std::mem::size_of::<u32>() as u32;
        let mut index = 0u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut status_size,
            &mut index,
        )?;
        if !(200..300).contains(&status) {
            bail!("GitHub respondió con el estado HTTP {status}");
        }

        let mut output = Vec::new();
        let mut buffer = [0u8; 32 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            )?;
            if read == 0 {
                break;
            }
            if output.len() + read as usize > max_size {
                bail!("la descarga de actualización supera el límite permitido");
            }
            output.extend_from_slice(&buffer[..read as usize]);
        }
        Ok(output)
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn http_get(_raw: &str, _max_size: usize) -> Result<Vec<u8>> {
    bail!("las actualizaciones automáticas sólo están disponibles en Windows")
}

#[cfg(test)]
mod tests {
    use super::valid_repository;

    #[test]
    fn validates_github_repository_names() {
        assert!(valid_repository("usuario/CRONI"));
        assert!(valid_repository("mi-org/croni.browser"));
        assert!(!valid_repository(""));
        assert!(!valid_repository("usuario"));
        assert!(!valid_repository("usuario/repo/extra"));
        assert!(!valid_repository("usuario/repo?otro"));
    }
}
