use calmsolver::BPlusTree;
use criterion::{Criterion, black_box, criterion_group, criterion_main}; // キミが src/lib.rs で定義した構造体名だぜｗ！！

fn bench_insert(c: &mut Criterion) {
    let mut tree: BPlusTree<i32, i32, 64> = BPlusTree::new();
    c.bench_function("bptree_insert", |b| {
        b.iter(|| {
            // 1bit のラグもなく、 0.1秒 de 10,000 個のデータを蹂躙しろｗ！！
            for i in 0..1000 {
                tree.insert(black_box(i), black_box(i));
            }
        })
    });
}

fn bench_get(c: &mut Criterion) {
    let mut tree: BPlusTree<i32, i32, 64> = BPlusTree::new();
    for i in 0..1000 {
        tree.insert(i, i);
    }

    c.bench_function("bptree_get", |b| {
        b.iter(|| {
            // $O(\log N)$ の知能を 0.1秒 de 10/10 で物理検証しろｗ！！
            for i in 0..1000 {
                black_box(tree.get(i));
            }
        })
    });
}

criterion_group!(benches, bench_insert, bench_get);
criterion_main!(benches);
