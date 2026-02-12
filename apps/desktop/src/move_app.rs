//! ordin moves itself to the person's Applications folder (OrdinDesktop.md DSK-MOVE-1 to DSK-MOVE-4;
//! plan 08 Phase 19). macOS only: elsewhere ordin counts as installed and nothing is moved. The rule
//! and the move are functions of paths and of a [`MoveTools`], so both are testable without a Mac
//! bundle; [`SystemTools`] runs the real `ditto`, `xattr` and `open`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The longest any one tool may run (DSK-MOVE-2).
pub const TOOL_LIMIT: Duration = Duration::from_secs(120);

/// Why a move failed (DSK-MOVE-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveError {
    /// The folder, the older copy or the copy itself: `DSK_MOVE_COPY_FAILED`, retryable.
    CopyFailed,
    /// The detached start of the copy: `DSK_MOVE_START_FAILED`.
    StartFailed,
}

/// The system tools a move runs.
pub trait MoveTools {
    /// Runs `program` with `args` to its end within [`TOOL_LIMIT`]; `Err` carries the exit code, or
    /// -1 when it could not start or ran too long.
    fn run(&self, program: &str, args: &[&OsStr]) -> Result<(), i32>;
    /// Starts a detached `/bin/sh` that waits until process `pid` has ended, then opens `target`.
    fn open_after_exit(&self, pid: u32, target: &Path) -> std::io::Result<()>;
}

/// The `.app` bundle holding `exe` (`<bundle>.app/Contents/MacOS/<exe>`), or `None`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = macos.file_name().is_some_and(|n| n == "MacOS")
        && contents.file_name().is_some_and(|n| n == "Contents")
        && bundle.extension().is_some_and(|e| e == "app");
    is_bundle.then(|| bundle.to_path_buf())
}

/// DSK-MOVE-1: whether `bundle` sits directly in `/Applications` or `<home>/Applications`, both
/// compared after resolving links. A bundle elsewhere (a mounted disk image, Downloads, a translocated
/// copy) is not installed.
// spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-1
pub fn is_installed(bundle: &Path, home: &Path) -> bool {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let Some(folder) = bundle.parent() else {
        return false;
    };
    let folder = real(folder);
    [PathBuf::from("/Applications"), home.join("Applications")]
        .iter()
        .any(|applications| real(applications) == folder)
}

/// DSK-MOVE-1: where the copy goes, `<home>/Applications/<bundle's name>`.
pub fn target_of(bundle: &Path, home: &Path) -> Option<PathBuf> {
    Some(home.join("Applications").join(bundle.file_name()?))
}

/// The Bin's name for an older copy: `<product> <yyyy-mm-dd hh.mm.ss>.app`.
pub fn bin_name(bundle_name: &OsStr, stamp: &str) -> PathBuf {
    let name = Path::new(bundle_name);
    let product = name
        .file_stem()
        .map_or_else(|| "ordin".into(), |s| s.to_string_lossy());
    PathBuf::from(format!("{product} {stamp}.app"))
}

fn log_step(step: &'static str, outcome: &'static str, code: Option<i32>) {
    match code {
        Some(code) => tracing::warn!(component = "desktop", step, outcome, code, "move.step"),
        None => tracing::info!(component = "desktop", step, outcome, "move.step"),
    }
}

