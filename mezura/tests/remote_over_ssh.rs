use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn a_remote_over_ssh_runs_the_ssh_command_of_the_persons_own_git_configuration() {
    let dir = std::env::temp_dir().join("mezura-own-ssh-command");
    let _ = std::fs::remove_dir_all(&dir);
    let origin = dir.join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    std::fs::write(origin.join("main.rs"), "fn main() {}\n").unwrap();
    let own_config = dir.join("gitconfig");
    std::fs::write(&own_config, "").unwrap();
    run_git(&own_config, &origin, &["init", "-q"]);
    run_git(&own_config, &origin, &["add", "main.rs"]);
    run_git(&own_config, &origin, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "c1"]);
    let helper = write_local_ssh_helper(&dir);
    run_git(&own_config, &dir, &["config", "--file", &spell_with_forward_slashes(&own_config), "core.sshCommand",
            &format!("'{}'", spell_with_forward_slashes(&helper))]);

    let counted = Command::new(env!("CARGO_BIN_EXE_mezura")).arg(format!("ssh://local/{}", spell_with_forward_slashes(&origin)))
            .env("MEZURA_DATA_DIR", dir.join("data")).env("GIT_CONFIG_GLOBAL", &own_config).env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_SSH_COMMAND").env_remove("GIT_SSH").output().unwrap();

    assert!(counted.status.success(), "{}", String::from_utf8_lossy(&counted.stderr));
    assert!(String::from_utf8_lossy(&counted.stdout).contains("Rust"));
    std::fs::remove_dir_all(&dir).unwrap();
}

fn run_git(own_config: &Path, at: &Path, arguments: &[&str]) {
    let outcome = Command::new("git").env("GIT_CONFIG_GLOBAL", own_config).env("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-C").arg(at).args(arguments).output().unwrap();
    assert!(outcome.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&outcome.stderr));
}

fn write_local_ssh_helper(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        let path = dir.join("ssh-helper.bat");
        std::fs::write(&path,
                "@echo off\r\n\
                 if \"%1\"==\"-G\" exit /b 1\r\n\
                 for /f \"tokens=2 delims='\" %%p in (%2) do set \"p=%%p\"\r\n\
                 if \"%p:~0,1%\"==\"/\" if \"%p:~2,1%\"==\":\" set \"p=%p:~1%\"\r\n\
                 git upload-pack \"%p%\"\r\n").unwrap();
        path
    } else {
        let path = dir.join("ssh-helper.sh");
        std::fs::write(&path, "#!/bin/sh\n[ \"$1\" = \"-G\" ] && exit 1\neval \"exec $2\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
}

fn spell_with_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
