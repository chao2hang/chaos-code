//! Facts about Windows path parsing, printed on Windows.
//!
//! Run it twice, from the directory holding this file:
//!
//! ```text
//! rustc -O model-path-windows-probe.rs -o probe && ./probe
//! docker build -t chaos-winprobe:local -f model-path-windows-probe.Dockerfile .
//! docker run --rm -v "$PWD":/work -e WINEPREFIX=/tmp/wp chaos-winprobe:local bash -c '
//!     x86_64-w64-mingw32-gcc -shared -o /tmp/bcryptprimitives.dll \
//!         /work/model-path-windows-probe-shim.c -lbcrypt
//!     rustc --target x86_64-pc-windows-gnu -C linker=x86_64-w64-mingw32-gcc \
//!         -O /work/model-path-windows-probe.rs -o /tmp/probe.exe
//!     cd /tmp && /usr/lib/wine/wine64 /tmp/probe.exe'
//! ```
//!
//! The DLL built from the shim is what lets a Rust binary load at all under
//! wine 8.0; see `docs/verification/model-path-drive-less-root-2026-10-03.log`
//! section 8. It is printed to on every call, and the recorded run shows no
//! such line.
//!
//! Every claim in the `resolve_model_path` decision is a claim about how
//! Windows reads a path, so every claim is checked here rather than argued
//! from documentation. Compile with the `x86_64-pc-windows-gnu` target and run
//! under wine; the same file compiled for Linux prints the same table with the
//! other platform's answers, which is the part worth seeing side by side.
//!
//! Nothing here calls into the repository. The two loops marked SHIPPED are
//! transcriptions of `crate::util::fs::join_relative` and of the component
//! match in `recover_dropped_root`, printed so the shipped code's Windows
//! behaviour can be read off this table; the shipped functions themselves are
//! asserted by the `#[cfg(windows)]` tests next to them.

use std::path::{Component, Path, PathBuf};

/// SHIPPED: `crate::util::fs::join_relative`, transcribed.
fn join_relative(base: &Path, relative: &Path) -> PathBuf {
    let mut joined = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => continue,
            other => joined.push(other.as_os_str()),
        }
    }
    joined
}

/// SHIPPED: the component match in `recover_dropped_root`, transcribed.
fn recover_dropped_root(body: &Path, base: &Path) -> Option<PathBuf> {
    let below_base: Vec<_> = base
        .components()
        .filter(|c| !matches!(c, Component::Prefix(_) | Component::RootDir))
        .collect();
    let mut rest = body.components();
    for expected in &below_base {
        match rest.next() {
            Some(found) if found == *expected => {}
            _ => return None,
        }
    }
    let mut suffix = PathBuf::new();
    for component in rest {
        suffix.push(component.as_os_str());
    }
    Some(suffix)
}

fn components_of(path: &Path) -> String {
    let names: Vec<String> = path
        .components()
        .map(|component| match component {
            Component::Prefix(prefix) => format!("Prefix({:?})", prefix.kind()),
            Component::RootDir => "RootDir".to_string(),
            Component::CurDir => "CurDir".to_string(),
            Component::ParentDir => "ParentDir".to_string(),
            Component::Normal(name) => format!("Normal({})", name.to_string_lossy()),
        })
        .collect();
    names.join(" ")
}

fn main() {
    println!("platform: {}", std::env::consts::OS);
    println!();

    println!("== 1. is_absolute, and what the components look like ==");
    let shapes = [
        r"C:\work\proj",
        r"\src\main.rs",
        "/src/main.rs",
        r"\\fileserver\share\plan.md",
        r"C:work\plan.md",
        r"src\main.rs",
        r"C:\",
        r"\",
        "//fileserver/share/plan.md",
        "~/x",
    ];
    println!("{:<34} {:<12} {}", "input", "absolute", "components");
    for shape in shapes {
        let path = Path::new(shape);
        println!(
            "{:<34} {:<12} {}",
            shape,
            path.is_absolute(),
            components_of(path),
        );
    }
    println!();

    println!("== 2. the tail the old code fell through to: cwd.join(input) ==");
    let cwd = Path::new(r"D:\worktree\abc");
    println!("{:<34} {}", "input", "cwd.join(input)");
    for shape in [
        r"\src\main.rs",
        "/src/main.rs",
        r"\home\user\project\src\main.rs",
        r"home\user\project\src\main.rs",
        r"C:work\plan.md",
        r"src\main.rs",
        r"\\fileserver\share\plan.md",
        r"C:\Users\me\.ssh\config",
    ] {
        println!(
            "{:<34} {}",
            shape,
            cwd.join(shape).to_string_lossy()
        );
    }
    println!();

    println!("== 3. joining component by component instead (join_relative) ==");
    println!("{:<34} {}", "relative body", "join_relative(cwd, body)");
    for shape in [r"src\main.rs", "src/main.rs", r"C:work\plan.md", r"\src\main.rs"] {
        println!(
            "{:<34} {}",
            shape,
            join_relative(cwd, Path::new(shape)).to_string_lossy()
        );
    }
    println!();

    println!("== 4. the recovery branch, old candidate vs component match ==");
    let display = Path::new(r"C:\home\user\project");
    println!(
        "{:<40} {:<10} {}",
        "body (drive/separator dropped)", "old", "component match"
    );
    for shape in [
        r"home\user\project\src\main.rs",
        "/home/user/project/src/main.rs",
        r"home\user\projectX\main.rs",
        r"other\main.rs",
    ] {
        // The old branch: rebuild an absolute-looking candidate, then prefix it.
        let as_absolute = PathBuf::from(format!("/{}", shape));
        let old = as_absolute.starts_with(display);
        let now = recover_dropped_root(Path::new(shape), display);
        println!(
            "{:<40} {:<10} {}",
            shape,
            old,
            match now {
                Some(suffix) => format!("Some({})", suffix.to_string_lossy()),
                None => "None".to_string(),
            },
        );
    }
    println!();

    println!("== 5. the rooted form, after its leading separator is dropped ==");
    println!("{:<44} {}", "rooted input", "recover_dropped_root(body, display)");
    for shape in [
        r"\home\user\project\src\main.rs",
        "/home/user/project/src/main.rs",
        r"\other\main.rs",
    ] {
        let path = Path::new(shape);
        let mut rest = path.components();
        let body = match rest.next() {
            Some(Component::RootDir) => rest.as_path(),
            _ => Path::new(""),
        };
        let suffix = recover_dropped_root(body, display);
        println!(
            "{:<44} {}",
            shape,
            match suffix {
                Some(suffix) => format!("Some({})", suffix.to_string_lossy()),
                None => "None".to_string(),
            },
        );
    }
    println!();

    println!("== 6. what the fold onto cwd produces for each of those ==");
    for shape in [
        r"\home\user\project\src\main.rs",
        "/home/user/project/src/main.rs",
        r"\other\main.rs",
        r"\src\main.rs",
    ] {
        let path = Path::new(shape);
        let mut rest = path.components();
        let body = match rest.next() {
            Some(Component::RootDir) => rest.as_path(),
            _ => Path::new(""),
        };
        let suffix = recover_dropped_root(body, display).unwrap_or_else(|| body.to_path_buf());
        println!(
            "{:<44} {}",
            shape,
            join_relative(cwd, &suffix).to_string_lossy()
        );
    }
}
