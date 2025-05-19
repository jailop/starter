use crate::config::Config;
use tokio::{io::AsyncBufReadExt, process::Command, sync::mpsc::{self, Sender, Receiver}};
use std::process::Stdio;

pub enum ProcessCommand {
    Start,
    Stop,
}

pub type OutputChannels = Vec<(String, Receiver<String>, Sender<ProcessCommand>)>;
pub type ProcessSpawnResult = (OutputChannels, ProcessManager);

pub struct ProcessManager {
    control_senders: Vec<Sender<ProcessCommand>>,
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        // Try to stop all processes by sending Stop command
        for tx in &self.control_senders {
            let _ = tx.try_send(ProcessCommand::Stop);
        }
        // Optionally: sleep a bit to allow processes to terminate
        // (tokio::time::sleep is async, so for Drop we can't await)
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

impl ProcessManager {
    pub fn stop_all(&mut self) {
        for tx in &self.control_senders {
            let _ = tx.try_send(ProcessCommand::Stop);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Spawns all processes defined in the config and returns their output channels and control senders.
///
/// For each process in the configuration, this function:
/// - Creates a channel for receiving output lines from the process.
/// - Creates a channel for sending control commands (start/stop) to the process.
/// - Spawns a task to manage the process lifecycle and output forwarding.
/// - Collects the process name, output receiver, and control sender into a vector.
///
/// Returns a vector of tuples, each containing:
/// - The process name (String)
/// - The receiver for output lines (Receiver<String>)
/// - The sender for control commands (Sender<ProcessCommand>)
pub async fn spawn_process(config: &Config) -> Result<ProcessSpawnResult, Box<dyn std::error::Error>> {
    let mut channels = Vec::new();
    let mut control_senders = Vec::new();
    for proc in &config.processes {
        let (tx, rx) = mpsc::channel::<String>(100);
        let (cmd_tx, cmd_rx) = mpsc::channel::<ProcessCommand>(10);
        spawn_reader(
            proc.command.clone(),
            proc.args.clone(),
            proc.cwd.clone(),
            tx,
            cmd_rx,
        );
        control_senders.push(cmd_tx.clone());
        channels.push((proc.name.clone(), rx, cmd_tx));
    };
    let manager = ProcessManager { control_senders };
    Ok((channels, manager))
}

/// Spawns a process reader task that manages process lifecycle and output forwarding.
///
/// This function launches an asynchronous task that:
/// - Listens for start/stop commands via a channel.
/// - When started, spawns the child process and sets its process group.
/// - Forwards the process's stdout and stderr lines to the provided channel.
/// - When stopped, kills the process and its process group.
/// - Cleans up resources when the task ends.
fn spawn_reader(
    command: String,
    args: Vec<String>,
    cwd: String,
    tx: Sender<String>,
    mut cmd_rx: Receiver<ProcessCommand>,
) {
    tokio::spawn(async move {
        let mut child = None;
        let mut child_pgid = None;
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                ProcessCommand::Start => {
                    if child.is_none() {
                        let (mut spawned, pgid) = unsafe { spawn_child(&command, &args, &cwd) };
                        child_pgid = pgid;
                        spawn_output_readers(&mut spawned, &tx);
                        child = Some(spawned);
                    }
                }
                ProcessCommand::Stop => {
                    stop_child(&mut child, &mut child_pgid);
                }
            }
        }
        stop_child(&mut child, &mut child_pgid);
    });
}

/// Spawns a new process with the given command, arguments, and working directory.
///
/// # Safety
/// This function uses `pre_exec` to set the process group ID before exec'ing the child.
/// This is required for proper process group management and signal handling.
///
/// Returns:
/// - The spawned `tokio::process::Child`
/// - The process group ID (pgid) as an Option<i32>
unsafe fn spawn_child(
    command: &str,
    args: &[String],
    cwd: &str,
) -> (tokio::process::Child, Option<i32>) {
    #[cfg(unix)]
    {
        let spawned = Command::new(command)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            })
            .spawn()
            .expect("Failed to start process");
        let pgid = spawned.id().map(|pid| pid as i32);
        (spawned, pgid)
    }
    #[cfg(windows)]
    {
        let spawned = Command::new(command)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to start process");
        let pgid = spawned.id().map(|pid| pid as i32); // Not used on Windows
        (spawned, pgid)
    }
}

/// Spawns asynchronous tasks to read from the child's stdout and stderr, forwarding lines to the given sender.
///
/// This function takes ownership of the child's stdout and stderr handles (if present)
/// and spawns a task for each that reads lines and sends them to the provided channel.
/// This avoids aliasing and undefined behavior by using `.take()` to move the handles out of the child.
fn spawn_output_readers(child: &mut tokio::process::Child, tx: &Sender<String>) {

    // Take ownership of stdio handles using .take() so no aliasing or UB occurs.
    if let Some(stdout) = child.stdout.take() {
        handle_output_owned(stdout, tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        handle_output_owned(stderr, tx.clone());
    }
}

/// Reads lines from the given stream and sends them to the provided channel.
///
/// This function is used by `spawn_output_readers` to asynchronously read lines from
/// a process's stdout or stderr and forward them to the main application via a channel.
/// Each line is trimmed of trailing newlines before sending.
fn handle_output_owned<T>(stream: T, tx: Sender<String>)
where
    T: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut reader = tokio::io::BufReader::new(stream);
    tokio::spawn(async move {
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            let _ = tx.send(line.trim_end().to_string()).await;
            line.clear();
        }
    });
}

/// Stops the given child process and its process group, if running.
///
/// This function:
/// - Sends a SIGKILL to the process group (if available) to ensure all subprocesses are killed.
/// - Calls `.kill()` on the main child process to ensure it is terminated.
/// - Cleans up the process handle and process group ID.
fn stop_child(child: &mut Option<tokio::process::Child>, child_pgid: &mut Option<i32>) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid;
        if let Some(mut c) = child.take() {
            if let Some(pgid) = child_pgid.take() {
                let _ = signal::killpg(Pid::from_raw(pgid), Signal::SIGKILL);
            }
            let _ = futures::executor::block_on(c.kill());
        }
    }
    #[cfg(windows)]
    {
        if let Some(mut c) = child.take() {
            let _ = futures::executor::block_on(c.kill());
        }
        // No process group support on Windows; only the main process is killed.
        let _ = child_pgid.take();
    }
}

