//! implements a reference counted rope for efficently editing text.
use std::rc::Rc;
use std::ops::{RangeBounds, Bound};
use std::cmp::PartialEq;
use std::vec::Vec;
use std::option::Option;
use std::str::Chars;
use crate::rcstring::RcString;

/// The node of a Rope tree
#[derive(Clone, Debug)]
pub struct Node {
    /// number of bytes in the left subtree
    leftn:   usize,
    /// number of newlines in the left subtree
    leftnnl: usize,
    /// the left subtree
    left:    Rope,
    /// the right subtree
    right:   Rope,
}

/// Ropes are immutable data structures for representing a string.
/// see [wikipedia](https://en.wikipedia.org/wiki/Rope_(data_structure))
/// Ropes are reference counted and can be cloned with no additional memory,
/// creating modified ropes uses O(log n) additional memory.
#[derive(Clone, Debug)]
pub enum Rope {
    Node(Rc<Node>),
    Leaf(RcString),
}

fn nth_line_idx(s: &str, lnum: usize) -> usize {
    if s.len() == 0 {
        return 0;
    }

    let mut n = lnum;
    let mut r = 0;
    for (i, ch) in s.chars().enumerate() {
        if n == 0 {
            return i;
        }
        if ch == '\n' {
            n -= 1;
        }
        r = i;
    }
    return r + 1;
}

impl Rope {

    /// concatenates two ropes into a new Rope.
    /// concat will clone one side if the other has zero length.
    pub fn concat(r1: &Self, r2: &Self) -> Self {
        if r1.len() == 0 {
            r2.clone()
        } else if r2.len() == 0 {
            r1.clone()
        } else {
            Rope::Node(Rc::new(Node {
                leftn:   r1.len(),
                leftnnl: r1.lenlines(),
                left:    r1.clone(),
                right:   r2.clone(),
            }))
        }
    }

    /// gets the length of a rope in bytes.
    /// note that this function is O(log n)
    pub fn len(&self) -> usize {
        match &self {
            Rope::Node(nd) => nd.leftn + nd.right.len(),
            Rope::Leaf(rcs) => rcs.len(),
        }
    }

    /// gets the length of a rope in lines.
    /// this function may need to scan some part of the rope to determine this.
    pub fn lenlines(&self) -> usize {
        match &self {
            Rope::Node(nd) => nd.leftnnl + nd.right.lenlines(),
            Rope::Leaf(rcs) => rcs.lenlines(),
        }
    }

    /// creates a new rope that is the substring from `idx` of length `n`.
    pub fn char_substr(&self, idx: usize, n: usize) -> Self {
        match &self {
            Rope::Leaf(rcs) => Rope::Leaf(rcs.substr(idx, n)),
            Rope::Node(nd) =>
                if idx >= nd.leftn {
                    nd.right.char_substr(idx-nd.leftn, n)
                } else if idx + n <= nd.leftn {
                    nd.left.char_substr(idx, n)
                } else {
                    Rope::concat(
                        &nd.left.char_substr(idx, nd.leftn),
                        &nd.right.char_substr(0, n - (nd.leftn - idx)))
                },
        }
    }

    /// creates a new rope that is the slice with bounds `r`
    pub fn char_slice(&self, r: impl RangeBounds<usize>) -> Self {
        let start = match r.start_bound() {
            Bound::Included(b) => *b,
            Bound::Excluded(b) => b+1,
            Bound::Unbounded => 0,
        };
        let len = match r.end_bound() {
            Bound::Included(b) => (b - start) + 1,
            Bound::Excluded(b) => b - start,
            Bound::Unbounded => self.len() - start,
        };
        self.char_substr(start, len)
    }

    /// find the byte offset of the `lnum`th line.
    /// will return the length of the rope for lines past the last one.
    pub fn line_start(&self, lnum: usize) -> usize {
        match &self {
            Rope::Leaf(rcs) => nth_line_idx(rcs.str(), lnum),
            Rope::Node(nd) => if lnum <= nd.leftnnl {
                nd.left.line_start(lnum)
            } else {
                nd.right.line_start(lnum - nd.leftnnl) + nd.leftn
            }
        }
    }

