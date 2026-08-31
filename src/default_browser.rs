#[cfg(target_os = "windows")]
pub fn register_and_open_settings() -> anyhow::Result<()> {
    use anyhow::{bail, Context};
    use std::{path::Path, process::Command};
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_DWORD, SHCNF_FLUSH};

    let executable =
        std::env::current_exe().context("no se pudo localizar el ejecutable de CRONI")?;
    let executable = executable
        .canonicalize()
        .unwrap_or(executable)
        .to_string_lossy()
        .into_owned();
    let icon = format!("\"{executable}\",0");
    let open_command = format!("\"{executable}\" \"%1\"");

    for (key, name, value) in [
        (
            r"HKCU\Software\CRONI\Capabilities",
            Some("ApplicationName"),
            "CRONI",
        ),
        (
            r"HKCU\Software\CRONI\Capabilities",
            Some("ApplicationDescription"),
            "Navegador web ligero con bloqueo de anuncios y pestañas de bajo consumo",
        ),
        (
            r"HKCU\Software\CRONI\Capabilities\URLAssociations",
            Some("http"),
            "CRONI.Url.Http",
        ),
        (
            r"HKCU\Software\CRONI\Capabilities\URLAssociations",
            Some("https"),
            "CRONI.Url.Https",
        ),
        (
            r"HKCU\Software\RegisteredApplications",
            Some("CRONI"),
            r"Software\CRONI\Capabilities",
        ),
        (r"HKCU\Software\Classes\CRONI.Url.Http", None, "CRONI HTTP"),
        (
            r"HKCU\Software\Classes\CRONI.Url.Http",
            Some("URL Protocol"),
            "",
        ),
        (
            r"HKCU\Software\Classes\CRONI.Url.Http\DefaultIcon",
            None,
            &icon,
        ),
        (
            r"HKCU\Software\Classes\CRONI.Url.Http\shell\open\command",
            None,
            &open_command,
        ),
        (
            r"HKCU\Software\Classes\CRONI.Url.Https",
            None,
            "CRONI HTTPS",
        ),
        (
            r"HKCU\Software\Classes\CRONI.Url.Https",
            Some("URL Protocol"),
            "",
        ),
        (
            r"HKCU\Software\Classes\CRONI.Url.Https\DefaultIcon",
            None,
            &icon,
        ),
        (
            r"HKCU\Software\Classes\CRONI.Url.Https\shell\open\command",
            None,
            &open_command,
        ),
        (
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\CRONI.exe",
            None,
            &executable,
        ),
    ] {
        let mut command = Command::new("reg.exe");
        command.args(["add", key]);
        match name {
            Some(name) => command.args(["/v", name]),
            None => command.arg("/ve"),
        };
        let status = command
            .args(["/t", "REG_SZ", "/d", value, "/f"])
            .status()
            .with_context(|| format!("no se pudo registrar {key}"))?;
        if !status.success() {
            bail!("Windows rechazó el registro de {key}");
        }
    }

    if let Some(parent) = Path::new(&executable).parent() {
        let status = Command::new("reg.exe")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\CRONI.exe",
                "/v",
                "Path",
                "/t",
                "REG_SZ",
                "/d",
                &parent.to_string_lossy(),
                "/f",
            ])
            .status()?;
        if !status.success() {
            bail!("Windows rechazó el registro de la ruta de CRONI");
        }
    }

    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_DWORD | SHCNF_FLUSH, None, None);
    }

    Command::new("explorer.exe")
        .arg("ms-settings:defaultapps?registeredAppUser=CRONI")
        .spawn()
        .context("no se pudo abrir Aplicaciones predeterminadas")?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn register_and_open_settings() -> anyhow::Result<()> {
    anyhow::bail!("esta función sólo está disponible en Windows")
}
