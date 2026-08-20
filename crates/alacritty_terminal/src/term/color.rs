use std::ops::{Index, IndexMut};

use crate::vte::ansi::{NamedColor, Rgb};

/// 终端颜色数量。
pub const COUNT: usize = 269;

/// 索引颜色数组。
///
/// | 索引      | 描述             |
/// | -------- | ----------------- |
/// | 0..16    | 命名 ANSI 颜色   |
/// | 16..232  | 色立方           |
/// | 233..256 | 灰度渐变         |
/// | 256      | 前景             |
/// | 257      | 背景             |
/// | 258      | 光标             |
/// | 259..267 | 暗色             |
/// | 267      | 亮前景           |
/// | 268      | 暗背景           |
#[derive(Copy, Clone)]
pub struct Colors([Option<Rgb>; COUNT]);

impl Default for Colors {
    fn default() -> Self {
        Self([None; COUNT])
    }
}

impl Index<usize> for Colors {
    type Output = Option<Rgb>;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl IndexMut<usize> for Colors {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

impl Index<NamedColor> for Colors {
    type Output = Option<Rgb>;

    #[inline]
    fn index(&self, index: NamedColor) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl IndexMut<NamedColor> for Colors {
    #[inline]
    fn index_mut(&mut self, index: NamedColor) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}