    /// produces a Rope that is the substring of `n` lines starting at `idx`.
    /// a trailing newline will be included if it exists.
    pub fn line_substr(&self, idx: usize, n: usize) -> Self {
        self.line_slice(idx..(idx+n))
    }

    /// produces a rope that is a slice of bounds `r`.
    /// a trailing newline will be included if it exists.
    pub fn line_slice(&self, r: impl RangeBounds<usize>) -> Self {
        //TODO: this could be faster by scanning from the start maybe
        let start = match r.start_bound() {
            Bound::Included(b) => self.line_start(*b),
            Bound::Excluded(b) => self.line_start(b+1),
            Bound::Unbounded => 0,
        };

        match r.end_bound() {
            Bound::Included(b) => self.char_slice(start..self.line_start(*b+1)),
            Bound::Excluded(b) => self.char_slice(start..self.line_start(*b)),
            Bound::Unbounded => self.char_slice(start..),
        }
    }

    /// returns an iterator over the leaves of the rope.
    pub fn str_iter(&self) -> RopeIter<StrIter> {
        RopeIter {
            stack:    vec![self],
            curriter: None,
        }
    }

    /// returns an iterator over the characters of the rope.
    pub fn char_iter(&self) -> RopeIter<CharIter> {
        RopeIter {
            stack:    vec![self],
            curriter: None,
        }
    }

    /// returns an iterator over the lines of the rope.
    /// newlines are considered line terminators, and will be included.
    pub fn line_iter(&self) -> LineIter {
        LineIter::from(self.clone())
    }

    /// creates a rope that has `rope` inserted at `idx`.
    /// it is okay to use slices of the rope you are inserting to, as this
    /// cannot create reference cycles.
    pub fn insert(&self, idx: usize, rope: Self) -> Self {
        let left = self.char_slice(..idx);
        let right = self.char_slice(idx..);

        //TODO: maybe use depth
        if left.len() > right.len() {
            Rope::concat(&left, &Rope::concat(&rope, &right))
        } else {
            Rope::concat(&Rope::concat(&left, &rope), &right)
        }
    }

    /// creates a new rope with range `r` deleted.
    pub fn delete(&self, r: impl RangeBounds<usize>) -> Self {
        //TODO: this is bad
        match (r.start_bound(), r.end_bound()) {
            (Bound::Unbounded, Bound::Unbounded) => Rope::from(""),
            (Bound::Unbounded, Bound::Included(hi)) =>
                self.char_slice(hi+1..),
            (Bound::Unbounded, Bound::Excluded(hi)) =>
                self.char_slice(hi..),
            (Bound::Included(lo), Bound::Unbounded) =>
                self.char_slice(..lo),
            (Bound::Included(lo), Bound::Included(hi)) =>
                Rope::concat(
                    &self.char_slice(..lo),
                    &self.char_slice(hi+1..)),
            (Bound::Included(lo), Bound::Excluded(hi)) =>
                Rope::concat(
                    &self.char_slice(..lo),
                    &self.char_slice(hi..)),
            (Bound::Excluded(_), _) => panic!(),
        }
    }
}

impl From<String> for Rope {
    fn from(s: String) -> Self {
        Rope::Leaf(RcString::from(s))
    }
}

impl From<&str> for Rope {
    fn from(s: &str) -> Self {
        Rope::from(String::from(s))
    }
}

impl PartialEq for Rope {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        for (a, b) in self.char_iter().zip(other.char_iter()) {
            if a != b {
                return false;
            }
        }

        return true;
    }
}

impl PartialEq<str> for Rope {
    fn eq(&self, other: &str) -> bool {
        if self.len() != other.len() {
            return false;
        }
        for (a, b) in self.char_iter().zip(other.chars()) {
            if a != b {
                return false;
            }
        }

        return true;
    }
}

impl PartialEq<Rope> for str {
    fn eq(&self, other: &Rope) -> bool {
        other == self
    }
}

pub trait LeafIter<'a>: Iterator {
    fn from(s: &'a RcString) -> Self;
}

pub struct StrIter<'a> {
    s: Option<&'a str>,
}

