# 参照元とライセンス

## G600r制御ソフト

同じワークスペースの `G600r制御ソフト`（MIT）から、G600rの設定通信、GUI、入力表示、バックアップと診断CLIを `source/g600` に取り込みました。MITライセンスの原文は `source/g600/LICENSE` にあります。

## dokutan/mouse_m908

- URL: https://github.com/dokutan/mouse_m908
- 取得commit: `965d71f957023ebdfe9bc3ea8d2affbfb21c10ff`
- ライセンス: GPL-3.0-or-later
- 使用箇所: `source/src/read_tables.rs` の読み取り要求、`write_tables.rs` の書き込み・DPI・キーコード表、`settings.rs`／`actions.rs`／`device.rs` の設定変換と通信手順。元ファイルは `include/m908/data.cpp`、`include/data.cpp`、`include/m908/readers.cpp`、`include/m908/writers.cpp`、`include/rd_mouse.cpp`。
- Windowsでの短い応答、応答ヘッダー、LED速度の位置、設定用HIDコレクションの選択は実機で検証して補正しました。書き込みは初期値の全送信ではなく、実機から読み取った未編集値を保持する差分更新です。
- ローカル参照資料: `_reference/mouse_m908`。本アプリのビルドに不要です。
- この新規プロジェクトはGPL-3.0-or-laterとし、全文を `LICENSE` に収録しています。外部へ実行ファイルを配布する際は、対応するソースとライセンスも提供してください。

Rust依存ライブラリはそれぞれのライセンスに従います。バージョンは `source/Cargo.lock` に固定されています。日本語フォントは実行時にWindowsのインストール済みフォントを読み込み、フォントファイル自体は配布しません。
