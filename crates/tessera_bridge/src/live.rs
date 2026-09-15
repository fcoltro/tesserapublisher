//! The bridge to the window on screen.
//!
//! A model's client speaks to a process it launched, over that process's
//! stdin and stdout — and the window somebody is working in is not that
//! process. So the window **listens** on a loopback port and writes the
//! port number to a file beside the preferences, and `tessera_app --mcp`
//! **relays**: when the file names a port that answers, every line from
//! stdin goes to the window and every reply comes back; when nothing
//! answers, it serves a headless Tessera of its own, as it always did.
//! The client configures one command and gets whichever is there.
//!
//! Inside the window, the socket's thread cannot touch the application —
//! the document belongs to the UI thread, and egui is not the kind of
//! thing to share. Each request crosses to the UI thread on a channel with
//! a reply channel of its own, the frame loop answers it ([`Listener::pump`])
//! and the socket's thread writes the answer back. A request wakes the
//! window, so a still canvas is not a silent one.
//!
//! Loopback only, and no secret: any process on this machine that can
//! reach the port can drive the document, which is the same trust a local
//! script has always had over a running application's files.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use tessera_ui::TesseraApp;

/// The name of the file the port is kept in, beside the preferences.
pub const PORT_FILE: &str = "bridge.port";

/// A request that arrived on the socket and is waiting for the UI thread.
struct Pending {
    line: String,
    reply: Sender<Option<String>>,
}

/// The window's end: a listening socket, and the requests it has received.
pub struct Listener {
    requests: Receiver<Pending>,
    port: u16,
    port_file: PathBuf,
}

impl Listener {
    /// Listen on a loopback port the system picks, record it in `port_file`,
    /// and call `wake` whenever a request arrives so the frame loop runs.
    pub fn start(
        port_file: PathBuf,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let socket = TcpListener::bind("127.0.0.1:0")?;
        let port = socket.local_addr()?.port();
        if let Some(dir) = port_file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&port_file, port.to_string())?;

        let (tx, requests) = channel::<Pending>();
        let wake = std::sync::Arc::new(wake);
        std::thread::Builder::new()
            .name("tessera-bridge".into())
            .spawn(move || {
                for stream in socket.incoming() {
                    let Ok(stream) = stream else { continue };
                    let tx = tx.clone();
                    let wake = wake.clone();
                    std::thread::spawn(move || serve_connection(stream, &tx, &*wake));
                }
            })?;
        Ok(Self {
            requests,
            port,
            port_file,
        })
    }

    /// The port this is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Answer every request that has arrived. Called once per frame by the
    /// window, with the application it is drawing.
    pub fn pump(&self, state: &mut TesseraApp) -> usize {
        let mut answered = 0;
        while let Ok(pending) = self.requests.try_recv() {
            let _ = pending.reply.send(crate::handle(state, &pending.line));
            answered += 1;
        }
        answered
    }
}

impl Drop for Listener {
    /// The port file names a window that is gone, so it goes too. A crash
    /// leaves it behind, and a stale port simply does not answer — the
    /// relay falls back to headless, which is the right answer for a model
    /// that asked for a Tessera and found no window.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.port_file);
    }
}

/// One client on the window's socket: each line crosses to the UI thread
/// and its reply comes back, in order, until the client hangs up.
fn serve_connection(stream: TcpStream, tx: &Sender<Pending>, wake: &dyn Fn()) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let (reply_tx, reply_rx) = channel();
        if tx
            .send(Pending {
                line,
                reply: reply_tx,
            })
            .is_err()
        {
            break; // the window is gone
        }
        wake();
        match reply_rx.recv() {
            Ok(Some(reply)) => {
                if writeln!(out, "{reply}").is_err() || out.flush().is_err() {
                    break;
                }
            }
            Ok(None) => {} // a notification: nothing to say
            Err(_) => break,
        }
    }
}

/// The port a running window recorded, if the file is there and readable.
pub fn recorded_port(port_file: &Path) -> Option<u16> {
    std::fs::read_to_string(port_file).ok()?.trim().parse().ok()
}

