extern crate bitstream;

use bitstream::{BitWriter, BitReader, NoPadding};

use std::io::prelude::*;
use std::io::{Error, ErrorKind};
use std::ops::Add;
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum HuffTree<V: Eq + Copy> {
    Leaf(V),
    Node(Box<HuffTree<V>>, Box<HuffTree<V>>),
}

impl<V: Eq + Copy> HuffTree<V> {
    pub fn new_leaf(value: V) -> Self {
        HuffTree::Leaf(value)
    }

    pub fn new_node(left: Self, right: Self) -> Self {
        HuffTree::Node(Box::new(left), Box::new(right))
    }
}

impl<V: Eq + Copy + Hash> HuffTree<V> {
    pub fn encoding(self) -> HashMap<V, Vec<bool>> {
        let trail: Vec<bool> = vec![];
        let mut map = HashMap::new();

        self.build_map(trail, &mut map);

        map
    }

    fn build_map(self, trail: Vec<bool>, map: &mut HashMap<V, Vec<bool>>) {
        match self {
            HuffTree::Leaf(v) => {
                map.insert(v, trail.clone());
            }
            HuffTree::Node(l, r) => {

                //handle left
                let mut left = trail.clone();
                left.push(false);
                l.build_map(left, map);

                //handle right
                let mut right = trail.clone();
                right.push(true);
                r.build_map(right, map);
            }
        }
    }
}

pub struct HuffBuilder<V: Eq + Copy, W: PartialOrd + Add<Output = W>> {
    nodes: Vec<(V, W)>,
}

impl<V: Eq + Copy, W: PartialOrd + Add<Output = W>> HuffBuilder<V, W> {
    pub fn new() -> Self {
        HuffBuilder { nodes: vec![] }
    }

    pub fn add(mut self, sym: V, weight: W) -> Self {
        self.nodes.push((sym, weight));
        self
    }

    pub fn build(mut self) -> Option<HuffTree<V>> {
        use std::cmp::Ordering;

        self.nodes.sort_by(|a, b| if b.1 > a.1 {
            Ordering::Greater
        } else if b.1 < a.1 {
            Ordering::Less
        } else {
            Ordering::Equal
        });

        let mut nodes: Vec<(HuffTree<V>, W)> = self.nodes
            .into_iter()
            .map(|(v, w)| (HuffTree::new_leaf(v), w))
            .collect();


        while nodes.len() > 1 {
            let (right_value, right_weight) = nodes.pop().unwrap();
            let (left_value, left_weight) = nodes.pop().unwrap();

            let new_weight = left_weight + right_weight;

            let node = HuffTree::new_node(left_value, right_value);

            let pos = {
                match nodes.binary_search_by(|a| if new_weight > a.1 {
                    Ordering::Greater
                } else if new_weight < a.1 {
                    Ordering::Less
                } else {
                    Ordering::Equal
                }) {
                    Ok(i) => i,
                    Err(i) => i,
                }
            };
            nodes.insert(pos, (node, new_weight));
        }

        match nodes.pop() {
            Some((val, _)) => Some(val),
            None => None,
        }
    }
}

impl<V: Eq + Copy + Hash, W: PartialOrd + Add<Output = W>> HuffBuilder<V, W> {
    pub fn add_table<I>(mut self, table: I) -> Self
    where
        I: IntoIterator<Item = (V, W)>,
    {
        for (val, weight) in table {
            self.nodes.push((val, weight));
        }

        self
    }
}

pub struct HuffWriter<V: Eq + Copy + Hash, W: Write> {
    encoding: HashMap<V, Vec<bool>>,
    writer: BitWriter<W, NoPadding>,
}

impl<V: Eq + Copy + Hash, W: Write> HuffWriter<V, W> {
    pub fn new(tree: HuffTree<V>, writer: W) -> Self {
        HuffWriter {
            encoding: tree.encoding(),
            writer: BitWriter::new(writer),
        }
    }

    pub fn write(&mut self, value: &V) -> std::io::Result<()> {
        let bits: &Vec<bool> = match self.encoding.get(value) {
            Some(bits) => bits,
            None => {
                return Err(Error::from(ErrorKind::InvalidInput));
            }
        };

        for bit in bits {
            self.writer.write_bit(*bit)?;
        }

        Ok(())
    }
}

