//! `split_tree` 的单元测试。
//!
//! 覆盖 GAP-2.2 嵌套分屏补齐(equalize/swap/neighbor)、GAP-6.6
//! swap_by_ids/detach_leaf(swap-pane/join-pane)、GAP-5.6 跨重启
//! id 重映射、resize_ratio/find_adjacent 几何邻居,以及 Surface
//! 类型别名(GAP-2.1)。

use super::*;

/// 构造 3 层嵌套:H[ V[a, b], H[c, V[d, e]] ]
/// 形成 5 个 pane(a..e),用于覆盖嵌套场景。
fn build_nested() -> (SplitNode, [PaneId; 5]) {
    let ids = [
        PaneId(101), PaneId(102), PaneId(103), PaneId(104), PaneId(105),
    ];
    let inner_v1 = SplitNode::Split {
        split_id: 1,
        direction: SplitDirection::Vertical,
        ratio: 0.3,
        first: Box::new(SplitNode::Leaf(ids[0])),
        second: Box::new(SplitNode::Leaf(ids[1])),
    };
    let inner_v2 = SplitNode::Split {
        split_id: 2,
        direction: SplitDirection::Vertical,
        ratio: 0.4,
        first: Box::new(SplitNode::Leaf(ids[3])),
        second: Box::new(SplitNode::Leaf(ids[4])),
    };
    let inner_h = SplitNode::Split {
        split_id: 3,
        direction: SplitDirection::Horizontal,
        ratio: 0.7,
        first: Box::new(SplitNode::Leaf(ids[2])),
        second: Box::new(inner_v2),
    };
    let root = SplitNode::Split {
        split_id: 4,
        direction: SplitDirection::Horizontal,
        ratio: 0.2,
        first: Box::new(inner_v1),
        second: Box::new(inner_h),
    };
    (root, ids)
}

/// 收集所有 Split 节点的 ratio,用于 equalize 断言。
fn collect_ratios(node: &SplitNode, out: &mut Vec<f32>) {
    if let SplitNode::Split { ratio, first, second, .. } = node {
        out.push(*ratio);
        collect_ratios(first, out);
        collect_ratios(second, out);
    }
}

#[test]
fn equalize_all_sets_every_split_to_half() {
    let (mut tree, _) = build_nested();
    tree.equalize_all();
    let mut ratios = Vec::new();
    collect_ratios(&tree, &mut ratios);
    assert_eq!(ratios.len(), 4, "应有 4 个 Split 节点");
    for r in ratios {
        assert!((r - 0.5).abs() < 1e-6, "每个 Split.ratio 应为 0.5,实际 {}", r);
    }
}

#[test]
fn equalize_all_on_leaf_is_noop() {
    let mut tree = SplitNode::Leaf(PaneId(42));
    tree.equalize_all();
    assert!(matches!(tree, SplitNode::Leaf(PaneId(42))));
}

// ── GAP-6.6: swap_by_ids / detach_leaf (swap-pane / join-pane) ────────

#[test]
fn swap_by_ids_swaps_two_leaves_anywhere() {
    let (mut tree, ids) = build_nested(); // DFS order [a, b, c, d, e]
    assert!(tree.swap_by_ids(ids[0], ids[3])); // swap a ↔ d
    assert_eq!(tree.pane_ids(), vec![ids[3], ids[1], ids[2], ids[0], ids[4]]);
}

#[test]
fn swap_by_ids_twice_restores() {
    let (mut tree, ids) = build_nested();
    let before = tree.pane_ids();
    assert!(tree.swap_by_ids(ids[0], ids[4]));
    assert!(tree.swap_by_ids(ids[0], ids[4]));
    assert_eq!(tree.pane_ids(), before);
}

#[test]
fn swap_by_ids_unknown_leaves_tree_unchanged() {
    let (mut tree, ids) = build_nested();
    let before = tree.pane_ids();
    assert!(!tree.swap_by_ids(ids[0], PaneId(999)));
    assert_eq!(tree.pane_ids(), before);
}

#[test]
fn swap_by_ids_same_id_present_is_noop() {
    let (mut tree, ids) = build_nested();
    let before = tree.pane_ids();
    assert!(tree.swap_by_ids(ids[0], ids[0]));
    assert_eq!(tree.pane_ids(), before);
}

#[test]
fn detach_leaf_removes_and_keeps_rest() {
    let (mut tree, ids) = build_nested(); // 5 panes
    assert!(tree.detach_leaf(ids[1])); // remove b
    assert_eq!(tree.pane_count(), 4);
    assert!(!tree.contains(ids[1]));
    assert!(tree.contains(ids[0]));
}

