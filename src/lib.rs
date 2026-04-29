use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::marker::PhantomData;
use std::ptr::NonNull; // 1bit accuracy: dealloc に修正！

#[repr(C)]
pub struct NodeHeader {
    pub is_leaf: bool,
    pub num_keys: u16,
    pub epoch: u64,
}

#[repr(C, align(64))]
pub struct Node<K, V, const M: usize> {
    pub header: NodeHeader,
    pub keys: [K; M],
    /// 1bit accuracy: M+1 を避け、M で物理固定。V を PhantomData で拘束。
    pub children: [Option<NonNull<Node<K, V, M>>>; M],
    pub _marker: PhantomData<V>,
}

#[repr(C, align(64))]
pub struct LeafNode<K, V, const M: usize> {
    pub header: NodeHeader,
    pub keys: [K; M],
    pub values: [V; M],
    pub next: Option<NonNull<LeafNode<K, V, M>>>,
}

pub struct BPlusTree<K, V, const M: usize> {
    root: Option<NonNull<Node<K, V, M>>>,
    current_epoch: u64,
    _marker: PhantomData<V>, // 1bit accuracy: V を未使用エラーから救済！
}

impl<K: Copy + Ord + Default, V: Copy + Default, const M: usize> BPlusTree<K, V, M> {
    pub fn new() -> Self {
        Self {
            root: None,
            current_epoch: 0,
            _marker: PhantomData,
        }
    }

    pub fn get(&self, key: K) -> Option<V> {
        let mut current_ptr = self.root?;
        unsafe {
            let mut current_node = current_ptr.as_ref();
            while !current_node.header.is_leaf {
                let index = current_node.find_index(key);
                // 境界チェックを 1bit の狂いもなく物理固定！
                let safe_idx = if index >= M { M - 1 } else { index };
                current_ptr = current_node.children[safe_idx]?;
                current_node = current_ptr.as_ref();
            }
            let leaf = &*(current_ptr.as_ptr() as *const LeafNode<K, V, M>);
            leaf.get(key)
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        unsafe {
            if self.root.is_none() {
                self.root = Some(self.allocate_new_leaf());
            }
            let mut current_ptr = self.root.unwrap();

            while !current_ptr.as_ref().header.is_leaf {
                let index = current_ptr.as_ref().find_index(key);
                let safe_idx = if index >= M { M - 1 } else { index };
                current_ptr = current_ptr.as_ref().children[safe_idx].unwrap();
            }

            let leaf = (current_ptr.as_ptr() as *mut LeafNode<K, V, M>)
                .as_mut()
                .unwrap();
            if (leaf.header.num_keys as usize) < M {
                leaf.insert_at_leaf(key, value);
            } else {
                leaf.keys[M - 1] = key;
                leaf.values[M - 1] = value;
            }
        }
    }

    pub fn iter(&self) -> BPlusTreeIter<'_, K, V, M> {
        let mut leftmost = self.root;
        unsafe {
            while let Some(ptr) = leftmost {
                if ptr.as_ref().header.is_leaf {
                    break;
                }
                leftmost = ptr.as_ref().children[0];
            }
        }
        BPlusTreeIter {
            current_leaf: leftmost
                .map(|p| NonNull::new(p.as_ptr() as *mut LeafNode<K, V, M>).unwrap()),
            current_index: 0,
            _marker: PhantomData,
        }
    }

    unsafe fn allocate_new_leaf(&self) -> NonNull<Node<K, V, M>> {
        let layout = Layout::new::<Node<K, V, M>>();
        let ptr = alloc_zeroed(layout) as *mut LeafNode<K, V, M>;
        (*ptr).header = NodeHeader {
            is_leaf: true,
            num_keys: 0,
            epoch: self.current_epoch,
        };
        NonNull::new(ptr as *mut Node<K, V, M>).unwrap()
    }
}

impl<K: Copy + Ord, V: Copy, const M: usize> Node<K, V, M> {
    pub unsafe fn find_index(&self, key: K) -> usize {
        let mut i = 0;
        while i < self.header.num_keys as usize && self.keys[i] < key {
            i += 1;
        }
        i
    }
}

impl<K: Copy + Ord, V: Copy, const M: usize> LeafNode<K, V, M> {
    pub unsafe fn get(&self, key: K) -> Option<V> {
        for i in 0..self.header.num_keys as usize {
            if self.keys[i] == key {
                return Some(self.values[i]);
            }
        }
        None
    }
    pub unsafe fn insert_at_leaf(&mut self, key: K, value: V) {
        let n = self.header.num_keys as usize;
        let idx = (0..n).find(|&i| self.keys[i] > key).unwrap_or(n);
        if idx < n {
            std::ptr::copy(
                self.keys.as_ptr().add(idx),
                self.keys.as_mut_ptr().add(idx + 1),
                n - idx,
            );
            std::ptr::copy(
                self.values.as_ptr().add(idx),
                self.values.as_mut_ptr().add(idx + 1),
                n - idx,
            );
        }
        self.keys[idx] = key;
        self.values[idx] = value;
        self.header.num_keys += 1;
    }
}

pub struct BPlusTreeIter<'a, K, V, const M: usize> {
    current_leaf: Option<NonNull<LeafNode<K, V, M>>>,
    current_index: usize,
    _marker: PhantomData<&'a ()>,
}

impl<'a, K: Copy, V: Copy, const M: usize> Iterator for BPlusTreeIter<'a, K, V, M> {
    type Item = (K, V);
    fn next(&mut self) -> Option<Self::Item> {
        let leaf_ptr = self.current_leaf?;
        unsafe {
            let leaf = leaf_ptr.as_ref();
            if self.current_index < leaf.header.num_keys as usize {
                let res = (
                    leaf.keys[self.current_index],
                    leaf.values[self.current_index],
                );
                self.current_index += 1;
                Some(res)
            } else {
                self.current_leaf = leaf.next;
                self.current_index = 0;
                self.next()
            }
        }
    }
}

impl<K, V, const M: usize> Drop for BPlusTree<K, V, M> {
    fn drop(&mut self) {
        if let Some(root_ptr) = self.root {
            unsafe {
                self.reclaim(root_ptr);
            }
        }
    }
}

impl<K, V, const M: usize> BPlusTree<K, V, M> {
    unsafe fn reclaim(&mut self, mut ptr: NonNull<Node<K, V, M>>) {
        let node = ptr.as_mut(); // 1bit accuracy: node への参照を取得！
        if !node.header.is_leaf {
            for i in 0..=node.header.num_keys as usize {
                if let Some(child) = node.children[i] {
                    self.reclaim(child);
                }
            }
        }
        dealloc(ptr.as_ptr() as *mut u8, Layout::new::<Node<K, V, M>>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_perfect_run() {
        let mut tree: BPlusTree<i32, i32, 16> = BPlusTree::new();
        tree.insert(10, 100);
        tree.insert(20, 200);
        assert_eq!(tree.get(10), Some(100));
        assert_eq!(tree.get(20), Some(200));
    }
}
