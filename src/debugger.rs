// The olcNES Part#2 debug screen: RAM pages, CPU registers and upcoming code on dark blue
use std::sync::Arc;

use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::bus::Bus;
use crate::canvas::{CYAN, Canvas, DARK_BLUE, GREEN, RED, Rgba, WHITE};
use crate::cpu::{self, CPU};
use crate::disasm;
use crate::font;

// Same layout as the olcNES demo: demo.Construct(680, 480, 2, 2)
const WIDTH: u32 = 680;
const HEIGHT: u32 = 480;
const SCALE: u32 = 2;

// How many instructions to list from PC onwards
const CODE_LINES: usize = 25;

struct Debugger {
    cpu: CPU,
    bus: Bus,
    font: Vec<bool>,
    // Shown above the key help, e.g. when stepping hits a missing instruction
    message: String,
}

impl Debugger {
    fn step(&mut self) {
        let next = disasm::decode(&self.bus, self.cpu.pc as u16);
        if !next.implemented {
            self.message = format!(
                "Can't step: {} at ${:04X} not implemented",
                next.text, next.addr
            );
            return;
        }
        self.message.clear();
        self.cpu.step(1, &mut self.bus);
    }

    fn reset(&mut self) {
        self.message.clear();
        self.cpu.reset(&self.bus);
    }

    fn draw(&self, c: &mut Canvas) {
        c.clear(DARK_BLUE);
        self.draw_ram(c, 2, 2, 0x0000, 16, 16);
        // Second page follows PC, so the code being run is always visible
        self.draw_ram(c, 2, 182, self.cpu.pc as u16 & 0xFF00, 16, 16);
        self.draw_cpu(c, 448, 2);
        self.draw_code(c, 448, 82);
        c.draw_string(10, 350, &self.message, RED);
        c.draw_string(
            10,
            370,
            "SPACE = Step Instruction    R = RESET    ESC = Quit",
            WHITE,
        );
    }

    fn draw_ram(&self, c: &mut Canvas, x: i32, y: i32, addr: u16, rows: u16, cols: u16) {
        for row in 0..rows {
            let start = addr.wrapping_add(row * cols);
            let mut line = format!("${:04X}:", start);
            for col in 0..cols {
                line += &format!(" {:02X}", self.bus.read(start.wrapping_add(col)));
            }
            c.draw_string(x, y + row as i32 * 10, &line, WHITE);
        }
    }

    fn draw_cpu(&self, c: &mut Canvas, x: i32, y: i32) {
        let cpu = &self.cpu;
        c.draw_string(x, y, "STATUS:", WHITE);
        let flags = [
            ("N", cpu::FLAG_NEGATIVE),
            ("V", cpu::FLAG_OVERFLOW),
            ("-", cpu::FLAG_UNUSED),
            ("B", cpu::FLAG_BREAK),
            ("D", cpu::FLAG_DECIMAL),
            ("I", cpu::FLAG_INTERRUPT_DISABLE),
            ("Z", cpu::FLAG_ZERO),
            ("C", cpu::FLAG_CARRY),
        ];
        for (i, (name, mask)) in flags.into_iter().enumerate() {
            let col = if cpu.sr & mask != 0 { GREEN } else { RED };
            c.draw_string(x + 64 + i as i32 * 16, y, name, col);
        }
        c.draw_string(x, y + 10, &format!("PC: ${:04X}", cpu.pc), WHITE);
        c.draw_string(
            x,
            y + 20,
            &format!("A: ${:02X}  [{}]", cpu.reg_a, cpu.reg_a),
            WHITE,
        );
        c.draw_string(
            x,
            y + 30,
            &format!("X: ${:02X}  [{}]", cpu.reg_x, cpu.reg_x),
            WHITE,
        );
        c.draw_string(
            x,
            y + 40,
            &format!("Y: ${:02X}  [{}]", cpu.reg_y, cpu.reg_y),
            WHITE,
        );
        c.draw_string(
            x,
            y + 50,
            &format!("Stack P: ${:04X}", 0x0100 + cpu.sp as u16),
            WHITE,
        );
        c.draw_string(x, y + 60, &format!("Cycles: {}", cpu.total_cycles), WHITE);
    }

    // Lists the next instructions from PC: current one in cyan, ones the CPU
    // can't run yet in red
    fn draw_code(&self, c: &mut Canvas, x: i32, y: i32) {
        let code = disasm::decode_range(&self.bus, self.cpu.pc as u16, CODE_LINES);
        for (i, ins) in code.iter().enumerate() {
            let col: Rgba = if i == 0 {
                CYAN
            } else if ins.implemented {
                WHITE
            } else {
                RED
            };
            let line = format!("${:04X}: {} {{{}}}", ins.addr, ins.text, ins.mode);
            c.draw_string(x, y + i as i32 * 10, &line, col);
        }
    }
}

#[derive(Default)]
struct App {
    debugger: Option<Debugger>,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("NES emulator - CPU debugger")
            .with_inner_size(LogicalSize::new(WIDTH * SCALE, HEIGHT * SCALE));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let size = window.inner_size();
        let surface = SurfaceTexture::new(size.width, size.height, window.clone());
        self.pixels = Some(Pixels::new(WIDTH, HEIGHT, surface).expect("create pixels"));
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(pixels), Some(dbg), Some(window)) = (
            self.pixels.as_mut(),
            self.debugger.as_mut(),
            self.window.as_ref(),
        ) else {
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
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed || event.repeat {
                    return;
                }
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Space) => dbg.step(),
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Character(ch) if ch.eq_ignore_ascii_case("r") => dbg.reset(),
                    _ => return,
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                dbg.draw(&mut Canvas {
                    frame: pixels.frame_mut(),
                    width: WIDTH as usize,
                    height: HEIGHT as usize,
                    font: &dbg.font,
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

// Opens the debugger window and runs until it's closed
pub fn run(cpu: CPU, bus: Bus) {
    let mut app = App {
        debugger: Some(Debugger {
            cpu,
            bus,
            font: font::build_sheet(),
            message: String::new(),
        }),
        ..Default::default()
    };
    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.run_app(&mut app).expect("run event loop");
}
