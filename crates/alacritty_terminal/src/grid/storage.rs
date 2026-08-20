use std::cmp::max;
use std::mem;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::Row;
use crate::index::Line;

/// 网格之外为性能优化缓冲的最大行数。
const MAX_CACHE_SIZE: usize = 1_000;

/// 为优化索引与旋转而设的环形缓冲。
///
/// [`Storage::rotate`] 与 [`Storage::rotate_down`] 只是对内部 [`zero`] 字段的快速模加法。
/// 相比之下 [`slice::rotate_left`] 必须重排内存中的
/// 元素。
///
/// 因此本类型重新实现了 [`Index`] 与 [`IndexMut`],以处理
/// 第零个元素不一定在分配起点的情况。
///
/// 由于某些 [`Vec`] 操作在本类型上不再合法,不提供 [`Deref`]
/// 实现。任何需要暴露的 [`Vec`] 能力都必须
/// 手动转发。
///
/// [`slice::rotate_left`]:文档见 Rust 标准库(链接受限,略)。
/// [`Deref`]:即 std::ops::Deref。
/// [`zero`]:即本结构体的 zero 字段。
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Storage<T> {
    inner: Vec<Row<T>>,

    /// 行存储的起点。
    ///
    /// 该值表示环形缓冲内的起始行偏移。偏移可以大于
    /// `len` 本身,并会回绕到起点构成环形缓冲。它代表终端的
    /// 最底行。
    zero: usize,

    /// 可见行数。
    visible_lines: usize,

    /// 终端中当前活动的总行数(回滚 + 可见)。
    ///
    /// 缩小此长度即可减少回滚缓冲的行数,
    /// 无需截断原始 `inner` 缓冲。
    /// 只要 `len` 大于 `inner`,同样可以在不做任何额外插入
    /// 的情况下增长回滚缓冲。
    len: usize,
}

impl<T: PartialEq> PartialEq for Storage<T> {
    fn eq(&self, other: &Self) -> bool {
        // 两个存储缓冲都需要截断并清零。
        assert_eq!(self.zero, 0);
        assert_eq!(other.zero, 0);

        self.inner == other.inner && self.len == other.len
    }
}

impl<T> Storage<T> {
    #[inline]
    pub fn with_capacity(visible_lines: usize, columns: usize) -> Storage<T>
    where
        T: Default,
    {
        // 初始化可见行;回滚缓冲动态初始化。
        let mut inner = Vec::with_capacity(visible_lines);
        inner.resize_with(visible_lines, || Row::new(columns));

        Storage { inner, zero: 0, visible_lines, len: visible_lines }
    }

    /// 增加缓冲中的行数。
    #[inline]
    pub fn grow_visible_lines(&mut self, next: usize)
    where
        T: Default,
    {
        // 缓冲需要增长的行数。
        let additional_lines = next - self.visible_lines;

        let columns = self[Line(0)].len();
        self.initialize(additional_lines, columns);

        // 更新可见行数。
        self.visible_lines = next;
    }

    /// 减少缓冲中的行数。
    #[inline]
    pub fn shrink_visible_lines(&mut self, next: usize) {
        // 不移除任何行地缩小尺寸。
        let shrinkage = self.visible_lines - next;
        self.shrink_lines(shrinkage);

        // 更新可见行数。
        self.visible_lines = next;
    }

    /// 缩小缓冲中的行数。
    #[inline]
    pub fn shrink_lines(&mut self, shrinkage: usize) {
        self.len -= shrinkage;

        // 释放内存。
        if self.inner.len() > self.len + MAX_CACHE_SIZE {
            self.truncate();
        }
    }

    /// 从原始缓冲截掉不可见元素。
    #[inline]
    pub fn truncate(&mut self) {
        self.rezero();

        self.inner.truncate(self.len);
    }

