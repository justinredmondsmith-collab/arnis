use std::{env, path::PathBuf, process::Command};

fn main() {
    emit_build_hash();
    emit_tiler_identity();

    #[cfg(feature = "gui")]
    tauri_build::build()
}

/// Exposes the commit the binary was built from as `ARNIS_BUILD_HASH`, so a
/// custom build is distinguishable from an official release. Falls back to
/// "unknown" when git is unavailable, e.g. building from a source tarball.
fn emit_build_hash() {
    let hash = match git(&["rev-parse", "--short=7", "HEAD"]) {
        Some(hash) => {
            // Untracked files are ignored so stray local files don't mark an
            // otherwise pristine checkout as modified.
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|status| !status.is_empty());
            if dirty {
                format!("{hash}-dirty")
            } else {
                hash
            }
        }
        None => "unknown".to_string(),
    };

    println!("cargo:rustc-env=ARNIS_BUILD_HASH={hash}");

    // Rebuild when the checked-out commit or the staged tree changes.
    for path in [".git/HEAD", ".git/index"] {
        if std::path::Path::new(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let result = Command::new(program).args(args).output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
}
fn emit_tiler_identity() {
    for path in [
        "src",
        "assets",
        "Cargo.toml",
        "Cargo.lock",
        "src/build.rs",
        "docs/contracts",
        "tauri.conf.json",
        "capabilities",
        "scripts",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for name in ["ARNIS_BUILD_COMMIT", "ARNIS_BUILD_DIRTY"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    // An archive nested inside an unrelated worktree must not borrow its identity.
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest dir"))
            .canonicalize()
            .expect("manifest directory exists");
    let own_git = output("git", &["rev-parse", "--show-toplevel"])
        .and_then(|root| PathBuf::from(root).canonicalize().ok())
        .is_some_and(|root| root == manifest_dir);
    for path in [
        ".cargo",
        "rust-toolchain",
        "rust-toolchain.toml",
        "rustfmt.toml",
        ".rustfmt.toml",
    ] {
        if PathBuf::from(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let commit = if own_git {
        output("git", &["rev-parse", "HEAD"]).expect("cannot read repository HEAD")
    } else {
        env::var("ARNIS_BUILD_COMMIT").expect("source archive builds require ARNIS_BUILD_COMMIT")
    };
    assert!(
        commit.len() == 40 && commit.bytes().all(|c| c.is_ascii_hexdigit()),
        "invalid source commit"
    );
    let dirty = if own_git {
        !output(
            "git",
            &["status", "--porcelain", "--untracked-files=normal"],
        )
        .expect("cannot read repository status")
        .is_empty()
    } else {
        match env::var("ARNIS_BUILD_DIRTY").as_deref() {
            Ok("true") => true,
            Ok("false") => false,
            _ => panic!("source archive builds require ARNIS_BUILD_DIRTY=true or false"),
        }
    };
    if own_git {
        // Worktrees use a .git indirection file and a separate common refs directory.
        for spec in ["HEAD", "index", "packed-refs"] {
            if let Some(path) = output("git", &["rev-parse", "--git-path", spec]) {
                println!("cargo:rerun-if-changed={path}");
            }
        }
        if let Some(reference) = output("git", &["symbolic-ref", "-q", "HEAD"]) {
            if let Some(path) = output("git", &["rev-parse", "--git-path", &reference]) {
                println!("cargo:rerun-if-changed={path}");
            }
        }
        if PathBuf::from(".git").exists() {
            println!("cargo:rerun-if-changed=.git");
        }
    }
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = output(&rustc, &["--version"]).expect("cannot determine Rust compiler identity");
    println!("cargo:rustc-env=ARNIS_SOURCE_COMMIT={commit}");
    println!("cargo:rustc-env=ARNIS_SOURCE_DIRTY={dirty}");
    println!(
        "cargo:rustc-env=ARNIS_BUILD_TARGET={}",
        env::var("TARGET").expect("Cargo TARGET")
    );
    println!("cargo:rustc-env=ARNIS_BUILD_RUSTC={version}");
}