impl<'a> LeafIter<'a> for StrIter<'a> {
    fn from(s: &'a RcString) -> Self {
        StrIter{ s: Some(s.str()) }
    }
}

impl<'a> Iterator for StrIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let r = self.s;
        self.s = None;
        r
    }
}

pub struct CharIter<'a> {
    it: Chars<'a>,
}

impl<'a> LeafIter<'a> for CharIter<'a> {
    fn from(s: &'a RcString) -> Self {
        CharIter{ it: s.str().chars()}
    }
}

impl<'a> Iterator for CharIter<'a> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        self.it.next()
    }
}

pub struct RopeIter<'a, T: LeafIter<'a>> {
    stack:    Vec<&'a Rope>,
    curriter: Option<T>,
}

impl<'a, T: LeafIter<'a>> Iterator for RopeIter<'a, T> {
    type Item = T::Item;

    fn next(&mut self) -> Option<Self::Item> {
        match match &mut self.curriter {
            None => None,
            Some(i) => i.next()
        }
        {
            Some(i) => Some(i),
            None => {
                while let Some(cur) = self.stack.pop() {
                    match cur {
                        Rope::Node(n) => {
                            self.stack.push(&n.right);
                            self.stack.push(&n.left);
                        },
                        Rope::Leaf(l) => {
                            self.curriter = Some(T::from(l));
                            return self.next();
                        },
                    }
                }
                return None
            }
        }
    }
}

pub struct LineIter {
    slice: Rope,
}

impl From<Rope> for LineIter {
    fn from(rope: Rope) -> Self {
        LineIter {
            slice: rope,
        }
    }
}

impl Iterator for LineIter {
    type Item = Rope;

