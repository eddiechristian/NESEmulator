use crate::font;

pub type Rgba = [u8; 4];

// olc::Pixel constants
pub const DARK_BLUE: Rgba = [0, 0, 128, 255];
pub const WHITE: Rgba = [255, 255, 255, 255];
pub const GREEN: Rgba = [0, 255, 0, 255];
pub const RED: Rgba = [255, 0, 0, 255];
pub const CYAN: Rgba = [0, 255, 255, 255];
pub const YELLOW: Rgba = [255, 255, 0, 255];
pub const MAGENTA: Rgba = [255, 0, 255, 255];
pub const GREY: Rgba = [160, 160, 160, 255];

// An RGBA frame buffer with olcPixelGameEngine-style drawing, ported from pixels_test
pub struct Canvas<'a> {
    pub frame: &'a mut [u8],
    pub width: usize,
    pub height: usize,
    pub font: &'a [bool],
}

impl Canvas<'_> {
    pub fn clear(&mut self, col: Rgba) {
        for px in self.frame.chunks_exact_mut(4) {
            px.copy_from_slice(&col);
        }
    }

    pub fn draw(&mut self, x: i32, y: i32, col: Rgba) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let i = (y as usize * self.width + x as usize) * 4;
        self.frame[i..i + 4].copy_from_slice(&col);
    }

    // Port of olc::PixelGameEngine::DrawString: 8x8 glyphs, '\n' starts a
    // new line, '\t' advances 4 characters
    pub fn draw_string(&mut self, x: i32, y: i32, text: &str, col: Rgba) {
        let (mut sx, mut sy) = (0, 0);
        for c in text.chars() {
            match c {
                '\n' => {
                    sx = 0;
                    sy += 8;
                }
                '\t' => sx += 8 * 4,
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
                            if self.font[(oy + j) * font::SHEET_W + ox + i] {
                                self.draw(x + sx + i as i32, y + sy + j as i32, col);
                            }
                        }
                    }
                    sx += 8;
                }
            }
        }
    }
}