pub struct HuffReader<V: Eq + Copy, R: Read> {
    tree: Box<HuffTree<V>>,
    reader: BitReader<R, NoPadding>,
}

impl<V: Eq + Copy, R: Read> HuffReader<V, R> {
    pub fn new(tree: HuffTree<V>, reader: R) -> Self {
        HuffReader {
            tree: Box::new(tree),
            reader: BitReader::new(reader),
        }
    }

    pub fn read(&mut self) -> std::io::Result<V> {

        let mut cursor = &self.tree;

        loop {
            match **cursor {
                HuffTree::Leaf(ref value) => return Ok(*value),
                HuffTree::Node(ref l, ref r) => {
                    let bit = self.reader.read_bit()?;
                    match bit {
                        Some(b) => {
                            cursor = if b { r } else { l };
                        }
                        None => return Err(Error::from(ErrorKind::UnexpectedEof)),
                    }
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn build_simple_tree() {
        let tree = HuffBuilder::<char, u32>::new()
            .add('a', 1)
            .add('b', 2)
            .add('d', 10)
            .build()
            .unwrap();

        let expected = HuffTree::new_node(
            HuffTree::new_leaf('d'),
            HuffTree::new_node(HuffTree::new_leaf('b'), HuffTree::new_leaf('a')),
        );

        assert_eq!(expected, tree);
    }

    #[test]
    fn build_flat_tree() {
        let tree = HuffBuilder::<char, u32>::new()
            .add('a', 1)
            .add('b', 1)
            .add('c', 1)
            .add('d', 1)
            .build()
            .unwrap();

        let expected = HuffTree::new_node(
            HuffTree::new_node(HuffTree::new_leaf('a'), HuffTree::new_leaf('b')),
            HuffTree::new_node(HuffTree::new_leaf('c'), HuffTree::new_leaf('d')),
        );

        assert_eq!(expected, tree);
    }

    #[test]
    fn build_tree_from_table() {
        let mut table = HashMap::new();
        {
            table.insert('a', 2);
            table.insert('b', 1);
        }

        let tree = HuffBuilder::new().add_table(table).build().unwrap();

        let expected = HuffTree::new_node(HuffTree::new_leaf('a'), HuffTree::new_leaf('b'));

        assert_eq!(expected, tree);
    }

    #[test]
    fn encoding_map() {
        let tree = HuffBuilder::<char, u32>::new()
            .add('a', 1)
            .add('b', 1)
            .add('c', 1)
            .add('d', 1)
            .build()
            .unwrap();

        let mut expected = HashMap::new();
        expected.insert('a', vec![false, false]);
        expected.insert('b', vec![false, true]);
        expected.insert('c', vec![true, false]);
        expected.insert('d', vec![true, true]);

        assert_eq!(expected, tree.encoding());
    }

    #[test]
    fn encode() {
        let tree = HuffBuilder::<char, u32>::new()
            .add('a', 1)
            .add('b', 1)
            .add('c', 1)
            .add('d', 1)
            .build()
            .unwrap();

        let mut output: Vec<u8> = vec![];
        {
            let mut writer = HuffWriter::new(tree, &mut output);

            for value in vec!['a', 'b', 'c', 'd', 'a'] {
                writer.write(&value).unwrap();
            }
        }

        let expected = vec![0b_00011011, 0b_00000000];

        assert_eq!(expected, output);
    }

    #[test]
    fn encode_value_error() {
        let tree = HuffBuilder::<char, u32>::new().add('a', 1).build().unwrap();

        let mut output: Vec<u8> = vec![];
        {
            let mut writer = HuffWriter::new(tree, &mut output);

            for value in vec!['b'] {
                match writer.write(&value) {
                    Ok(_) => panic!(),
                    Err(_) => (),
                }
            }
        }
    }


    #[test]
    fn decode() {

        let tree = HuffBuilder::<char, u32>::new()
            .add('a', 1)
            .add('b', 1)
            .add('c', 1)
            .add('d', 1)
            .build()
            .unwrap();

        let input = vec![0b_00011011, 0b_00000000];

        let mut reader = HuffReader::new(tree, Cursor::new(input));
        let mut output = vec![];

        for _ in 0..5 {
            output.push(reader.read().unwrap());
        }

        assert_eq!(vec!['a', 'b', 'c', 'd', 'a'], output);
    }
}
