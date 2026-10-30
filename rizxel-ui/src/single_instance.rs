//! One Rizxel window per user: a second launch (double-clicking a `.rzxl`
//! while Rizxel runs) hands its files to the running window and exits.
//!
//! The running instance listens on a loopback TCP port and writes the port
//! and a random token to `instance.lock` in the app data folder. A new
//! launch reads the lock, connects and sends the token and its paths; the
//! running window opens them as tabs and comes to the front. A lock whose
//! port doesn't answer was left by a crashed instance and is taken over.
//! (Loopback TCP needs only `std`; named pipes and Unix sockets would need
//! `unsafe` platform calls or another crate.)

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// First line of a hand-off.
const MAGIC: &str = "RIZXEL-OPEN 1";
/// How long a new launch waits for the running window.
const TIMEOUT: Duration = Duration::from_millis(1500);

/// Contents of `instance.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Lock {
    port: u16,
    token: String,
    pid: u32,
}

/// Callback run when files arrive, shared with the listener thread.
type Waker = std::sync::Arc<std::sync::Mutex<Option<Box<dyn Fn() + Send>>>>;

/// Result of [`acquire`].
pub enum Instance {
    /// This process is the running window; files from later launches
    /// arrive through the [`Server`].
    Primary(Server),
    /// The running window took the files; this process should exit.
    HandedOff,
}

/// The running window's end: receives paths from later launches.
pub struct Server {
    rx: Receiver<Vec<PathBuf>>,
    lock_file: PathBuf,
    token: String,
    /// Called when files arrive (wakes the UI).
    waker: Waker,
}

impl Server {
    /// Files handed over since the last call.
    pub fn received(&self) -> Vec<PathBuf> {
        self.poll().unwrap_or_default()
    }

    /// `Some` (possibly empty) when a later launch reached this instance
    /// since the last call, with the files it handed over.
    pub fn poll(&self) -> Option<Vec<PathBuf>> {
        let mut got: Option<Vec<PathBuf>> = None;
        for paths in self.rx.try_iter() {
            got.get_or_insert_with(Vec::new).extend(paths);
        }
        got
    }

    /// `wake` runs on the listener thread whenever files arrive (e.g.
    /// requests a repaint so they open while the window is idle).
    pub fn set_waker(&self, wake: impl Fn() + Send + 'static) {
        if let Ok(mut w) = self.waker.lock() {
            *w = Some(Box::new(wake));
        }
    }
}

impl Drop for Server {
    /// Removes the lock unless another instance has taken it over.
    fn drop(&mut self) {
        if read_lock(&self.lock_file).is_some_and(|l| l.token == self.token) {
            std::fs::remove_file(&self.lock_file).ok();
        }
    }
}

/// Becomes the running instance, or hands `paths` to the one already
/// running. Any failure to talk to it makes this process the primary, so
/// Rizxel always starts.
pub fn acquire(lock_file: &Path, paths: &[PathBuf]) -> std::io::Result<Instance> {
    if let Some(lock) = read_lock(lock_file) {
        match hand_off(&lock, paths) {
            Ok(()) => return Ok(Instance::HandedOff),
            Err(e) => log::info!(
                "instance lock of pid {} is stale ({e}); taking over",
                lock.pid
            ),
        }
    }
    serve(lock_file).map(Instance::Primary)
}

fn read_lock(file: &Path) -> Option<Lock> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// A random-enough token from the std hasher's per-process random keys.
fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let part = || {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_u32(std::process::id());
        h.finish()
    };
    format!("{:016x}{:016x}", part(), part())
}

fn hand_off(lock: &Lock, paths: &[PathBuf]) -> std::io::Result<()> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, lock.port));
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut msg = format!("{MAGIC}\n{}\n", lock.token);
    for p in paths {
        // Relative paths are the new launch's; the window has another cwd.
        let p = std::path::absolute(p).unwrap_or_else(|_| p.clone());
        msg.push_str(&p.to_string_lossy());
        msg.push('\n');
    }
    msg.push('\n');
    stream.write_all(msg.as_bytes())?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    if reply.trim() == "OK" {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("unexpected reply {reply:?}")))
    }
}

