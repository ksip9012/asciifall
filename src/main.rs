// ── 標準ライブラリ ──────────────────────────────────────────────────────────
// io::Write  : 出力バッファへの書き込みインターフェース
// Duration   : 時間の長さ（例: 33ミリ秒）
// Instant    : 現在時刻のスナップショット（経過時間の計測に使う）
use std::io::{self, Write};
use std::time::{Duration, Instant};

// ── 外部クレート: crossterm ──────────────────────────────────────────────────
// crossterm はターミナル（端末）を制御するライブラリ。
// カーソル移動・色設定・キー入力検知などをクロスプラットフォームで提供する。
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    queue, // ターミナルへのコマンド送信マクロ
    style::{Color, Print, SetForegroundColor},
    terminal::{self, ClearType},
};

// ── 外部クレート: rand ────────────────────────────────────────────────────────
// 乱数生成に使う。文字や速度をランダムに決めるため。
use rand::Rng;

// ── アニメーション定数 ────────────────────────────────────────────────────────
const TARGET_FPS: u64 = 30; // 目標フレームレート（毎秒30回描画）
const FRAME_MS: u64 = 1000 / TARGET_FPS; // 1フレームの時間 = 33ミリ秒

const MIN_TRAIL: u16 = 8; // 落下ストリームの最小の長さ（行数）
const MAX_TRAIL: u16 = 25; // 落下ストリームの最大の長さ（行数）

const MIN_SPEED: u8 = 1; // 最速: 毎tick 1行進む
const MAX_SPEED: u8 = 3; // 最遅: 3tick に 1行しか進まない

// ── 色の定義（RGB カラー）────────────────────────────────────────────────────
// 先頭（ヘッド）: 明るい白緑
const COLOR_HEAD: Color = Color::Rgb {
    r: 220,
    g: 255,
    b: 220,
};
// 先頭近く（近い軌跡）: 鮮やかな緑
const COLOR_NEAR: Color = Color::Rgb {
    r: 0,
    g: 255,
    b: 70,
};
// 末尾付近（遠い軌跡）: 暗い緑
const COLOR_FAR: Color = Color::Rgb {
    r: 0,
    g: 128,
    b: 35,
};

