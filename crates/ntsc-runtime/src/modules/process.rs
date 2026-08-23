//! NTSC standard library: `process` module.
//! Process spawning plus the threaded workers used by `collections.channel`.

use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::registry;

fn make_response_json(status: i64, stdout: &str, stderr: &str) -> String {
    format!(
        "{{\"status\":{},\"stdout\":\"{}\",\"stderr\":\"{}\"}}",
        status,
        stdout.replace('"', "\\\"").replace('\n', "\\n"),
        stderr.replace('"', "\\\"").replace('\n', "\\n")
    )
}

/// `process.exec(command)` — runs through `/bin/sh -c`; returns the exit
/// code.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_process_exec(command: i64) -> i64 {
    let cmd = registry::get_string(command).unwrap_or_default();
    let mut process = Command::new("sh");
    super::os::apply_environment(&mut process);
    match process.arg("-c").arg(&cmd).status() {
        Ok(status) => status.code().unwrap_or(-1) as i64,
        Err(e) => super::throw_str(format!("process.exec: cannot start command: {e}")),
    }
}

/// `process.exec_output(command)` — JSON with `status`, `stdout`, `stderr`.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_process_exec_output(command: i64) -> i64 {
    let cmd = registry::get_string(command).unwrap_or_default();
    let mut process = Command::new("sh");
    super::os::apply_environment(&mut process);
    match process.arg("-c").arg(&cmd).output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1) as i64;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            registry::put_string(make_response_json(status, &stdout, &stderr))
        }
        Err(e) => super::throw_str(format!("process.exec_output: cannot start command: {e}")),
    }
}

/// `process.spawn(program, args)` — `args` is split on spaces; there is no
/// quoting.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_process_spawn(program: i64, args: i64) -> i64 {
    let prog = registry::get_string(program).unwrap_or_default();
    let args_str = registry::get_string(args).unwrap_or_default();
    let arg_vec: Vec<&str> = if args_str.is_empty() {
        vec![]
    } else {
        args_str.split(' ').collect()
    };
    let mut process = Command::new(&prog);
    super::os::apply_environment(&mut process);
    match process.args(&arg_vec).output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1) as i64;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            registry::put_string(make_response_json(status, &stdout, &stderr))
        }
        Err(e) => super::throw_str(format!("process.spawn: cannot start '{prog}': {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_process_pid() -> i64 {
    std::process::id() as i64
}

// ── Threads ─────────────────────────────────────────────────────────────

static NEXT_THREAD_ID: AtomicI64 = AtomicI64::new(1);

static THREAD_HANDLES: LazyLock<Mutex<HashMap<i64, std::thread::JoinHandle<()>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// `process.spawn_thread(f, arg)` — `f` is generated `extern "C" fn(i64)`;
/// returns an opaque id for `thread_join`. An uncaught exception inside the
/// thread aborts the process.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_process_spawn_thread(f: extern "C" fn(i64), arg: i64) -> i64 {
    let id = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);
    match std::thread::Builder::new().spawn(move || f(arg)) {
        Ok(handle) => {
            if let Ok(mut threads) = THREAD_HANDLES.lock() {
                threads.insert(id, handle);
            }
            id
        }
        Err(e) => super::throw_str(format!("process.spawn_thread: cannot start thread: {e}")),
    }
}

/// `process.thread_join(id)` — 1 when the thread was joined, 0 when `id`
/// does not name a live spawned thread.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_process_thread_join(id: i64) -> i8 {
    let handle = match THREAD_HANDLES.lock() {
        Ok(mut threads) => threads.remove(&id),
        Err(poisoned) => poisoned.into_inner().remove(&id),
    };
    match handle {
        Some(handle) => match handle.join() {
            Ok(()) => 1,
            Err(_) => 1, // the thread finished even though it panicked
        },
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    fn read(id: i64) -> String {
        let value = registry::get_string(id).unwrap_or_default();
        let _ = registry::take_string(id);
        value
    }

    #[test]
    fn test_exec() {
        let r = ntsc_process_exec(put("echo hello"));
        assert_eq!(r, 0);
    }

    #[test]
    fn test_exec_output_observes_environment_overlay() {
        let name = put("NTSC_PROCESS_ENV_OVERLAY_TEST");
        let value = put("child-value");
        assert_eq!(crate::modules::os::ntsc_os_setenv(name, value), 1);

        let result =
            ntsc_process_exec_output(put("printf '%s' \"$NTSC_PROCESS_ENV_OVERLAY_TEST\""));
        assert!(
            read(result).contains("\"stdout\":\"child-value\""),
            "child process did not receive the environment overlay"
        );

        assert_eq!(
            crate::modules::os::ntsc_os_unsetenv(put("NTSC_PROCESS_ENV_OVERLAY_TEST")),
            1
        );
        let result =
            ntsc_process_exec_output(put("printf '%s' \"$NTSC_PROCESS_ENV_OVERLAY_TEST\""));
        assert!(
            read(result).contains("\"stdout\":\"\""),
            "child process did not receive the environment removal"
        );
    }

    #[test]
    fn test_pid() {
        assert!(ntsc_process_pid() > 0);
    }

    #[test]
    fn test_spawn_missing_program_throws() {
        use crate::modules::test_util::catch_throw;
        let err = catch_throw(|| {
            let _ = ntsc_process_spawn(put("/nonexistent_program_xyz_123"), put(""));
        });
        let msg = err.unwrap();
        assert!(msg.contains("process.spawn"), "unexpected message: {msg}");
    }

    #[test]
    fn test_spawn_thread_and_join() {
        use std::sync::atomic::{AtomicI64, Ordering};
        extern "C" fn worker(arg: i64) {
            registry::with_opaque_mut::<_, AtomicI64>(arg, |counter| {
                counter.fetch_add(10, Ordering::SeqCst);
            });
        }
        let counter = registry::put_opaque(AtomicI64::new(0));
        let id = ntsc_process_spawn_thread(worker, counter);
        assert!(id > 0);
        assert_eq!(ntsc_process_thread_join(id), 1);
        assert_eq!(
            registry::with_opaque::<_, AtomicI64>(counter, |c| c.load(Ordering::SeqCst)),
            Some(10)
        );
        let _ = registry::take_opaque::<AtomicI64>(counter);
    }

    #[test]
    fn test_join_unknown_thread_returns_zero() {
        assert_eq!(ntsc_process_thread_join(999999), 0);
    }
}
