//! Standard Docker image browser processes. Native hosts never instantiate this backend.
use crate::process::BrowserProcess;
use crate::{
    driver::{Driver, DriverState},
    BrowserAction, BrowserBackend, BrowserDispatch, BrowserFuture,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::FileSystem;
use talos_types::{BrowserReadiness, BrowserTargetKind, McpCallToolResponse};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    net::TcpStream,
    process::Command,
    sync::Mutex as AsyncMutex,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Package {
    schema_version: u32,
    browser_use_version: String,
    chromium_version: String,
    python: PathBuf,
    driver: PathBuf,
    chromium: PathBuf,
}
struct Components {
    children: Vec<BrowserProcess>,
    browser_index: usize,
    endpoint: String,
    _profile_lock: talos_fs::FileLock,
}
impl Components {
    fn ensure_running(&mut self) -> TalosResult<()> {
        for (index, child) in self.children.iter_mut().enumerate() {
            if let Some(status) = child.inner_mut().try_wait().map_err(|error| {
                TalosError::internal_with_source(
                    "Browser process health could not be checked.",
                    error,
                )
            })? {
                return Err(TalosError::from_source(
                    ErrorCode::ServiceUnavailable,
                    "A managed browser component exited. Restart this Worker before continuing.",
                    std::io::Error::other(format!("Component {index} exited with {status}.")),
                ));
            }
        }
        Ok(())
    }
}

pub struct ManagedBrowser {
    package: Package,
    home: PathBuf,
    driver: Driver,
    components: AsyncMutex<Option<Components>>,
    state: AsyncMutex<DriverState>,
    status: AtomicU8,
    failure: Mutex<Option<talos_error::ApiError>>,
}
impl ManagedBrowser {
    /// Only the composition root passes the standard image's protected resource directory.
    pub(crate) fn prepared(
        home: PathBuf,
        resources: &Path,
        python: &Path,
        version: &str,
    ) -> TalosResult<Arc<Self>> {
        if !cfg!(target_os = "linux") {
            return Err(unavailable(
                "Managed Docker browser resources require Linux.",
            ));
        }
        let mut package: Package = talos_fs::read_json(&resources.join("runtime.json"))?;
        if package.schema_version != 1
            || package.browser_use_version.is_empty()
            || package.chromium_version != "156.0.8078.4"
        {
            return Err(unavailable(
                "The packaged browser runtime version is unsupported.",
            ));
        }
        for path in [&package.driver, &package.chromium] {
            if !path.is_absolute() || !path.is_file() || !path.starts_with(resources) {
                return Err(unavailable("The packaged browser runtime is incomplete."));
            }
        }
        if !python.is_absolute() || !python.is_file() || version.is_empty() {
            return Err(unavailable(
                "The verified browser dependency environment is incomplete.",
            ));
        }
        package.python = python.to_path_buf();
        package.browser_use_version = version.to_owned();
        let downloads = home.join("browser/downloads");
        Ok(Arc::new(Self {
            driver: Driver {
                python: package.python.clone(),
                script: package.driver.clone(),
                downloads,
                pressed: Default::default(),
                cleanup: Default::default(),
            },
            package,
            home,
            components: AsyncMutex::new(None),
            state: AsyncMutex::new(Default::default()),
            status: AtomicU8::new(0),
            failure: Mutex::new(None),
        }))
    }
    pub async fn endpoint(&self) -> TalosResult<String> {
        let mut components = self.components.lock().await;
        if let Some(running) = components.as_mut() {
            if let Err(error) = running.ensure_running() {
                self.status.store(3, Ordering::Release);
                return Err(error);
            }
            return Ok(running.endpoint.clone());
        }
        self.driver.cleanup.drain().await?;
        self.status.store(1, Ordering::Release);
        let _starting = Starting(&self.status);
        let started = tokio::time::timeout(Duration::from_secs(25), self.start())
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "The managed browser did not become ready in time.",
                    error,
                )
            })
            .and_then(|value| value);
        match started {
            Ok(started) => {
                let endpoint = started.endpoint.clone();
                *components = Some(started);
                self.status.store(2, Ordering::Release);
                Ok(endpoint)
            }
            Err(error) => {
                self.status.store(3, Ordering::Release);
                *self
                    .failure
                    .lock()
                    .map_err(|_| TalosError::internal("Browser readiness lock is poisoned."))? =
                    Some(error.to_api_error());
                Err(error)
            }
        }
    }
    async fn start(&self) -> TalosResult<Components> {
        #[cfg(unix)]
        if unsafe { libc::geteuid() } == 0 {
            return Err(unavailable(
                "The managed browser must run as a non-root user.",
            ));
        }
        let root = self.home.join("browser");
        protect_directory(&root)?;
        protect_directory(&self.driver.downloads)?;
        let profile = root.join("profile");
        protect_directory(&profile)?;
        let lock = talos_fs::StdFileSystem
            .try_file_lock(&root.join("profile.lock"))?
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::Conflict,
                    "Another Worker owns this browser profile.",
                )
            })?;
        let authority = root.join("Xauthority");
        let mut auth = Command::new("/usr/bin/xauth");
        auth.args(["-f"])
            .arg(&authority)
            .args(["add", ":99", "."])
            .arg(uuid::Uuid::new_v4().simple().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let result = auth.status().await.map_err(|error| {
            TalosError::internal_with_source(
                "Failed to prepare browser display authentication.",
                error,
            )
        })?;
        if !result.success() {
            return Err(unavailable(
                "Browser display authentication could not be prepared.",
            ));
        }
        cleanup_stale_x()?;
        let mut running = Components {
            children: Vec::new(),
            browser_index: 0,
            endpoint: String::new(),
            _profile_lock: lock,
        };
        let mut homepage = command(&self.package.python, &authority);
        homepage
            .arg("-I")
            .arg("/opt/talos/browser/desktop/home_server.py")
            .stdout(Stdio::piped());
        let mut homepage = BrowserProcess::spawn(homepage, self.driver.cleanup.clone())?;
        let output = homepage
            .inner_mut()
            .stdout()
            .take()
            .ok_or_else(|| unavailable("The packaged homepage startup output is missing."))?;
        let mut ready = String::new();
        BufReader::new(output.take(7))
            .read_line(&mut ready)
            .await
            .map_err(|error| {
                TalosError::internal_with_source("The packaged homepage did not start.", error)
            })?;
        let port = ready
            .strip_suffix('\n')
            .filter(|line| {
                !line.is_empty()
                    && line.len() <= 5
                    && line.bytes().all(|byte| byte.is_ascii_digit())
            })
            .and_then(|line| line.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| unavailable("The packaged homepage endpoint is invalid."))?;
        let home_url = format!("http://127.0.0.1:{port}/home.html");
        running.children.push(homepage);
        running.ensure_running()?;
        let mut xvfb = command("/usr/bin/Xvfb", &authority);
        xvfb.args([
            ":99",
            "-screen",
            "0",
            "1440x900x24",
            "-nolisten",
            "tcp",
            "-auth",
        ])
        .arg(&authority);
        running
            .children
            .push(BrowserProcess::spawn(xvfb, self.driver.cleanup.clone())?);
        loop {
            running.ensure_running()?;
            let mut probe = command("/usr/bin/xdpyinfo", &authority);
            probe.arg("-display").arg(":99");
            if tokio::time::timeout(Duration::from_secs(1), probe.status())
                .await
                .is_ok_and(|value| value.is_ok_and(|status| status.success()))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut wm = command("/usr/bin/openbox", &authority);
        wm.args([
            "--sm-disable",
            "--config-file",
            "/opt/talos/browser/desktop/openbox.xml",
        ])
        .env("TALOS_BROWSER_PROFILE", &profile)
        .env("TALOS_BROWSER_HOME_URL", &home_url);
        running
            .children
            .push(BrowserProcess::spawn(wm, self.driver.cleanup.clone())?);
        loop {
            running.ensure_running()?;
            let mut probe = command("/usr/bin/xprop", &authority);
            probe
                .args(["-root", "_NET_SUPPORTING_WM_CHECK"])
                .stdout(Stdio::piped())
                .kill_on_drop(true);
            if tokio::time::timeout(Duration::from_secs(1), probe.output())
                .await
                .is_ok_and(|result| {
                    result.is_ok_and(|output| {
                        output.status.success() && window_manager_ready(&output.stdout)
                    })
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut background = command("/usr/bin/feh", &authority);
        background.args([
            "--no-fehbg",
            "--bg-fill",
            "/opt/talos/browser/desktop/wallpaper.png",
        ]);
        let mut background = BrowserProcess::spawn(background, self.driver.cleanup.clone())?;
        let result = background.wait().await.map_err(|error| {
            TalosError::internal_with_source(
                "Failed to prepare the Worker desktop background.",
                error,
            )
        })?;
        if !result.success() {
            return Err(unavailable("The Worker desktop background is unavailable."));
        }
        let mut dock = command("/usr/bin/tint2", &authority);
        dock.args(["-c", "/opt/talos/browser/desktop/tint2rc"])
            .env("TALOS_BROWSER_PROFILE", &profile)
            .env("TALOS_BROWSER_HOME_URL", &home_url);
        running
            .children
            .push(BrowserProcess::spawn(dock, self.driver.cleanup.clone())?);
        let active = profile.join("DevToolsActivePort");
        if active.exists() {
            talos_fs::remove_file(&active)?;
        }
        let mut chrome = command(&self.package.chromium, &authority);
        chrome
            .args([
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-component-update",
                // The pinned Chrome for Testing keeps its supervised process/CDP
                // alive when users close the last window; the dock opens a new one.
                "--keep-alive-for-test",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("--window-size=1280,760")
            .arg("--window-position=80,50")
            .arg(format!("--homepage={home_url}"))
            .arg(home_url);
        running.browser_index = running.children.len();
        running
            .children
            .push(BrowserProcess::spawn(chrome, self.driver.cleanup.clone())?);
        loop {
            if let Ok(value) = tokio::fs::read_to_string(&active).await {
                let mut lines = value.lines();
                let port = lines.next().and_then(|port| port.parse::<u16>().ok());
                let path = lines.next();
                if let (Some(port), Some(path)) = (port, path) {
                    if port == 0
                        || !path.starts_with("/devtools/browser/")
                        || path.len() > 128
                        || path.chars().any(char::is_control)
                    {
                        return Err(unavailable("The managed browser endpoint is invalid."));
                    }
                    if TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                        .await
                        .is_ok()
                    {
                        running.endpoint = format!("ws://127.0.0.1:{port}{path}");
                        break;
                    }
                }
            }
            running.ensure_running()?;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut vnc = command("/usr/bin/x11vnc", &authority);
        vnc.args([
            "-display",
            ":99",
            "-rfbport",
            "5900",
            "-listen",
            "127.0.0.1",
            "-no6",
            "-rfbportv6",
            "0",
            "-forever",
            "-shared",
            "-nopw",
            "-noxdamage",
            "-xkb",
            "-nosel",
            "-noclipboard",
            "-noprimary",
        ]);
        running
            .children
            .push(BrowserProcess::spawn(vnc, self.driver.cleanup.clone())?);
        loop {
            running.ensure_running()?;
            if TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, 5900))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        running.ensure_running()?;
        Ok(running)
    }
}
impl BrowserBackend for ManagedBrowser {
    fn desktop(&self) -> BrowserFuture<'_, TcpStream> {
        Box::pin(async move {
            self.endpoint().await?;
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, 5900))
                .await
                .map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::ServiceUnavailable,
                        "The private desktop server is unavailable.",
                        error,
                    )
                })
        })
    }
    fn readiness(&self, target: BrowserTargetKind) -> BrowserReadiness {
        if target != BrowserTargetKind::Managed {
            return BrowserReadiness::Unavailable {
                error: unavailable("Docker Workers use their managed browser.").to_api_error(),
            };
        }
        match self.status.load(Ordering::Acquire) {
            0 => BrowserReadiness::Stopped,
            1 => BrowserReadiness::Starting,
            2 => BrowserReadiness::Ready,
            _ => BrowserReadiness::Unavailable {
                error: self
                    .failure
                    .lock()
                    .ok()
                    .and_then(|value| value.clone())
                    .unwrap_or_else(|| {
                        unavailable("The managed browser is unavailable.").to_api_error()
                    }),
            },
        }
    }
    fn desktop_available(&self) -> bool {
        // This backend is instantiated only for the trusted Docker bundle.
        // Transient process failures belong to readiness and desktop() errors.
        true
    }
    fn execute(
        &self,
        action: BrowserAction,
        dispatch: BrowserDispatch,
    ) -> BrowserFuture<'_, McpCallToolResponse> {
        Box::pin(async move {
            let mut state = self.state.lock().await;
            dispatch.check()?;
            let endpoint = self.endpoint().await?;
            dispatch.check()?;
            {
                let _input = dispatch.input_permit().await?;
                self.release_inputs().await?;
            }
            self.driver
                .execute(endpoint, action, dispatch, &mut state)
                .await
        })
    }
    fn connect(&self, _user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async {
            Err(unavailable(
                "Docker Workers do not connect a user's native Chrome.",
            ))
        })
    }
    fn disconnect(&self, _user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async {
            Err(unavailable(
                "Docker Workers do not connect a user's native Chrome.",
            ))
        })
    }
    fn open_local(&self, _user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async {
            Err(unavailable(
                "Open this Docker Worker's remote desktop from the conversation.",
            ))
        })
    }
    fn downloads_saved(
        &self,
        _user: String,
        _target: BrowserTargetKind,
        files: Vec<talos_types::ToolOutputFile>,
    ) -> BrowserFuture<'_, ()> {
        Box::pin(async move { crate::files::acknowledge_downloads(&self.driver.downloads, &files) })
    }
    fn release_inputs(&self) -> BrowserFuture<'_, ()> {
        Box::pin(async move {
            let held = self.driver.pressed.snapshot()?;
            if held.is_empty() {
                return Ok(());
            }
            let endpoint = self
                .components
                .lock()
                .await
                .as_ref()
                .map(|running| running.endpoint.clone())
                .ok_or_else(|| unavailable("The browser input release cannot be confirmed."))?;
            crate::proxy::release(endpoint, held).await?;
            self.driver.pressed.clear()
        })
    }
    fn shutdown(&self) -> BrowserFuture<'_, ()> {
        Box::pin(async move {
            if let Some(mut running) = self.components.lock().await.take() {
                // Ask Chrome to flush its own profile before terminating the process groups.
                if crate::proxy::close_browser(running.endpoint.clone())
                    .await
                    .is_ok()
                {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(3),
                        running.children[running.browser_index].wait(),
                    )
                    .await;
                }
                for child in running.children.iter_mut().rev() {
                    if let Err(error) = child.start_kill() {
                        if error.raw_os_error() != Some(libc::ESRCH) {
                            return Err(TalosError::from_source(
                                ErrorCode::RunCleanupUnconfirmed,
                                "Browser process termination could not be confirmed.",
                                error,
                            ));
                        }
                    }
                    tokio::time::timeout(Duration::from_secs(3), child.wait())
                        .await
                        .map_err(|error| {
                            TalosError::from_source(
                                ErrorCode::RunCleanupUnconfirmed,
                                "Browser process cleanup timed out.",
                                error,
                            )
                        })?
                        .map_err(|error| {
                            TalosError::from_source(
                                ErrorCode::RunCleanupUnconfirmed,
                                "Browser process exit could not be observed.",
                                error,
                            )
                        })?;
                }
            }
            self.driver.cleanup.drain().await?;
            self.status.store(0, Ordering::Release);
            Ok(())
        })
    }
}
fn command(program: impl AsRef<std::ffi::OsStr>, authority: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_clear()
        .env(
            "HOME",
            authority.parent().unwrap_or(Path::new("/home/talos")),
        )
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("DISPLAY", ":99")
        .env("XAUTHORITY", authority)
        .env("LANG", "C.UTF-8");
    command
}
fn protect_directory(path: &Path) -> TalosResult<()> {
    talos_fs::create_directory(path, true)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(
            |error| {
                TalosError::internal_with_source(
                    "Failed to protect the browser profile directory.",
                    error,
                )
            },
        )?;
    }
    Ok(())
}
fn cleanup_stale_x() -> TalosResult<()> {
    let lock = Path::new("/tmp/.X99-lock");
    if lock.exists() {
        let pid = std::fs::read_to_string(lock)
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Failed to inspect the previous browser display.",
                    error,
                )
            })?
            .trim()
            .parse::<i32>()
            .map_err(|error| {
                TalosError::internal_with_source(
                    "The previous browser display lock is invalid.",
                    error,
                )
            })?;
        #[cfg(unix)]
        if pid <= 0
            || unsafe { libc::kill(pid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Another process owns the browser display.",
            ));
        }
        talos_fs::remove_file(lock)?;
        let socket = Path::new("/tmp/.X11-unix/X99");
        if socket.exists() {
            talos_fs::remove_file(socket)?;
        }
    }
    Ok(())
}
fn unavailable(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::ServiceUnavailable, message)
}