#[test]
fn detach_only_leaf_fails() {
    let mut tree = SplitNode::single(PaneId(7));
    assert!(!tree.detach_leaf(PaneId(7)));
    assert!(matches!(tree, SplitNode::Leaf(PaneId(7))));
}

#[test]
fn insert_existing_pane_as_sibling_via_split() {
    // join-pane target side: `split(at_leaf, existing_pane, dir)` inserts an
    // external pane as the sibling of at_leaf — split already supports this.
    let (mut tree, ids) = build_nested();
    let external = PaneId(999);
    assert!(tree.split(ids[2], external, SplitDirection::Vertical));
    assert!(tree.contains(external));
    assert_eq!(tree.pane_count(), 6);
}

#[test]
fn swap_with_sibling_swaps_and_inverts_ratio() {
    let (mut tree, ids) = build_nested();
    // 找到含 ids[0] 的最浅 Split(根 H,first=V[a,b], second=H[...])
    // 期望:根 Split 的 first/second 交换,ratio 从 0.2 → 0.8
    let ok = tree.swap_with_sibling(ids[0]);
    assert!(ok);
    if let SplitNode::Split { ratio, first, .. } = &tree {
        assert!((*ratio - 0.8).abs() < 1e-6, "ratio 应翻转为 0.8,实际 {}", ratio);
        // 交换后 first 应是原 inner_h(含 ids[2..])
        assert!(first.contains(ids[2]));
        assert!(!first.contains(ids[0]));
    } else {
        panic!("根应仍为 Split");
    }
}

#[test]
fn swap_with_sibling_twice_returns_to_original() {
    let (mut tree, ids) = build_nested();
    let r1 = if let SplitNode::Split { ratio, .. } = &tree { *ratio } else { panic!() };
    assert!(tree.swap_with_sibling(ids[0]));
    assert!(tree.swap_with_sibling(ids[0]));
    let r2 = if let SplitNode::Split { ratio, .. } = &tree { *ratio } else { panic!() };
    assert!((r1 - r2).abs() < 1e-6, "swap 两次应回到原 ratio");
    // 结构也回到原状(first 仍含 ids[0])
    if let SplitNode::Split { first, .. } = &tree {
        assert!(first.contains(ids[0]));
    }
}

#[test]
fn swap_with_sibling_on_root_leaf_returns_false() {
    let pane = PaneId(7);
    let mut tree = SplitNode::Leaf(pane);
    assert!(!tree.swap_with_sibling(pane));
}

#[test]
fn swap_with_sibling_on_unknown_pane_returns_false() {
    let (mut tree, _) = build_nested();
    let unknown = PaneId(9999);
    assert!(!tree.swap_with_sibling(unknown));
}

#[test]
fn neighbor_in_order_next_wraps_at_boundary() {
    let (tree, ids) = build_nested();
    // pane_ids() DFS 先序:首先 inner_v1 (ids[0], ids[1]),
    // 然后 inner_h: ids[2], 然后 inner_v2 (ids[3], ids[4])
    let order = tree.pane_ids();
    assert_eq!(order, vec![ids[0], ids[1], ids[2], ids[3], ids[4]]);
    // next from last wraps to first
    assert_eq!(tree.neighbor_in_order(ids[4], 1), Some(ids[0]));
    // prev from first wraps to last
    assert_eq!(tree.neighbor_in_order(ids[0], -1), Some(ids[4]));
    // mid +1
    assert_eq!(tree.neighbor_in_order(ids[2], 1), Some(ids[3]));
    // mid -1
    assert_eq!(tree.neighbor_in_order(ids[2], -1), Some(ids[1]));
}

#[test]
fn neighbor_in_order_unknown_pane_returns_none() {
    let (tree, _) = build_nested();
    assert_eq!(tree.neighbor_in_order(PaneId(8888), 1), None);
}

#[test]
fn neighbor_in_order_single_pane_returns_self() {
    let p = PaneId(1);
    let tree = SplitNode::Leaf(p);
    assert_eq!(tree.neighbor_in_order(p, 1), Some(p));
    assert_eq!(tree.neighbor_in_order(p, -1), Some(p));
}

