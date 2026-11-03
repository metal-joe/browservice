//! Port of `src/rect.hpp` and `src/image_slice.{hpp,cpp}`.
//!
//! An [`ImageSlice`] is a reference to a rectangular part of a shared BGRx
//! image buffer (4 bytes per pixel: blue, green, red, unused). Cloning a slice
//! is cheap and refers to the same buffer, like copying the C++ object.
//! Buffers are only used on the CEF UI thread.

use std::cell::UnsafeCell;
use std::rc::Rc;

/// Rectangle `[start_x, end_x) x [start_y, end_y)`. The empty rectangle is
/// represented by all zeros.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub start_x: i32,
    pub end_x: i32,
    pub start_y: i32,
    pub end_y: i32,
}

impl Rect {
    pub fn new(start_x: i32, end_x: i32, start_y: i32, end_y: i32) -> Rect {
        if start_x >= end_x || start_y >= end_y {
            Rect::default()
        } else {
            Rect {
                start_x,
                end_x,
                start_y,
                end_y,
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start_x >= self.end_x || self.start_y >= self.end_y
    }

    pub fn translate(rect: Rect, dx: i32, dy: i32) -> Rect {
        Rect::new(
            rect.start_x + dx,
            rect.end_x + dx,
            rect.start_y + dy,
            rect.end_y + dy,
        )
    }

    pub fn intersection(a: Rect, b: Rect) -> Rect {
        Rect::new(
            a.start_x.max(b.start_x),
            a.end_x.min(b.end_x),
            a.start_y.max(b.start_y),
            a.end_y.min(b.end_y),
        )
    }
}

struct Buffer(UnsafeCell<Vec<u8>>);

#[derive(Clone, Default)]
pub struct ImageSlice {
    global_buf: Option<Rc<Buffer>>,
    /// Byte offset of pixel (0, 0) of this slice in the buffer.
    offset: usize,
    width: i32,
    height: i32,
    pitch: i32,
    global_x: i32,
    global_y: i32,
}

impl ImageSlice {
    /// Creates a new independent `width` x `height` image filled with
    /// `(r, g, b)`.
    pub fn create_image_rgb(width: i32, height: i32, r: u8, g: u8, b: u8) -> ImageSlice {
        let slice = Self::create_image(width, height, 0);
        slice.fill_rgb(0, width, 0, height, r, g, b);
        slice
    }

    /// Creates a new independent image with every byte set to `rgb`.
    pub fn create_image(width: i32, height: i32, rgb: u8) -> ImageSlice {
        if !(width >= 0 && height >= 0) {
            crate::panic_log!("Requirement 'width >= 0 && height >= 0' failed");
        }
        let limit = i32::MAX / 9;
        if !(width < limit && height < limit) {
            crate::panic_log!("Requirement 'width < Limit && height < Limit' failed");
        }
        if height > 0 && width >= limit / height {
            crate::panic_log!("Requirement 'width < Limit / height' failed");
        }

        let size = 4 * width as usize * height as usize;
        ImageSlice {
            global_buf: Some(Rc::new(Buffer(UnsafeCell::new(vec![rgb; size])))),
            offset: 0,
            width,
            height,
            pitch: width,
            global_x: 0,
            global_y: 0,
        }
    }

    pub fn width(&self) -> i32 {
        self.width
    }
    pub fn height(&self) -> i32 {
        self.height
    }
    pub fn pitch(&self) -> i32 {
        self.pitch
    }
    pub fn global_x(&self) -> i32 {
        self.global_x
    }
    pub fn global_y(&self) -> i32 {
        self.global_y
    }

    pub fn contains_global_point(&self, gx: i32, gy: i32) -> bool {
        let x = gx - self.global_x;
        let y = gy - self.global_y;
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Pointer to the first pixel. For all `0 <= y < height` and
    /// `0 <= x < width`, `buf[4 * (y * pitch + x) + c]` is the value of
    /// channel blue, green, red for c = 0, 1, 2. Null for a default slice.
    pub fn buf_ptr(&self) -> *mut u8 {
        match &self.global_buf {
            Some(buf) => unsafe { (*buf.0.get()).as_mut_ptr().add(self.offset) },
            None => std::ptr::null_mut(),
        }
    }

    /// Pointer to pixel (x, y); no bounds checking (x = width gives the
    /// past-the-end-of-line pointer).
    pub fn pixel_ptr(&self, x: i32, y: i32) -> *mut u8 {
        let idx = 4 * (y as isize * self.pitch as isize + x as isize);
        unsafe { self.buf_ptr().offset(idx) }
    }

    /// Returns a copy of row `y` of the slice (BGRx bytes).
    pub fn row(&self, y: i32) -> Vec<u8> {
        if y < 0 || y >= self.height || self.width == 0 {
            return Vec::new();
        }
        let len = 4 * self.width as usize;
        unsafe { std::slice::from_raw_parts(self.pixel_ptr(0, y), len) }.to_vec()
    }

    pub fn set_pixel(&self, x: i32, y: i32, r: u8, g: u8, b: u8) {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            unsafe {
                let pos = self.pixel_ptr(x, y);
                *pos.add(2) = r;
                *pos.add(1) = g;
                *pos = b;
            }
        }
    }

    /// Returns (r, g, b) of pixel (x, y), which must be inside the slice.
    pub fn get_pixel(&self, x: i32, y: i32) -> (u8, u8, u8) {
        assert!(x >= 0 && x < self.width && y >= 0 && y < self.height);
        unsafe {
            let pos = self.pixel_ptr(x, y);
            (*pos.add(2), *pos.add(1), *pos)
        }
    }

    /// Copies the contents of `src` such that its top left corner is at
    /// (x, y); overflowing parts are discarded.
    pub fn put_image(&self, src: &ImageSlice, x: i32, y: i32) {
        let rect = Rect::intersection(
            Rect::new(0, src.width, 0, src.height),
            Rect::translate(Rect::new(0, self.width, 0, self.height), -x, -y),
        );
        if !rect.is_empty() {
            for line_y in rect.start_y..rect.end_y {
                unsafe {
                    std::ptr::copy(
                        src.pixel_ptr(rect.start_x, line_y),
                        self.pixel_ptr(rect.start_x + x, line_y + y),
                        4 * (rect.end_x - rect.start_x) as usize,
                    );
                }
            }
        }
    }

    fn clamp_x(&self, x: i32) -> i32 {
        x.min(self.width).max(0)
    }
    fn clamp_y(&self, y: i32) -> i32 {
        y.min(self.height).max(0)
    }

    /// Slice for subrectangle `[start_x, end_x) x [start_y, end_y)`, clamped
    /// to this slice.
    pub fn sub_rect(&self, start_x: i32, end_x: i32, start_y: i32, end_y: i32) -> ImageSlice {
        let start_x = self.clamp_x(start_x);
        let end_x = self.clamp_x(end_x).max(start_x);
        let start_y = self.clamp_y(start_y);
        let end_y = self.clamp_y(end_y).max(start_y);

        let mut ret = self.clone();
        ret.width = end_x - start_x;
        ret.height = end_y - start_y;
        ret.offset += 4 * (start_y as usize * ret.pitch as usize + start_x as usize);
        ret.global_x += start_x;
        ret.global_y += start_y;
        ret
    }

    pub fn split_x(&self, x: i32) -> (ImageSlice, ImageSlice) {
        (
            self.sub_rect(0, x, 0, self.height),
            self.sub_rect(x, self.width, 0, self.height),
        )
    }

    pub fn split_y(&self, y: i32) -> (ImageSlice, ImageSlice) {
        (
            self.sub_rect(0, self.width, 0, y),
            self.sub_rect(0, self.width, y, self.height),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_rgb(&self, start_x: i32, end_x: i32, start_y: i32, end_y: i32, r: u8, g: u8, b: u8) {
        let start_x = self.clamp_x(start_x);
        let end_x = self.clamp_x(end_x).max(start_x);
        let start_y = self.clamp_y(start_y);
        let end_y = self.clamp_y(end_y).max(start_y);
        for y in start_y..end_y {
            let mut pos = self.pixel_ptr(start_x, y);
            for _ in start_x..end_x {
                unsafe {
                    *pos.add(2) = r;
                    *pos.add(1) = g;
                    *pos = b;
                    pos = pos.add(4);
                }
            }
        }
    }

    /// Fills the subrectangle with every byte (including the unused channel)
    /// set to `rgb`.
    pub fn fill(&self, start_x: i32, end_x: i32, start_y: i32, end_y: i32, rgb: u8) {
        let start_x = self.clamp_x(start_x);
        let end_x = self.clamp_x(end_x).max(start_x);
        let start_y = self.clamp_y(start_y);
        let end_y = self.clamp_y(end_y).max(start_y);
        for y in start_y..end_y {
            unsafe {
                std::ptr::write_bytes(
                    self.pixel_ptr(start_x, y),
                    rgb,
                    4 * (end_x - start_x) as usize,
                );
            }
        }
    }

    /// Creates an independent copy of the contents (global coordinates reset
    /// to zero).
    pub fn clone_image(&self) -> ImageSlice {
        let mut data = Vec::with_capacity(4 * self.width as usize * self.height as usize);
        for y in 0..self.height {
            data.extend_from_slice(&self.row(y));
        }
        ImageSlice {
            global_buf: Some(Rc::new(Buffer(UnsafeCell::new(data)))),
            offset: 0,
            width: self.width,
            height: self.height,
            pitch: self.width,
            global_x: 0,
            global_y: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_ops() {
        assert!(Rect::new(5, 5, 0, 10).is_empty());
        assert_eq!(Rect::new(5, 3, 0, 10), Rect::default());
        let r = Rect::intersection(Rect::new(0, 10, 0, 10), Rect::new(5, 20, -5, 5));
        assert_eq!(r, Rect::new(5, 10, 0, 5));
        assert_eq!(Rect::translate(r, 1, 2), Rect::new(6, 11, 2, 7));
    }

    #[test]
    fn slices_share_buffer() {
        let img = ImageSlice::create_image(10, 8, 255);
        let (top, bottom) = img.split_y(3);
        assert_eq!((top.width(), top.height()), (10, 3));
        assert_eq!((bottom.global_x(), bottom.global_y()), (0, 3));
        assert_eq!(bottom.pitch(), 10);
        bottom.set_pixel(1, 0, 1, 2, 3);
        assert_eq!(img.get_pixel(1, 3), (1, 2, 3));
        assert!(bottom.contains_global_point(0, 3));
        assert!(!bottom.contains_global_point(0, 2));

        let sub = img.sub_rect(-5, 4, 2, 100);
        assert_eq!((sub.width(), sub.height(), sub.global_y()), (4, 6, 2));
        sub.fill_rgb(0, 100, 0, 1, 9, 8, 7);
        assert_eq!(img.get_pixel(3, 2), (9, 8, 7));
        assert_eq!(img.get_pixel(4, 2), (255, 255, 255));

        let copy = img.clone_image();
        img.fill(0, 10, 0, 8, 0);
        assert_eq!(copy.get_pixel(3, 2), (9, 8, 7));
        assert_eq!(img.get_pixel(3, 2), (0, 0, 0));

        img.put_image(&copy.sub_rect(3, 5, 2, 3), 8, 7);
        assert_eq!(img.get_pixel(8, 7), (9, 8, 7));
        assert_eq!(img.get_pixel(9, 7), (255, 255, 255));
    }

    #[test]
    fn rgb_image() {
        let img = ImageSlice::create_image_rgb(2, 2, 10, 20, 30);
        assert_eq!(img.row(1), vec![30, 20, 10, 0, 30, 20, 10, 0]);
    }
}