fn serve(lock_file: &Path) -> std::io::Result<Server> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let lock = Lock {
        port: listener.local_addr()?.port(),
        token: new_token(),
        pid: std::process::id(),
    };
    if let Some(dir) = lock_file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec(&lock).map_err(std::io::Error::other)?;
    std::fs::write(lock_file, json)?;
    let (tx, rx) = mpsc::channel();
    let waker: Waker = Default::default();
    let thread_waker = waker.clone();
    let token = lock.token.clone();
    std::thread::Builder::new()
        .name("rizxel-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                match read_request(stream, &token) {
                    Ok(paths) => {
                        if tx.send(paths).is_err() {
                            return;
                        }
                        if let Ok(w) = thread_waker.lock() {
                            if let Some(wake) = w.as_ref() {
                                wake();
                            }
                        }
                    }
                    Err(e) => log::warn!("instance hand-off refused: {e}"),
                }
            }
        })?;
    Ok(Server {
        rx,
        lock_file: lock_file.to_path_buf(),
        token: lock.token,
        waker,
    })
}

fn read_request(stream: TcpStream, token: &str) -> std::io::Result<Vec<PathBuf>> {
    stream.set_read_timeout(Some(TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let mut lines = BufReader::new(stream).lines();
    let mut next = || lines.next().transpose().map(Option::unwrap_or_default);
    if next()? != MAGIC || next()? != token {
        return Err(std::io::Error::other("bad header or token"));
    }
    let mut paths = Vec::new();
    loop {
        let line = next()?;
        if line.is_empty() {
            break;
        }
        paths.push(PathBuf::from(line));
    }
    writer.write_all(b"OK\n")?;
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("rizxel_instance_{name}_{}", std::process::id()))
            .join("instance.lock")
    }

    /// Child half of the two-process test: hands its path to the parent.
    #[test]
    fn ipc_child_hand_off() {
        let Some(lock) = std::env::var_os("RIZXEL_IPC_CHILD_LOCK") else {
            return; // Only runs as the child of `second_launch_hands_off`.
        };
        let file = PathBuf::from("child/model.rzxl");
        match acquire(Path::new(&lock), &[file]).unwrap() {
            Instance::HandedOff => {}
            Instance::Primary(_) => panic!("the parent should be running"),
        }
    }

    #[test]
    fn second_launch_hands_off_to_the_running_one() {
        let lock = lock_path("handoff");
        let Instance::Primary(server) = acquire(&lock, &[]).unwrap() else {
            panic!("no instance runs yet");
        };
        let woke = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = woke.clone();
        server.set_waker(move || flag.store(true, std::sync::atomic::Ordering::SeqCst));
        // A second process: this test binary running the child test.
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "single_instance::tests::ipc_child_hand_off",
                "--nocapture",
            ])
            .env("RIZXEL_IPC_CHILD_LOCK", &lock)
            .status()
            .unwrap();
        assert!(status.success());
        let got = server.received();
        assert_eq!(got.len(), 1);
        assert!(got[0].is_absolute());
        assert!(got[0].ends_with("child/model.rzxl"));
        assert!(woke.load(std::sync::atomic::Ordering::SeqCst));
        drop(server);
        assert!(!lock.exists(), "a clean exit removes the lock");
        std::fs::remove_dir_all(lock.parent().unwrap()).ok();
    }

    #[test]
    fn stale_lock_does_not_block_start_up() {
        let lock = lock_path("stale");
        // A port that was free a moment ago: nothing answers there now.
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        let stale = Lock {
            port,
            token: "dead".into(),
            pid: 1,
        };
        std::fs::write(&lock, serde_json::to_vec(&stale).unwrap()).unwrap();
        let Instance::Primary(server) = acquire(&lock, &[PathBuf::from("a.rzxl")]).unwrap() else {
            panic!("a stale lock is taken over");
        };
        assert_ne!(read_lock(&lock).unwrap().token, "dead");
        // A garbage lock file is ignored too.
        drop(server);
        std::fs::write(&lock, b"garbage").unwrap();
        assert!(matches!(acquire(&lock, &[]).unwrap(), Instance::Primary(_)));
        std::fs::remove_dir_all(lock.parent().unwrap()).ok();
    }

    #[test]
    fn wrong_token_is_refused() {
        let lock = lock_path("token");
        let Instance::Primary(server) = acquire(&lock, &[]).unwrap() else {
            panic!();
        };
        let mut forged = read_lock(&lock).unwrap();
        forged.token = "forged".into();
        assert!(hand_off(&forged, &[PathBuf::from("x.rzxl")]).is_err());
        assert!(server.received().is_empty());
        drop(server);
        std::fs::remove_dir_all(lock.parent().unwrap()).ok();
    }
}
