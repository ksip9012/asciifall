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

// 先頭から何行を、近い軌跡の色にするか
const NEAR_ROWS: i32 = 3;

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

    /// 1 tick 進め、描き直すセルを返す。画面の外の行も含む（描くときに外す）。
    /// 画面全体を描き直さず、変わったセル（消える末尾・色が変わる 2か所・新しい先頭）だけを描くため。
    fn advance(&mut self, char_pool: &[char], rng: &mut impl Rng, height: u16) -> Vec<Cell> {
        self.tick_counter += 1;
        if self.tick_counter < self.speed {
            return Vec::new();
        }
        self.tick_counter = 0;

        let old_head = self.head;
        self.head += 1;
        let mut cells = Vec::new();

        cells.push(Cell {
            row: old_head - self.trail_length as i32,
            ch: ' ',
            color: Color::Reset,
        });

        // 先頭から NEAR_ROWS 行上で、近い軌跡の色から遠い軌跡の色に変える。
        // chars[k] が先頭から k 行上の文字なので、同じ文字のまま色だけを変えられる
        if let Some(&ch) = self.chars.get(NEAR_ROWS as usize) {
            cells.push(Cell {
                row: old_head - NEAR_ROWS,
                ch,
                color: COLOR_FAR,
            });
        }

        cells.push(Cell {
            row: old_head,
            ch: self.chars[0],
            color: COLOR_NEAR,
        });

        // 先頭が画面の下に抜けたあとも chars はずらす。ずらさないと、行と chars[k] の対応がずれ、
        // 残りの軌跡を描き直すときに別の文字になる
        let new_char = char_pool[rng.random_range(0..char_pool.len())];
        self.chars.insert(0, new_char);
        self.chars.truncate(self.trail_length as usize);
        if self.head >= 0 && self.head < height as i32 {
            cells.push(Cell {
                row: self.head,
                ch: new_char,
                color: COLOR_HEAD,
            });
        }

        cells
    }
}

/// 描き直す 1セル。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cell {
    row: i32,
    ch: char,
    color: Color,
}

