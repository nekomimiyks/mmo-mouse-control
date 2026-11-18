//! g600ctl: control tool for the Logicool G600 / G600r.
//!
//! Every write first saves an automatic backup next to the executable.

use g600ctl::protocol::*;
use g600ctl::device::*;
use g600ctl::backup::*;
use g600ctl::operations;

use hidapi::{HidApi, HidDevice};

use std::io::{self, Write};


const USAGE: &str = "\
g600ctl - G600 / G600r 制御ツール

表示:
  g600gui                      GUIで設定の表示・編集・バックアップ
  g600ctl                      現在の設定を表示（ダブルクリック起動向け）
  g600ctl show [--raw]         現在の設定を表示（--raw で生データも）
  g600ctl list                 接続中のLogicool製HIDデバイスを一覧表示
  g600ctl backup [ファイル名]  設定をファイルに保存
  g600ctl diag descriptor      設定用HIDレポートディスクリプタを表示

書き込み（実行前に自動で backups フォルダへバックアップします）:
  g600ctl profile <1-3>                          使用するプロファイルを切り替え
  g600ctl set <1-3> dpi 400,1200,2000,3200 [--default 2]
                                                 DPI（最大4つ、200〜8200、0で無効）
  g600ctl set <1-3> dpi-shift 400                DPIシフト時のDPI
  g600ctl set <1-3> led 00FF00 [solid|breathe|cycle] [秒1-15]
  g600ctl set <1-3> led-gshift FFFFFF            Gシフト中のLED色
  g600ctl set <1-3> poll 1000                    125/142/166/200/250/333/500/1000
  g600ctl set <1-3> button <ボタン> <動作> [--gshift]
  g600ctl restore <バックアップファイル> [--profile N]
                                                 バックアップから復元（--profile で1つだけ）
  g600ctl diag write-test <バックアップファイル> プロファイル3の書き込み診断

ボタン: 1〜20、left / right / middle、G4〜G20
  （G4/G5=ホイール左右チルト、G6=薬指、G7/G8、G9〜G20=サイド12ボタン）
動作: left right middle back forward none
      dpi-up dpi-down dpi-cycle dpi-shift profile-cycle gshift
      キー: a, f5, num1, ctrl+c, shift+alt+tab など（キーは1つ＋修飾キー）
      raw:XX:YY:ZZ（3バイト直接指定）

オプション:
  --pid XXXX   製品IDを指定（既定: c24a）

※ G HUB が起動していると競合します。終了してから使ってください。";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let interactive = args.is_empty();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("エラー: {e}");
            1
        }
    };
    if interactive {
        print!("\nEnterキーで終了します...");
        let _ = io::stdout().flush();
        let _ = io::stdin().read_line(&mut String::new());
    }
    std::process::exit(code);
}