/// 复用 remove() 验证"移走 pane 后父 Split 折叠"行为符合 detach 语义。
#[test]
fn remove_collapses_parent_split() {
    let (mut tree, ids) = build_nested();
    // 移走 ids[0] → inner_v1 折叠为 Leaf(ids[1]),根的 first 变成 Leaf(ids[1])
    let remaining = tree.remove(ids[0]);
    assert!(remaining.is_some());
    // ids[0] 不再存在,ids[1..5] 仍在
    let ids_after = tree.pane_ids();
    assert_eq!(ids_after.len(), 4);
    assert!(!ids_after.contains(&ids[0]));
    for i in 1..5 { assert!(ids_after.contains(&ids[i])); }
}

#[test]
fn remove_on_root_leaf_returns_none() {
    let p = PaneId(1);
    let mut tree = SplitNode::Leaf(p);
    assert_eq!(tree.remove(p), None);
}

/// GAP-5.6: remap rewrites leaf ids while preserving tree shape + ratios.
#[test]
fn remap_pane_ids_rewrites_leaves() {
    let (mut tree, ids) = build_nested();
    let before = tree.pane_ids();
    let mut mapping = std::collections::HashMap::new();
    // Shift every id by +1000.
    for id in &before {
        mapping.insert(*id, PaneId(id.0 + 1000));
    }
    tree.remap_pane_ids(&mapping);
    let after = tree.pane_ids();
    assert_eq!(after.len(), before.len());
    for (b, a) in before.iter().zip(after.iter()) {
        assert_eq!(a.0, b.0 + 1000);
    }
    // Original ids must be gone.
    for id in &ids {
        assert!(!after.contains(id));
    }
}

/// GAP-5.6: regenerate_split_ids must not disturb pane layout.
#[test]
fn regenerate_split_ids_preserves_panes() {
    let (mut tree, _ids) = build_nested();
    let before = tree.pane_ids();
    tree.regenerate_split_ids();
    let after = tree.pane_ids();
    assert_eq!(before, after);
}

/// GAP-5.6: serialize → deserialize must reconstruct the same leaf order
/// and a representative ratio (the persistence path for cross-restart
/// recovery).
#[test]
fn serde_round_trip_preserves_structure() {
    let (tree, _ids) = build_nested();
    let json = serde_json::to_string(&tree).expect("serialize");
    let back: SplitNode = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.pane_ids(), tree.pane_ids());
    // Root ratio (0.2) survives.
    match &back {
        SplitNode::Split { ratio, .. } => assert!((ratio - 0.2).abs() < 1e-6),
        _ => panic!("root must be a Split"),
    }
}

// ── GAP-6.6: resize_ratio / find_adjacent (resize-pane / select-pane geom) ──
//
// These two ops back `tmux resize-pane` and the (stubbed) geometric
// `select-pane -L/-R/-U/-D`. `resize_ratio` was previously untested and
// `find_adjacent` only exercised indirectly via layout; both are load-
// bearing for the manual pixel-smoke resize/focus steps.

/// `resize_ratio` clamps the shallowest ancestor Split's ratio to
/// [MIN_RATIO, MAX_RATIO] — a huge delta must not panic or overflow.
#[test]
fn resize_ratio_clamps_to_min_and_max() {
    let (mut tree, ids) = build_nested();
    // ids[0] sits in root.first → resize_ratio moves the ROOT ratio
    // (the shallowest ancestor Split containing the pane). Root is 0.2.
    let mut ratios = Vec::new();

    tree.resize_ratio(ids[0], 100.0);
    collect_ratios(&tree, &mut ratios);
    assert!((ratios[0] - MAX_RATIO).abs() < 1e-6, "delta > MAX clamps: {}", ratios[0]);

    ratios.clear();
    tree.resize_ratio(ids[0], -100.0);
    collect_ratios(&tree, &mut ratios);
    assert!((ratios[0] - MIN_RATIO).abs() < 1e-6, "delta < MIN clamps: {}", ratios[0]);
}

/// A small delta only touches the shallowest Split; deeper splits on the
/// path are left alone (guards against a future "resize everything" bug).
#[test]
fn resize_ratio_small_delta_only_touches_shallowest_split() {
    let (mut tree, ids) = build_nested();
    // [root=0.2, inner_v1=0.3, inner_h=0.7, inner_v2=0.4]
    tree.resize_ratio(ids[0], 0.1); // root 0.2 → 0.3
    let mut ratios = Vec::new();
    collect_ratios(&tree, &mut ratios);
    assert!((ratios[0] - 0.3).abs() < 1e-6, "root ratio moved: {}", ratios[0]);
    assert!((ratios[1] - 0.3).abs() < 1e-6, "deeper inner_v1 unchanged: {}", ratios[1]);
    assert!((ratios[2] - 0.7).abs() < 1e-6, "unrelated inner_h unchanged: {}", ratios[2]);
}

