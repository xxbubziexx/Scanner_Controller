//! Build, CI verification, cross-compilation, and release packaging helper tasks (`xtask`).

#![deny(unsafe_code)]
#![allow(clippy::print_stdout)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "xtask",
    about = "Build, verification, cross-compile, and packaging tasks for Scanner_Controller"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<TaskCommand>,
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    /// Run full workspace quality gate (fmt check, clippy -D warnings, and test suite).
    Ci,

    /// Build a release `scanner-controller` binary (optionally for a specific target triple).
    BuildRelease {
        /// Target triple (e.g., `x86_64-pc-windows-gnu`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`).
        #[arg(long)]
        target: Option<String>,
    },

    /// Package the release binary and deployment assets into `dist/`.
    Package {
        /// Optional target triple.
        #[arg(long)]
        target: Option<String>,
    },
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn run_cargo(args: &[&str], cwd: &Path) -> Result<()> {
    println!("> cargo {}", args.join(" "));
    let status = Command::new("cargo")
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("Failed to execute cargo {}", args.join(" ")))?;
    if !status.success() {
        bail!(
            "Command `cargo {}` failed with status {status}",
            args.join(" ")
        );
    }
    Ok(())
}

fn run_ci(ws_root: &Path) -> Result<()> {
    run_cargo(&["fmt", "--all", "--", "--check"], ws_root)?;
    run_cargo(
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        ws_root,
    )?;
    run_cargo(&["test", "--workspace"], ws_root)?;
    println!("CI quality gate passed.");
    Ok(())
}

fn run_build_release(ws_root: &Path, target: Option<&str>) -> Result<PathBuf> {
    let mut args = vec!["build", "--release", "--bin", "scanner-controller"];
    if let Some(t) = target {
        args.push("--target");
        args.push(t);
    }
    run_cargo(&args, ws_root)?;

    let exe_name = if target.map_or(cfg!(windows), |t| t.contains("windows")) {
        "scanner-controller.exe"
    } else {
        "scanner-controller"
    };

    let bin_path = if let Some(t) = target {
        ws_root
            .join("target")
            .join(t)
            .join("release")
            .join(exe_name)
    } else {
        ws_root.join("target").join("release").join(exe_name)
    };

    println!("Release binary built at: {}", bin_path.display());
    Ok(bin_path)
}

fn run_package(ws_root: &Path, target: Option<&str>) -> Result<()> {
    let bin_path = run_build_release(ws_root, target)?;
    let repo_root = ws_root.parent().unwrap_or(ws_root);
    let dist_dir = repo_root.join("dist");
    fs::create_dir_all(&dist_dir).context("Failed to create dist/ directory")?;

    if let Some(file_name) = bin_path.file_name() {
        let dest = dist_dir.join(file_name);
        fs::copy(&bin_path, &dest).with_context(|| {
            format!(
                "Failed to copy {} to {}",
                bin_path.display(),
                dest.display()
            )
        })?;
        println!("Packaged binary -> {}", dest.display());
    }

    for asset in ["scanscribe-scanner.service", "install.sh"] {
        let src = repo_root.join(asset);
        if src.exists() {
            let dst = dist_dir.join(asset);
            let _ = fs::copy(&src, &dst);
        }
    }

    println!("Packaging complete in {}", dist_dir.display());
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ws_root = workspace_root();

    match cli.command {
        None => {
            println!("xtask helper runner ready. Use `ci`, `build-release`, or `package`.");
            Ok(())
        }
        Some(TaskCommand::Ci) => run_ci(&ws_root),
        Some(TaskCommand::BuildRelease { target }) => {
            let _ = run_build_release(&ws_root, target.as_deref())?;
            Ok(())
        }
        Some(TaskCommand::Package { target }) => run_package(&ws_root, target.as_deref()),
    }
}