fn run(args: &[String]) -> Result<(), String> {
    g600ctl::storage::initialize()?;
    let mut pid = PRODUCT_G600;
    let mut raw = false;
    let mut gshift = false;
    let mut default_slot: Option<u8> = None;
    let mut only_profile: Option<u8> = None;
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--raw" => raw = true,
            "--gshift" => gshift = true,
            "--profile" => only_profile = Some(parse_profile(it.next().map(|s| s.as_str()))?),
            "--default" => {
                let v = it.next().ok_or("--default の後にスロット番号(1-4)を指定してください")?;
                default_slot = Some(v.parse().map_err(|_| format!("スロット番号が不正です: {v}"))?);
            }
            "--pid" => {
                let v = it.next().ok_or("--pid の後に製品IDを指定してください")?;
                pid = u16::from_str_radix(v.trim_start_matches("0x"), 16)
                    .map_err(|_| format!("製品IDが不正です: {v}"))?;
            }
            "-h" | "--help" | "help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => positional.push(a.as_str()),
        }
    }

    let api = HidApi::new().map_err(|e| format!("HIDの初期化に失敗: {e}"))?;
    match positional.first().copied().unwrap_or("show") {
        "list" => {
            list(&api);
            Ok(())
        }
        "show" => {
            let dev = open_g600(&api, pid)?;
            show(&dev, raw)
        }
        "backup" => {
            let dev = open_g600(&api, pid)?;
            let path = positional
                .get(1)
                .map(|s| s.to_string())
                .map(Ok).unwrap_or_else(|| g600ctl::storage::backup_dir().map(|p| new_path(&p, "manual").to_string_lossy().into_owned()))?;
            backup(&dev, &path)
        }
        "diag" => match positional.get(1).copied() {
            Some("descriptor") if positional.len() == 2 => {
                let dev = open_g600(&api, pid)?;
                descriptor(&dev)
            }
            Some("write-test") if positional.len() == 3 => {
                let path = positional[2];
                let mut buf = read_test_profile(path)?;
                buf[1..6].copy_from_slice(&[0x11, 0x22, 0x33, 0x01, 0x05]);
                let dev = open_g600(&api, pid)?;
                auto_backup(&dev)?;
                println!("送信データ: {}", hex(&buf));
                io::stdout().flush().map_err(|e| format!("表示に失敗: {e}"))?;
                set_report(&dev, &buf)?;
                let mut failed = false;
                for attempt in 1..=2 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    match get_report(&dev, REPORT_PROFILES[2], PROFILE_LEN) {
                        Ok(back) => {
                            println!("読み戻し{attempt}: {}", hex(&back));
                            let diff: Vec<String> = (0..PROFILE_LEN).filter(|&i| buf[i] != back[i])
                                .map(|i| format!("{i}:{:02X}->{:02X}", buf[i], back[i])).collect();
                            println!("差分（位置:送信->読戻）: {}", if diff.is_empty() { "なし".into() } else { diff.join(" ") });
                            let matches = insertion_positions(&buf, &back);
                            if matches.is_empty() {
                                println!("1バイト挿入のパターンに一致しません。");
                            } else {
                                for (pos, value) in matches {
                                    println!("挿入位置: {pos}、挿入値: {value:02X}");
                                }
                            }
                        }
                        Err(e) => { eprintln!("読み戻し{attempt}失敗: {e}"); failed = true; }
                    }
                }
                if failed { return Err("読み戻しに失敗しました".into()); }
                Ok(())
            }
            _ => Err("使い方: g600ctl diag descriptor | diag write-test <バックアップファイル>".into()),
        },
        "profile" => {
            let n = parse_profile(positional.get(1).copied())?;
            let dev = open_g600(&api, pid)?;
            auto_backup(&dev)?;
            select_profile(&dev, n)?;
            println!("プロファイル{}に切り替えました。", n + 1);
            Ok(())
        }
        "set" => {
            let n = parse_profile(positional.get(1).copied())?;
            let what = positional.get(2).copied().ok_or("設定項目を指定してください")?;
            let vals = &positional[3.min(positional.len())..];
            let dev = open_g600(&api, pid)?;
            let mut buf = get_report(&dev, REPORT_PROFILES[n as usize], PROFILE_LEN)?;
            buf.truncate(PROFILE_LEN);
            apply_set(&mut buf, what, vals, gshift, default_slot)?;
            operations::write_profiles(&dev, &[(n, buf)], &operations::backup_dir()?, |step, path| {
                if step.is_none() { println!("バックアップを保存しました: {}", path.display()); }
            }).map_err(|e| e.message)?;
            let active = Active::parse(&get_report(&dev, REPORT_ACTIVE, ACTIVE_LEN)?)?;
            let p = Profile::parse(&get_report(&dev, REPORT_PROFILES[n as usize], PROFILE_LEN)?)?;
            print_profile(n as usize, &p, active.profile, false);
            Ok(())
        }
        "restore" => {
            let path = positional.get(1).ok_or("バックアップファイルを指定してください")?;
            let (active, profiles) = read_backup(path)?;
            let dev = open_g600(&api, pid)?;
            let selected: Vec<_> = profiles.into_iter().enumerate()
                .filter(|(i, _)| only_profile.map_or(true, |n| n as usize == *i))
                .map(|(i, b)| (i as u8, b)).collect();
            operations::write_profiles(&dev, &selected, &operations::backup_dir()?, |step, path| {
                match step {
                    None => println!("バックアップを保存しました: {}", path.display()),
                    Some(i) => println!("プロファイル{}を復元しました（読み戻し一致）。", i + 1),
                }
            }).map_err(|e| e.message)?;
            let active_target = only_profile.is_none().then_some(active.profile);
            if let Some(profile) = active_target {
                select_profile(&dev, profile)?;
            }
            let actual = g600ctl::device::read_settings(&dev)?;
            operations::verify_readback(
                &actual, selected.iter().map(|(slot, data)| (*slot, data.as_slice())), active_target,
            )
        }
        other => Err(format!("不明なコマンド: {other}\n\n{USAGE}")),
    }
}

fn insertion_positions(sent: &[u8], read: &[u8]) -> Vec<(usize, u8)> {
    (1..=10)
        .filter(|&p| read[..p] == sent[..p] && read[p + 1..PROFILE_LEN] == sent[p..PROFILE_LEN - 1])
        .map(|p| (p, read[p]))
        .collect()
}

fn show(dev: &HidDevice, raw: bool) -> Result<(), String> {
    let active = Active::parse(&get_report(dev, REPORT_ACTIVE, ACTIVE_LEN)?)?;
    println!(
        "使用中: プロファイル{} / DPIスロット{}",
        active.profile + 1,
        active.dpi_slot + 1
    );
    for (i, &id) in REPORT_PROFILES.iter().enumerate() {
        let p = Profile::parse(&get_report(dev, id, PROFILE_LEN)?)?;
        print_profile(i, &p, active.profile, raw);
    }
    Ok(())
}

