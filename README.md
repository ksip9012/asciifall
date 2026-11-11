# asciifall

```
  3   .   K   .   Z   .   Q   .   7
  8   .   2   .   A   F   5   .   R
  A   .   9   .   I   3   .   .   4
  1   .   .   B   N   8   .   M   .
  .   .   .   7   .   2   .   A   .
  .   P   .   .   .   9   .   T   .
  .   6   .   .   .   .   .   .   .
  .   .   .   .   .   .   .   .   .
```
> 実際は RGB カラー（白緑 → 鮮緑 → 暗緑のグラデーション）でターミナルに描画されます。

---

Rust 製のターミナルアニメーション。The Matrix 風の緑の文字ストリームが画面を流れ落ちる。  
`crossterm` で raw mode・代替スクリーンバッファを制御し、30fps（`--speed` で 15〜60fps）で描画する。

## 特徴

- **RGB 3段グラデーション** — ヘッド（白緑）→ 近い軌跡（鮮緑）→ 遠い軌跡（暗緑）。`--color` で青・赤・琥珀にも変えられる
- **文字セットの切り替え** — 英数字・半角カタカナ・0/1 を `--chars` で選べる
- **ターミナルリサイズ対応** — ウィンドウサイズ変更を即座に検知して再構築
- **RAII による安全なクリーンアップ** — `TerminalGuard` がパニック時でも raw mode を確実に解除
- **単一ファイル実装** — `src/main.rs` 約 600 行（半分ほどは単体テスト）、外部依存は `crossterm`・`rand`・`clap` のみ

## ビルド・実行

```zsh
# 開発用: ビルドして、そのまま実行する
cargo run

# リリース用: 最適化してビルドする
cargo build --release

# ビルドしたバイナリを実行する
./target/release/asciifall
```

> Rust の版は [mise](https://mise.jdx.dev/) の `mise.toml` で決めています。mise を使う場合は `mise install` で準備でき、`mise run check` で format・clippy・テストを動かせます。

## インストール

どのディレクトリからでも `asciifall` で実行できるように、`~/.cargo/bin` に配置します。

```zsh
cargo install --path .

# PATH に通っているかを確かめる（asciifall のパスが表示されれば OK）
which asciifall

# 実行する
asciifall
```

`which` で見つからないときは、`~/.zshrc` に次の行を足してターミナルを開き直してください。

```zsh
export PATH="$HOME/.cargo/bin:$PATH"
```

インストール後は、Rust 環境がなくても動作します。

## オプション

```zsh
asciifall --speed 3 --chars katakana --color blue
```

| オプション | 値 | 既定 | 内容 |
|------|------|------|------|
| `--speed` | `1`〜`3` | `2` | 落ちる速さ（1: 15fps、2: 30fps、3: 60fps） |
| `--chars` | `ascii`・`katakana`・`binary` | `ascii` | 流れる文字（英数字・半角カタカナ・0/1） |
| `--color` | `green`・`blue`・`red`・`amber` | `green` | 色 |
| `--help` | | | オプションの一覧を出す |
| `--version` | | | 版を出す |

## 操作

| キー | 動作 |
|------|------|
| `q` | 終了 |
| `Ctrl+C` | 終了 |

終了時はターミナルが自動で元の状態（通常スクリーン・カーソル表示）に戻ります。

## 動作要件

- RGB カラー対応ターミナル（iTerm2、macOS Terminal.app 10.15+）
- 代替スクリーンバッファ対応（現代的なターミナルなら問題なし）
- 等幅フォント

## アーキテクチャ

| 構造体 | 役割 |
|--------|------|
| `Stream` | 1本の落下ストリーム。ヘッド位置・軌跡文字列・速度を管理する |
| `Column` | 列ごとに `Stream` を生成・保持し、spawn タイマーで間欠的に湧かせる |
| `Rain` | 全 `Column` を保持するトップレベル状態。リサイズ時に全体を再生成する |
| `TerminalGuard` | RAII パターンでターミナル状態を管理。スコープを抜けると自動復元する |

アニメーションループは `crossterm::event::poll(Duration::ZERO)` でノンブロッキングにキー／リサイズイベントを処理しつつ、フレーム時間（既定は 33ms。`--speed` で変わる）を差し引いてスリープすることで、一定のフレームレートを保つ。描き直すのは変わったセルだけで、`Stream::advance` が返すセル（`Cell`）を `draw` が描く。色は `Shade`（先頭・近い軌跡・遠い軌跡・消す）で持ち、描くときに `--color` のテーマの色にする