// ── 文字プールの生成 ──────────────────────────────────────────────────────────
// '0'〜'9' と 'A'〜'Z' を連結した Vec<char>（文字の動的配列）を返す。
// 落下する文字は、このプールからランダムに選ばれる。
fn build_char_pool() -> Vec<char> {
    ('0'..='9').chain('A'..='Z').collect()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// struct Drop — 1本の「落下ストリーム」
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// struct は Rust でデータをまとめる型（他言語のクラスのデータ部分に相当）。
struct Drop {
    head: i32,         // 先頭文字の現在の行番号（画面上端が0。負値は画面外上方）
    trail_length: u16, // 軌跡の長さ（文字数）
    chars: Vec<char>,  // 軌跡に表示されている文字の列（先頭が index 0）
    speed: u8,         // 速度: この tick 数ごとに 1行進む（1=最速, 3=最遅）
    tick_counter: u8,  // speed に達するまでカウントするタイマー
}

// impl Drop { ... } = Drop 型のメソッド（関数）をまとめるブロック
impl Drop {
    // ── コンストラクタ ────────────────────────────────────────────────────────
    // rng: 乱数生成器、char_pool: 文字プール、height: 画面の高さ
    fn new(rng: &mut impl Rng, char_pool: &[char], height: u16) -> Self {
        // 軌跡の長さをランダムに決める
        let trail_length = rng.random_range(MIN_TRAIL..=MAX_TRAIL);

        // 初期位置: 軌跡全体が画面上方の外から始まり、少し位置をずらす
        // → 全ストリームが同時に画面上端から現れないようにするため
        let head = -(trail_length as i32) + rng.random_range(0..=(height / 4)) as i32;

        // 初期文字列をランダムに生成
        let chars = (0..trail_length)
            .map(|_| char_pool[rng.random_range(0..char_pool.len())])
            .collect();

        Drop {
            head,
            trail_length,
            chars,
            speed: rng.random_range(MIN_SPEED..=MAX_SPEED),
            tick_counter: 0,
        }
    }

    // ── 死亡判定 ──────────────────────────────────────────────────────────────
    // 軌跡の末尾（head - trail_length）が画面下端を超えたら「死亡」とみなす。
    // 死亡した Drop は Column が削除して新しい Drop に置き換える。
    fn is_dead(&self, height: u16) -> bool {
        self.head - self.trail_length as i32 >= height as i32
    }

    // ── 1tick の描画更新 ──────────────────────────────────────────────────────
    // x      : この Drop が属する列番号（画面左端が0）
    // stdout : 描画コマンドをバッファリングする出力先
    fn tick(
        &mut self,
        stdout: &mut impl Write,
        x: u16,
        height: u16,
        char_pool: &[char],
        rng: &mut impl Rng,
    ) -> std::io::Result<()> {
        // speed に達していなければ、今tick は何もしない（速度制御）
        self.tick_counter += 1;
        if self.tick_counter < self.speed {
            return Ok(());
        }
        self.tick_counter = 0;

        // 先頭を 1行下へ移動
        let old_head = self.head;
        self.head += 1;

        // ── 末尾の消去 ──────────────────────────────────────────────────────
        // 軌跡が 1行下にずれたので、一番古い末尾セルをスペースで消す。
        let tail_row = old_head - self.trail_length as i32;
        if tail_row >= 0 && tail_row < height as i32 {
            queue!(
                stdout,
                cursor::MoveTo(x, tail_row as u16),
                SetForegroundColor(Color::Reset),
                Print(' ')
            )?;
        }

        // ── 色のグラデーション更新（near→far の境界セル）────────────────────
        // 先頭から3行上のセルを COLOR_FAR（暗い緑）で再描画する。
        // こうすることで「先頭に近いほど明るい」グラデーションが生まれる。
        let shift_row = old_head - 3;
        if shift_row >= 0 && shift_row < height as i32 {
            let idx = (self.trail_length as i32 - (old_head - shift_row)) as usize;
            if idx < self.chars.len() {
                queue!(
                    stdout,
                    cursor::MoveTo(x, shift_row as u16),
                    SetForegroundColor(COLOR_FAR),
                    Print(self.chars[idx])
                )?;
            }
        }

        // ── 前の先頭セルを COLOR_NEAR（鮮やかな緑）で再描画 ─────────────────
        // 先頭が 1行下りたので、前の先頭位置は「近い軌跡」になる。
        if old_head >= 0 && old_head < height as i32 {
            queue!(
                stdout,
                cursor::MoveTo(x, old_head as u16),
                SetForegroundColor(COLOR_NEAR),
                Print(self.chars[0])
            )?;
        }

        // ── 新しい先頭セルを COLOR_HEAD（白緑）で描画 ────────────────────────
        // 新しい文字をランダムに選び、chars の先頭に挿入する。
        // chars は trail_length 以上にならないよう末尾を切り捨てる。
        if self.head >= 0 && self.head < height as i32 {
            let new_char = char_pool[rng.random_range(0..char_pool.len())];
            self.chars.insert(0, new_char);
            self.chars.truncate(self.trail_length as usize);
            queue!(
                stdout,
                cursor::MoveTo(x, self.head as u16),
                SetForegroundColor(COLOR_HEAD),
                Print(new_char)
            )?;
        }

        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// struct Column — 画面の縦1列を管理する
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
struct Column {
    x: u16,           // 列番号（画面左端が0）
    drops: Vec<Drop>, // この列に現在存在する Drop の一覧（複数本が同時に落ちうる）
    spawn_timer: u32, // 次の Drop が生まれるまでの残りtick数
}

impl Column {
    fn new(x: u16, rng: &mut impl Rng, char_pool: &[char], height: u16) -> Self {
        let mut col = Column {
            x,
            drops: Vec::new(),
            spawn_timer: 0,
        };
        // 起動時は各列に1本の Drop を配置する
        col.drops.push(Drop::new(rng, char_pool, height));
        // 次の Drop が湧くタイミングをランダムにずらす（画面がバラバラに見えるよう）
        col.spawn_timer = rng.random_range(20..80);
        col
    }

    fn tick(
        &mut self,
        stdout: &mut impl Write,
        height: u16,
        char_pool: &[char],
        rng: &mut impl Rng,
    ) -> std::io::Result<()> {
        // この列に属する全 Drop を 1tick 進める
        for drop in &mut self.drops {
            drop.tick(stdout, self.x, height, char_pool, rng)?;
        }

        // 画面外に出た（死亡した）Drop を削除する
        // retain: 条件を満たす要素だけを残すメソッド
        self.drops.retain(|d| !d.is_dead(height));

        // Drop がすべて消えたら、タイマーを数えて次の Drop を生成する
        if self.drops.is_empty() {
            if self.spawn_timer == 0 {
                self.drops.push(Drop::new(rng, char_pool, height));
                self.spawn_timer = rng.random_range(10..60);
            } else {
                self.spawn_timer -= 1;
            }
        }

        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// struct Rain — アニメーション全体の状態を保持するトップレベル構造体
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
struct Rain {
    columns: Vec<Column>, // 全列（画面幅と同じ数だけある）
    width: u16,           // 現在のターミナル幅（列数）
    height: u16,          // 現在のターミナル高さ（行数）
    char_pool: Vec<char>, // 全 Drop で共有する文字プール
}

impl Rain {
    fn new(width: u16, height: u16) -> Self {
        let char_pool = build_char_pool();
        let mut rng = rand::rng();
        // 幅の分だけ Column を生成する（0列目〜width-1列目）
        let columns = (0..width)
            .map(|x| Column::new(x, &mut rng, &char_pool, height))
            .collect();
        Rain {
            columns,
            width,
            height,
            char_pool,
        }
    }

    // ── ターミナルサイズ変更への対応 ──────────────────────────────────────────
    // リサイズ時は画面を全消去して Column を作り直す。
    fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        let char_pool = build_char_pool();
        let mut rng = rand::rng();
        self.columns = (0..width)
            .map(|x| Column::new(x, &mut rng, &char_pool, height))
            .collect();
        self.char_pool = char_pool;
    }

    // ── 1フレーム分の更新 ────────────────────────────────────────────────────
    // 全 Column を 1tick 進める。実際の画面への反映は stdout.flush() が行う。
    fn tick(&mut self, stdout: &mut impl Write) -> std::io::Result<()> {
        let mut rng = rand::rng();
        for col in &mut self.columns {
            col.tick(stdout, self.height, &self.char_pool, &mut rng)?;
        }
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// struct TerminalGuard — ターミナル状態の安全な復元を保証する番人
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Rust の「RAII」パターン: TerminalGuard がスコープを抜けるとき（プログラム終了
// やパニック時でも）に自動で drop() が呼ばれ、ターミナルを元の状態に戻す。
// これにより「q で終了したらターミナルが壊れたまま」という問題を防ぐ。
struct TerminalGuard;

impl TerminalGuard {
    // ターミナルを「アニメーション用モード」にセットアップする。
    // - enable_raw_mode: キー入力を1文字ずつ即時受信できるモード
    // - EnterAlternateScreen: 通常の画面と切り替わる別バッファ（終了後に元に戻る）
    // - cursor::Hide: カーソルを非表示にしてちらつきを防ぐ
    fn setup(stdout: &mut impl Write) -> std::io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        Ok(TerminalGuard)
    }
}

// Drop トレイト = Rust でオブジェクト破棄時に呼ばれるデストラクタ
// TerminalGuard が破棄されるときに必ずターミナルを元に戻す
impl std::ops::Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            SetForegroundColor(Color::Reset),
            terminal::LeaveAlternateScreen, // 元の画面バッファに戻る
            cursor::Show                    // カーソルを再表示
        );
        let _ = terminal::disable_raw_mode();
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// fn run() — メインループ（エラーを Result で返す）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
fn run() -> std::io::Result<()> {
    let mut stdout = io::stdout();

    // TerminalGuard を生成してターミナルを設定する。
    // _guard は使わないが、この変数がスコープにある限り自動クリーンアップが保証される。
    let _guard = TerminalGuard::setup(&mut stdout)?;

    let (width, height) = terminal::size()?;
    let mut rain = Rain::new(width, height);

    // ── メインループ ─────────────────────────────────────────────────────────
    loop {
        let frame_start = Instant::now();

        // キューに溜まったイベント（キー押下・リサイズ）を全部処理する
        // poll(Duration::ZERO) = ブロックせず「今すぐイベントがあるか」を確認
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                // 'q' キー → 終了
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                }) => return Ok(()),

                // Ctrl+C → 終了
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => return Ok(()),

                // ターミナルのリサイズ → 画面を再構築
                Event::Resize(w, h) => {
                    execute!(stdout, terminal::Clear(ClearType::All))?;
                    rain.resize(w, h);
                }

                // その他のイベントは無視
                _ => {}
            }
        }

        // 1フレーム分のアニメーションを描画バッファに書き込む
        rain.tick(&mut stdout)?;

        // バッファを実際の画面に一括反映する（ちらつき防止のため一気に出力）
        stdout.flush()?;

        // フレームレートを TARGET_FPS に合わせるためスリープする
        // 描画に時間がかかったフレームはスリープなし（追いつきを優先）
        let elapsed = frame_start.elapsed().as_millis() as u64;
        if elapsed < FRAME_MS {
            std::thread::sleep(Duration::from_millis(FRAME_MS - elapsed));
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// fn main() — エントリーポイント
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// run() がエラーを返したときだけここで表示して異常終了する。
// 正常終了は run() が Ok(()) を返すと自動的に main() が終わる。
fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