fn print_profile(i: usize, p: &Profile, active: u8, raw: bool) {
    let mark = if i as u8 == active { "（使用中）" } else { "" };
    println!("\n==== プロファイル{}{mark} ====", i + 1);
    println!(
        "LED: #{:02X}{:02X}{:02X}  効果: {}",
        p.led_rgb[0],
        p.led_rgb[1],
        p.led_rgb[2],
        match p.led_effect {
            LedEffect::Solid => "点灯".to_string(),
            LedEffect::Breathe => format!("呼吸（{}秒）", p.led_seconds),
            LedEffect::Cycle => format!("色循環（{}秒）", p.led_seconds),
            LedEffect::Unknown(x) => format!("不明(0x{x:02X})"),
        }
    );
    println!(
        "Gシフト時LED: #{:02X}{:02X}{:02X}",
        p.gshift_rgb[0], p.gshift_rgb[1], p.gshift_rgb[2]
    );
    println!("ポーリングレート: {} Hz", p.poll_hz);
    let dpis: Vec<String> = p
        .dpi
        .iter()
        .enumerate()
        .map(|(j, &d)| {
            let v = if d == 0 { "無効".to_string() } else { d.to_string() };
            if j as u8 + 1 == p.dpi_default { format!("[{v}]") } else { v }
        })
        .collect();
    println!("DPI: {}   （[ ]=既定）  DPIシフト: {}", dpis.join(" / "), p.dpi_shift);
    println!("ボタン割り当て（番号 ボタン / 通常時 / Gシフト時）:");
    for b in 0..BUTTONS {
        println!(
            "  #{:<2} {} / {} / {}",
            b + 1,
            button_label(b),
            p.buttons[b].describe(),
            p.gshift_buttons[b].describe()
        );
    }
    if raw {
        println!("生データ: {}", hex(&p.raw));
    }
}

fn parse_profile(s: Option<&str>) -> Result<u8, String> {
    let s = s.ok_or("プロファイル番号(1-3)を指定してください")?;
    match s.parse::<u8>() {
        Ok(n @ 1..=3) => Ok(n - 1),
        _ => Err(format!("プロファイル番号は1〜3です: {s}")),
    }
}

fn apply_set(
    buf: &mut [u8],
    what: &str,
    vals: &[&str],
    gshift: bool,
    default_slot: Option<u8>,
) -> Result<(), String> {
    let mut e = ProfileEdit(buf);
    let first = || vals.first().copied().ok_or_else(|| format!("{what} の値を指定してください"));
    match what {
        "dpi" => {
            // PowerShell passes `400,800` to native programs as `400 800`,
            // so accept commas and separate arguments alike.
            first()?;
            let slots: Vec<u32> = vals
                .join(",")
                .split(',')
                .filter(|x| !x.trim().is_empty())
                .map(|x| x.trim().parse().map_err(|_| format!("DPIが不正です: {x}")))
                .collect::<Result<_, _>>()?;
            e.set_dpi(&slots, default_slot)
        }
        "dpi-shift" => e.set_dpi_shift(first()?.parse().map_err(|_| "DPIが不正です".to_string())?),
        "led" => {
            let rgb = parse_rgb(first()?)?;
            let effect = match vals.get(1).copied().unwrap_or("solid") {
                "solid" => LedEffect::Solid,
                "breathe" => LedEffect::Breathe,
                "cycle" => LedEffect::Cycle,
                x => return Err(format!("効果は solid / breathe / cycle です: {x}")),
            };
            let secs = match vals.get(2) {
                Some(v) => v.parse().map_err(|_| format!("秒数が不正です: {v}"))?,
                None => 4,
            };
            e.set_led(rgb, effect, secs)
        }
        "led-gshift" => {
            e.set_gshift_led(parse_rgb(first()?)?);
            Ok(())
        }
        "poll" => e.set_poll(first()?.parse().map_err(|_| "数値を指定してください".to_string())?),
        "button" => {
            let slot = parse_button(first()?)?;
            let action = Action::from_str(vals.get(1).ok_or("動作を指定してください")?)?;
            e.set_button(slot, gshift, action);
            Ok(())
        }
        x => Err(format!("不明な設定項目です: {x}（dpi / dpi-shift / led / led-gshift / poll / button）")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_inserted_byte_and_ambiguous_positions() {
        let sent: Vec<u8> = (0..PROFILE_LEN as u8).collect();
        let mut read = sent.clone();
        read.insert(4, 0xEE);
        read.truncate(PROFILE_LEN);
        assert_eq!(insertion_positions(&sent, &read), vec![(4, 0xEE)]);
        read[100] ^= 1;
        assert!(insertion_positions(&sent, &read).is_empty());

        let sent = vec![0; PROFILE_LEN];
        assert_eq!(insertion_positions(&sent, &sent), (1..=10).map(|p| (p, 0)).collect::<Vec<_>>());
    }
}