    /// 运行时动态增长存储缓冲。
    #[inline]
    pub fn initialize(&mut self, additional_rows: usize, columns: usize)
    where
        T: Default,
    {
        if self.len + additional_rows > self.inner.len() {
            self.rezero();

            let realloc_size = self.inner.len() + max(additional_rows, MAX_CACHE_SIZE);
            self.inner.resize_with(realloc_size, || Row::new(columns));
        }

        self.len += additional_rows;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Row<T> 的交换实现。
    ///
    /// 利用 Row<T> 的已知大小,产出比经 slice::swap 略高效的
    /// 交换。
    ///
    /// swap 的默认实现生成 8 条 movups 与 4 条 movaps
    /// 指令。本实现只用 8 条 movups
    /// 指令完成交换。
    pub fn swap(&mut self, a: Line, b: Line) {
        debug_assert_eq!(mem::size_of::<Row<T>>(), mem::size_of::<usize>() * 4);

        let a = self.compute_index(a);
        let b = self.compute_index(b);

        unsafe {
            // 转成 qword 数组以绕过复制限制并避免
            // drop 风险。字节数组不行,因为 LLVM
            // 不肯优化它。
            let a_ptr = self.inner.as_mut_ptr().add(a) as *mut MaybeUninit<usize>;
            let b_ptr = self.inner.as_mut_ptr().add(b) as *mut MaybeUninit<usize>;

            // 每次复制 1 个 qword。
            //
            // 优化器会展开并向量化该循环。
            let mut tmp: MaybeUninit<usize>;
            for i in 0..4 {
                tmp = *a_ptr.offset(i);
                *a_ptr.offset(i) = *b_ptr.offset(i);
                *b_ptr.offset(i) = tmp;
            }
        }
    }

    /// 旋转网格,把所有行在历史中上移/下移。
    #[inline]
    pub fn rotate(&mut self, count: isize) {
        debug_assert!(count.unsigned_abs() <= self.inner.len());

        let len = self.inner.len();
        self.zero = (self.zero as isize + count + len as isize) as usize % len;
    }

    /// 把所有既有行在历史中下移。
    ///
    /// 这是 [`rotate_left`] 的更快特化版本。
    ///
    /// [`rotate_left`]:文档见 Rust 标准库 Vec::rotate_left。
    #[inline]
    pub fn rotate_down(&mut self, count: usize) {
        self.zero = (self.zero + count) % self.inner.len();
    }

    /// 更新原始存储缓冲。
    #[inline]
    pub fn replace_inner(&mut self, vec: Vec<Row<T>>) {
        self.len = vec.len();
        self.inner = vec;
        self.zero = 0;
    }

    /// 从存储移除所有行。
    #[inline]
    pub fn take_all(&mut self) -> Vec<Row<T>> {
        self.truncate();

        let mut buffer = Vec::new();

        mem::swap(&mut buffer, &mut self.inner);
        self.len = 0;

        buffer
    }

    /// 给定请求索引,计算底层存储中的实际索引。
    #[inline]
    fn compute_index(&self, requested: Line) -> usize {
        debug_assert!(requested.0 < self.visible_lines as i32);

        let positive = -(requested - self.visible_lines).0 as usize - 1;

        debug_assert!(positive < self.len);

        let zeroed = self.zero + positive;

        // 此处用 if/else 而非取余以提升性能。
        //
        // 要求 `zeroed` 小于 `self.inner.len() * 2`,
        // 但 `self.zero` 与 `requested` 都恒小于 `self.inner.len()`。
        if zeroed >= self.inner.len() { zeroed - self.inner.len() } else { zeroed }
    }

    /// 旋转环形缓冲,把 `self.zero` 复位到索引 `0`。
    #[inline]
    fn rezero(&mut self) {
        if self.zero == 0 {
            return;
        }

        self.inner.rotate_left(self.zero);
        self.zero = 0;
    }
}

impl<T> Index<Line> for Storage<T> {
    type Output = Row<T>;

    #[inline]
    fn index(&self, index: Line) -> &Self::Output {
        let index = self.compute_index(index);
        &self.inner[index]
    }
}

impl<T> IndexMut<Line> for Storage<T> {
    #[inline]
    fn index_mut(&mut self, index: Line) -> &mut Self::Output {
        let index = self.compute_index(index);
        &mut self.inner[index]
    }
}

#[cfg(test)]
mod tests {
    use crate::grid::GridCell;
    use crate::grid::row::Row;
    use crate::grid::storage::{MAX_CACHE_SIZE, Storage};
    use crate::index::{Column, Line};
    use crate::term::cell::Flags;

    impl GridCell for char {
        fn is_empty(&self) -> bool {
            *self == ' ' || *self == '\t'
        }

        fn reset(&mut self, template: &Self) {
            *self = *template;
        }

        fn flags(&self) -> &Flags {
            unimplemented!();
        }

        fn flags_mut(&mut self) -> &mut Flags {
            unimplemented!();
        }
    }

