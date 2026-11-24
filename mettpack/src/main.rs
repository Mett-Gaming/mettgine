//! Prueft, baut und packt ein Spiel. Aufruf im Ordner des Spiels:
//! `cargo run --release -p mettpack -- pruefen | bauen | packen [--version v1.2.3]`

mod check;
mod config;
mod pack;
mod zip;

use config::Config;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const HELP: &str = "Aufruf: mettpack pruefen | bauen | packen [--version v1.2.3] [--ordner <Spiel>]
  pruefen  spiel.toml, Daten, Lizenzen und Erfolge prüfen
  bauen    prüfen und das Spiel bauen
  packen   prüfen, bauen und dist/ mit Ordner, Zip und Dateiliste füllen";

struct Args {
    action: String,
    version: Option<String>,
    root: PathBuf,
}

fn args() -> Option<Args> {
    let mut list = std::env::args().skip(1);
    let mut out = Args {
        action: list.next()?,
        version: None,
        root: PathBuf::from("."),
    };
    while let Some(a) = list.next() {
        match a.as_str() {
            "--version" => out.version = Some(list.next()?),
            "--ordner" => out.root = list.next()?.into(),
            _ => return None,
        }
    }
    Some(out)
}

fn run(a: &Args) -> Result<(), String> {
    let c = Config::load(&a.root)?;
    let problems = check::run(&a.root, &c);
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    println!("{}: alles in Ordnung", c.name);
    if a.action == "pruefen" {
        return Ok(());
    }
    let version = version(a)?;
    build(a, &c, &version)?;
    if a.action == "packen" {
        pack::run(&a.root, &c, &version)?;
    }
    Ok(())
}

/// Version: angegeben, sonst die Marke in der CI, sonst `git describe`.
fn version(a: &Args) -> Result<String, String> {
    let from_ci = || {
        let name = std::env::var("GITHUB_REF_NAME").ok()?;
        Some(name.replace(
            |c: char| !c.is_ascii_alphanumeric() && !".-_".contains(c),
            "-",
        ))
    };
    if let Some(v) = a.version.clone().or_else(from_ci) {
        return Ok(v);
    }
    let out = Command::new("git")
        .args(["describe", "--tags", "--always"])
        .current_dir(&a.root)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty())
        .then_some(v)
        .ok_or_else(|| "Version unbekannt, bitte --version angeben".into())
}

fn build(a: &Args, c: &Config, version: &str) -> Result<(), String> {
    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.args(["build", "--release", "-p", &c.package])
        .current_dir(&a.root);
    // Version fuer Absturzberichte und Anzeige (game_build.rs)
    if version.starts_with('v') {
        cmd.env("GITHUB_REF_NAME", version);
    }
    let status = cmd.status().map_err(|e| format!("cargo: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "Bauen fehlgeschlagen".into())
}

fn main() -> ExitCode {
    let Some(a) = args().filter(|a| ["pruefen", "bauen", "packen"].contains(&a.action.as_str()))
    else {
        eprintln!("{HELP}");
        return ExitCode::from(2);
    };
    match run(&a) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("FEHLER:\n{e}");
            ExitCode::FAILURE
        }
    }
}