fn draw(stdout: &mut impl Write, x: u16, height: u16, cells: &[Cell]) -> io::Result<()> {
    for cell in cells {
        if cell.row >= 0 && cell.row < height as i32 {
            queue!(
                stdout,
                cursor::MoveTo(x, cell.row as u16),
                SetForegroundColor(cell.color),
                Print(cell.ch)
            )?;
        }
    }
    Ok(())
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
            let cells = stream.advance(char_pool, rng, height);
            draw(stdout, self.x, height, &cells)?;
        }
        self.spawn(height, char_pool, rng);
        Ok(())
    }

    /// 抜けきった列を消し、列がなくなったら間をおいて次の列を出す。
    fn spawn(&mut self, height: u16, char_pool: &[char], rng: &mut impl Rng) {
        self.streams.retain(|s| !s.is_dead(height));

        if self.streams.is_empty() {
            if self.spawn_timer == 0 {
                self.streams.push(Stream::new(rng, char_pool, height));
                self.spawn_timer = rng.random_range(10..60);
            } else {
                self.spawn_timer -= 1;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0)
    }

    fn stream(head: i32, chars: &str, speed: u8) -> Stream {
        Stream {
            head,
            trail_length: chars.chars().count() as u16,
            chars: chars.chars().collect(),
            speed,
            tick_counter: 0,
        }
    }

    #[test]
    fn new_stream_starts_above_the_screen() {
        let mut rng = rng();
        let pool = build_char_pool();
        for _ in 0..100 {
            let s = Stream::new(&mut rng, &pool, 40);
            assert!((MIN_TRAIL..=MAX_TRAIL).contains(&s.trail_length));
            assert_eq!(s.chars.len(), s.trail_length as usize);
            assert!((MIN_SPEED..=MAX_SPEED).contains(&s.speed));
            // 先頭は画面の上 4分の 1 より下には出ない
            assert!(s.head <= 40 / 4 - s.trail_length as i32);
        }
    }

    #[test]
    fn stream_is_dead_after_the_tail_leaves_the_screen() {
        // 高さ 10 の画面で、長さ 4 の列の先頭が 13行目なら、軌跡は 10〜13行目にあり、
        // 9行目の文字は次に進むときに消す。まだ消していないので生きている
        assert!(!stream(13, "ABCD", 1).is_dead(10));
        // 先頭が 14行目なら 9行目も消し終わり、全部が画面の外
        assert!(stream(14, "ABCD", 1).is_dead(10));
    }

    #[test]
    fn stream_moves_once_every_speed_ticks() {
        let mut rng = rng();
        let pool = build_char_pool();
        let mut s = stream(5, "ABCDEFGH", 3);
        assert!(s.advance(&pool, &mut rng, 20).is_empty());
        assert!(s.advance(&pool, &mut rng, 20).is_empty());
        assert!(!s.advance(&pool, &mut rng, 20).is_empty());
        assert_eq!(s.head, 6);
    }

    #[test]
    fn advance_erases_the_tail_and_draws_the_new_head() {
        let mut rng = rng();
        let pool = build_char_pool();
        let mut s = stream(10, "ABCDEFGH", 1);
        let cells = s.advance(&pool, &mut rng, 20);

        assert_eq!(
            cells[0],
            Cell {
                row: 2,
                ch: ' ',
                color: Color::Reset
            }
        );
        assert!(cells.contains(&Cell {
            row: 10,
            ch: 'A',
            color: COLOR_NEAR
        }));
        let head = cells.last().unwrap();
        assert_eq!((head.row, head.color), (11, COLOR_HEAD));
        assert_eq!(s.chars[0], head.ch);
        assert_eq!(s.chars.len(), 8);
    }

    #[test]
    fn far_color_keeps_the_same_char() {
        // #7: 先頭から 3行上（chars[3]）の文字を、色だけ変えて描き直す
        let mut rng = rng();
        let pool = build_char_pool();
        let mut s = stream(10, "ABCDEFGH", 1);
        let cells = s.advance(&pool, &mut rng, 20);
        assert!(cells.contains(&Cell {
            row: 7,
            ch: 'D',
            color: COLOR_FAR
        }));
    }

    #[test]
    fn redrawn_cells_never_change_the_char_on_screen() {
        // 画面を文字の表で再現し、色を変えるために描き直すセルの文字が、
        // すでに出ている文字と同じかを確かめる（先頭が画面の下に抜けたあとも含む）
        let mut rng = rng();
        let pool = build_char_pool();
        let height = 30;
        for _ in 0..50 {
            let mut s = Stream::new(&mut rng, &pool, height);
            let mut screen: Vec<Option<char>> = vec![None; height as usize];
            while !s.is_dead(height) {
                for cell in s.advance(&pool, &mut rng, height) {
                    if cell.row < 0 || cell.row >= height as i32 {
                        continue;
                    }
                    let shown = &mut screen[cell.row as usize];
                    if (cell.color == COLOR_NEAR || cell.color == COLOR_FAR)
                        && let Some(c) = *shown
                    {
                        assert_eq!(c, cell.ch, "{} 行目の文字が描き替わった", cell.row);
                    }
                    *shown = (cell.ch != ' ').then_some(cell.ch);
                }
            }
        }
    }

    #[test]
    fn column_spawns_a_new_stream_after_the_timer() {
        let mut rng = rng();
        let pool = build_char_pool();
        let mut col = Column {
            x: 0,
            streams: vec![stream(30, "AB", 1)],
            spawn_timer: 2,
        };
        col.spawn(10, &pool, &mut rng); // 抜けきった列を消す。タイマーは 2 → 1
        assert!(col.streams.is_empty());
        col.spawn(10, &pool, &mut rng); // 1 → 0
        assert!(col.streams.is_empty());
        col.spawn(10, &pool, &mut rng);
        assert_eq!(col.streams.len(), 1);
    }

    #[test]
    fn rain_rebuilds_columns_on_resize() {
        let mut rain = Rain::new(5, 10);
        assert_eq!(rain.columns.len(), 5);
        rain.resize(8, 20);
        assert_eq!(rain.columns.len(), 8);
        assert_eq!(rain.height, 20);
        let xs: Vec<u16> = rain.columns.iter().map(|c| c.x).collect();
        assert_eq!(xs, (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn draw_skips_cells_outside_the_screen() {
        let cells = [
            Cell {
                row: -1,
                ch: 'A',
                color: COLOR_HEAD,
            },
            Cell {
                row: 10,
                ch: 'B',
                color: COLOR_HEAD,
            },
        ];
        let mut out = Vec::new();
        draw(&mut out, 0, 10, &cells).unwrap();
        assert!(out.is_empty());
    }
}
