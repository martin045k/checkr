use std::{path::Path, process::Stdio};

use color_eyre::eyre::{bail, Context};
use tokio::process::Command;

// TODO: use a better path for this
const GIT_SSH_COMMAND: &str =
    "ssh -o ControlPath=~/.ssh/cm_socket/%r@%h:%p -o ControlMaster=auto -o ControlPersist=60";

pub async fn clone_or_pull(git: &str, path: impl AsRef<Path>) -> color_eyre::Result<()> {
    let path = path.as_ref();
    if !path.join(".git").try_exists().unwrap_or(false) {
        clone(git, path).await
    } else {
        pull(git, path).await
    }
}

pub async fn clone(git: &str, path: impl AsRef<Path>) -> color_eyre::Result<()> {
    tracing::info!(?git, "cloning group git repository");
    let status = Command::new("git")
        .arg("clone")
        .arg(git)
        .args(["."])
        .env("GIT_SSH_COMMAND", GIT_SSH_COMMAND)
        .current_dir(path)
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .status()
        .await
        .wrap_err_with(|| format!("could not clone group git repository: '{git}'"))?;
    tracing::debug!(code=?status.code(), "git clone status");
    if !status.success() {
        bail!("git clone failed");
    }
    Ok(())
}

pub async fn pull(git: &str, path: impl AsRef<Path>) -> color_eyre::Result<()> {
    tracing::info!(?git, "pulling group git repository");
    let status = Command::new("git")
        .arg("pull")
        .env("GIT_SSH_COMMAND", GIT_SSH_COMMAND)
        .current_dir(&path)
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .status()
        .await
        .wrap_err_with(|| format!("could not pull group git repository: '{git}'"))?;
    tracing::debug!(code=?status.code(), "git pull status");
    if !status.success() {
        bail!("git pull failed");
    }
    Ok(())
}

pub async fn hash(path: impl AsRef<Path>) -> color_eyre::Result<String> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(path)
        .stderr(Stdio::inherit())
        .stdout(Stdio::piped())
        .output()
        .await
        .wrap_err("could not get git hash")?;
    if !output.status.success() {
        bail!("git rev-parse HEAD failed");
    }
    let hash = String::from_utf8(output.stdout).wrap_err("git hash is not valid utf8")?;
    Ok(hash.trim().to_string())
}

pub async fn checkout_latest_before(
    path: impl AsRef<Path>,
    before: chrono::NaiveDateTime,
) -> color_eyre::Result<()> {
    tracing::info!(?before, "checking out latest commit before");
    let before = before.format("%Y-%m-%d %H:%M:%S").to_string();
    let result = Command::new("git")
        .args(["rev-list", "-n", "1"])
        .arg(format!("--before={before}"))
        .arg("HEAD")
        .current_dir(&path)
        .output()
        .await
        .wrap_err_with(|| format!("could not get latest commit before {before}"))?;
    if !result.status.success() {
        bail!("git rev-list failed");
    }
    let commit_rev_bytes = result.stdout;
    let commit_rev = std::str::from_utf8(&commit_rev_bytes).unwrap();
    let result = Command::new("git")
        .arg("checkout")
        .arg(commit_rev)
        .current_dir(&path)
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()
        .await
        .wrap_err_with(|| format!("could not checkout latest commit: {commit_rev}"))?;
    if !result.status.success() {
        bail!("git checkout failed");
    }
    Ok(())
}