    #[test]
    fn with_capacity() {
        let storage = Storage::<char>::with_capacity(3, 1);

        assert_eq!(storage.inner.len(), 3);
        assert_eq!(storage.len, 3);
        assert_eq!(storage.zero, 0);
        assert_eq!(storage.visible_lines, 3);
    }

    #[test]
    fn indexing() {
        let mut storage = Storage::<char>::with_capacity(3, 1);

        storage[Line(0)] = filled_row('0');
        storage[Line(1)] = filled_row('1');
        storage[Line(2)] = filled_row('2');

        storage.zero += 1;

        assert_eq!(storage[Line(0)], filled_row('2'));
        assert_eq!(storage[Line(1)], filled_row('0'));
        assert_eq!(storage[Line(2)], filled_row('1'));
    }

    #[test]
    #[should_panic]
    #[cfg(debug_assertions)]
    fn indexing_above_inner_len() {
        let storage = Storage::<char>::with_capacity(1, 1);
        let _ = &storage[Line(-1)];
    }

    #[test]
    fn rotate() {
        let mut storage = Storage::<char>::with_capacity(3, 1);
        storage.rotate(2);
        assert_eq!(storage.zero, 2);
        storage.shrink_lines(2);
        assert_eq!(storage.len, 1);
        assert_eq!(storage.inner.len(), 3);
        assert_eq!(storage.zero, 2);
    }

    /// 在缓冲末尾增长一行。
    ///
    /// 之前:
    ///   0: 0 <- Zero(零点)
    ///   1: 1
    ///   2: -
    /// 之后:
    ///   0: 0 <- Zero(零点)
    ///   1: 1
    ///   2: -
    ///   3: \0
    ///   ...
    ///   MAX_CACHE_SIZE: \0(空位)
    #[test]
    fn grow_after_zero() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![filled_row('0'), filled_row('1'), filled_row('-')],
            zero: 0,
            visible_lines: 3,
            len: 3,
        };

        // 增长缓冲。
        storage.grow_visible_lines(4);

        // 确保结果正确。
        let mut expected = Storage {
            inner: vec![filled_row('0'), filled_row('1'), filled_row('-')],
            zero: 0,
            visible_lines: 4,
            len: 4,
        };
        expected.inner.append(&mut vec![filled_row('\0'); MAX_CACHE_SIZE]);

