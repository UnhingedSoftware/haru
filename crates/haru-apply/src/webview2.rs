//! The Microsoft Edge WebView2 Runtime, which kirie's web wallpapers run in on
//! Windows.
//!
//! It is a Windows component, not part of kirie: Windows 11 ships it, and Edge's
//! updater has put it on nearly every Windows 10 machine. For the rest,
//! Microsoft publishes a small bootstrapper that fetches and installs it. haru
//! checks for it, and when asked, downloads that bootstrapper, makes sure it
//! really is Microsoft's, and runs it with an administrator prompt, so the
//! runtime is installed for every account on the machine rather than just this
//! one.
//!
//! Everywhere but Windows, web wallpapers use something else, so the runtime is
//! never needed there.

/// Microsoft's permanent link to the Evergreen bootstrapper.
pub const BOOTSTRAPPER: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// The runtime's id in EdgeUpdate's registry, from Microsoft's own guidance on
/// detecting it.
#[cfg_attr(not(windows), allow(dead_code))]
const CLIENT: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// Where an installed runtime records its version: per machine (under the
/// 32-bit view on 64-bit Windows, and the native one on 32-bit Windows), and
/// per user.
#[cfg_attr(not(windows), allow(dead_code))]
fn keys() -> [String; 3] {
    [
        format!(r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{CLIENT}"),
        format!(r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{CLIENT}"),
        format!(r"HKCU\Software\Microsoft\EdgeUpdate\Clients\{CLIENT}"),
    ]
}

/// Whether this platform's web wallpapers need the runtime at all.
#[must_use]
pub const fn needed() -> bool {
    cfg!(windows)
}

/// The installed runtime's version, or `None` when there is none.
///
/// Always `None` off Windows, where nothing needs it.
#[must_use]
#[cfg(windows)]
pub fn installed() -> Option<String> {
    keys().iter().find_map(|key| {
        let said = crate::child::quiet("reg")
            .args(["query", key, "/v", "pv"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !said.status.success() {
            return None;
        }
        version_in(&String::from_utf8_lossy(&said.stdout))
    })
}

#[must_use]
#[cfg(not(windows))]
pub const fn installed() -> Option<String> {
    None
}

/// The version `reg query ... /v pv` printed, if it names a real install.
///
/// An uninstall can leave the key behind with `pv` empty or `0.0.0.0`, which
/// Microsoft's guidance says to read as not installed.
#[must_use]
pub fn version_in(listing: &str) -> Option<String> {
    listing.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        if !words.next()?.eq_ignore_ascii_case("pv") || !words.next()?.starts_with("REG_") {
            return None;
        }
        let version = words.next()?.trim();
        (!version.is_empty() && version != "0.0.0.0").then(|| version.to_owned())
    })
}

/// Download Microsoft's bootstrapper and run it with an administrator prompt.
///
/// Returns the version now installed. Fails without running anything if the
/// download is not signed by Microsoft, and says so plainly when the prompt is
/// declined.
#[cfg(windows)]
pub fn install(progress: &mut dyn FnMut(u64, u64)) -> Result<String, String> {
    if let Some(version) = installed() {
        return Ok(version);
    }
    let setup = std::env::temp_dir().join(format!(
        "MicrosoftEdgeWebview2Setup.{}.exe",
        std::process::id()
    ));
    download(&setup, progress)?;
    let ran = run_elevated(&setup);
    let _ = std::fs::remove_file(&setup);
    ran?;
    installed().ok_or_else(|| {
        "the installer finished, but the WebView2 Runtime still is not there".to_owned()
    })
}

#[cfg(not(windows))]
pub fn install(_progress: &mut dyn FnMut(u64, u64)) -> Result<String, String> {
    Err("the WebView2 Runtime is only needed on Windows".to_owned())
}

#[cfg(windows)]
fn download(to: &std::path::Path, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    use std::io::{Read as _, Write as _};

    // The bootstrapper is under 2 MB; anything much bigger is not it.
    const LIMIT: u64 = 16 * 1024 * 1024;

    let response = ureq::get(BOOTSTRAPPER)
        .set("User-Agent", crate::install::AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .call()
        .map_err(|error| format!("could not download the WebView2 installer ({error})"))?;
    let total = response
        .header("Content-Length")
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let mut body = response.into_reader().take(LIMIT);
    let mut file = std::fs::File::create(to)
        .map_err(|error| format!("cannot save the WebView2 installer ({error})"))?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut done = 0_u64;
    loop {
        let read = body
            .read(&mut buffer)
            .map_err(|error| format!("the WebView2 download stopped ({error})"))?;
        if read == 0 {
            break;
        }
        let chunk = buffer.get(..read).ok_or("short read")?;
        file.write_all(chunk)
            .map_err(|error| format!("cannot save the WebView2 installer ({error})"))?;
        done += read as u64;
        progress(done, total.max(done));
    }
    Ok(())
}

/// What the elevation script exits with, besides the installer's own code.
#[cfg_attr(not(windows), allow(dead_code))]
const NOT_MICROSOFT: i32 = 90;
#[cfg_attr(not(windows), allow(dead_code))]
const DECLINED: i32 = 91;

/// The PowerShell that checks the download's signature and then runs it
/// through Windows' administrator prompt.
///
/// `Start-Process -Verb RunAs` is the documented way to ask for elevation from
/// a script, and it throws when the user says no. The signature check comes
/// first because the file is about to be run as administrator: a valid
/// Authenticode signature from Microsoft Corporation, or nothing runs.
#[must_use]
pub fn elevation_script(setup: &std::path::Path) -> String {
    let quoted = setup.display().to_string().replace('\'', "''");
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $setup = '{quoted}'\n\
         $signed = Get-AuthenticodeSignature -LiteralPath $setup\n\
         if ($signed.Status -ne 'Valid' -or $signed.SignerCertificate.Subject -notlike '*O=Microsoft Corporation*') {{ exit {NOT_MICROSOFT} }}\n\
         try {{ $run = Start-Process -FilePath $setup -ArgumentList '/silent','/install' -Verb RunAs -Wait -PassThru }} catch {{ exit {DECLINED} }}\n\
         exit $run.ExitCode\n"
    )
}

#[cfg(windows)]
fn run_elevated(setup: &std::path::Path) -> Result<(), String> {
    let status = crate::child::quiet("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &elevation_script(setup),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| format!("could not start PowerShell ({error})"))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(NOT_MICROSOFT) => {
            Err("the downloaded installer is not signed by Microsoft, so it was not run".to_owned())
        }
        Some(DECLINED) => Err("the administrator prompt was declined".to_owned()),
        Some(code) => Err(format!("the WebView2 installer failed (exit code {code})")),
        None => Err("the WebView2 installer was stopped".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registered_version_is_read_from_reg_output() {
        let listing = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}\r\n    pv    REG_SZ    128.0.2739.79\r\n\r\n";
        assert_eq!(version_in(listing).as_deref(), Some("128.0.2739.79"));
    }

    #[test]
    fn a_key_left_behind_by_an_uninstall_is_not_an_install() {
        assert_eq!(version_in("    pv    REG_SZ    0.0.0.0\r\n"), None);
        assert_eq!(version_in("    pv    REG_SZ    \r\n"), None);
        assert_eq!(
            version_in(
                "ERROR: The system was unable to find the specified registry key or value.\r\n"
            ),
            None
        );
    }

    #[test]
    fn the_script_checks_the_signature_before_elevating() {
        let script = elevation_script(std::path::Path::new(
            r"C:\Users\o'neil\AppData\Local\Temp\setup.exe",
        ));
        let checked = script.find("Get-AuthenticodeSignature");
        let elevated = script.find("-Verb RunAs");
        assert!(
            matches!((checked, elevated), (Some(checked), Some(elevated)) if checked < elevated),
            "the signature is checked before anything is elevated:\n{script}"
        );
        assert!(script.contains(r"'C:\Users\o''neil\AppData\Local\Temp\setup.exe'"));
        assert!(script.contains("O=Microsoft Corporation"));
    }
}
