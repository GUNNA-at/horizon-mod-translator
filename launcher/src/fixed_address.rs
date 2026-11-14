//! Starting a large game at the address it was linked for.
//!
//! macOS on Apple silicon loads an executable in the 2 GiB of addresses below the system's
//! shared libraries, at a place the kernel moves by a random amount each time (ASLR): 0, 32 or
//! 64 MiB and up to 16 MiB more, as 4,000 starts of a small program showed. A program that
//! leaves too little of that room does not start every time: dyld cannot map the shared
//! libraries ("dyld cache ... not loaded") and the process dies before any of its code runs.
//! Of 300 starts of a program that left 64 MiB, 94 died so; with 75 MiB left 47, with 80 MiB
//! 29, with 84 MiB 2, and with 88 MiB or more none.
//!
//! A game built with the native mods of a large pack fills nearly all of the room: 2.07 GB was
//! measured for a pack of 367 mods, which leaves 75 MiB. 14 starts in 64 of that game died, and
//! none in 64 with the move turned off.
//!
//! So a game that large is started through the launcher itself: `launcher
//! --exec-at-linked-address <program> [arguments...]` becomes the program (as `exec` does), with
//! the random move off for that one process, which is what a debugger asks for the program it
//! starts. The price is that process's address randomisation, so it is paid only where a start
//! could fail: a game with room for any move keeps its random address.
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

/// The launcher's argument for [`run_worker`].
pub const WORKER_ARG: &str = "--exec-at-linked-address";

/// The addresses an executable has below the shared libraries.
const ROOM: u64 = 2 << 30;

/// The room left over that is taken as enough for any move. 88 MiB was enough in every start
/// measured; the rest is for what a file's size does not show of the space its image takes,
/// and for a system that moves a little further.
const ENOUGH: u64 = 128 << 20;

/// Whether a program of `size` bytes needs its linked address here.
pub fn needed(size: u64) -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64")) && size > ROOM - ENOUGH
}

/// The command that starts `program`: itself, or through this launcher where the program is
/// one that must load at its linked address ([`needed`]). Arguments, environment and folder are
/// the caller's to add, and reach the program either way. A launcher whose own file is gone
/// (replaced by an update while it runs) starts the program as it is: most starts still work.
pub fn command(program: &Path) -> Command {
    let large = std::fs::metadata(program).is_ok_and(|m| needed(m.len()));
    match std::env::current_exe() {
        Ok(launcher) if large && launcher.is_file() => {
            let mut cmd = Command::new(launcher);
            cmd.arg(WORKER_ARG).arg(program);
            cmd
        }
        _ => Command::new(program),
    }
}

/// `launcher --exec-at-linked-address <program> [arguments...]`: becomes the program. None for
/// any other command line; the exit code when the program could not be started.
pub fn run_worker() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != WORKER_ARG {
        return None;
    }
    let Some(program) = args.next() else {
        return Some(2);
    };
    let rest: Vec<OsString> = args.collect();
    Some(exec(&program, &rest))
}

/// Replaces this process with `program`, loaded at its linked address. Returns only when that
/// failed, with the exit code to end with.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn exec(program: &std::ffi::OsStr, rest: &[OsString]) -> i32 {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    /// `<spawn.h>`'s private flag a debugger uses: no random slide for the new image.
    const DISABLE_ASLR: libc::c_short = 0x0100;
    let c = |s: &std::ffi::OsStr| CString::new(s.as_bytes()).ok();
    let Some(path) = c(program) else { return 2 };
    let mut owned = vec![path.clone()];
    for a in rest {
        match c(a) {
            Some(a) => owned.push(a),
            None => return 2,
        }
    }
    let mut argv: Vec<*mut libc::c_char> = owned.iter().map(|a| a.as_ptr() as *mut _).collect();
    argv.push(std::ptr::null_mut());
    // SAFETY: the attribute object is initialised before it is used and lives until the
    // call; every string is a NUL-terminated copy kept in `owned`, and the list ends with a
    // null pointer, as the process's own environment does. With POSIX_SPAWN_SETEXEC the call
    // returns only on failure.
    let error = unsafe {
        let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
        if libc::posix_spawnattr_init(&mut attr) != 0 {
            return 126;
        }
        libc::posix_spawnattr_setflags(
            &mut attr,
            libc::POSIX_SPAWN_SETEXEC as libc::c_short | DISABLE_ASLR,
        );
        let environment = *libc::_NSGetEnviron();
        libc::posix_spawn(
            std::ptr::null_mut(),
            path.as_ptr(),
            std::ptr::null(),
            &attr,
            argv.as_ptr(),
            environment,
        )
    };
    eprintln!(
        "{}: {}",
        program.to_string_lossy(),
        std::io::Error::from_raw_os_error(error)
    );
    126
}

/// Elsewhere nothing needs it: the program is run as it is.
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn exec(program: &std::ffi::OsStr, rest: &[OsString]) -> i32 {
    match Command::new(program).args(rest).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            126
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a program that may not fit after a random move is started at its linked address,
    /// and only where the room is that tight: the pack's game is, one with 200 MiB of room or a
    /// game of the usual size is not, and neither is a program that is not there.
    #[test]
    fn only_a_program_that_may_not_fit_is_started_at_its_linked_address() {
        let tight = cfg!(all(target_os = "macos", target_arch = "aarch64"));
        assert_eq!(needed(2_068_933_648), tight);
        assert!(!needed((2 << 30) - (200 << 20)));
        assert!(!needed(180_000_000));
        let missing = Path::new("/nonexistent/horizon-client");
        assert_eq!(command(missing).get_program(), missing.as_os_str());
        assert_eq!(command(missing).get_args().count(), 0);
    }

    /// A game that large is started through the launcher: the worker's argument, the game, and
    /// then whatever the caller adds, which is the command line the worker reads.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn a_large_game_is_started_through_the_launcher() {
        let dir =
            std::env::temp_dir().join(format!("horizon-fixed-address-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let game = dir.join("horizon-client");
        // Only its length is asked: a file of that size with nothing written takes no space.
        std::fs::File::create(&game)
            .unwrap()
            .set_len(2_068_933_648)
            .unwrap();
        let mut cmd = command(&game);
        cmd.arg("--world").arg("W");
        let launcher = std::env::current_exe().unwrap();
        assert_eq!(cmd.get_program(), launcher.as_os_str());
        let args: Vec<_> = cmd.get_args().collect();
        let expected: [&std::ffi::OsStr; 4] = [
            WORKER_ARG.as_ref(),
            game.as_os_str(),
            "--world".as_ref(),
            "W".as_ref(),
        ];
        assert_eq!(args, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The worker becomes the program it is given, with its arguments: the program's own exit
    /// code is the worker's.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn the_worker_becomes_the_program() {
        const CHILD: &str = "HORIZON_TEST_FIXED_ADDRESS_CHILD";
        if std::env::var_os(CHILD).is_some() {
            // The child: this very test in a process of its own, which becomes `sh`.
            let args = [OsString::from("-c"), OsString::from("exit 7")];
            std::process::exit(exec(std::ffi::OsStr::new("/bin/sh"), &args));
        }
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fixed_address::tests::the_worker_becomes_the_program",
            ])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(7));
    }
}