        assert_eq!(storage.visible_lines, expected.visible_lines);
        assert_eq!(storage.inner, expected.inner);
        assert_eq!(storage.zero, expected.zero);
        assert_eq!(storage.len, expected.len);
    }

    /// 在缓冲开头增长一行。
    ///
    /// 之前:
    ///   0: -
    ///   1: 0 <- Zero(零点)
    ///   2: 1
    /// 之后:
    ///   0: 0 <- Zero(零点)
    ///   1: 1
    ///   2: -
    ///   3: \0
    ///   ...
    ///   MAX_CACHE_SIZE: \0(空位)
    #[test]
    fn grow_before_zero() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![filled_row('-'), filled_row('0'), filled_row('1')],
            zero: 1,
            visible_lines: 3,
            len: 3,
        };

        // 增长缓冲。
        storage.grow_visible_lines(4);

        // 确保结果正确。
        let mut expected = Storage {
            inner: vec![filled_row('0'), filled_row('1'), filled_row('-')],
            zero: 0,
            visible_lines: 4,
            len: 4,
        };
        expected.inner.append(&mut vec![filled_row('\0'); MAX_CACHE_SIZE]);

        assert_eq!(storage.visible_lines, expected.visible_lines);
        assert_eq!(storage.inner, expected.inner);
        assert_eq!(storage.zero, expected.zero);
        assert_eq!(storage.len, expected.len);
    }

    /// 在缓冲开头缩小一行。
    ///
    /// 之前:
    ///   0: 2
    ///   1: 0 <- Zero(零点)
    ///   2: 1
    /// 之后:
    ///   0: 2 <- Hidden(隐藏)
    ///   0: 0 <- Zero(零点)
    ///   1: 1
    #[test]
    fn shrink_before_zero() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![filled_row('2'), filled_row('0'), filled_row('1')],
            zero: 1,
            visible_lines: 3,
            len: 3,
        };

        // 缩小缓冲。
        storage.shrink_visible_lines(2);

        // 确保结果正确。
        let expected = Storage {
            inner: vec![filled_row('2'), filled_row('0'), filled_row('1')],
            zero: 1,
            visible_lines: 2,
            len: 2,
        };
        assert_eq!(storage.visible_lines, expected.visible_lines);
        assert_eq!(storage.inner, expected.inner);
        assert_eq!(storage.zero, expected.zero);
        assert_eq!(storage.len, expected.len);
    }

    /// 在缓冲末尾缩小一行。
    ///
    /// 之前:
    ///   0: 0 <- Zero(零点)
    ///   1: 1
    ///   2: 2
    /// 之后:
    ///   0: 0 <- Zero(零点)
    ///   1: 1
    ///   2: 2 <- Hidden(隐藏)
    #[test]
    fn shrink_after_zero() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![filled_row('0'), filled_row('1'), filled_row('2')],
            zero: 0,
            visible_lines: 3,
            len: 3,
        };

        // 缩小缓冲。
        storage.shrink_visible_lines(2);

        // 确保结果正确。
        let expected = Storage {
            inner: vec![filled_row('0'), filled_row('1'), filled_row('2')],
            zero: 0,
            visible_lines: 2,
            len: 2,
        };
        assert_eq!(storage.visible_lines, expected.visible_lines);
        assert_eq!(storage.inner, expected.inner);
        assert_eq!(storage.zero, expected.zero);
        assert_eq!(storage.len, expected.len);
    }

    /// 在缓冲开头与末尾缩小。
    ///
    /// 之前:
    ///   0: 4
    ///   1: 5
    ///   2: 0 <- Zero(零点)
    ///   3: 1
    ///   4: 2
    ///   5: 3
    /// 之后:
    ///   0: 4 <- Hidden(隐藏)
    ///   1: 5 <- Hidden(隐藏)
    ///   2: 0 <- Zero(零点)
    ///   3: 1
    ///   4: 2 <- Hidden(隐藏)
    ///   5: 3 <- Hidden(隐藏)
    #[test]
    fn shrink_before_and_after_zero() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![
                filled_row('4'),
                filled_row('5'),
                filled_row('0'),
                filled_row('1'),
                filled_row('2'),
                filled_row('3'),
            ],
            zero: 2,
            visible_lines: 6,
            len: 6,
        };

        // 缩小缓冲。
        storage.shrink_visible_lines(2);

        // 确保结果正确。
        let expected = Storage {
            inner: vec![
                filled_row('4'),
                filled_row('5'),
                filled_row('0'),
                filled_row('1'),
                filled_row('2'),
                filled_row('3'),
            ],
            zero: 2,
            visible_lines: 2,
            len: 2,
        };
        assert_eq!(storage.visible_lines, expected.visible_lines);
        assert_eq!(storage.inner, expected.inner);
        assert_eq!(storage.zero, expected.zero);
        assert_eq!(storage.len, expected.len);
    }

    /// 检查截断时全部隐藏行都从原始缓冲移除。
    ///
    /// 之前:
    ///   0: 4 <- Hidden(隐藏)
    ///   1: 5 <- Hidden(隐藏)
    ///   2: 0 <- Zero(零点)
    ///   3: 1
    ///   4: 2 <- Hidden(隐藏)
    ///   5: 3 <- Hidden(隐藏)
    /// 之后:
    ///   0: 0 <- Zero(零点)
    ///   1: 1
    #[test]
    fn truncate_invisible_lines() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![
                filled_row('4'),
                filled_row('5'),
                filled_row('0'),
                filled_row('1'),
                filled_row('2'),
                filled_row('3'),
            ],
            zero: 2,
            visible_lines: 1,
            len: 2,
        };

        // 截断缓冲。
        storage.truncate();

        // 确保结果正确。
        let expected = Storage {
            inner: vec![filled_row('0'), filled_row('1')],
            zero: 0,
            visible_lines: 1,
            len: 2,
        };
        assert_eq!(storage.visible_lines, expected.visible_lines);
        assert_eq!(storage.inner, expected.inner);
        assert_eq!(storage.zero, expected.zero);
        assert_eq!(storage.len, expected.len);
    }

    /// 仅在开头截断缓冲。
    ///
    /// 之前:
    ///   0: 1
    ///   1: 2 <- Hidden(隐藏)
    ///   2: 0 <- Zero(零点)
    /// 之后:
    ///   0: 1
    ///   0: 0 <- Zero(零点)
    #[test]
    fn truncate_invisible_lines_beginning() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![filled_row('1'), filled_row('2'), filled_row('0')],
            zero: 2,
            visible_lines: 1,
            len: 2,
        };

        // 截断缓冲。
        storage.truncate();

        // 确保结果正确。
        let expected = Storage {
            inner: vec![filled_row('0'), filled_row('1')],
            zero: 0,
            visible_lines: 1,
            len: 2,
        };
        assert_eq!(storage.visible_lines, expected.visible_lines);
        assert_eq!(storage.inner, expected.inner);
        assert_eq!(storage.zero, expected.zero);
        assert_eq!(storage.len, expected.len);
    }

    /// 先缩小缓冲再重新增长。
    ///
    /// 之前:
    ///   0: 4
    ///   1: 5
    ///   2: 0 <- Zero(零点)
    ///   3: 1
    ///   4: 2
    ///   5: 3
    /// 缩小后:
    ///   0: 4 <- Hidden(隐藏)
    ///   1: 5 <- Hidden(隐藏)
    ///   2: 0 <- Zero(零点)
    ///   3: 1
    ///   4: 2
    ///   5: 3 <- Hidden(隐藏)
    /// 增长后:
    ///   0: 4
    ///   1: 5
    ///   2: -
    ///   3: 0 <- Zero(零点)
    ///   4: 1
    ///   5: 2
    ///   6: 3
    #[test]
    fn shrink_then_grow() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![
                filled_row('4'),
                filled_row('5'),
                filled_row('0'),
                filled_row('1'),
                filled_row('2'),
                filled_row('3'),
            ],
            zero: 2,
            visible_lines: 0,
            len: 6,
        };

        // 缩小缓冲。
        storage.shrink_lines(3);

        // 确保缩小后的结果正确。
        let shrinking_expected = Storage {
            inner: vec![
                filled_row('4'),
                filled_row('5'),
                filled_row('0'),
                filled_row('1'),
                filled_row('2'),
                filled_row('3'),
            ],
            zero: 2,
            visible_lines: 0,
            len: 3,
        };
        assert_eq!(storage.inner, shrinking_expected.inner);
        assert_eq!(storage.zero, shrinking_expected.zero);
        assert_eq!(storage.len, shrinking_expected.len);

        // 增长缓冲。
        storage.initialize(1, 1);

        // 确保先前释放的元素被复用。
        let growing_expected = Storage {
            inner: vec![
                filled_row('4'),
                filled_row('5'),
                filled_row('0'),
                filled_row('1'),
                filled_row('2'),
                filled_row('3'),
            ],
            zero: 2,
            visible_lines: 0,
            len: 4,
        };

        assert_eq!(storage.inner, growing_expected.inner);
        assert_eq!(storage.zero, growing_expected.zero);
        assert_eq!(storage.len, growing_expected.len);
    }

    #[test]
    fn initialize() {
        // 布置存储区。
        let mut storage: Storage<char> = Storage {
            inner: vec![
                filled_row('4'),
                filled_row('5'),
                filled_row('0'),
                filled_row('1'),
                filled_row('2'),
                filled_row('3'),
            ],
            zero: 2,
            visible_lines: 0,
            len: 6,
        };

        // 初始化额外的行。
        let init_size = 3;
        storage.initialize(init_size, 1);

        // 生成期望的网格。
        let mut expected_inner = vec![
            filled_row('0'),
            filled_row('1'),
            filled_row('2'),
            filled_row('3'),
            filled_row('4'),
            filled_row('5'),
        ];
        let expected_init_size = std::cmp::max(init_size, MAX_CACHE_SIZE);
        expected_inner.append(&mut vec![filled_row('\0'); expected_init_size]);
        let expected_storage = Storage { inner: expected_inner, zero: 0, visible_lines: 0, len: 9 };

        assert_eq!(storage.len, expected_storage.len);
        assert_eq!(storage.zero, expected_storage.zero);
        assert_eq!(storage.inner, expected_storage.inner);
    }

    #[test]
    fn rotate_wrap_zero() {
        let mut storage: Storage<char> = Storage {
            inner: vec![filled_row('-'), filled_row('-'), filled_row('-')],
            zero: 2,
            visible_lines: 0,
            len: 3,
        };

        storage.rotate(2);

        assert!(storage.zero < storage.inner.len());
    }

    fn filled_row(content: char) -> Row<char> {
        let mut row = Row::new(1);
        row[Column(0)] = content;
        row
    }
}