/// Carry lines from `input` to the window on `port` and its replies to
/// `output`, until either end closes. `false` — with nothing consumed —
/// when nothing answers on the port.
pub fn relay(port: u16, input: impl Read + Send + 'static, output: impl Write) -> bool {
    let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let Ok(mut to_window) = stream.try_clone() else {
        return false;
    };
    // Up: the client's lines to the window, on a thread of their own, so a
    // slow reply never blocks the next request from being sent.
    std::thread::spawn(move || {
        let mut input = BufReader::new(input);
        let mut line = String::new();
        while let Ok(n) = input.read_line(&mut line) {
            if n == 0 || to_window.write_all(line.as_bytes()).is_err() {
                break;
            }
            let _ = to_window.flush();
            line.clear();
        }
        let _ = to_window.shutdown(std::net::Shutdown::Write);
    });
    // Down: the window's replies to the client, here.
    let mut output = output;
    let mut from_window = BufReader::new(stream);
    let mut line = String::new();
    while let Ok(n) = from_window.read_line(&mut line) {
        if n == 0 || output.write_all(line.as_bytes()).is_err() {
            break;
        }
        let _ = output.flush();
        line.clear();
    }
    true
}

/// `tessera_app --mcp`: relay to the window named in `port_file` when one
/// answers, and serve a headless Tessera on stdio otherwise.
pub fn serve_or_relay_stdio(port_file: &Path) {
    if let Some(port) = recorded_port(port_file)
        && relay(port, std::io::stdin(), std::io::stdout())
    {
        return;
    }
    crate::serve_stdio();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn port_file() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tessera-live-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        dir.join(PORT_FILE)
    }

    /// Run `pump` on `state` until `done` is set, the way the frame loop
    /// would — so a test's socket thread has something answering it.
    fn pump_until(
        listener: &Listener,
        state: &mut TesseraApp,
        done: &std::sync::atomic::AtomicBool,
    ) {
        let start = std::time::Instant::now();
        while !done.load(Ordering::SeqCst) {
            listener.pump(state);
            assert!(start.elapsed() < std::time::Duration::from_secs(10), "hung");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        listener.pump(state);
    }

    #[test]
    fn a_request_on_the_socket_is_answered_by_the_window_and_wakes_it() {
        let file = port_file();
        let woken = Arc::new(AtomicUsize::new(0));
        let listener = {
            let woken = woken.clone();
            Listener::start(file.clone(), move || {
                woken.fetch_add(1, Ordering::SeqCst);
            })
            .expect("a port")
        };
        assert_eq!(recorded_port(&file), Some(listener.port()));

        let mut state = TesseraApp::headless();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let client = {
            let (port, done) = (listener.port(), done.clone());
            std::thread::spawn(move || {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut reply = String::new();
                for (i, request) in [
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"add_rectangle","arguments":{"x":0,"y":0,"width":10,"height":10}}}"#,
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                    r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
                ]
                .iter()
                .enumerate()
                {
                    writeln!(stream, "{request}").unwrap();
                    if i != 1 {
                        reply.clear();
                        reader.read_line(&mut reply).unwrap();
                        assert!(reply.contains("\"result\""), "{reply}");
                    }
                }
                done.store(true, Ordering::SeqCst);
                reply
            })
        };
        pump_until(&listener, &mut state, &done);
        let last = client.join().expect("the client thread");
        assert!(
            last.contains("\"id\":2"),
            "the ping was answered last: {last}"
        );
        assert_eq!(
            state.active().document().frames.len(),
            1,
            "the rectangle landed in the window's document"
        );
        assert!(
            woken.load(Ordering::SeqCst) >= 2,
            "each request woke the window"
        );

        drop(listener);
        assert_eq!(
            recorded_port(&file),
            None,
            "the port file goes with the listener"
        );
    }

    #[test]
    fn the_relay_carries_lines_both_ways_and_says_when_nothing_answers() {
        let file = port_file();
        let listener = Listener::start(file, || {}).expect("a port");
        let mut state = TesseraApp::headless();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // A client whose stdin is one ping, whose stdout is a pipe we read.
        let (port, done_flag) = (listener.port(), done.clone());
        let relayed = std::thread::spawn(move || {
            let input = std::io::Cursor::new(
                b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n".to_vec(),
            );
            let mut output = Vec::new();
            let ok = relay(port, input, &mut output);
            done_flag.store(true, Ordering::SeqCst);
            (ok, String::from_utf8(output).unwrap())
        });
        pump_until(&listener, &mut state, &done);
        let (ok, output) = relayed.join().unwrap();
        assert!(ok);
        assert!(output.contains("\"id\":7"), "{output}");

        // Nothing on a port that is not listening.
        let dead = {
            let s = TcpListener::bind("127.0.0.1:0").unwrap();
            s.local_addr().unwrap().port()
        };
        assert!(!relay(dead, std::io::Cursor::new(Vec::new()), Vec::new()));
    }
}
