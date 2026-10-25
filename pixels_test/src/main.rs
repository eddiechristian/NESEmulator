mod font;

use std::sync::Arc;

use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// Same layout as the olcNES Part#2 demo: demo.Construct(680, 480, 2, 2)
const WIDTH: u32 = 680;
const HEIGHT: u32 = 480;
const SCALE: u32 = 2;

type Rgba = [u8; 4];

// olc::Pixel constants
const DARK_BLUE: Rgba = [0, 0, 128, 255];
const WHITE: Rgba = [255, 255, 255, 255];
const GREEN: Rgba = [0, 255, 0, 255];
const RED: Rgba = [255, 0, 0, 255];
const CYAN: Rgba = [0, 255, 255, 255];

struct Canvas<'a> {
    frame: &'a mut [u8],
    font: &'a [bool],
}

impl Canvas<'_> {
    fn clear(&mut self, col: Rgba) {
        for px in self.frame.chunks_exact_mut(4) {
            px.copy_from_slice(&col);
        }
    }

    fn draw(&mut self, x: i32, y: i32, col: Rgba) {
        if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
            return;
        }
        let i = (y as usize * WIDTH as usize + x as usize) * 4;
        self.frame[i..i + 4].copy_from_slice(&col);
    }

    /// Port of olc::PixelGameEngine::DrawString: 8x8 glyphs, '\n' starts a
    /// new line, '\t' advances 4 characters.
    fn draw_string(&mut self, x: i32, y: i32, text: &str, col: Rgba, scale: i32) {
        let (mut sx, mut sy) = (0, 0);
        for c in text.chars() {
            match c {
                '\n' => {
                    sx = 0;
                    sy += 8 * scale;
                }
                '\t' => sx += 8 * 4 * scale,
                _ => {
                    let code = c as u32;
                    let code = if (32..128).contains(&code) {
                        code
                    } else {
                        '?' as u32
                    };
                    let ox = ((code - 32) % 16) as usize * 8;
                    let oy = ((code - 32) / 16) as usize * 8;
                    for j in 0..8 {
                        for i in 0..8 {
                            if !self.font[(oy + j) * font::SHEET_W + ox + i] {
                                continue;
                            }
                            for js in 0..scale {
                                for is in 0..scale {
                                    self.draw(
                                        x + sx + i as i32 * scale + is,
                                        y + sy + j as i32 * scale + js,
                                        col,
                                    );
                                }
                            }
                        }
                    }
                    sx += 8 * scale;
                }
            }
        }
    }
}

fn draw_frame(canvas: &mut Canvas) {
    canvas.clear(DARK_BLUE);

    let (x, y) = (448, 2);
    canvas.draw_string(x, y, "STATUS:", WHITE, 1);
    for (i, (flag, set)) in [
        ('N', true),
        ('V', false),
        ('-', true),
        ('B', false),
        ('D', false),
        ('I', true),
        ('Z', false),
        ('C', true),
    ]
    .into_iter()
    .enumerate()
    {
        canvas.draw_string(
            x + 64 + i as i32 * 16,
            y,
            &flag.to_string(),
            if set { GREEN } else { RED },
            1,
        );
    }
    canvas.draw_string(x, y + 10, "PC: 0x0600", WHITE, 1);
    canvas.draw_string(x, y + 20, "A: 0x00  [0]", WHITE, 1);
    canvas.draw_string(x, y + 30, "X: 0x00  [0]", WHITE, 1);
    canvas.draw_string(x, y + 40, "Y: 0x00  [0]", WHITE, 1);
    canvas.draw_string(x, y + 50, "Stack P: 0x00FD", WHITE, 1);
    canvas.draw_string(x, y + 72, "$0600: LDA #$01 {IMM}", CYAN, 1);

    canvas.draw_string(10, 30, "Hello, NES!", WHITE, 3);
    canvas.draw_string(
        10,
        370,
        "SPACE = Step Instruction    R = RESET    I = IRQ    N = NMI",
        WHITE,
        1,
    );
}

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    font: Vec<bool>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("pixels_test")
            .with_inner_size(LogicalSize::new(WIDTH * SCALE, HEIGHT * SCALE));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let size = window.inner_size();
        let surface = SurfaceTexture::new(size.width, size.height, window.clone());
        self.pixels = Some(Pixels::new(WIDTH, HEIGHT, surface).expect("create pixels"));
        self.font = font::build_sheet();
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    pixels
                        .resize_surface(size.width, size.height)
                        .expect("resize surface");
                }
            }
            WindowEvent::RedrawRequested => {
                draw_frame(&mut Canvas {
                    frame: pixels.frame_mut(),
                    font: &self.font,
                });
                if let Err(e) = pixels.render() {
                    eprintln!("render error: {e}");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("create event loop");
    event_loop
        .run_app(&mut App::default())
        .expect("run event loop");
}
