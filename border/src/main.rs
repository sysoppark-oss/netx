use border::{check, report};
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "국경경비대 출국 심사

쓰는 법:
  border exit [--repo 폴더] [--base 기준점] [--head 심사할 자리] [--visa 파일] [--out 파일]

  --base   기준점. 기본은 main. 비자는 기준점의 .border/visa.toml 에서 읽는다.
  --head   심사할 자리. 기본은 HEAD.
  --visa   기준점에 비자가 없을 때 시험 삼아 쓸 로컬 비자 파일. 권위가 없다.
  --out    보고서를 이 파일에도 쓴다.

끝날 때의 값:
  0 통과 · 1 사람 확인 필요 · 2 비자 밖 · 3 검사하지 못함";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("exit") => {}
        Some("--help") | Some("-h") | None => {
            println!("{HELP}");
            return ExitCode::from(if std::env::args().count() > 1 { 0 } else { 3 });
        }
        Some(_) => {
            eprintln!("{HELP}");
            return ExitCode::from(3);
        }
    }
    let mut repo = PathBuf::from(".");
    let mut base = "main".to_string();
    let mut head = "HEAD".to_string();
    let mut visa: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    while let Some(flag) = args.next() {
        let Some(val) = args.next() else {
            eprintln!("값이 빠졌습니다: {}", report::clean(&flag));
            return ExitCode::from(3);
        };
        match flag.as_str() {
            "--repo" => repo = PathBuf::from(val),
            "--base" => base = val,
            "--head" => head = val,
            "--visa" => visa = Some(PathBuf::from(val)),
            "--out" => out = Some(PathBuf::from(val)),
            _ => {
                eprintln!("모르는 옵션입니다: {}", report::clean(&flag));
                return ExitCode::from(3);
            }
        }
    }
    let req = check::Request { repo: &repo, base: &base, head: &head, visa_file: visa.as_deref() };
    match check::run(&req) {
        Ok(o) => {
            let text = report::render(&o);
            print!("{text}");
            if let Some(p) = out {
                if std::fs::write(&p, &text).is_err() {
                    eprintln!("보고서를 파일에 쓰지 못했습니다");
                    return ExitCode::from(3);
                }
            }
            ExitCode::from(o.code() as u8)
        }
        Err(e) => {
            println!("# 출국 심사\n\n판정: **{}**\n\n까닭: {}", report::verdict(3), report::clean(&e));
            ExitCode::from(3)
        }
    }
}