    fn next(&mut self) -> Option<Self::Item> {
        println!("slice: {:?})", self.slice);
        if self.slice.len() == 0 {
            return None
        }
        let line = self.slice.line_slice(0..1);
        self.slice = self.slice.line_slice(1..);
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    extern crate itertools;
    use crate::rope::Rope;
    use itertools::zip_eq;

    #[test]
    fn test_concat() {
        let r1 = Rope::concat(
            &Rope::from("aaa"),
            &Rope::concat(
                &Rope::from("bbb"),
                &Rope::from("ccc")));

        assert_eq!(&r1, "aaabbbccc");
    }

    #[test]
    fn test_len() {
        let r1 = Rope::concat(
            &Rope::from("aaa"),
            &Rope::concat(
                &Rope::from("bbb"),
                &Rope::from("ccc")));

        assert_eq!(r1.len(), 9);
        assert_eq!(Rope::from("").len(), 0);
    }

    #[test]
    fn test_lenlines() {
        let r1 = Rope::concat(
            &Rope::from("aaa\n"),
            &Rope::concat(
                &Rope::from("b\nb\nb"),
                &Rope::from("ccc\n")));

        assert_eq!(r1.lenlines(), 4);
        assert_eq!(Rope::from("").lenlines(), 0);
        assert_eq!(Rope::from("\n").lenlines(), 1);
    }

    #[test]
    fn test_char_substr() {
        let r1 = Rope::concat(
            &Rope::from("aaa"),
            &Rope::concat(
                &Rope::from("bbb"),
                &Rope::from("ccc")));

        assert_eq!(&r1.char_substr(1, 4), "aabb");
        assert_eq!(&r1.char_substr(0, r1.len()), "aaabbbccc");
        assert_eq!(&r1.char_substr(4, 0), "");
        assert_eq!(&r1.char_substr(3, 6), "bbbccc");
    }

    #[test]
    fn test_char_slice() {
        let r1 = Rope::concat(
            &Rope::from("aaa"),
            &Rope::concat(
                &Rope::from("bbb"),
                &Rope::from("ccc")));

        assert_eq!(&r1.char_slice(1..5), "aabb");
        assert_eq!(&r1.char_slice(..), "aaabbbccc");
        assert_eq!(&r1.char_slice(4..4), "");
        assert_eq!(&r1.char_slice(3..9), "bbbccc");
    }

    #[test]
    fn test_line_start() {
        let r0 = Rope::from("\nhel");
        assert_eq!(r0.line_start(0), 0);
        assert_eq!(r0.line_start(1), 1);
        assert_eq!(r0.line_start(2), 4);
        let r1 = Rope::from("hello\nworld\nhi\n");
        assert_eq!(r1.line_start(0), 0);
        assert_eq!(r1.line_start(1), 6);
        assert_eq!(r1.line_start(2), 12);
        let r2 = Rope::concat(
            &Rope::from("hello\n"),
            &Rope::from("world\n"));
        assert_eq!(r2.line_start(0), 0);
        assert_eq!(r2.line_start(1), 6);

        let r3 = Rope::concat(
            &Rope::from("hello"),
            &Rope::from("\nworld\n"));
        assert_eq!(r3.line_start(0), 0);
        assert_eq!(r3.line_start(1), 6);
        assert_eq!(r3.line_start(2), 12);

        let r4 = Rope::concat(
            &Rope::from(""),
            &Rope::from("world\n"));
        assert_eq!(r4.line_start(0), 0);
    }

    #[test]
    fn test_line_substr() {
        let r1 = Rope::concat(
            &Rope::from("aa\na"),
            &Rope::concat(
                &Rope::from("\nbbb\n"),
                &Rope::from("ccc")));

        assert_eq!(&r1.line_substr(0, 4), "aa\na\nbbb\nccc");
        assert_eq!(&r1.line_substr(1, 2), "a\nbbb\n");
        assert_eq!(&r1.line_substr(2, 1), "bbb\n");
    }

    #[test]
    fn test_line_slice() {
        let r1 = Rope::concat(
            &Rope::from("aa\na"),
            &Rope::concat(
                &Rope::from("\nbbb\n"),
                &Rope::from("ccc")));

        assert_eq!(&r1.line_slice(..), "aa\na\nbbb\nccc");
        assert_eq!(&r1.line_slice(1..=2), "a\nbbb\n");
        assert_eq!(&r1.line_slice(2..3), "bbb\n");
    }

    #[test]
    fn test_str_iter() {
        let r1 = Rope::concat(
            &Rope::from("aa\na"),
            &Rope::concat(
                &Rope::from("\nbbb\n"),
                &Rope::from("ccc")));

        for (s1, s2) in zip_eq(r1.str_iter(), vec!["aa\na", "\nbbb\n", "ccc"]) {
            assert_eq!(s1, s2);
        }
    }

    #[test]
    fn test_char_iter() {
        let r1 = Rope::concat(
            &Rope::from("aa\na"),
            &Rope::concat(
                &Rope::from("\nbbb\n"),
                &Rope::from("ccc")));

        for (ch1, ch2) in zip_eq(r1.char_iter(), "aa\na\nbbb\nccc".chars()) {
            assert_eq!(ch1, ch2);
        }
    }

    #[test]
    fn test_line_iter() {
        let r1 = Rope::concat(
            &Rope::from("aa\na"),
            &Rope::concat(
                &Rope::from("\nbbb\n"),
                &Rope::from("ccc")));

        for (l1, l2) in zip_eq(r1.line_iter(), vec!["aa\n", "a\n", "bbb\n", "ccc"]) {
            println!("{:?}", l1);
            assert_eq!(&l1, l2);
        }
    }

    #[test]
    fn test_insert() {
        let r1 = Rope::from("hello world");
        assert_eq!(&r1, "hello world");
        assert_eq!(&r1.insert(0, Rope::from("s: ")), "s: hello world");
        assert_eq!(&r1.insert(5, Rope::from(" to the")), "hello to the world");
        assert_eq!(&r1.insert(11, Rope::from(".")), "hello world.");
    }

    #[test]
    fn test_delete() {
        let r1 = Rope::concat(
            &Rope::from("aaa"),
            &Rope::concat(
                &Rope::from("bbb"),
                &Rope::from("ccc")));
        assert_eq!(&r1.delete(..), "");
        assert_eq!(&r1.delete(..3), "bbbccc");
        assert_eq!(&r1.delete(1..=4), "abccc");
        assert_eq!(&r1.delete(1..5), "abccc");
        assert_eq!(&r1.delete(4..), "aaab");
    }
}