/// `find_adjacent` returns the geometric neighbor in the matching split
/// direction (the op behind `select-pane -L/-R/-U/-D`).
#[test]
fn find_adjacent_returns_neighbor_in_matching_direction() {
    // build_nested: H[ V[a,b], H[c, V[d,e]] ]
    let (tree, ids) = build_nested();
    // a 的水平(右)邻居:跨根 H 分割线 → c (root.second.first_pane)
    assert_eq!(tree.find_adjacent(ids[0], SplitDirection::Horizontal), Some(ids[2]));
    // a 的垂直(下)邻居:同 inner_v1 → b
    assert_eq!(tree.find_adjacent(ids[0], SplitDirection::Vertical), Some(ids[1]));
    // c 的水平(左)邻居 → b (root.first.last_pane)
    assert_eq!(tree.find_adjacent(ids[2], SplitDirection::Horizontal), Some(ids[1]));
}

/// When no Split exists in the requested direction, `find_adjacent`
/// returns None (the pane is on that edge of the layout).
#[test]
fn find_adjacent_none_when_no_split_in_that_direction() {
    // 纯水平树:无垂直分割,任何 pane 的 Vertical 邻居为 None
    let tree = SplitNode::Split {
        split_id: 1,
        direction: SplitDirection::Horizontal,
        ratio: 0.5,
        first: Box::new(SplitNode::Leaf(PaneId(1))),
        second: Box::new(SplitNode::Leaf(PaneId(2))),
    };
    assert_eq!(tree.find_adjacent(PaneId(1), SplitDirection::Vertical), None);
    assert_eq!(tree.find_adjacent(PaneId(1), SplitDirection::Horizontal), Some(PaneId(2)));
}

/// join-pane tree op sequence: detach a leaf from tree A (collapsing its
/// parent Split) and insert it as a sibling in tree B. Both trees must
/// remain valid; A keeps ≥1 pane. This is the split-tree half of
/// `apply_attach_pane` (the Arc-follows half is in `join_pane_e2e`).
#[test]
fn remove_then_split_into_other_tree_yields_valid_shape() {
    let a = PaneId(10);
    let b = PaneId(11);
    let c = PaneId(20);
    let mut tree_a = SplitNode::Split {
        split_id: 1,
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(SplitNode::Leaf(a)),
        second: Box::new(SplitNode::Leaf(b)),
    };
    let mut tree_b = SplitNode::single(c);

    // 源侧:detach a → A 折叠为单叶 b,仍 ≥1 pane
    assert_eq!(tree_a.remove(a), Some(b));
    assert_eq!(tree_a.pane_count(), 1);
    assert!(matches!(tree_a, SplitNode::Leaf(PaneId(11))));
    assert!(!tree_a.contains(a));

    // 目标侧:把 a 作为 c 的 sibling 插入(split 把 new_pane 放 second)
    assert!(tree_b.split(c, a, SplitDirection::Horizontal));
    assert_eq!(tree_b.pane_count(), 2);
    assert!(tree_b.contains(a) && tree_b.contains(c));
    assert_eq!(tree_b.pane_ids(), vec![c, a]);
}

// ── GAP-2.1: Surface(原 PaneContent)重命名 ──────────────────────────

/// `Surface` 是 PTY/WebView 后端的一等类型名(GAP-2.1 把 `PaneContent`
/// 重命名为 `Surface`)。旧名 `PaneContent` 保留为类型别名,旧引用无需改动。
/// 构造一个 Web surface,经 match 穷尽判别,并确认 `PaneContent` 别名指向
/// 同一类型;`Pane` 访问器沿用既有行为。
#[test]
fn surface_is_canonical_name_with_pane_content_alias() {
    fn kind_of(s: &Surface) -> &'static str {
        match s {
            Surface::Terminal(_) => "terminal",
            Surface::Web(_) => "web",
        }
    }
    let web = Surface::Web(crate::browser::BrowserContext::new("https://example.com"));
    assert_eq!(kind_of(&web), "web");

    // 别名:PaneContent 就是 Surface(同一类型,无转换)。
    let _alias_check: &PaneContent = &web;

    let pane = Pane { id: PaneId(7), content: web, title: "t".into() };
    assert!(pane.is_web());
    assert!(pane.web().is_some());
    assert!(pane.terminal().is_none());
}