fn window_manager_ready(output: &[u8]) -> bool {
    std::str::from_utf8(output)
        .ok()
        .and_then(|text| text.split("window id #").nth(1))
        .and_then(|id| id.trim().strip_prefix("0x"))
        .and_then(|id| u64::from_str_radix(id, 16).ok())
        .is_some_and(|id| id != 0)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn window_manager_readiness_requires_an_announced_nonzero_window() {
        assert!(window_manager_ready(
            b"_NET_SUPPORTING_WM_CHECK(WINDOW): window id # 0x20020b\n"
        ));
        for output in [
            b"_NET_SUPPORTING_WM_CHECK:  no such atom on any window.\n".as_slice(),
            b"_NET_SUPPORTING_WM_CHECK(WINDOW): window id # 0x0\n",
            b"invalid",
        ] {
            assert!(!window_manager_ready(output));
        }
    }

    #[tokio::test]
    async fn startup_health_rejects_an_exited_component_before_reporting_ready() {
        let cleanup = Arc::new(crate::process::ProcessCleanup::default());
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 7"]);
        let mut child = BrowserProcess::spawn(command, cleanup.clone()).unwrap();
        child.wait().await.unwrap();
        let home = tempfile::tempdir().unwrap();
        let lock = talos_fs::StdFileSystem
            .try_file_lock(&home.path().join("profile.lock"))
            .unwrap()
            .unwrap();
        let mut components = Components {
            children: vec![child],
            browser_index: 0,
            endpoint: String::new(),
            _profile_lock: lock,
        };
        assert!(components
            .ensure_running()
            .unwrap_err()
            .is_code(ErrorCode::ServiceUnavailable));
        drop(components);
        cleanup.drain().await.unwrap();
    }
}

#[cfg(all(test, unix))]
#[path = "managed_tests.rs"]
mod capability_tests;

struct Starting<'a>(&'a AtomicU8);
impl Drop for Starting<'_> {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}
