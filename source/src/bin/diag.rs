use m908ctl::{device, settings::Snapshot};
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("list");
    if command == "list" {
        let devices = device::enumerate()?;
        if devices.is_empty() {
            println!("M908 (04D9:FC4D) が見つかりません。USBアクセス権も確認してください。");
        }
        for d in devices {
            println!("{}\n  {}", d.label(), d.path.to_string_lossy());
        }
        return Ok(());
    }
    if command == "help" {
        println!("m908diag list | show | backup [保存先] | write バックアップ プロファイル番号 | profile 番号 | verify バックアップ | self-test\nself-testは未使用プロファイルを一時変更し、元へ戻す実機診断です。");
        return Ok(());
    }
    if command == "input-watch" {
        let seconds = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(90);
        return m908ctl::input::watch(seconds, |s| println!("{s}"));
    }
    if command == "vendor-watch" {
        let seconds = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(90);
        let stop = args.get(2).map(std::path::Path::new);
        return device::watch_input_until(seconds, stop, |s| println!("{s}"));
    }
    if command == "recover-identification" {
        let path = args.get(1).ok_or("復元記録を指定してください")?;
        println!(
            "復元実施: {}",
            m908ctl::identification::recover(std::path::Path::new(path))?
        );
        return Ok(());
    }
    let d = device::find()?;
    let dir = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("実行フォルダがありません")?
        .join("backups");
    match command {
        "multi-key-save-self-test" => println!("{}", device::multi_key_save_self_test(&d, &dir)?),
        "read-packing-benchmark" => println!("{}", device::benchmark_read_packing(&d)?),
        "identification-latency-benchmark" => {
            let original = device::read(&d)?;
            device::backup(&dir, &original, "before-latency-test")?;
            for trial in 1..=3 {
                let keep = std::cell::Cell::new(true);
                let start = std::time::Instant::now();
                let mut restoring = start;
                m908ctl::identification::run_session(
                    &d,
                    &dir.join(m908ctl::identification::JOURNAL),
                    || keep.get(),
                    |line| {
                        if line == "READY" {
                            println!("試行{trial} 準備: {:.4}秒", start.elapsed().as_secs_f64());
                            keep.set(false);
                        }
                        if line == "RESTORING" {
                            restoring = std::time::Instant::now();
                        }
                        if line.starts_with("SNAPSHOT ") {
                            println!(
                                "試行{trial} 復元: {:.4}秒",
                                restoring.elapsed().as_secs_f64()
                            );
                        }
                    },
                )?;
                if !original.same_settings(&device::read(&d)?, false) {
                    return Err("速度測定後の全設定・マクロ・使用中番号が一致しません".into());
                }
                println!("試行{trial}: 全プロファイル・マクロ・使用中番号の復元一致");
            }
        }
        "transport-benchmark" => println!("{}", device::benchmark_transport(&d)?),
        "identification-transport-benchmark" => {
            println!("{}", device::benchmark_identification_transport(&d, &dir)?)
        }
        "read-benchmark" => println!("{}", device::benchmark_reads(&d)?),
        "fast-save-self-test" => println!("{}", device::fast_save_self_test(&d, &dir)?),
        "probe" | "show" => println!("{}", device::summary(&device::read(&d)?)?),
        "backup" => {
            let s = device::read(&d)?;
            let p = if let Some(path) = args.get(1) {
                let path = std::path::PathBuf::from(path);
                device::save_raw(&path, &s.to_text())?;
                path
            } else {
                device::backup(&dir, &s, "manual")?
            };
            println!("保存しました: {}\n{}", p.display(), device::summary(&s)?);
        }
        "write" => {
            let path = args
                .get(1)
                .ok_or("バックアップファイルを指定してください")?;
            let profile = args
                .get(2)
                .ok_or("プロファイル番号を指定してください")?
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=5).contains(n))
                .ok_or("プロファイルは1〜5です")?
                - 1;
            let target =
                Snapshot::from_text(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?;
            let base = device::read(&d)?;
            let a = device::apply(&d, &base, profile, &target.profiles()?[profile], &dir)?;
            println!(
                "書き込み・読み戻し一致: {:?}\nバックアップ: {:?}",
                a.changes, a.backup
            );
        }
        "profile" => {
            let profile = args
                .get(1)
                .ok_or("プロファイル番号を指定してください")?
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=5).contains(n))
                .ok_or("プロファイルは1〜5です")?
                - 1;
            let base = device::read(&d)?;
            let a = device::switch(&d, &base, profile, &dir)?;
            println!(
                "使用中: プロファイル{} / 読み戻し一致",
                a.snapshot.active() + 1
            );
        }
        "verify" => {
            let path = args
                .get(1)
                .ok_or("バックアップファイルを指定してください")?;
            let expected =
                Snapshot::from_text(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?;
            let current = device::read(&d)?;
            if !(if expected.has_macros {
                expected.same_settings(&current, false)
            } else {
                expected.same_controls(&current, false)
            }) {
                return Err("実機とバックアップが一致しません".into());
            }
            println!(
                "{}・使用中プロファイルが一致しました。",
                if expected.has_macros {
                    "全プロファイル・マクロ領域"
                } else {
                    "全プロファイルの通常設定（マクロ領域は比較対象外）"
                }
            );
        }
        "self-test" => println!("{}", device::self_test(&d, &dir, |s| println!("{s}"))?),
        "identification-self-test" => {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let before = device::read(&d)?;
            let backup = device::backup(&dir, &before, "before-identification-test")?;
            println!("診断前: {}", backup.display());
            let keep = std::cell::Cell::new(true);
            m908ctl::identification::run_session(
                &d,
                &dir.join(m908ctl::identification::JOURNAL),
                || keep.get(),
                |s| {
                    println!("{s}");
                    if s == "READY" {
                        keep.set(false);
                    }
                },
            )?;
            let after = device::read(&d)?;
            if !before.same_settings(&after, false) {
                return Err("識別診断後の全体設定が一致しません".into());
            }
            println!("識別用設定・切替・復元と、全プロファイル・マクロ領域・使用中プロファイルの一致を確認しました。");
        }
        _ => return Err("不明なコマンドです。helpを実行してください。".into()),
    }
    Ok(())
}
fn main() {
    device::initialize_hid();
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