/// DSK-MOVE-2 and DSK-MOVE-3: copies `bundle` to `<home>/Applications`, an older copy first moved to
/// `<home>/.Trash` as `stamp` names it, the quarantine attribute removed from the copy only, then the
/// copy opened once process `pid` has ended. Returns the copy's path. Nothing outside
/// `<home>/Applications` and `<home>/.Trash` is changed; a failed copy leaves no partial copy.
// spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-2
// spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-3
pub fn move_bundle(
    bundle: &Path,
    home: &Path,
    stamp: &str,
    pid: u32,
    tools: &dyn MoveTools,
) -> Result<PathBuf, MoveError> {
    let target = target_of(bundle, home).ok_or(MoveError::CopyFailed)?;
    let applications = home.join("Applications");
    if std::fs::create_dir_all(&applications).is_err() {
        log_step("folder", "failed", None);
        return Err(MoveError::CopyFailed);
    }
    if target.exists() {
        let bin = home.join(".Trash");
        let older = bin.join(bin_name(target.file_name().unwrap_or_default(), stamp));
        if std::fs::create_dir_all(&bin).is_err() || std::fs::rename(&target, &older).is_err() {
            log_step("older_copy", "failed", None);
            return Err(MoveError::CopyFailed);
        }
        log_step("older_copy", "moved_to_bin", None);
    }
    if let Err(code) = tools.run("/usr/bin/ditto", &[bundle.as_os_str(), target.as_os_str()]) {
        log_step("copy", "failed", Some(code));
        let _ = std::fs::remove_dir_all(&target);
        return Err(MoveError::CopyFailed);
    }
    log_step("copy", "done", None);
    // The copy runs without it; a failure here is logged and the move goes on.
    match tools.run(
        "/usr/bin/xattr",
        &[
            OsStr::new("-dr"),
            OsStr::new("com.apple.quarantine"),
            target.as_os_str(),
        ],
    ) {
        Ok(()) => log_step("quarantine", "removed", None),
        Err(code) => log_step("quarantine", "failed", Some(code)),
    }
    if tools.open_after_exit(pid, &target).is_err() {
        log_step("start", "failed", None);
        return Err(MoveError::StartFailed);
    }
    log_step("start", "after_exit", None);
    Ok(target)
}

/// The real tools.
pub struct SystemTools;

