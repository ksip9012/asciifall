use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{Color, Print, SetForegroundColor},
    terminal::{self, ClearType},
};
use rand::Rng;

const TARGET_FPS: u64 = 30;
const FRAME_MS: u64 = 1000 / TARGET_FPS;

// 軌跡の長さ（行数）
const MIN_TRAIL: u16 = 8;
const MAX_TRAIL: u16 = 25;

// 何 tick ごとに 1行進むか（1 が最速）。列ごとに速さを変えて、画面が単調にならないようにする
const MIN_SPEED: u8 = 1;
const MAX_SPEED: u8 = 3;

// 先頭ほど明るい 3段のグラデーション
const COLOR_HEAD: Color = Color::Rgb {
    r: 220,
    g: 255,
    b: 220,
};
const COLOR_NEAR: Color = Color::Rgb {
    r: 0,
    g: 255,
    b: 70,
};
const COLOR_FAR: Color = Color::Rgb {
    r: 0,
    g: 128,
    b: 35,
};

fn build_char_pool() -> Vec<char> {
    ('0'..='9').chain('A'..='Z').collect()
}

/// 1本の落下する文字の列。
struct Stream {
    /// 先頭の行。負の値は画面の上の外にいる
    head: i32,
    trail_length: u16,
    /// 軌跡の文字。`chars[0]` が先頭
    chars: Vec<char>,
    speed: u8,
    tick_counter: u8,
}

impl Stream {
    fn new(rng: &mut impl Rng, char_pool: &[char], height: u16) -> Self {
        let trail_length = rng.random_range(MIN_TRAIL..=MAX_TRAIL);
        // 画面の上の外から始め、出てくる時刻を列ごとにずらす（全部の列が一斉に現れないように）
        let head = -(trail_length as i32) + rng.random_range(0..=(height / 4)) as i32;
        let chars = (0..trail_length)
            .map(|_| char_pool[rng.random_range(0..char_pool.len())])
            .collect();

        Stream {
            head,
            trail_length,
            chars,
            speed: rng.random_range(MIN_SPEED..=MAX_SPEED),
            tick_counter: 0,
        }
    }

    /// 軌跡の末尾まで画面の下に抜けたか。
    fn is_dead(&self, height: u16) -> bool {
        self.head - self.trail_length as i32 >= height as i32
    }

    // 画面全体を描き直さず、変わったセル（消える末尾・色が変わる 2か所・新しい先頭）だけを描く
    fn tick(
        &mut self,
        stdout: &mut impl Write,
        x: u16,
        height: u16,
        char_pool: &[char],
        rng: &mut impl Rng,
    ) -> io::Result<()> {
        self.tick_counter += 1;
        if self.tick_counter < self.speed {
            return Ok(());
        }
        self.tick_counter = 0;

        let old_head = self.head;
        self.head += 1;

        let tail_row = old_head - self.trail_length as i32;
        if tail_row >= 0 && tail_row < height as i32 {
            queue!(
                stdout,
                cursor::MoveTo(x, tail_row as u16),
                SetForegroundColor(Color::Reset),
                Print(' ')
            )?;
        }

        // 先頭から 3行上で、近い軌跡の色から遠い軌跡の色に変える
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

        if old_head >= 0 && old_head < height as i32 {
            queue!(
                stdout,
                cursor::MoveTo(x, old_head as u16),
                SetForegroundColor(COLOR_NEAR),
                Print(self.chars[0])
            )?;
        }

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

/// 画面の縦 1列。前の列が抜けきってから、間をおいて次の列を出す。
struct Column {
    x: u16,
    streams: Vec<Stream>,
    /// 次の列を出すまでの残りの tick
    spawn_timer: u32,
}

impl Column {
    fn new(x: u16, rng: &mut impl Rng, char_pool: &[char], height: u16) -> Self {
        Column {
            x,
            streams: vec![Stream::new(rng, char_pool, height)],
            spawn_timer: rng.random_range(20..80),
        }
    }

    fn tick(
        &mut self,
        stdout: &mut impl Write,
        height: u16,
        char_pool: &[char],
        rng: &mut impl Rng,
    ) -> io::Result<()> {
        for stream in &mut self.streams {
            stream.tick(stdout, self.x, height, char_pool, rng)?;
        }
        self.streams.retain(|s| !s.is_dead(height));

        if self.streams.is_empty() {
            if self.spawn_timer == 0 {
                self.streams.push(Stream::new(rng, char_pool, height));
                self.spawn_timer = rng.random_range(10..60);
            } else {
                self.spawn_timer -= 1;
            }
        }

        Ok(())
    }
}

/// 画面全体。リサイズしたら、列を全部作り直す。
struct Rain {
    columns: Vec<Column>,
    height: u16,
    char_pool: Vec<char>,
}

impl Rain {
    fn new(width: u16, height: u16) -> Self {
        let char_pool = build_char_pool();
        let columns = Self::build_columns(width, height, &char_pool);
        Rain {
            columns,
            height,
            char_pool,
        }
    }

    fn build_columns(width: u16, height: u16, char_pool: &[char]) -> Vec<Column> {
        let mut rng = rand::rng();
        (0..width)
            .map(|x| Column::new(x, &mut rng, char_pool, height))
            .collect()
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.height = height;
        self.columns = Self::build_columns(width, height, &self.char_pool);
    }

    fn tick(&mut self, stdout: &mut impl Write) -> io::Result<()> {
        let mut rng = rand::rng();
        for col in &mut self.columns {
            col.tick(stdout, self.height, &self.char_pool, &mut rng)?;
        }
        Ok(())
    }
}

/// 生きているあいだ、ターミナルを raw mode と代替スクリーンにしておく。
/// 抜けるとき（パニックを含む）に元に戻すので、終わったあとにターミナルが壊れたままにならない。
struct TerminalGuard;

impl TerminalGuard {
    fn setup(stdout: &mut impl Write) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            SetForegroundColor(Color::Reset),
            terminal::LeaveAlternateScreen,
            cursor::Show
        );
        let _ = terminal::disable_raw_mode();
    }
}

fn run() -> io::Result<()> {
    let mut stdout = io::stdout();
    let _guard = TerminalGuard::setup(&mut stdout)?;

    let (width, height) = terminal::size()?;
    let mut rain = Rain::new(width, height);

    loop {
        let frame_start = Instant::now();

        // 描画を止めないよう、待たずにたまったイベントだけを読む
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                }) => return Ok(()),
                // raw mode では Ctrl+C がシグナルにならず、キーとして届く
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => return Ok(()),
                Event::Resize(w, h) => {
                    execute!(stdout, terminal::Clear(ClearType::All))?;
                    rain.resize(w, h);
                }
                _ => {}
            }
        }

        rain.tick(&mut stdout)?;
        // 1フレーム分をまとめて出して、ちらつきを防ぐ
        stdout.flush()?;

        // 描画に時間がかかったフレームは待たずに次へ進む
        let elapsed = frame_start.elapsed().as_millis() as u64;
        if elapsed < FRAME_MS {
            std::thread::sleep(Duration::from_millis(FRAME_MS - elapsed));
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