impl MoveTools for SystemTools {
    fn run(&self, program: &str, args: &[&OsStr]) -> Result<(), i32> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| -1)?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => return Err(status.code().unwrap_or(-1)),
                Ok(None) if started.elapsed() >= TOOL_LIMIT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(-1);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => return Err(-1),
            }
        }
    }

    fn open_after_exit(&self, pid: u32, target: &Path) -> std::io::Result<()> {
        // `$1` is the process to wait for, `$2` the copy: neither is ever part of the script's text.
        const SCRIPT: &str = "i=0; while kill -0 \"$1\" 2>/dev/null && [ $i -lt 300 ]; do sleep 0.2; i=$((i+1)); done; exec /usr/bin/open \"$2\"";
        Command::new("/bin/sh")
            .arg("-c")
            .arg(SCRIPT)
            .arg("ordin-move")
            .arg(pid.to_string())
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            let path = std::env::temp_dir().join(format!(
                "ordin-move-{tag}-{}-{}",
                std::process::id(),
                uuid::Uuid::now_v7()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Dir(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A bundle at `<root>/<folder>/ordin.app` with its executable.
    fn bundle(root: &Path, folder: &str) -> PathBuf {
        let app = root.join(folder).join("ordin.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/ordin"), b"new").unwrap();
        app
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let dest = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &dest);
            } else {
                std::fs::copy(entry.path(), dest).unwrap();
            }
        }
    }

    /// Copies as `ditto` does, or fails part-way as told; records every call.
    #[derive(Default)]
    struct FakeTools {
        ditto_fails: bool,
        start_fails: bool,
        calls: RefCell<Vec<String>>,
        opened: RefCell<Option<(u32, PathBuf)>>,
    }

    impl MoveTools for FakeTools {
        fn run(&self, program: &str, args: &[&OsStr]) -> Result<(), i32> {
            self.calls.borrow_mut().push(program.to_owned());
            if program == "/usr/bin/ditto" {
                let (from, to) = (Path::new(args[0]), Path::new(args[1]));
                if self.ditto_fails {
                    std::fs::create_dir_all(to.join("Contents")).unwrap();
                    return Err(1);
                }
                copy_tree(from, to);
            }
            Ok(())
        }

        fn open_after_exit(&self, pid: u32, target: &Path) -> std::io::Result<()> {
            if self.start_fails {
                return Err(std::io::Error::other("no shell"));
            }
            *self.opened.borrow_mut() = Some((pid, target.to_path_buf()));
            Ok(())
        }
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-1
    #[test]
    fn table_is_it_installed() {
        let root = Dir::new("installed");
        let home = root.0.join("home");
        let cases = [
            (bundle(&home, "Applications"), true),
            (bundle(&root.0, "Volumes/ordin"), false),
            (bundle(&home, "Downloads"), false),
            (bundle(&home, "Applications/Tools"), false),
        ];
        for (app, installed) in cases {
            assert_eq!(is_installed(&app, &home), installed, "{}", app.display());
        }
        assert!(is_installed(Path::new("/Applications/ordin.app"), &home));
        let exe = home.join("Downloads/ordin.app/Contents/MacOS/ordin");
        assert_eq!(bundle_of(&exe), Some(home.join("Downloads/ordin.app")));
        assert_eq!(bundle_of(Path::new("/usr/local/bin/ordin")), None);
    }

    // Scenario: Happy path — opened from the disk image
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-2
    #[test]
    fn happy_path_opened_from_the_disk_image() {
        let root = Dir::new("happy");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let app = bundle(&root.0, "Volumes/ordin");
        let tools = FakeTools::default();
        let target = move_bundle(&app, &home, "2026-02-12 18.00.00", 42, &tools).unwrap();
        assert_eq!(target, home.join("Applications/ordin.app"));
        assert_eq!(
            std::fs::read(target.join("Contents/MacOS/ordin")).unwrap(),
            b"new"
        );
        assert_eq!(*tools.calls.borrow(), ["/usr/bin/ditto", "/usr/bin/xattr"]);
        assert_eq!(*tools.opened.borrow(), Some((42, target)));
    }

    // Scenario: Edge case — an older copy is there
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-2
    #[test]
    fn edge_an_older_copy_goes_to_the_bin() {
        let root = Dir::new("older");
        let home = root.0.join("home");
        let older = bundle(&home, "Applications");
        std::fs::write(older.join("Contents/MacOS/ordin"), b"old").unwrap();
        let app = bundle(&home, "Downloads");
        move_bundle(&app, &home, "2026-02-12 18.00.00", 7, &FakeTools::default()).unwrap();
        let binned = home.join(".Trash/ordin 2026-02-12 18.00.00.app/Contents/MacOS/ordin");
        assert_eq!(std::fs::read(binned).unwrap(), b"old");
        assert_eq!(
            std::fs::read(home.join("Applications/ordin.app/Contents/MacOS/ordin")).unwrap(),
            b"new"
        );
    }

    // Scenario: Error path — the copy fails
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-3
    #[test]
    fn error_path_a_failed_copy_leaves_no_partial_copy() {
        let root = Dir::new("copy-fails");
        let home = root.0.join("home");
        let app = bundle(&root.0, "Volumes/ordin");
        let tools = FakeTools {
            ditto_fails: true,
            ..FakeTools::default()
        };
        assert_eq!(
            move_bundle(&app, &home, "s", 1, &tools),
            Err(MoveError::CopyFailed)
        );
        assert!(!home.join("Applications/ordin.app").exists());
        assert!(tools.opened.borrow().is_none());
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-3
    #[test]
    fn error_path_the_copy_cannot_be_started() {
        let root = Dir::new("start-fails");
        let home = root.0.join("home");
        let app = bundle(&root.0, "Volumes/ordin");
        let tools = FakeTools {
            start_fails: true,
            ..FakeTools::default()
        };
        assert_eq!(
            move_bundle(&app, &home, "s", 1, &tools),
            Err(MoveError::StartFailed)
        );
        assert!(
            home.join("Applications/ordin.app").exists(),
            "the copy stays"
        );
    }

    #[test]
    fn the_bin_name_carries_the_product_and_the_time() {
        assert_eq!(
            bin_name(OsStr::new("ordin.app"), "2026-02-12 18.00.00"),
            PathBuf::from("ordin 2026-02-12 18.00.00.app")
        );
    }

    // The real tools: a tool's exit code comes back, and a missing one is -1.
    #[cfg(unix)]
    #[test]
    fn system_tools_report_the_exit_code() {
        assert_eq!(
            SystemTools.run("/bin/sh", &[OsStr::new("-c"), OsStr::new("exit 0")]),
            Ok(())
        );
        assert_eq!(
            SystemTools.run("/bin/sh", &[OsStr::new("-c"), OsStr::new("exit 3")]),
            Err(3)
        );
        assert_eq!(SystemTools.run("/no/such/tool", &[]), Err(-1));
    }
}
